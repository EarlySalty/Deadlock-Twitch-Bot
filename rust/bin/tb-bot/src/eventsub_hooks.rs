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
use tb_monitoring::{EventSubHooks, LiveStateStore, SubscriptionManager};
use tb_raid::{
    classify_partner_raid_arrival, PendingRaidStore, RaidArrivalInput, RaidArrivalRuntime,
    RaidBlacklistStore, RaidSignalCorrelationService, RaidSignalOutcome, TokenProvider,
};
use tb_transport_twitch::HelixClient;

use crate::auto_raid::OfflineRaidHandler;
use crate::partner_lookup::{
    is_target_partner, known_source, resolve_active_partner_id_by_login, PrefetchedLookups,
};
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

// ─── Hook-Bündel ─────────────────────────────────────────────────────────────

/// Vollständige EventSub-Hooks (Monitoring → Raid).
pub struct RaidEventSubHooks {
    pub manager: Arc<SubscriptionManager>,
    pub score_resolver: ScoreRefreshResolver,
    pub live_state: LiveStateStore,
    pub offline: Arc<OfflineRaidHandler>,
    pub arrival: RaidArrivalCoordinator,
    pub guard: BlacklistRaidGuard,
}

#[async_trait::async_trait]
impl EventSubHooks for RaidEventSubHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
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

    async fn on_stream_offline(&self, twitch_user_id: &str, login: Option<&str>) {
        self.offline
            .handle_streamer_offline(twitch_user_id, login)
            .await;
    }

    async fn on_channel_raid(&self, event: &Value, _message_id: Option<&str>) {
        self.arrival.handle_channel_raid(event).await;
    }

    async fn on_channel_moderate(&self, broadcaster_id: &str, login: &str, event: &Value) {
        self.guard.handle(broadcaster_id, login, event).await;
    }
}
