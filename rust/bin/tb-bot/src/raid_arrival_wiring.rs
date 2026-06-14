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
    classify_partner_raid_arrival, decide_blacklist_action, serialize_confirmation_signals,
    ArrivalConfirmationService, ArrivalSignalContext, ArrivalTrackingStore, BlacklistScheduleAction,
    ConfirmedExternalRecruitmentRaid, ExternalRecruitmentStore, ManualRaidSuppression, PendingRaid,
    PendingRaidStore, RaidArrivalSink, RaidBlacklistStore, RaidHistoryStore, RecordArrivalInput,
    ScoreTrackingStore, EXTERNAL_RECRUITMENT_BLACKLIST_GRACE_SECONDS,
    EXTERNAL_RECRUITMENT_RAID_LIMIT,
};

use crate::confirm_resolver::{ConfirmContext, ConfirmResolver};
use crate::partner_lookup::{is_target_partner, known_source, PrefetchedLookups};
use crate::score_refresh::ScoreRefreshResolver;

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
    fn pop_orphan(&self, to_broadcaster_id: &str, from_broadcaster_login: &str) {
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

            let drop_entry = processed
                || orphan.observed_at.elapsed().as_secs() >= ORPHAN_RETENTION_SECS;
            if drop_entry {
                let key =
                    orphan_key(&orphan.to_broadcaster_id, &orphan.from_broadcaster_login);
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
                if let Some(from_id) =
                    from_broadcaster_id.map(str::trim).filter(|s| !s.is_empty())
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
        self.pop_orphan(to_broadcaster_id, from_broadcaster_login);

        // 1. Pending entfernen (pop) — wie Python `pop_pending_raid`.
        let pending = match self.pending.lock() {
            Ok(mut store) => store.pop(to_broadcaster_id, Some(from_broadcaster_login)),
            Err(_) => None,
        };
        let Some(pending) = pending else { return };

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
        let Some(decision) =
            svc.confirm_pending_raid_arrival(pending, &ctx, signal_type, None, None, None)
        else {
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
                    viewer_count,
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
                viewer_count,
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
                    viewer_count,
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
            .find_recent_arrival(to_broadcaster_id, from_broadcaster_login, RECENT_ARRIVAL_TTL_SECS)
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
            existing_signals.split(',').chain(std::iter::once(signal_type)),
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
        _message_id: Option<&str>,
        _event_timestamp: Option<&str>,
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
            "CREATE TABLE twitch_partners (twitch_user_id TEXT, twitch_login TEXT, status TEXT)",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, last_started_at TEXT, last_game TEXT, active_session_id BIGINT)",
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
        let sink =
            RaidArrivalSinkImpl::new(pool.clone(), pending_store.clone(), suppression, "deadlock");
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
    async fn confirm_nicht_partner_ziel_kein_score_tracking() {
        let pool = setup("t6e_arrival_sink_nonpartner").await;
        // Ziel 200 NICHT Partner; kein twitch_partners-Eintrag.
        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        pending_store
            .lock()
            .unwrap()
            .store(PendingRaid::new("src", "200"));
        let suppression = Arc::new(Mutex::new(tb_raid::ManualRaidSuppression::new()));
        let sink =
            RaidArrivalSinkImpl::new(pool.clone(), pending_store.clone(), suppression, "deadlock");
        sink.confirm_pending_raid("channel.raid", "200", "dst", "src", Some("100"), 42)
            .await;

        let track_cnt: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_score_tracking")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(track_cnt, 0, "kein Partner-Ziel -> kein Score-Tracking");
    }
}
