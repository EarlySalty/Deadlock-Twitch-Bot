//! Echte EventSub-Hook-Implementierung: verdrahtet Monitoring-Events mit dem
//! Raid-Subsystem. Ersetzt den Interim-`SubscriptionEventSubHooks` (nur
//! Go-Live) — alle vier Raid-Kopplungen aus `04-cutover-plan.md` sind hier echt:
//!
//! - `on_stream_went_live`  → stream.offline-Subscription (wie bisher)
//! - `on_score_refresh`     → Partner-Score-Refresh (ScoreRefreshResolver)
//! - `on_stream_offline`    → Auto-Raid ([`OfflineRaidHandler`])
//! - `on_channel_raid`      → Arrival-Korrelation ([`RaidArrivalCoordinator`])
//! - `on_channel_moderate`  → Blacklist-Raid-Guard ([`BlacklistRaidGuard`])
//!
//! Abweichung von Python: Score-Refreshes laufen inline statt als
//! debounced Background-Task — ein Einzel-Partner-Refresh ist nur eine
//! Handvoll DB-Reads, und der Dispatcher verarbeitet Events sequenziell
//! pro message_id-Guard.

use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use tb_monitoring::{EventSubHooks, LiveStateStore, ModeratorProvisioner, SubscriptionManager};
use tb_raid::pending_raids::normalize_broadcaster_login;
use tb_raid::signal_correlation::{ChatNotificationInput, ChatUnraidInput};
use tb_raid::{
    classify_partner_raid_arrival, PendingRaidStore, RaidArrivalInput, RaidArrivalRuntime,
    RaidBlacklistStore, RaidSignalCorrelationService, RaidSignalOutcome, TokenProvider,
};
use tb_transport_twitch::{AddModeratorOutcome, HelixClient};

use crate::auto_raid::OfflineRaidHandler;
use crate::offline_side_effects::OfflineSideEffects;
use crate::partner_lookup::{
    is_target_partner, known_source, resolve_active_partner_id_by_login, PrefetchedLookups,
};
use crate::reauth_reminder::ReauthReminder;
use crate::score_refresh::ScoreRefreshResolver;

fn event_str<'a>(event: &'a Value, key: &str) -> &'a str {
    event.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

// ─── channel.raid → Arrival-Korrelation ─────────────────────────────────────

/// Orchestriert ein `channel.raid`-Event: Pending-Lookup → Plan
/// (Signal-Korrelation) → Plan-Ausführung gegen den Sink. Port des
/// channel.raid-Pfads aus `raid_arrival_runtime.py` (Z. 420–490).
///
/// Abweichung von Python: die Unabhängig-Erkennung ist hier eine reine
/// Klassifikation — die Schreib-Effekte (Arrival-Zeile, Suppression-Mark)
/// laufen ausschließlich über die Plan-Actions. Python schrieb beides
/// doppelt (Pre-Check UND Action führten `process_independent_…` aus).
pub struct RaidArrivalCoordinator {
    pool: PgPool,
    pending: Arc<Mutex<PendingRaidStore>>,
    runtime: RaidArrivalRuntime,
}

impl RaidArrivalCoordinator {
    pub fn new(
        pool: PgPool,
        pending: Arc<Mutex<PendingRaidStore>>,
        runtime: RaidArrivalRuntime,
    ) -> Self {
        Self {
            pool,
            pending,
            runtime,
        }
    }

    pub async fn handle_channel_raid(&self, event: &Value) {
        let from_login = event_str(event, "from_broadcaster_user_login").to_lowercase();
        let from_id = Some(event_str(event, "from_broadcaster_user_id"))
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let to_id = event_str(event, "to_broadcaster_user_id").to_string();
        let to_login = event_str(event, "to_broadcaster_user_login").to_lowercase();
        let viewer_count = event.get("viewers").and_then(Value::as_i64).unwrap_or(0) as i32;

        if from_login.is_empty() {
            tracing::warn!("channel.raid-Event ohne from_broadcaster_user_login");
            return;
        }
        if to_id.is_empty() {
            tracing::warn!(from = %from_login, "channel.raid-Event ohne to_broadcaster_user_id");
            return;
        }
        tracing::info!(from = %from_login, to = %to_login, viewer_count, "EventSub: channel.raid");

        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&to_id, Some(&from_login))
            .cloned();

        // Unabhängig-Erkennung nur ohne Pending (Python Z. 445–457):
        // Ziel-Partner-Status + Quell-Auflösung vorab laden, dann pure
        // Klassifikation — Some(_) = manueller/externer Raid auf einen Partner.
        let independent_manual_detected = if pending.is_none() {
            let lookups = PrefetchedLookups {
                target_is_partner: is_target_partner(&self.pool, &to_id, &to_login).await,
                known_source: known_source(&self.pool, from_id.as_deref(), &from_login).await,
            };
            classify_partner_raid_arrival(
                Some(&from_login),
                from_id.as_deref(),
                Some(&to_id),
                Some(&to_login),
                &lookups,
                &lookups,
            )
            .classification
            .is_some()
        } else {
            false
        };

        // Manual-Raid-Key: from-ID, sonst Auflösung über den Partner-Login.
        let manual_raid_source_key = match &from_id {
            Some(id) => Some(id.clone()),
            None => resolve_active_partner_id_by_login(&self.pool, &from_login).await,
        };

        let plan = RaidSignalCorrelationService.plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: to_id,
            to_broadcaster_login: to_login,
            from_broadcaster_login: from_login.clone(),
            from_broadcaster_id: from_id,
            viewer_count,
            pending_raid: pending,
            recent_arrival_present: false,
            independent_manual_detected,
            manual_raid_source_key,
        });

        let outcome = plan.outcome.clone();
        self.runtime.execute_plan(&plan).await;

        if outcome == RaidSignalOutcome::PendingMismatch {
            tracing::warn!(
                expected = plan
                    .pending_raid
                    .as_ref()
                    .map(|p| p.from_broadcaster_login.as_str())
                    .unwrap_or("?"),
                actual = %from_login,
                "Raid-Arrival-Mismatch: Quelle passt nicht zum Pending"
            );
        }
    }

    /// Ziel-seitige `channel.chat.notification`-Raidmeldung (notice_type=raid,
    /// B7-01). Sekundärpfad zum `channel.raid`-Webhook: `broadcaster_*` ist das
    /// Ziel, `raid.user_*` die Quelle. Pending nachschlagen → Plan über
    /// `plan_chat_notification` → ausführen (Orphan/Mismatch/Match werden in den
    /// Plan-Actions abgehandelt). Port von `on_chat_raid_notification`
    /// (raid_arrival_runtime.py Z. 534–614).
    pub async fn handle_chat_raid_notification(&self, event: &Value, message_id: Option<&str>) {
        let to_id = event_str(event, "broadcaster_user_id").to_string();
        let to_login = event_str(event, "broadcaster_user_login").to_lowercase();
        // Quelle (Raider) sitzt im `raid`-Sub-Objekt (Twitch chat.notification).
        let raid = event.get("raid");
        let from_login = raid
            .map(|r| event_str(r, "user_login"))
            .unwrap_or("")
            .to_lowercase();
        let from_id = raid
            .map(|r| event_str(r, "user_id"))
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let viewer_count = raid
            .and_then(|r| r.get("viewer_count"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32;

        if to_id.is_empty() || from_login.is_empty() {
            tracing::debug!(
                to = %to_login,
                from = %from_login,
                "chat.notification-Raid ohne Ziel-ID/Quell-Login ignoriert"
            );
            return;
        }
        tracing::info!(from = %from_login, to = %to_login, viewer_count, "EventSub: chat.notification raid");

        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&to_id, Some(&from_login))
            .cloned();

        let plan = RaidSignalCorrelationService.plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: to_id,
            to_broadcaster_login: to_login,
            from_broadcaster_login: from_login.clone(),
            from_broadcaster_id: from_id,
            viewer_count,
            message_id: message_id.map(str::to_string),
            event_timestamp: None,
            pending_raid: pending,
            recent_arrival_present: false,
        });

        let outcome = plan.outcome.clone();
        self.runtime.execute_plan(&plan).await;

        if outcome == RaidSignalOutcome::PendingMismatch {
            tracing::warn!(
                expected = plan
                    .pending_raid
                    .as_ref()
                    .map(|p| p.from_broadcaster_login.as_str())
                    .unwrap_or("?"),
                actual = %from_login,
                "chat.notification-Raid-Mismatch: Quelle passt nicht zum Pending"
            );
        }
    }

    /// `channel.chat.notification`-Unraidmeldung (notice_type=unraid, B7-02/03).
    /// Verzweigt wie Pythons Chat-Bot-Routing (bot.py Z. 1883–1926):
    ///
    /// - **Source-Self-Unraid** (Unraider == Kanal-Inhaber): der Quell-Streamer
    ///   bricht seine eigene Raid-Sequenz ab → ausstehende Auto-Raids dieses
    ///   Quell-Streamers stornieren (B7-03, [`Self::cancel_pending_for_source`]).
    /// - **Ziel-seitiges Unraid** (sonst): diagnostisch über `plan_chat_unraid`
    ///   am vorhandenen Pending vermerken (kein Mismatch-/Confirm-Pfad).
    pub async fn handle_chat_unraid_notification(&self, event: &Value, message_id: Option<&str>) {
        let broadcaster_id = event_str(event, "broadcaster_user_id").to_string();
        let broadcaster_login = event_str(event, "broadcaster_user_login").to_lowercase();
        // Unraider = chatter (chat.notification). Fällt der auf den Kanal-Inhaber,
        // ist es ein Source-Self-Unraid.
        let from_login = event_str(event, "chatter_user_login").to_lowercase();
        let from_id = Some(event_str(event, "chatter_user_id"))
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if broadcaster_id.is_empty() || broadcaster_login.is_empty() {
            return;
        }

        // Source-Self-Unraid: Unraider-Login == Kanal-Inhaber UND (ID unbekannt
        // oder gleich). Python bot.py Z. 1898–1918.
        let is_source_self = !from_login.is_empty()
            && from_login == broadcaster_login
            && from_id
                .as_deref()
                .map(|id| id == broadcaster_id)
                .unwrap_or(true);
        if is_source_self {
            let canceled = self.cancel_pending_for_source(&broadcaster_login, message_id);
            if canceled == 0 {
                tracing::info!(
                    source = %broadcaster_login,
                    message_id = message_id.unwrap_or("n/a"),
                    "Source-Self-Unraid ohne ausstehenden Auto-Raid"
                );
            }
            return;
        }

        // Ziel-seitiges Unraid: nur diagnostisch am Pending vermerken.
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&broadcaster_id, Some(&from_login))
            .cloned();
        let plan = RaidSignalCorrelationService.plan_chat_unraid(ChatUnraidInput {
            to_broadcaster_id: broadcaster_id,
            to_broadcaster_login: broadcaster_login.clone(),
            from_broadcaster_login: from_login.clone(),
            from_broadcaster_id: from_id,
            pending_raid: pending,
            recent_arrival_present: false,
            event_timestamp: None,
        });
        self.runtime.execute_plan(&plan).await;
        tracing::info!(
            from = %from_login,
            to = %broadcaster_login,
            "chat.notification unraid ohne bestätigten Raid beobachtet"
        );
    }

    /// Storniert alle ausstehenden Auto-Raids eines Quell-Streamers (B7-03,
    /// Source-Self-Unraid) über die B7-`iter()`-basierte Store-API
    /// ([`PendingRaidStore::cancel_from_source`]) und loggt die Treffer. Port von
    /// `cancel_pending_raids_for_source_unraid` (raid_tracking_runtime.py Z. 160–220).
    fn cancel_pending_for_source(
        &self,
        from_broadcaster_login: &str,
        message_id: Option<&str>,
    ) -> usize {
        let normalized_from = normalize_broadcaster_login(from_broadcaster_login);
        if normalized_from.is_empty() {
            return 0;
        }
        let canceled = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .cancel_from_source(&normalized_from);
        for raid in &canceled {
            tracing::info!(
                source = %normalized_from,
                target_id = %raid.to_broadcaster_id,
                message_id = message_id.unwrap_or("n/a"),
                "Ausstehender Auto-Raid durch Source-Unraid storniert"
            );
        }
        canceled.len()
    }
}

// ─── channel.moderate → Blacklist-Raid-Guard ────────────────────────────────

/// Bricht manuell gestartete Raids auf Blacklist-Ziele ab. Port von
/// `eventsub_mixin.py` `_guard_blacklisted_outgoing_raid`.
///
/// Der Streamer-Whisper folgt mit dem Chat-Cutover (Schritt 5): der
/// Bot-Token wird vom Python-Chat-Prozess verwaltet (Auto-Refresh mit
/// Rotation) — ein zweiter Refresher in Rust würde die Refresh-Token-Kette
/// beider Prozesse gegenseitig invalidieren. Bis dahin: Cancel + Warn-Log.
pub struct BlacklistRaidGuard {
    blacklist: RaidBlacklistStore,
    token_provider: Arc<TokenProvider>,
    helix: HelixClient,
}

impl BlacklistRaidGuard {
    pub fn new(
        blacklist: RaidBlacklistStore,
        token_provider: Arc<TokenProvider>,
        helix: HelixClient,
    ) -> Self {
        Self {
            blacklist,
            token_provider,
            helix,
        }
    }

    pub async fn handle(&self, broadcaster_id: &str, login: &str, event: &Value) {
        if !event_str(event, "action").eq_ignore_ascii_case("raid") {
            return;
        }
        let Some(raid_info) = event.get("raid").filter(|v| v.is_object()) else {
            return;
        };
        let target_login = event_str(raid_info, "user_login").to_lowercase();
        let target_id = event_str(raid_info, "user_id").to_string();
        if target_login.is_empty() && target_id.is_empty() {
            return;
        }

        let blacklisted = match self
            .blacklist
            .is_blacklisted(Some(&target_id), &target_login)
            .await
        {
            Ok(hit) => hit,
            Err(error) => {
                tracing::error!(%error, target = %target_login, "Blacklist-Prüfung fehlgeschlagen");
                return;
            }
        };
        if !blacklisted {
            return;
        }

        tracing::warn!(
            streamer = login,
            target = %target_login,
            "Manueller Raid auf Blacklist-Ziel erkannt — versuche Abbruch"
        );

        let cancelled = self.cancel_raid(broadcaster_id).await;
        if cancelled {
            tracing::warn!(
                streamer = login,
                target = %target_login,
                "Raid auf Blacklist-Ziel abgebrochen (Streamer-Hinweis folgt mit Chat-Cutover)"
            );
        } else {
            tracing::warn!(
                streamer = login,
                target = %target_login,
                "Raid-Abbruch nicht möglich — Raid auf Blacklist-Ziel lief durch"
            );
        }
    }

    async fn cancel_raid(&self, broadcaster_id: &str) -> bool {
        let token = match self
            .token_provider
            .get_valid_token(broadcaster_id, Utc::now())
            .await
        {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(broadcaster_id, "Kein gültiger Token für Raid-Abbruch");
                return false;
            }
            Err(error) => {
                tracing::error!(%error, broadcaster_id, "Token-Lookup für Raid-Abbruch fehlgeschlagen");
                return false;
            }
        };
        match self.helix.cancel_raid(broadcaster_id, &token).await {
            Ok(Ok(())) => true,
            Ok(Err(api_error)) => {
                tracing::warn!(broadcaster_id, %api_error, "Cancel-Raid abgelehnt");
                false
            }
            Err(error) => {
                tracing::warn!(broadcaster_id, %error, "Cancel-Raid-Request fehlgeschlagen");
                false
            }
        }
    }
}

// ─── 403-Selbstheilung: Bot als Moderator nachsetzen (P1.2) ──────────────────

/// Setzt den Bot zur Laufzeit (wieder) als Moderator eines Kanals ein, wenn ein
/// Chat-Join/Sub-Create mit 403 fehlschlägt. Port von Pythons
/// `_ensure_bot_is_mod` (chat/connection.py:961): Streamer-Token auflösen
/// (`get_tokens_for_user` → [`TokenProvider::get_valid_token_unrestricted`]),
/// dann `POST /moderation/moderators` mit Bot-User-ID. Ohne gültigen
/// Streamer-Token oder Bot-ID heilt der Join nicht (`false`).
pub struct HelixModeratorProvisioner {
    token_provider: Arc<TokenProvider>,
    helix: HelixClient,
    bot_user_id: String,
}

impl HelixModeratorProvisioner {
    pub fn new(
        token_provider: Arc<TokenProvider>,
        helix: HelixClient,
        bot_user_id: String,
    ) -> Self {
        Self {
            token_provider,
            helix,
            bot_user_id,
        }
    }
}

#[async_trait::async_trait]
impl ModeratorProvisioner for HelixModeratorProvisioner {
    async fn ensure_bot_is_mod(&self, broadcaster_id: &str, login: &str) -> bool {
        // Python connection.py:975-978: ohne Bot-ID kein Remod.
        if self.bot_user_id.trim().is_empty() {
            tracing::debug!(channel = login, "ensure_bot_is_mod: keine Bot-ID verfügbar");
            return false;
        }
        // Streamer-Token auflösen (connection.py:986 `get_tokens_for_user`) —
        // unrestricted, da der Remod auch bei deaktivierten Raids greifen muss.
        let token = match self
            .token_provider
            .get_valid_token_unrestricted(broadcaster_id, Utc::now())
            .await
        {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(
                    channel = login,
                    "ensure_bot_is_mod: keine gültige Streamer-Autorisierung verfügbar"
                );
                return false;
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    channel = login,
                    "ensure_bot_is_mod: Streamer-Token-Lookup fehlgeschlagen"
                );
                return false;
            }
        };
        match self
            .helix
            .add_channel_moderator(broadcaster_id, &self.bot_user_id, &token)
            .await
        {
            // 200/204 (Added) und 422/"already a mod" (AlreadyModerator) gelten
            // beide als Erfolg (connection.py:1013-1027).
            Ok(AddModeratorOutcome::Added) => {
                tracing::info!(
                    channel = login,
                    bot_user_id = %self.bot_user_id,
                    "ensure_bot_is_mod: Bot wieder als Moderator gesetzt"
                );
                true
            }
            Ok(AddModeratorOutcome::AlreadyModerator) => {
                tracing::info!(channel = login, "ensure_bot_is_mod: Bot ist bereits Moderator");
                true
            }
            Ok(AddModeratorOutcome::Failed { status, body }) => {
                tracing::warn!(
                    channel = login,
                    status,
                    body = %body,
                    "ensure_bot_is_mod: Remod fehlgeschlagen"
                );
                false
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    channel = login,
                    "ensure_bot_is_mod: Remod-Request fehlgeschlagen"
                );
                false
            }
        }
    }
}

// ─── Session-Finalize → Raid-Score-Tracking-Resolve (B7) ─────────────────────

/// Verdrahtet den [`tb_monitoring::RaidTrackingResolver`]-Port gegen den
/// tb-raid-Score-Tracking-Store. Beim Session-Finalize werden die offenen
/// Deadlock-Raid-Zeilen der Session aufgelöst (sonst bleiben sie dauerhaft
/// `resolved_at IS NULL`). Reicht das Ziel-Spiel durch (Python `_target_game_lower`).
pub struct RaidTrackingResolverAdapter {
    store: tb_raid::ScoreTrackingStore,
    target_game_lower: String,
}

impl RaidTrackingResolverAdapter {
    pub fn new(pool: PgPool, target_game: &str) -> Self {
        Self {
            store: tb_raid::ScoreTrackingStore::new(pool),
            target_game_lower: target_game.trim().to_lowercase(),
        }
    }
}

#[async_trait::async_trait]
impl tb_monitoring::RaidTrackingResolver for RaidTrackingResolverAdapter {
    async fn resolve_for_session(
        &self,
        twitch_user_id: Option<&str>,
        streamer_login: &str,
        session_id: i64,
        session_ended_at: chrono::DateTime<Utc>,
    ) -> i64 {
        self.store
            .resolve_for_session(
                twitch_user_id,
                streamer_login,
                Some(session_id),
                Some(session_ended_at),
                &self.target_game_lower,
            )
            .await
    }
}

// ─── Hook-Bündel ─────────────────────────────────────────────────────────────

/// Vollständige EventSub-Hooks (Monitoring → Raid).
pub struct RaidEventSubHooks {
    pub manager: Arc<SubscriptionManager>,
    pub score_resolver: ScoreRefreshResolver,
    pub live_state: LiveStateStore,
    pub offline: Arc<OfflineRaidHandler>,
    pub side_effects: OfflineSideEffects,
    pub arrival: RaidArrivalCoordinator,
    pub guard: BlacklistRaidGuard,
    /// Go-Live-ReAuth-Reminder (B11); `None`, wenn kein nativer Chat-Send-Pfad
    /// gebootet ist (TB_CHAT_ENABLED≠1).
    pub reauth_reminder: Option<Arc<ReauthReminder>>,
    /// Pool für die Post-Stream-Analyse (B11), die in `on_stream_offline`
    /// fire-and-forget getriggert wird.
    pub pool: PgPool,
}

#[async_trait::async_trait]
impl EventSubHooks for RaidEventSubHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
        // Go-Live-Followup (B11): Partner mit needs_reauth einmalig im Chat
        // an die fällige Re-Authentifizierung erinnern. Best-effort, eigener
        // Dedupe-Guard — der stream.offline-Sub-Pfad bleibt davon unberührt.
        if let Some(reminder) = &self.reauth_reminder {
            reminder.maybe_remind(twitch_user_id, login).await;
        }
    }

    async fn on_score_refresh(
        &self,
        twitch_user_id: &str,
        login: Option<&str>,
        trigger: &'static str,
    ) {
        let user_id = twitch_user_id.trim();
        if user_id.is_empty() {
            return;
        }
        // Login auflösen, falls das Event keinen mitliefert (Sessions-Lookup
        // im Resolver läuft über den Login).
        let login = match login.map(str::trim).filter(|l| !l.is_empty()) {
            Some(l) => l.to_lowercase(),
            None => match self.live_state.login_for_user_id(user_id).await {
                Ok(Some(l)) => l,
                _ => {
                    tracing::debug!(user_id, trigger, "Score-Refresh ohne auflösbaren Login");
                    return;
                }
            },
        };
        match self
            .score_resolver
            .refresh_scores(&[(user_id.to_string(), login.clone())], Utc::now())
            .await
        {
            Ok(written) => {
                tracing::debug!(user_id, %login, trigger, written, "Partner-Score refresht");
            }
            Err(error) => {
                tracing::error!(%error, user_id, %login, trigger, "Score-Refresh fehlgeschlagen");
            }
        }
    }

    async fn on_stream_offline_engagement(&self, _twitch_user_id: &str, login: Option<&str>) {
        // Engagement-Auto-Off VOR dem Throttle (Python `eventsub_mixin.py`:1861):
        // feuert auch bei einem als Duplikat gedrosselten Offline, damit der
        // Engagement-Layer ans Stream-Leben gekoppelt bleibt.
        if let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) {
            self.side_effects.run_engagement_auto_off(login).await;
        }
    }

    async fn on_stream_offline_global_ban(&self, twitch_user_id: &str, login: Option<&str>) {
        // Global-Ban-Sweep NACH bestandenem Throttle, VOR State-Finalize
        // (Python `eventsub_mixin.py`:1908).
        if let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) {
            self.side_effects
                .run_global_ban_sweep(twitch_user_id, login)
                .await;
        }
    }

    async fn on_stream_offline(&self, twitch_user_id: &str, login: Option<&str>) {
        // Engagement-Off und Global-Ban-Sweep liefen bereits früher (siehe
        // on_stream_offline_engagement / on_stream_offline_global_ban). Hier nur
        // noch der Auto-Raid + Post-Stream-Analyse nach State-Finalize.
        self.offline
            .handle_streamer_offline(twitch_user_id, login)
            .await;

        // Post-Stream-Analyse (B11): fire-and-forget wie Python `create_task`,
        // damit der KI-schwere A/B-Trigger den sequenziellen EventSub-Dispatcher
        // nicht blockiert. Der Login (lowercased) genügt — der Trigger sucht die
        // letzte abgeschlossene Session selbst (im stream_offline_state-Effekt
        // wurde sie bereits finalisiert).
        if let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) {
            let pool = self.pool.clone();
            let streamer = login.to_lowercase();
            tokio::spawn(async move {
                tb_analytics::post_stream::trigger_post_stream_analysis(&pool, &streamer, None).await;
            });
        }
    }

    async fn on_channel_raid(&self, event: &Value, _message_id: Option<&str>) {
        self.arrival.handle_channel_raid(event).await;
    }

    async fn on_channel_moderate(&self, broadcaster_id: &str, login: &str, event: &Value) {
        self.guard.handle(broadcaster_id, login, event).await;
    }

    async fn on_chat_raid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.arrival
            .handle_chat_raid_notification(event, message_id)
            .await;
    }

    async fn on_chat_unraid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.arrival
            .handle_chat_unraid_notification(event, message_id)
            .await;
    }
}
