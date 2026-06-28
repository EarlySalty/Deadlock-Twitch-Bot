//! `RaidArrivalSink`-Adapter — verbindet die Arrival-Runtime (Plan-Dispatcher)
//! mit den echten Stores + dem Klassifikator + dem ConfirmResolver. Port der
//! Effekte aus `raid_arrival_runtime.py` (`confirm_pending_raid_arrival` u. a.).
//!
//! **Sync/Async-Brücke:** `ArrivalConfirmationService` hat synchrone Lookups,
//! die DB-Status ist aber async. Der Adapter beschafft Partner-/Known-Status
//! **vorab** per async-Query und wrappt sie in `Prefetched*`-Lookups — dann
//! klassifiziert die sync-Engine ohne await.

use std::sync::{Arc, Mutex};

use chrono::Utc;
use sqlx::PgPool;
use tb_raid::{
    build_partner_raid_message, build_recruitment_message, classify_partner_raid_arrival,
    classify_partner_raid_arrival_with_expectation, decide_blacklist_action,
    plan_recruitment_delivery, serialize_confirmation_signals, ArrivalConfirmationService,
    ArrivalSignalContext, ArrivalTrackingStore, BlacklistScheduleAction,
    ConfirmedExternalRecruitmentRaid, ExternalRecruitmentStore, ManualRaidSuppression, PendingRaid,
    PendingRaidStore, RaidArrivalSink, RaidBlacklistStore, RaidHistoryStore, RecordArrivalInput,
    RecruitmentDeliveryConfig, RecruitmentDeliveryRequest, ScoreTrackingStore,
    EXTERNAL_RECRUITMENT_BLACKLIST_GRACE_SECONDS, EXTERNAL_RECRUITMENT_RAID_LIMIT,
};

use crate::confirm_resolver::{ConfirmContext, ConfirmResolver};
use crate::partner_lookup::{is_target_partner, known_source, PrefetchedLookups};
use crate::score_refresh::ScoreRefreshResolver;
// Trait muss im Scope sein, damit OutboundSuppressionStore::check_suppression
// (B3-2d Partner-Raid-Suppression-Gate) aufrufbar ist.
use tb_chat::moderation::OutboundSuppressionCheck;

/// Recent-Fenster für Sekundär-Signale (Python
/// `recent_raid_arrival_ttl_seconds = 600`, raid_state_store.py:16).
const RECENT_ARRIVAL_TTL_SECS: i64 = 600;
/// Grace-Period, bevor ein Orphan als eigenständiger Arrival promotet wird
/// (Python `orphan_chat_notification_grace_seconds = 15`).
const ORPHAN_GRACE_SECS: u64 = 15;
/// Aufbewahrung nicht-promotbarer Orphans (Python
/// `orphan_chat_notification_retention_seconds = 900`).
const ORPHAN_RETENTION_SECS: u64 = 900;

/// Verwaiste `channel.chat.notification`, die auf ein korrelierendes
/// Raid-Event wartet (Python-Payload-Dict in `orphan_chat_raid_notifications`).
#[derive(Debug, Clone)]
struct OrphanChatNotification {
    to_broadcaster_id: String,
    to_broadcaster_login: String,
    from_broadcaster_id: Option<String>,
    from_broadcaster_login: String,
    viewer_count: i32,
    message_id: Option<String>,
    event_timestamp: Option<String>,
    observed_at: std::time::Instant,
}

/// Cache-Key wie Python `build_raid_arrival_cache_key` (Ziel-ID + Quell-Login).
fn orphan_key(to_broadcaster_id: &str, from_broadcaster_login: &str) -> String {
    format!(
        "{}|{}",
        to_broadcaster_id.trim(),
        from_broadcaster_login.trim().to_lowercase()
    )
}

/// Prozessweit eindeutige Flow-ID, wenn das Pending keine trägt (Python
/// `_next_flow_id`). Da das Pending beim Confirm gepoppt wird, kann derselbe
/// Raid nicht doppelt bestätigt werden — Zähler + Millis genügen.
fn next_flow_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{n}", Utc::now().timestamp_millis())
}

// ─── Adapter ────────────────────────────────────────────────────────────────

pub struct RaidArrivalSinkImpl {
    pool: PgPool,
    pending: Arc<Mutex<PendingRaidStore>>,
    suppression: Arc<Mutex<ManualRaidSuppression>>,
    arrival_store: ArrivalTrackingStore,
    score_tracking: ScoreTrackingStore,
    external_recruitment: ExternalRecruitmentStore,
    blacklist: RaidBlacklistStore,
    raid_history: RaidHistoryStore,
    score_refresh: ScoreRefreshResolver,
    confirm_resolver: ConfirmResolver,
    /// Chat-Send-Port für die Partner-Raid-Dankesnachricht (B3-2d). `None`,
    /// wenn kein Bot-Token gebootet wurde — dann wird nicht gesendet (Python
    /// `get_chat_bot()` → None, partner_raid_delivery.py:233).
    chat_api: Option<Arc<dyn tb_chat::ChatApi>>,
    /// DB-Chat-Suppression-Store (NICHT die In-Memory-Manual-Suppression):
    /// blockt die Partner-Raid-Message, wenn der Ziel-Channel sie für die
    /// Source `partner_raid` unterdrückt hat (Python
    /// `lookup_outbound_chat_suppression`, partner_raid_delivery.py:239).
    outbound_suppression: Option<Arc<tb_chat::moderation::OutboundSuppressionStore>>,
    /// Verwaiste Chat-Notifications, vom Sweeper periodisch promotet
    /// (Python `orphan_chat_raid_notifications` in `raid_state_store.py`).
    orphans: Mutex<std::collections::HashMap<String, OrphanChatNotification>>,
}

impl RaidArrivalSinkImpl {
    pub fn new(
        pool: PgPool,
        pending: Arc<Mutex<PendingRaidStore>>,
        suppression: Arc<Mutex<ManualRaidSuppression>>,
        target_game_lower: &str,
        chat_api: Option<Arc<dyn tb_chat::ChatApi>>,
        outbound_suppression: Option<Arc<tb_chat::moderation::OutboundSuppressionStore>>,
    ) -> Self {
        Self {
            arrival_store: ArrivalTrackingStore::new(pool.clone()),
            score_tracking: ScoreTrackingStore::new(pool.clone()),
            external_recruitment: ExternalRecruitmentStore::new(pool.clone()),
            blacklist: RaidBlacklistStore::new(pool.clone()),
            raid_history: RaidHistoryStore::new(pool.clone()),
            score_refresh: ScoreRefreshResolver::new(pool.clone()),
            confirm_resolver: ConfirmResolver::new(pool.clone(), target_game_lower),
            pool,
            pending,
            suppression,
            chat_api,
            outbound_suppression,
            orphans: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Verarbeitet fällige verzögerte Recruitment-Blacklists (Grace abgelaufen):
    /// trägt das Ziel tatsächlich in die Raid-Blacklist ein, sofern es nicht
    /// bereits gelistet oder (wieder) Partner ist; danach wird das Pending
    /// aufgeräumt. Python `process_due_external_recruitment_blacklist_pending`
    /// (raid_blacklist.py:296). Wird vom Maintenance-Task in `main.rs` periodisch
    /// aufgerufen.
    pub async fn process_due_recruitment_blacklists(&self) {
        let due = match self.external_recruitment.load_due_blacklist_pending().await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::debug!(%error, "load_due_blacklist_pending fehlgeschlagen");
                return;
            }
        };

        for entry in due {
            // Bereits gelistet → nur das Pending aufräumen.
            match self
                .blacklist
                .is_blacklisted(Some(&entry.target_id), &entry.target_login)
                .await
            {
                Ok(true) => {
                    self.cleanup_blacklist_pending(&entry.target_id).await;
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::debug!(
                        %error, target = %entry.target_login,
                        "is_blacklisted-Check (due) fehlgeschlagen"
                    );
                    continue;
                }
            }

            // Partner-Ziele sind ausgenommen.
            if is_target_partner(&self.pool, &entry.target_id, &entry.target_login).await {
                self.cleanup_blacklist_pending(&entry.target_id).await;
                continue;
            }

            let reason = format!(
                "confirmed_external_recruitment_limit_grace_expired: count={} limit={} threshold_reached_at={}",
                entry.confirmed_raid_count,
                EXTERNAL_RECRUITMENT_RAID_LIMIT,
                entry.threshold_reached_at.to_rfc3339(),
            );
            if let Err(error) = self
                .blacklist
                .add(
                    Some(&entry.target_id),
                    &entry.target_login,
                    &reason,
                    Utc::now(),
                )
                .await
            {
                // Pending NICHT löschen → der nächste Lauf versucht es erneut.
                tracing::error!(
                    %error, target = %entry.target_login,
                    "add_to_blacklist (Recruitment-Grace abgelaufen) fehlgeschlagen"
                );
                continue;
            }
            tracing::info!(
                target = %entry.target_login,
                count = entry.confirmed_raid_count,
                "Externes Recruitment-Ziel nach Grace auf die Raid-Blacklist gesetzt"
            );
            self.cleanup_blacklist_pending(&entry.target_id).await;
        }
    }

    async fn cleanup_blacklist_pending(&self, target_id: &str) {
        if let Err(error) = self
            .external_recruitment
            .delete_blacklist_pending(target_id)
            .await
        {
            tracing::debug!(%error, "delete_blacklist_pending (Cleanup) fehlgeschlagen");
        }
    }

    /// Entfernt einen wartenden Orphan, sobald das korrelierende Raid-Event
    /// eintrifft (Python `raid_tracking_runtime.py:477-490` — nur Log).
    fn take_orphan(
        &self,
        to_broadcaster_id: &str,
        from_broadcaster_login: &str,
    ) -> Option<OrphanChatNotification> {
        let key = orphan_key(to_broadcaster_id, from_broadcaster_login);
        let popped = self
            .orphans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&key);
        if popped.is_some() {
            tracing::info!(
                from = %from_broadcaster_login,
                to_id = %to_broadcaster_id,
                "Pending-Raid hat frühere channel.chat.notification korreliert"
            );
        }
        popped
    }

    /// Promotet Orphans nach Ablauf der Grace-Period als eigenständige
    /// Arrival-Zeilen; verwirft nicht-promotbare nach der Retention-Zeit
    /// (Python `promote_stale_orphan_chat_raid_notifications`,
    /// `raid_state_store.py:225-266`). Wird vom Sweeper-Task in `main.rs`
    /// periodisch aufgerufen.
    pub async fn promote_stale_orphans(&self) {
        let stale: Vec<OrphanChatNotification> = {
            let map = self
                .orphans
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            map.values()
                .filter(|o| o.observed_at.elapsed().as_secs() >= ORPHAN_GRACE_SECS)
                .cloned()
                .collect()
        };

        for orphan in stale {
            let processed = self
                .record_independent_orphan_arrival(
                    &orphan.to_broadcaster_id,
                    &orphan.to_broadcaster_login,
                    orphan.from_broadcaster_id.as_deref(),
                    &orphan.from_broadcaster_login,
                    orphan.viewer_count,
                )
                .await;

            let drop_entry =
                processed || orphan.observed_at.elapsed().as_secs() >= ORPHAN_RETENTION_SECS;
            if drop_entry {
                let key = orphan_key(&orphan.to_broadcaster_id, &orphan.from_broadcaster_login);
                self.orphans
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&key);
                if !processed {
                    tracing::info!(
                        from = %orphan.from_broadcaster_login,
                        to = %orphan.to_broadcaster_login,
                        "Verwaiste channel.chat.notification ohne Korrelation verworfen"
                    );
                }
            }
        }
    }

    /// Schreibt einen promoteten Orphan als eigenständigen Arrival
    /// (Python `process_independent_partner_raid_arrival` mit
    /// `correlation_status="orphan_chat_notification"`). Gibt `true` zurück,
    /// wenn der Eintrag verarbeitet ist (auch: Ziel kein Partner).
    async fn record_independent_orphan_arrival(
        &self,
        to_broadcaster_id: &str,
        to_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        from_broadcaster_login: &str,
        viewer_count: i32,
    ) -> bool {
        let lookups = self
            .prefetch_lookups(
                to_broadcaster_id,
                to_broadcaster_login,
                from_broadcaster_id,
                from_broadcaster_login,
            )
            .await;
        let resolution = classify_partner_raid_arrival(
            Some(from_broadcaster_login),
            from_broadcaster_id,
            Some(to_broadcaster_id),
            Some(to_broadcaster_login),
            &lookups,
            &lookups,
        );
        let Some(classification) = resolution.classification else {
            // Ziel kein Partner → wie Python processed=False; der Aufrufer
            // verwirft den Eintrag erst nach der Retention-Zeit.
            return false;
        };
        match self
            .arrival_store
            .record_arrival(&RecordArrivalInput {
                from_broadcaster_id: from_broadcaster_id.map(str::to_string),
                from_broadcaster_login: from_broadcaster_login.to_string(),
                to_broadcaster_id: to_broadcaster_id.to_string(),
                to_broadcaster_login: to_broadcaster_login.to_string(),
                viewer_count,
                classification,
                confirmation_signals: "channel.chat.notification".to_string(),
                primary_signal: "channel.chat.notification".to_string(),
                correlation_status: "orphan_chat_notification".to_string(),
                correlation_detail: Some(
                    "channel.chat.notification arrived before pending raid registration"
                        .to_string(),
                ),
                source_resolution: resolution.source_resolution,
                raid_history_id: None,
                raid_history_executed_at: None,
                unraid_seen: false,
            })
            .await
        {
            Ok(_) => {
                // Suppression wie Python (mark_manual_raid_started, 180 s) —
                // verhindert einen Auto-Raid direkt nach dem externen Raid.
                if let Some(from_id) = from_broadcaster_id.map(str::trim).filter(|s| !s.is_empty())
                {
                    self.suppression
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .mark(from_id, 180.0, None);
                }
                tracing::info!(
                    from = %from_broadcaster_login,
                    to = %to_broadcaster_login,
                    "Orphan-Chat-Notification als eigenständiger Raid-Arrival promotet"
                );
                true
            }
            Err(error) => {
                tracing::error!(%error, "Orphan-Promotion: Arrival-Insert fehlgeschlagen");
                false
            }
        }
    }

    /// Vorab geladene Lookups für die sync Klassifikations-Engine.
    async fn prefetch_lookups(
        &self,
        to_id: &str,
        to_login: &str,
        from_id: Option<&str>,
        from_login: &str,
    ) -> PrefetchedLookups {
        PrefetchedLookups {
            target_is_partner: is_target_partner(&self.pool, to_id, to_login).await,
            known_source: known_source(&self.pool, from_id, from_login).await,
        }
    }

    /// Partner-Raid-Dankesnachricht an den Ziel-Channel (B3-2d). Port von
    /// `send_partner_raid_message` (partner_raid_delivery.py:224) inkl. des
    /// vorgelagerten `silent_raid`-Gates (raid_arrival_runtime.py:395-409).
    ///
    /// Reihenfolge exakt wie Python:
    /// 1. `silent_raid`-Gate ZUERST (runtime_core.py:408 `_lookup_silent_raid_enabled`):
    ///    aktive Partner-Zeile des Ziels, Spalte `silent_raid` — wenn gesetzt,
    ///    gar nicht senden.
    /// 2. DB-Chat-Suppression (partner_raid_delivery.py:239
    ///    `lookup_outbound_chat_suppression`, source `partner_raid`).
    /// 3. `received_raid_count` (partner_raid_delivery.py:252
    ///    `count_received_network_raids`, raid_metrics_store.py:46-52).
    /// 4. Nachricht bauen (`build_partner_raid_message`).
    /// 5. 5 s Delay (partner_raid_delivery.py:21 `delay_seconds=5.0`), dann
    ///    senden — NICHT-BLOCKIEREND via `tokio::spawn`, damit
    ///    `confirm_pending_raid` nicht 5 s blockiert.
    ///
    /// Reads (1–3) und Message-Build (4) laufen SYNCHRON vor dem spawn; nur
    /// `sleep` + `send` liegen im spawn.
    async fn send_partner_raid_message(
        &self,
        from_broadcaster_login: &str,
        to_broadcaster_login: &str,
        to_broadcaster_id: &str,
        viewer_count: i32,
    ) {
        // Ohne Bot-Token kein Send-Port (Python get_chat_bot() → None).
        let Some(chat_api) = self.chat_api.clone() else {
            tracing::debug!("Chat-API nicht verfügbar für Partner-Raid-Nachricht");
            return;
        };

        // 1. silent_raid-Gate ZUERST (raid_arrival_runtime.py:395 +
        //    runtime_core.py:408): aktive Partner-Zeile des Ziels prüfen.
        if self.target_silent_raid(to_broadcaster_login).await {
            tracing::debug!(
                from = %from_broadcaster_login,
                to = %to_broadcaster_login,
                "Partner-Raid-Nachricht unterdrückt (silent_raid)"
            );
            return;
        }

        // 2. DB-Chat-Suppression (partner_raid_delivery.py:239): wenn der
        //    Ziel-Channel die Source `partner_raid` unterdrückt hat → skip.
        if let Some(suppression) = self.outbound_suppression.as_ref() {
            if let Some(entry) = suppression
                .check_suppression(to_broadcaster_login, "partner_raid")
                .await
            {
                tracing::info!(
                    to = %to_broadcaster_login,
                    reason_code = %entry.reason_code,
                    until = %entry.suppressed_until,
                    "Partner-Raid-Nachricht übersprungen (gespeicherte Chat-Suppression)"
                );
                return;
            }
        }

        // 3. received_raid_count (partner_raid_delivery.py:252 +
        //    raid_metrics_store.py:46): erfolgreiche eingegangene Netzwerk-Raids
        //    des Ziels zählen; Python klemmt <= 0 auf 1.
        let received_raid_count = match sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_raid_history \
             WHERE to_broadcaster_id = $1 AND COALESCE(success, FALSE) IS TRUE",
        )
        .bind(to_broadcaster_id)
        .fetch_one(&self.pool)
        .await
        {
            Ok(count) if count > 0 => count,
            // count <= 0 → 1 (partner_raid_delivery.py:253-254).
            Ok(_) => 1,
            Err(error) => {
                tracing::debug!(
                    %error,
                    to = %to_broadcaster_login,
                    "count_received_network_raids fehlgeschlagen — fällt auf 1 zurück"
                );
                1
            }
        };

        // 4. Nachricht bauen (build_partner_raid_message, raid_messaging.rs:28).
        let message = build_partner_raid_message(
            from_broadcaster_login,
            to_broadcaster_login,
            viewer_count,
            received_raid_count,
        );

        // 5. 5 s Delay (partner_raid_delivery.py:21 delay_seconds=5.0), dann
        //    senden — NICHT-BLOCKIEREND (Vorbild: Werbefrei-Pitch in
        //    chat_wiring.rs). confirm_pending_raid darf nicht 5 s warten.
        let to_broadcaster_id = to_broadcaster_id.to_string();
        let to_broadcaster_login = to_broadcaster_login.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            match chat_api.send_message(&to_broadcaster_id, &message).await {
                Ok(tb_chat::types::SendOutcome::Sent) => tracing::info!(
                    to = %to_broadcaster_login,
                    "Partner-Raid-Nachricht gesendet"
                ),
                Ok(tb_chat::types::SendOutcome::Dropped { code, message }) => tracing::debug!(
                    to = %to_broadcaster_login,
                    %code,
                    drop_message = %message,
                    "Partner-Raid-Nachricht von Twitch verworfen"
                ),
                Ok(tb_chat::types::SendOutcome::HttpError { status, .. }) => tracing::debug!(
                    to = %to_broadcaster_login,
                    status,
                    "Partner-Raid-Nachricht-Send: HTTP-Fehler"
                ),
                Err(error) => tracing::debug!(
                    %error,
                    to = %to_broadcaster_login,
                    "Partner-Raid-Nachricht-Send fehlgeschlagen"
                ),
            }
        });
    }

    /// silent_raid-Lookup für den Ziel-Channel (Python `_lookup_silent_raid_enabled`,
    /// runtime_core.py:408): aktive Partner-Zeile, Spalte `silent_raid`. Keine
    /// Zeile / Lookup-Fehler → `false` (Python schluckt Fehler → False). Für
    /// externe Recruitment-Ziele (kein aktiver Partner-Eintrag) stets `false`.
    async fn target_silent_raid(&self, to_broadcaster_login: &str) -> bool {
        match sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(silent_raid, 0) <> 0 FROM twitch_partners \
             WHERE LOWER(twitch_login) = LOWER($1) AND status = 'active' LIMIT 1",
        )
        .bind(to_broadcaster_login)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(true)) => true,
            Ok(_) => false,
            Err(error) => {
                tracing::debug!(
                    %error,
                    to = %to_broadcaster_login,
                    "silent_raid-Lookup fehlgeschlagen — als nicht-silent behandelt"
                );
                false
            }
        }
    }

    /// Recruitment-Nachricht an einen extern geraideten Nicht-Partner-Channel
    /// (B3, Recruitment-Teil). Port von `send_recruitment_message_now`
    /// (recruitment_messaging.py:494) inkl. silent_raid-Gate
    /// (raid_arrival_runtime.py:395; für externe Ziele stets vacuous).
    ///
    /// `total_recruitment_raid_count` ist der bereits persistierte Count aus dem
    /// `record_confirmed_raid`-Schritt (Python `confirmed_external_raid_count`).
    ///
    /// Reihenfolge wie Python: silent_raid → Suppression(source=recruitment) →
    /// recent_raids-Count → plan_recruitment_delivery (followers_total=None, da
    /// invite_variant nur ungenutzte Metadaten beeinflusst) → Nachricht bauen →
    /// 15 s Delay-spawn → send; bei erfolgreichem Send (Sent) wird der verzögerte
    /// Bot-Ban-Check geplant (recruitment_messaging.py:678, 3600 s).
    async fn send_recruitment_message(
        &self,
        from_broadcaster_login: &str,
        to_broadcaster_login: &str,
        to_broadcaster_id: &str,
        total_recruitment_raid_count: i64,
    ) {
        let Some(chat_api) = self.chat_api.clone() else {
            tracing::debug!("Chat-API nicht verfügbar für Recruitment-Nachricht");
            return;
        };

        if self.target_silent_raid(to_broadcaster_login).await {
            tracing::debug!(
                to = %to_broadcaster_login,
                "Recruitment-Nachricht unterdrückt (silent_raid)"
            );
            return;
        }

        if let Some(suppression) = self.outbound_suppression.as_ref() {
            if let Some(entry) = suppression
                .check_suppression(to_broadcaster_login, "recruitment")
                .await
            {
                tracing::info!(
                    to = %to_broadcaster_login,
                    reason_code = %entry.reason_code,
                    until = %entry.suppressed_until,
                    "Recruitment-Nachricht übersprungen (gespeicherte Chat-Suppression)"
                );
                return;
            }
        }

        // recent_raids: erfolgreiche eingegangene Raids des Ziels in den letzten
        // 24 h (recruitment_messaging.py:803). Lookup-Fehler → 0 (Python-Default).
        let recent_raid_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_raid_history \
             WHERE to_broadcaster_id = $1 AND COALESCE(success, FALSE) IS TRUE \
               AND executed_at > NOW() - INTERVAL '1 day'",
        )
        .bind(to_broadcaster_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        // followers_total = None: beeinflusst in Python nur invite_variant, das
        // weder in die Nachricht noch in persistierten Zustand einfließt.
        let request = RecruitmentDeliveryRequest {
            from_broadcaster_login: from_broadcaster_login.to_string(),
            to_broadcaster_login: to_broadcaster_login.to_string(),
            target_id: Some(to_broadcaster_id.to_string()),
            recent_raid_count,
            total_recruitment_raid_count: Some(total_recruitment_raid_count),
            followers_total: None,
            chat_bot_available: true,
            outbound_chat_suppressed: false,
        };
        let plan = plan_recruitment_delivery(&request, &RecruitmentDeliveryConfig::default());
        if !plan.should_deliver() {
            tracing::info!(
                to = %to_broadcaster_login,
                reason = plan.reason.unwrap_or("blocked"),
                "Recruitment-Nachricht übersprungen"
            );
            return;
        }
        let Some(message_variant) = plan.message_variant else {
            return;
        };

        let message = build_recruitment_message(message_variant, to_broadcaster_login);

        let pool = self.pool.clone();
        let to_broadcaster_id = to_broadcaster_id.to_string();
        let to_broadcaster_login = to_broadcaster_login.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            match chat_api.send_message(&to_broadcaster_id, &message).await {
                Ok(tb_chat::types::SendOutcome::Sent) => {
                    tracing::info!(to = %to_broadcaster_login, "Recruitment-Nachricht gesendet");
                    if let Err(error) = ExternalRecruitmentStore::new(pool)
                        .schedule_bot_ban_check(
                            &to_broadcaster_id,
                            &to_broadcaster_login,
                            "recruitment",
                            3600,
                        )
                        .await
                    {
                        tracing::debug!(
                            %error,
                            to = %to_broadcaster_login,
                            "schedule_external_target_ban_check fehlgeschlagen"
                        );
                    }
                }
                Ok(tb_chat::types::SendOutcome::Dropped { code, message }) => tracing::debug!(
                    to = %to_broadcaster_login,
                    %code,
                    drop_message = %message,
                    "Recruitment-Nachricht von Twitch verworfen"
                ),
                Ok(tb_chat::types::SendOutcome::HttpError { status, .. }) => tracing::debug!(
                    to = %to_broadcaster_login,
                    status,
                    "Recruitment-Nachricht-Send: HTTP-Fehler"
                ),
                Err(error) => tracing::debug!(
                    %error,
                    to = %to_broadcaster_login,
                    "Recruitment-Nachricht-Send fehlgeschlagen"
                ),
            }
        });
    }
}

#[async_trait::async_trait]
impl tb_raid::OrphanReplay for RaidArrivalSinkImpl {
    async fn pop_orphan(
        &self,
        to_broadcaster_id: &str,
        from_broadcaster_login: &str,
    ) -> Option<tb_raid::OrphanChatNotification> {
        self.take_orphan(to_broadcaster_id, from_broadcaster_login)
            .map(|orphan| tb_raid::OrphanChatNotification {
                to_broadcaster_id: orphan.to_broadcaster_id,
                to_broadcaster_login: orphan.to_broadcaster_login,
                from_broadcaster_login: orphan.from_broadcaster_login,
                viewer_count: orphan.viewer_count,
                from_broadcaster_id: orphan.from_broadcaster_id,
                message_id: orphan.message_id,
                event_timestamp: orphan.event_timestamp,
            })
    }

    async fn replay(&self, orphan: tb_raid::OrphanChatNotification) {
        self.confirm_pending_raid(
            "channel.chat.notification",
            &orphan.to_broadcaster_id,
            &orphan.to_broadcaster_login,
            &orphan.from_broadcaster_login,
            orphan.from_broadcaster_id.as_deref(),
            orphan.viewer_count,
        )
        .await;
    }
}

#[async_trait::async_trait]
impl RaidArrivalSink for RaidArrivalSinkImpl {
    async fn store_pending_raid(&self, pending: &PendingRaid) {
        if let Ok(mut store) = self.pending.lock() {
            store.store(pending.clone());
        }
    }

    async fn confirm_pending_raid(
        &self,
        signal_type: &str,
        to_broadcaster_id: &str,
        to_broadcaster_login: &str,
        from_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        viewer_count: i32,
    ) {
        // 0. Frühere Orphan-Chat-Notification korrelieren (Python
        // raid_tracking_runtime.py:477-490 — pop + Log, kein Doppel-Insert).
        self.take_orphan(to_broadcaster_id, from_broadcaster_login);

        // 1. Pending entfernen (pop) — wie Python `pop_pending_raid`.
        let pending = match self.pending.lock() {
            Ok(mut store) => store.pop(to_broadcaster_id, Some(from_broadcaster_login)),
            Err(_) => None,
        };
        let Some(pending) = pending else { return };

        // P1.10: Effektive Zuschauerzahl. Trifft das Bestätigungs-Signal ohne
        // Zuschauerzahl ein (viewer_count==0, z. B. channel.chat.notification),
        // fällt Python (raid_arrival_runtime.py:263) auf die bei Raid-Erkennung
        // registrierte registered_viewer_count zurück. Sonst persistiert das
        // Arrival-Tracking 0 und die Dankesnachricht lautet "mit 0 Zuschauern".
        // `int(viewer_count or pending.registered_viewer_count or 0)`.
        let effective_viewer_count = if viewer_count != 0 {
            viewer_count
        } else {
            pending.registered_viewer_count
        };

        // 2. Partner-/Known-Status vorab async laden, dann sync klassifizieren.
        let lookups = self
            .prefetch_lookups(
                to_broadcaster_id,
                to_broadcaster_login,
                from_broadcaster_id,
                from_broadcaster_login,
            )
            .await;
        let known = lookups.known_source;

        // P1.13: expected_partner-Override. Ein Partner-Raid (pending.is_partner_raid)
        // zu einem noch NICHT in twitch_partners eingetragenen Ziel würde sonst als
        // `suppressed_external` klassifiziert (Ziel-Lookup = false) und die
        // Dankesnachricht/das Tracking unterdrückt. Wie Python
        // (runtime_factories.py:546-563) berechnet der Aufrufer vorab die
        // Klassifikation MIT expected_partner=is_partner_raid und reicht das
        // Ergebnis als Overrides durch, damit der zweite Pass `ours_to_partner`
        // erzwingt. Muss VOR dem Bau des Service laufen (der `lookups` verschiebt).
        let expectation = classify_partner_raid_arrival_with_expectation(
            Some(from_broadcaster_login),
            from_broadcaster_id,
            Some(to_broadcaster_id),
            Some(to_broadcaster_login),
            &lookups,
            &lookups,
            pending.is_partner_raid,
        );
        let classification_override = Some(expectation.classification.clone());
        let source_resolution_override = Some(Some(expectation.source_resolution.clone()));
        let target_is_partner_override = Some(expectation.classification.is_some());

        let svc = ArrivalConfirmationService::new(
            Box::new(PrefetchedLookups {
                target_is_partner: lookups.target_is_partner,
                known_source: known,
            }),
            Box::new(lookups),
        );
        let ctx = ArrivalSignalContext {
            from_broadcaster_login,
            from_broadcaster_id,
            to_broadcaster_id,
            to_broadcaster_login: Some(to_broadcaster_login),
        };
        let Some(decision) = svc.confirm_pending_raid_arrival(
            pending,
            &ctx,
            signal_type,
            classification_override,
            source_resolution_override,
            target_is_partner_override,
        ) else {
            return;
        };

        // 2b. Python raid_arrival_runtime.py:265 — ein Partner-Ziel darf nie auf
        // der externen Recruitment-Blacklist stehen: evtl. wartendes Pending löschen.
        if decision.should_delete_external_recruitment_blacklist_pending {
            if let Err(error) = self
                .external_recruitment
                .delete_blacklist_pending(to_broadcaster_id)
                .await
            {
                tracing::error!(
                    %error,
                    to = %to_broadcaster_login,
                    "delete_external_recruitment_blacklist_pending fehlgeschlagen"
                );
            }
        }

        // 2c. Python raid_arrival_runtime.py:272 — jüngste erfolgreiche
        // Raid-History-Referenz laden, um den bestätigten Arrival mit dem
        // tatsächlich ausgeführten Raid-Eintrag zu verknüpfen.
        let (raid_history_id, raid_history_executed_at) =
            if decision.should_load_recent_raid_history_reference {
                match self
                    .raid_history
                    .find_recent_reference(from_broadcaster_login, to_broadcaster_id)
                    .await
                {
                    Ok(Some((id, executed_at))) => (Some(id), executed_at),
                    Ok(None) => (None, None),
                    Err(error) => {
                        tracing::debug!(%error, "find_recent_reference fehlgeschlagen");
                        (None, None)
                    }
                }
            } else {
                (None, None)
            };

        // 3. Bei Partner-Ziel: Arrival-Tracking schreiben.
        if decision.target_is_partner {
            if let Err(e) = self
                .arrival_store
                .record_arrival(&RecordArrivalInput {
                    from_broadcaster_id: from_broadcaster_id.map(str::to_string),
                    from_broadcaster_login: from_broadcaster_login.to_string(),
                    to_broadcaster_id: to_broadcaster_id.to_string(),
                    to_broadcaster_login: to_broadcaster_login.to_string(),
                    viewer_count: effective_viewer_count,
                    classification: decision.classification.clone().unwrap_or_default(),
                    confirmation_signals: signal_type.to_string(),
                    primary_signal: signal_type.to_string(),
                    // Python raid_arrival_runtime.py:302: confirm-pending-Pfad schreibt
                    // "matched_pending" (nicht "confirmed") und correlation_detail=None.
                    correlation_status: "matched_pending".to_string(),
                    correlation_detail: None,
                    source_resolution: decision.source_resolution.clone(),
                    raid_history_id,
                    raid_history_executed_at,
                    unraid_seen: false,
                })
                .await
            {
                tracing::error!(
                    error = %e,
                    from = %from_broadcaster_login,
                    to = %to_broadcaster_login,
                    "Arrival-Tracking-Insert (confirm_pending_raid) fehlgeschlagen"
                );
            }
        }

        // 4a. Python raid_arrival_runtime.py:374 — nach einem eingehenden,
        // bestätigten Partner-Raid den Partner-Score-Cache des Ziels auffrischen
        // (sonst veraltet er, wenn keine Online/Offline-Events kamen).
        if decision.should_refresh_partner_score_cache {
            if let Err(error) = self
                .score_refresh
                .refresh_scores(
                    &[(
                        to_broadcaster_id.to_string(),
                        to_broadcaster_login.to_string(),
                    )],
                    Utc::now(),
                )
                .await
            {
                tracing::debug!(
                    %error,
                    to = %to_broadcaster_login,
                    "refresh_partner_score_cache fehlgeschlagen"
                );
            }
        }

        // 4. Bei ours_to_partner: bestätigten Partner-Raid tracken (Score-Effekt).
        if decision.should_track_confirmed_partner_raid {
            let confirm_ctx = ConfirmContext {
                signal_type,
                to_broadcaster_id,
                to_broadcaster_login,
                from_broadcaster_login,
                from_broadcaster_id,
                viewer_count: effective_viewer_count,
            };
            match self
                .confirm_resolver
                .resolve(&confirm_ctx, Utc::now())
                .await
            {
                Ok(input) => {
                    // Score-Effekt des bestätigten Raids — DB-Fehler dürfen
                    // den Arrival-Pfad nicht stoppen, aber nie still bleiben.
                    if let Err(error) = self.score_tracking.track_confirmed(&input).await {
                        tracing::error!(
                            %error,
                            from = %from_broadcaster_login,
                            to = %to_broadcaster_login,
                            "score_tracking.track_confirmed fehlgeschlagen"
                        );
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "Confirm-Resolver fehlgeschlagen");
                }
            }
        }

        // 4b. Partner-Raid-Dankesnachricht an den Ziel-Channel (B3-2d). Python
        // raid_arrival_runtime.py:395-409: silent_raid-Gate, dann bei
        // should_send_partner_raid_message die Nachricht senden. Der Versand
        // (silent_raid/Suppression/Count-Reads synchron, sleep+send im spawn)
        // steckt in send_partner_raid_message. Der gegenseitig ausschließende
        // Recruitment-Pfad (should_send_recruitment_message) folgt in Schritt 5b.
        if decision.should_send_partner_raid_message {
            self.send_partner_raid_message(
                from_broadcaster_login,
                to_broadcaster_login,
                to_broadcaster_id,
                effective_viewer_count,
            )
            .await;
        }

        // 5. Externe Recruitment-Raids: bestätigten Raid persistieren und bei
        // Schwellenüberschreitung die verzögerte Blacklist planen. Python
        // raid_arrival_runtime.py:338. (Partner-Track in Schritt 4 und externer
        // Recruitment-Pfad schließen sich gegenseitig aus — follow_up_kind.)
        if decision.should_persist_confirmed_external_recruitment_raid {
            let raid_flow_id = decision
                .pending_raid
                .raid_flow_id
                .clone()
                .unwrap_or_else(|| next_flow_id("raid-arrival"));
            let count = match self
                .external_recruitment
                .record_confirmed_raid(&ConfirmedExternalRecruitmentRaid {
                    raid_flow_id: Some(raid_flow_id.clone()),
                    from_broadcaster_id: from_broadcaster_id.map(str::to_string),
                    from_broadcaster_login: from_broadcaster_login.to_string(),
                    to_broadcaster_id: to_broadcaster_id.to_string(),
                    to_broadcaster_login: to_broadcaster_login.to_string(),
                    viewer_count: effective_viewer_count,
                    confirmation_signal: Some(signal_type.to_string()),
                })
                .await
            {
                Ok(count) => count,
                // Python: konnte nicht persistiert werden → externen Follow-up
                // (Schedule + spätere Recruitment-Message) überspringen.
                Err(error) => {
                    tracing::error!(
                        %error,
                        to = %to_broadcaster_login,
                        "record_confirmed_external_recruitment_raid fehlgeschlagen; externer Follow-up übersprungen"
                    );
                    return;
                }
            };

            if decision.should_schedule_external_recruitment_blacklist_pending {
                match decide_blacklist_action(count, decision.target_is_partner) {
                    BlacklistScheduleAction::None => {}
                    BlacklistScheduleAction::Delete => {
                        if let Err(error) = self
                            .external_recruitment
                            .delete_blacklist_pending(to_broadcaster_id)
                            .await
                        {
                            tracing::error!(
                                %error,
                                to = %to_broadcaster_login,
                                "delete_blacklist_pending (Ziel ist Partner) fehlgeschlagen"
                            );
                        }
                    }
                    BlacklistScheduleAction::Schedule => {
                        let count_i32 = i32::try_from(count).unwrap_or(i32::MAX);
                        if let Err(error) = self
                            .external_recruitment
                            .schedule_blacklist_pending(
                                to_broadcaster_id,
                                to_broadcaster_login,
                                count_i32,
                                Some(&raid_flow_id),
                                EXTERNAL_RECRUITMENT_BLACKLIST_GRACE_SECONDS,
                            )
                            .await
                        {
                            tracing::error!(
                                %error,
                                to = %to_broadcaster_login,
                                "schedule_external_recruitment_blacklist_pending fehlgeschlagen"
                            );
                        }
                    }
                }
            }

            // 5b. Recruitment-Nachricht an den externen Ziel-Channel (B3,
            // Recruitment-Teil). Python raid_arrival_runtime.py:410: bei
            // should_send_recruitment_message (= is_external = should_persist)
            // mit dem soeben persistierten Count senden. silent_raid/Suppression/
            // recent-Count/Plan/Delay/Send + Ban-Check stecken in der Methode.
            if decision.should_send_recruitment_message {
                self.send_recruitment_message(
                    from_broadcaster_login,
                    to_broadcaster_login,
                    to_broadcaster_id,
                    count,
                )
                .await;
            }
        }
    }

    async fn record_pending_observation(
        &self,
        pending: &PendingRaid,
        signal_type: &str,
        status: &str,
        reason: Option<&str>,
        detail: Option<&str>,
    ) {
        // Diagnostische Beobachtung auf dem gespeicherten Pending vermerken.
        if let Ok(mut store) = self.pending.lock() {
            if let Some(mut existing) = store.pop(
                &pending.to_broadcaster_id,
                Some(&pending.from_broadcaster_login),
            ) {
                existing.record_signal_observation(
                    signal_type,
                    status,
                    reason.map(str::to_string),
                    detail.map(str::to_string),
                );
                store.store(existing);
            }
        }
    }

    async fn record_secondary_signal(
        &self,
        signal_type: &str,
        from_broadcaster_login: &str,
        _from_broadcaster_id: Option<&str>,
        to_broadcaster_login: &str,
        to_broadcaster_id: &str,
        _viewer_count: i32,
        unraid_seen: bool,
    ) {
        // Sekundär-Signal auf einen bereits getrackten Raid: Signal-Liste der
        // jüngsten Arrival-Zeile erweitern (Python
        // `_handle_secondary_confirmed_signal`, raid_arrival_runtime.py:102-141;
        // viewer_count wird dort nur im Cache, NICHT in der DB aktualisiert).
        let recent = match self
            .arrival_store
            .find_recent_arrival(
                to_broadcaster_id,
                from_broadcaster_login,
                RECENT_ARRIVAL_TTL_SECS,
            )
            .await
        {
            Ok(r) => r,
            Err(error) => {
                tracing::error!(%error, "Sekundär-Signal: Arrival-Lookup fehlgeschlagen");
                return;
            }
        };
        let Some((arrival_id, existing_signals)) = recent else {
            // Wie Python: kein jüngerer Arrival im Fenster → no-op.
            tracing::debug!(
                from = %from_broadcaster_login,
                to = %to_broadcaster_login,
                signal = %signal_type,
                "Sekundär-Signal ohne jüngeren Arrival — ignoriert"
            );
            return;
        };

        let merged = serialize_confirmation_signals(
            existing_signals
                .split(',')
                .chain(std::iter::once(signal_type)),
        );
        if let Err(error) = self
            .arrival_store
            .update_arrival(arrival_id, &merged, unraid_seen)
            .await
        {
            tracing::error!(%error, arrival_id, "Sekundär-Signal: Arrival-Update fehlgeschlagen");
            return;
        }
        tracing::info!(
            from = %from_broadcaster_login,
            to = %to_broadcaster_login,
            signal = %signal_type,
            signals = %merged,
            unraid_seen,
            "Raid-Arrival-Sekundär-Signal vermerkt"
        );
    }

    async fn store_orphan_chat_notification(
        &self,
        to_broadcaster_id: &str,
        to_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        from_broadcaster_login: &str,
        viewer_count: i32,
        message_id: Option<&str>,
        event_timestamp: Option<&str>,
    ) {
        // Chat-Notification ohne Pending-Kontext: für spätere Korrelation
        // vormerken (Python `store_orphan_chat_raid_notification`,
        // raid_state_store.py:200-211). Kommt innerhalb der Grace-Period ein
        // echtes Raid-Event, wird der Eintrag gepoppt; sonst promotet ihn der
        // Sweeper nach 15 s als eigenständigen Arrival.
        let key = orphan_key(to_broadcaster_id, from_broadcaster_login);
        let orphan = OrphanChatNotification {
            to_broadcaster_id: to_broadcaster_id.to_string(),
            to_broadcaster_login: to_broadcaster_login.to_string(),
            from_broadcaster_id: from_broadcaster_id.map(str::to_string),
            from_broadcaster_login: from_broadcaster_login.to_string(),
            viewer_count,
            message_id: message_id.map(str::to_string),
            event_timestamp: event_timestamp.map(str::to_string),
            observed_at: std::time::Instant::now(),
        };
        self.orphans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, orphan);
    }

    async fn mark_manual_raid_started(&self, source_key: &str, ttl_seconds: f64) {
        // Manual-Raid-TTL-Lock: unterdrückt den Auto-Raid kurz nach einem
        // manuellen/externen Raid (sonst Doppel-Raid beim Offline-Gehen).
        self.suppression
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mark(source_key, ttl_seconds, None);
    }

    async fn record_independent_raid_arrival(
        &self,
        signal_type: &str,
        from_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        to_broadcaster_login: &str,
        to_broadcaster_id: &str,
        viewer_count: i32,
    ) {
        // Manueller/externer Raid auf einen Partner ohne Pending-Kontext —
        // klassifizieren + Arrival-Zeile schreiben (Python
        // `process_independent_partner_raid_arrival`; der Suppression-Mark
        // läuft als eigene Plan-Action über `mark_manual_raid_started`).
        let lookups = self
            .prefetch_lookups(
                to_broadcaster_id,
                to_broadcaster_login,
                from_broadcaster_id,
                from_broadcaster_login,
            )
            .await;
        let resolution = classify_partner_raid_arrival(
            Some(from_broadcaster_login),
            from_broadcaster_id,
            Some(to_broadcaster_id),
            Some(to_broadcaster_login),
            &lookups,
            &lookups,
        );
        let Some(classification) = resolution.classification else {
            return; // Ziel kein Partner → nichts zu tracken.
        };
        if let Err(error) = self
            .arrival_store
            .record_arrival(&RecordArrivalInput {
                from_broadcaster_id: from_broadcaster_id.map(str::to_string),
                from_broadcaster_login: from_broadcaster_login.to_string(),
                to_broadcaster_id: to_broadcaster_id.to_string(),
                to_broadcaster_login: to_broadcaster_login.to_string(),
                viewer_count,
                classification,
                confirmation_signals: signal_type.to_string(),
                primary_signal: signal_type.to_string(),
                correlation_status: "independent_channel_raid".to_string(),
                correlation_detail: None,
                source_resolution: resolution.source_resolution,
                raid_history_id: None,
                raid_history_executed_at: None,
                unraid_seen: false,
            })
            .await
        {
            tracing::error!(%error, "Independent-Arrival nicht speicherbar");
        }
    }
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tb_raid::PendingRaid;

    async fn setup(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_partners (twitch_user_id TEXT, twitch_login TEXT, status TEXT, silent_raid INTEGER DEFAULT 0)",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT, last_started_at TEXT, last_game TEXT, active_session_id BIGINT)",
            "CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TEXT, ended_at TEXT)",
            "CREATE TABLE twitch_raid_history (id BIGSERIAL PRIMARY KEY, from_broadcaster_id TEXT, from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT, executed_at TIMESTAMPTZ, success BOOLEAN)",
            "CREATE TABLE twitch_partner_raid_scores (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT DEFAULT '', avg_duration_sec INTEGER DEFAULT 0, time_pattern_score_base DOUBLE PRECISION DEFAULT 0.5, received_successful_raids_total INTEGER DEFAULT 0, is_new_partner_preferred INTEGER DEFAULT 0, new_partner_multiplier DOUBLE PRECISION DEFAULT 1.0, raid_boost_multiplier DOUBLE PRECISION DEFAULT 1.0, is_live INTEGER DEFAULT 0, current_started_at TEXT, current_uptime_sec INTEGER DEFAULT 0, duration_score DOUBLE PRECISION DEFAULT 0.5, time_pattern_score DOUBLE PRECISION DEFAULT 0.5, readiness_score DOUBLE PRECISION DEFAULT 0.5, fairness_score DOUBLE PRECISION DEFAULT 0.5, base_score DOUBLE PRECISION DEFAULT 0.5, final_score DOUBLE PRECISION DEFAULT 0.5, internal_sent_raids_30d INTEGER DEFAULT 0, internal_received_raids_30d INTEGER DEFAULT 0, internal_received_raids_7d INTEGER DEFAULT 0, today_received_raids INTEGER DEFAULT 0, last_computed_at TEXT DEFAULT '')",
            "CREATE TABLE twitch_raid_arrival_tracking (id SERIAL PRIMARY KEY, detected_at TIMESTAMPTZ DEFAULT NOW(), last_signal_at TIMESTAMPTZ, from_broadcaster_id TEXT, from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT, viewer_count INTEGER, classification TEXT, confirmation_signals TEXT, primary_signal TEXT, correlation_status TEXT, correlation_detail TEXT, source_resolution TEXT, raid_history_id BIGINT, raid_history_executed_at TIMESTAMPTZ, unraid_seen BOOLEAN, last_unraid_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_partner_raid_score_tracking (id SERIAL PRIMARY KEY, raid_history_id BIGINT, from_broadcaster_id TEXT, from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT, viewer_count INTEGER, confirmed_at TEXT, target_session_id INTEGER, target_stream_started_at TEXT, score_last_computed_at TEXT, final_score DOUBLE PRECISION, base_score DOUBLE PRECISION, duration_score DOUBLE PRECISION, time_pattern_score DOUBLE PRECISION, new_partner_multiplier DOUBLE PRECISION, raid_boost_multiplier DOUBLE PRECISION, today_received_raids INTEGER, was_deadlock_at_raid INTEGER, deadlock_continued_until TEXT, deadlock_continued_sec INTEGER, resolved_at TEXT, resolution_reason TEXT, raid_history_executed_at TIMESTAMPTZ, readiness_score DOUBLE PRECISION, fairness_score DOUBLE PRECISION)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn confirm_partner_quelle_bekannt_schreibt_arrival_und_score_tracking() {
        let pool = setup("t6e_arrival_sink").await;
        // Ziel 200 ist aktiver Partner; Quelle 100 ist bekannter Streamer mit ID.
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('200','dst','active')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ('100','src')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, last_started_at, last_game, active_session_id) VALUES ('200','2026-06-10T16:00:00+00:00','Deadlock',5)").execute(&pool).await.unwrap();

        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        // Pending-Raid 100(src) -> 200 ablegen.
        pending_store
            .lock()
            .unwrap()
            .store(PendingRaid::new("src", "200"));

        let suppression = Arc::new(Mutex::new(tb_raid::ManualRaidSuppression::new()));
        let sink = RaidArrivalSinkImpl::new(
            pool.clone(),
            pending_store.clone(),
            suppression,
            "deadlock",
            None,
            None,
        );
        sink.confirm_pending_raid("channel.raid", "200", "dst", "src", Some("100"), 42)
            .await;

        // Pending wurde gepoppt.
        assert_eq!(pending_store.lock().unwrap().len(), 0, "Pending gepoppt");
        // Arrival-Tracking geschrieben mit ours_to_partner.
        let (cls, cnt): (String, i64) = sqlx::query_as(
            "SELECT classification, COUNT(*) OVER () FROM twitch_raid_arrival_tracking WHERE to_broadcaster_id='200'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(cnt, 1);
        assert_eq!(cls, "ours_to_partner");
        // Score-Tracking geschrieben (should_track bei ours_to_partner).
        let track_cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_partner_raid_score_tracking WHERE to_broadcaster_id='200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(track_cnt, 1, "bestaetigter Partner-Raid getrackt");
        let deadlock: i32 = sqlx::query_scalar("SELECT was_deadlock_at_raid FROM twitch_partner_raid_score_tracking WHERE to_broadcaster_id='200'").fetch_one(&pool).await.unwrap();
        assert_eq!(deadlock, 1, "live_state.last_game=Deadlock -> was_deadlock");
    }

    #[tokio::test]
    async fn confirm_viewer_count_null_faellt_auf_registered_zurueck() {
        // P1.10: Bestätigungs-Signal ohne Zuschauerzahl (viewer_count=0, z. B.
        // channel.chat.notification) muss auf pending.registered_viewer_count
        // zurückfallen — sonst persistiert das Arrival-Tracking 0 und die
        // Dankesnachricht lautet "mit 0 Zuschauern".
        let pool = setup("t6e_arrival_sink_p1_10").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('200','dst','active')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ('100','src')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, last_started_at, last_game, active_session_id) VALUES ('200','2026-06-10T16:00:00+00:00','Deadlock',5)").execute(&pool).await.unwrap();

        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        let mut pending = PendingRaid::new("src", "200");
        pending.registered_viewer_count = 137;
        pending_store.lock().unwrap().store(pending);

        let suppression = Arc::new(Mutex::new(tb_raid::ManualRaidSuppression::new()));
        let sink = RaidArrivalSinkImpl::new(
            pool.clone(),
            pending_store.clone(),
            suppression,
            "deadlock",
            None,
            None,
        );
        // viewer_count=0 simuliert die zuschauerlose chat.notification-Bestätigung.
        sink.confirm_pending_raid(
            "channel.chat.notification",
            "200",
            "dst",
            "src",
            Some("100"),
            0,
        )
        .await;

        let persisted: i32 = sqlx::query_scalar(
            "SELECT viewer_count FROM twitch_raid_arrival_tracking WHERE to_broadcaster_id='200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            persisted, 137,
            "viewer_count=0 -> Fallback auf registered_viewer_count=137"
        );
        // Score-Tracking trägt ebenfalls den effektiven Count.
        let tracked: i32 = sqlx::query_scalar(
            "SELECT viewer_count FROM twitch_partner_raid_score_tracking WHERE to_broadcaster_id='200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(tracked, 137, "Score-Tracking nutzt effektiven Count");
    }

    #[tokio::test]
    async fn confirm_viewer_count_vorhanden_ueberschreibt_registered() {
        // Gegenprobe: ein nicht-null viewer_count gewinnt gegen registered.
        let pool = setup("t6e_arrival_sink_p1_10_override").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('200','dst','active')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ('100','src')").execute(&pool).await.unwrap();

        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        let mut pending = PendingRaid::new("src", "200");
        pending.registered_viewer_count = 137;
        pending_store.lock().unwrap().store(pending);

        let suppression = Arc::new(Mutex::new(tb_raid::ManualRaidSuppression::new()));
        let sink = RaidArrivalSinkImpl::new(
            pool.clone(),
            pending_store.clone(),
            suppression,
            "deadlock",
            None,
            None,
        );
        sink.confirm_pending_raid("channel.raid", "200", "dst", "src", Some("100"), 42)
            .await;

        let persisted: i32 = sqlx::query_scalar(
            "SELECT viewer_count FROM twitch_raid_arrival_tracking WHERE to_broadcaster_id='200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(persisted, 42, "vorhandener viewer_count gewinnt");
    }

    #[tokio::test]
    async fn confirm_nicht_partner_ziel_kein_score_tracking() {
        let pool = setup("t6e_arrival_sink_nonpartner").await;
        // Ziel 200 NICHT Partner; kein twitch_partners-Eintrag.
        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        pending_store
            .lock()
            .unwrap()
            .store(PendingRaid::new("src", "200"));
        let suppression = Arc::new(Mutex::new(tb_raid::ManualRaidSuppression::new()));
        let sink = RaidArrivalSinkImpl::new(
            pool.clone(),
            pending_store.clone(),
            suppression,
            "deadlock",
            None,
            None,
        );
        sink.confirm_pending_raid("channel.raid", "200", "dst", "src", Some("100"), 42)
            .await;

        let track_cnt: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_score_tracking")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(track_cnt, 0, "kein Partner-Ziel -> kein Score-Tracking");
    }

    #[tokio::test]
    async fn target_silent_raid_aktiver_partner_vs_extern() {
        let pool = setup("t6e_silent_raid").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, silent_raid) \
             VALUES ('300','silentpartner','active',1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, silent_raid) \
             VALUES ('301','loudpartner','active',0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        let suppression = Arc::new(Mutex::new(tb_raid::ManualRaidSuppression::new()));
        let sink = RaidArrivalSinkImpl::new(
            pool.clone(),
            pending_store,
            suppression,
            "deadlock",
            None,
            None,
        );
        // aktiver Partner mit silent_raid=1 → true; mit 0 → false; externer
        // Kanal ohne (aktiven) Partner-Eintrag → false (keine Zeile).
        assert!(sink.target_silent_raid("silentpartner").await);
        assert!(!sink.target_silent_raid("loudpartner").await);
        assert!(!sink.target_silent_raid("externer_kanal").await);
    }
}
