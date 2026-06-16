//! EventSub-Subscription-Lifecycle (Webhook-Transport, ADR 0004).
//!
//! Rust verwaltet die **Core-Subscriptions** (stream.online/offline,
//! channel.update) mit App-Token selbst — das ist alles, was das Monitoring
//! braucht. Die Broadcaster-/Moderator-Telemetrie-Subs (Bits/Subs/Bans/…)
//! brauchen User-Tokens aus dem verschlüsselten `twitch_raid_auth`-Store
//! (Raid-Subsystem, Phase 6) und bleiben bis dahin bei Python; bei Twitch
//! bestehende Subscriptions liefern unabhängig vom Ersteller weiter an
//! dieselbe Callback-URL (Cutover-Kopplung, siehe Plan-Doc).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::PgPool;

use crate::inbox_runtime::{epoch_clock, ClockFn};
use crate::poller::source::SourceError;

// ── Capacity-Snapshot-Konfiguration (Env, Python-Clamps) ───────────────────────

/// Default-Sampling-Intervall der Capacity-Zeitreihe (Sekunden).
/// Python `_eventsub_capacity_sample_interval_seconds`: Default 300, Clamp 30–3600.
const CAPACITY_SAMPLE_DEFAULT_SECONDS: u64 = 300;
const CAPACITY_SAMPLE_MIN_SECONDS: u64 = 30;
const CAPACITY_SAMPLE_MAX_SECONDS: u64 = 3600;

/// Default-Retention der Capacity-Zeitreihe (Tage).
/// Python `_eventsub_capacity_retention_days`: Default 45, Clamp 7–365.
const CAPACITY_RETENTION_DEFAULT_DAYS: i64 = 45;
const CAPACITY_RETENTION_MIN_DAYS: i64 = 7;
const CAPACITY_RETENTION_MAX_DAYS: i64 = 365;

/// Retention-Cleanup läuft höchstens stündlich (Python: `>= 3600`).
const CAPACITY_CLEANUP_INTERVAL_SECONDS: f64 = 3600.0;

/// Sample-Intervall aus `TWITCH_EVENTSUB_CAPACITY_SAMPLE_SECONDS`, geclamped.
fn capacity_sample_interval_seconds() -> u64 {
    parse_env_clamped(
        "TWITCH_EVENTSUB_CAPACITY_SAMPLE_SECONDS",
        CAPACITY_SAMPLE_DEFAULT_SECONDS,
        CAPACITY_SAMPLE_MIN_SECONDS,
        CAPACITY_SAMPLE_MAX_SECONDS,
    )
}

/// Retention-Fenster aus `TWITCH_EVENTSUB_CAPACITY_RETENTION_DAYS`, geclamped.
fn capacity_retention_days() -> i64 {
    parse_env_clamped(
        "TWITCH_EVENTSUB_CAPACITY_RETENTION_DAYS",
        CAPACITY_RETENTION_DEFAULT_DAYS,
        CAPACITY_RETENTION_MIN_DAYS,
        CAPACITY_RETENTION_MAX_DAYS,
    )
}

fn parse_env_clamped<T>(key: &str, default: T, min: T, max: T) -> T
where
    T: std::str::FromStr + Ord,
{
    match std::env::var(key) {
        Ok(raw) => match raw.trim().parse::<T>() {
            Ok(value) => value.clamp(min, max),
            Err(_) => default,
        },
        Err(_) => default,
    }
}

/// Core-Subscriptions des Monitorings: (Typ, Version).
pub const CORE_SUBSCRIPTIONS: [(&str, &str); 3] = [
    ("stream.online", "1"),
    ("stream.offline", "1"),
    ("channel.update", "2"),
];

/// Broadcaster-Telemetrie-Subscriptions: (Typ, Version, benötigter Scope).
/// Brauchen den **Broadcaster-User-Token** (verschlüsselter `twitch_raid_auth`-
/// Store), nicht den App-Token — Port von `eventsub_mixin.py` `broadcaster_subs`
/// (Z. 1623). Werden nur angelegt, wenn der Token den jeweiligen Scope trägt.
/// Die zugehörigen Events verarbeitet der Dispatcher bereits (`store_telemetry`).
///
/// `channel.moderate` wird separat im Chat-Sub-Reconcile (`chat_wiring.rs`) mit
/// dem **Rust-Bot-Token** angelegt (`ensure_moderator_subscription`) — es speist
/// den `BlacklistRaidGuard`. Das ist möglich, seit der Python-Chat-Prozess
/// abgeschaltet ist und Rust den Bot-Token allein refresht (kein Dual-Refresh-
/// Race mehr). Die Daten-Moderator-Subs (channel.follow/ban/unban/shoutout) legt
/// [`SubscriptionManager::ensure_moderator_telemetry_subscriptions`] an (B5-02).
pub const BROADCASTER_TELEMETRY_SUBSCRIPTIONS: [(&str, &str, &str); 12] = [
    ("channel.cheer", "1", "bits:read"),
    ("channel.bits.use", "1", "bits:read"),
    ("channel.hype_train.begin", "1", "channel:read:hype_train"),
    ("channel.hype_train.progress", "1", "channel:read:hype_train"),
    ("channel.hype_train.end", "1", "channel:read:hype_train"),
    ("channel.subscribe", "1", "channel:read:subscriptions"),
    ("channel.subscription.gift", "1", "channel:read:subscriptions"),
    ("channel.subscription.message", "1", "channel:read:subscriptions"),
    ("channel.subscription.end", "1", "channel:read:subscriptions"),
    ("channel.ad_break.begin", "1", "channel:read:ads"),
    (
        "channel.channel_points_automatic_reward_redemption.add",
        "2",
        "channel:read:redemptions",
    ),
    (
        "channel.channel_points_custom_reward_redemption.add",
        "1",
        "channel:read:redemptions",
    ),
];

/// Moderator-Daten-Telemetrie-Subscriptions: (Typ, Version, benötigter Scope).
/// Brauchen einen **Moderator-User-Token** (bevorzugt der Rust-Bot-Token) und
/// tragen in der Condition `moderator_user_id` — Port von `eventsub_mixin.py`
/// `moderator_subs` (Z. 1704, ohne `channel.moderate`, das der Chat-Reconcile
/// als Guard-Quelle separat anlegt). `channel.follow` braucht Version 2. Die
/// zugehörigen Events verarbeitet der Dispatcher bereits (Follower-Funnel,
/// Ban-Analytics, Shoutouts).
pub const MODERATOR_TELEMETRY_SUBSCRIPTIONS: [(&str, &str, &str); 5] = [
    ("channel.ban", "1", "moderator:manage:banned_users"),
    ("channel.unban", "1", "moderator:manage:banned_users"),
    ("channel.shoutout.create", "1", "moderator:manage:shoutouts"),
    ("channel.shoutout.receive", "1", "moderator:manage:shoutouts"),
    ("channel.follow", "2", "moderator:read:followers"),
];

/// Eine bei Twitch registrierte Subscription (Transport-neutrale Sicht).
#[derive(Debug, Clone)]
pub struct RemoteSubscription {
    pub id: String,
    pub sub_type: String,
    pub status: String,
    pub callback: Option<String>,
    pub broadcaster_user_id: Option<String>,
}

/// Port zum Subscription-Backend (Helix-Adapter lebt in `tb-bot`).
#[async_trait::async_trait]
pub trait SubscriptionTransport: Send + Sync {
    /// Anlegen; `true` = existierte bereits (409-as-success). `bearer_override`
    /// = Some(User-/Broadcaster-Token) für Telemetrie-Subs; `None` = App-Token.
    async fn create(
        &self,
        sub_type: &str,
        version: &str,
        condition: &Value,
        callback: &str,
        secret: &str,
        bearer_override: Option<&str>,
    ) -> Result<bool, SourceError>;
    async fn list(&self) -> Result<Vec<RemoteSubscription>, SourceError>;
    async fn delete(&self, id: &str) -> Result<(), SourceError>;
}

/// Webhook-Konfiguration (Callback-URL + Secret aus der Env).
#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    pub callback_url: String,
    pub secret: String,
}

/// Throttle-Zustand der periodischen Capacity-Zeitreihe (monotone Epoch-Sek.).
#[derive(Default)]
struct CapacityThrottle {
    /// Zeitpunkt des letzten geschriebenen Snapshots (`None` = noch keiner).
    last_snapshot: Option<f64>,
    /// Zeitpunkt des letzten Retention-Cleanups.
    last_cleanup: Option<f64>,
}

pub struct SubscriptionManager {
    transport: Arc<dyn SubscriptionTransport>,
    config: SubscriptionConfig,
    capacity: CapacitySnapshotStore,
    /// In-Memory-Tracking (Typ, broadcaster_user_id) — wie Pythons
    /// `_eventsub_has_sub`, beim Start via [`Self::rehydrate`] gefüllt.
    tracked: Mutex<HashSet<(String, String)>>,
    /// Permanent-Fehler (Typ, broadcaster_user_id): 403 = Bot gebannt oder
    /// Kanal sperrt externe Subs. Kein Retry bis Neustart.
    perm_failed: Mutex<HashSet<(String, String)>>,
    /// Drosselung der periodischen Capacity-Zeitreihe (B5-08).
    capacity_throttle: Mutex<CapacityThrottle>,
    /// Monotone Uhr (Epoch-Sek.) für die Throttle-Fenster — in Tests injizierbar.
    clock: ClockFn,
}

impl SubscriptionManager {
    pub fn new(
        transport: Arc<dyn SubscriptionTransport>,
        config: SubscriptionConfig,
        capacity: CapacitySnapshotStore,
    ) -> Self {
        Self {
            transport,
            config,
            capacity,
            tracked: Mutex::new(HashSet::new()),
            perm_failed: Mutex::new(HashSet::new()),
            capacity_throttle: Mutex::new(CapacityThrottle::default()),
            clock: Arc::new(epoch_clock),
        }
    }

    /// Ersetzt die Throttle-Uhr (Tests). Default ist die System-Epoch-Uhr.
    #[must_use]
    pub fn with_clock(mut self, clock: ClockFn) -> Self {
        self.clock = clock;
        self
    }

    fn is_tracked(&self, sub_type: &str, broadcaster_id: &str) -> bool {
        self.tracked
            .lock()
            .expect("tracked lock")
            .contains(&(sub_type.to_string(), broadcaster_id.to_string()))
    }

    fn track(&self, sub_type: &str, broadcaster_id: &str) {
        self.tracked
            .lock()
            .expect("tracked lock")
            .insert((sub_type.to_string(), broadcaster_id.to_string()));
    }

    /// Tracking aus dem Twitch-Bestand aufbauen (enabled + unsere Callback).
    pub async fn rehydrate(&self) {
        match self.transport.list().await {
            Ok(subs) => {
                let mut tracked = self.tracked.lock().expect("tracked lock");
                tracked.clear();
                for sub in subs {
                    if sub.status != "enabled" {
                        continue;
                    }
                    if sub.callback.as_deref() != Some(self.config.callback_url.as_str()) {
                        continue;
                    }
                    if let Some(bid) = &sub.broadcaster_user_id {
                        tracked.insert((sub.sub_type, bid.clone()));
                    }
                }
                tracing::info!(count = tracked.len(), "EventSub-Tracking rehydriert");
            }
            Err(error) => {
                tracing::warn!(%error, "EventSub-Tracking konnte nicht rehydriert werden");
            }
        }
    }

    /// stream.offline für einen Broadcaster sicherstellen
    /// (Python `_ensure_eventsub_offline_subscription`, Webhook-Pfad).
    pub async fn ensure_offline_subscription(&self, broadcaster_id: &str, login: &str) -> bool {
        self.ensure_subscription("stream.offline", "1", broadcaster_id, login)
            .await
    }

    /// `channel.raid`-Subscription für ein Raid-Ziel: Condition ist
    /// `to_broadcaster_user_id` (Events kommen beim ZIEL an, nicht der Quelle).
    /// Python `ensure_raid_arrival_subscription_ready` (best-effort).
    pub async fn ensure_raid_subscription(&self, to_broadcaster_id: &str, login: &str) -> bool {
        self.ensure_subscription_with_key(
            "channel.raid",
            "1",
            "to_broadcaster_user_id",
            to_broadcaster_id,
            login,
        )
        .await
    }

    /// Alle Core-Subscriptions für einen Broadcaster sicherstellen.
    pub async fn ensure_core_subscriptions(&self, broadcaster_id: &str, login: &str) {
        for (sub_type, version) in CORE_SUBSCRIPTIONS {
            self.ensure_subscription(sub_type, version, broadcaster_id, login)
                .await;
        }
    }

    async fn ensure_subscription(
        &self,
        sub_type: &str,
        version: &str,
        broadcaster_id: &str,
        login: &str,
    ) -> bool {
        self.ensure_subscription_with_key(
            sub_type,
            version,
            "broadcaster_user_id",
            broadcaster_id,
            login,
        )
        .await
    }

    /// Chat-Subscriptions für einen Partner-Kanal (Welle B):
    /// `channel.chat.message` + `channel.chat.notification`, Condition
    /// `{broadcaster_user_id, user_id: <bot>}`. Per Webhook + App-Token
    /// erlaubt, sobald der Bot-Account `user:bot` und der Broadcaster
    /// `channel:bot` autorisiert hat — exakt der Scope-Filter des
    /// Python-`join_partner_channels` (connection.py). „Join" = diese Subs.
    pub async fn ensure_chat_subscriptions(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        login: &str,
    ) -> bool {
        let mut ok = true;
        for sub_type in ["channel.chat.message", "channel.chat.notification"] {
            let condition = serde_json::json!({
                "broadcaster_user_id": broadcaster_id,
                "user_id": bot_user_id,
            });
            ok &= self
                .ensure_subscription_with_condition(
                    sub_type,
                    "1",
                    condition,
                    broadcaster_id,
                    login,
                    None,
                )
                .await;
        }
        ok
    }

    /// `channel.moderate`-Subscription für einen Partner-Kanal, in dem der Bot
    /// Moderator ist (Port von `eventsub_mixin.py:1711`). Condition
    /// `{broadcaster_user_id, moderator_user_id: <bot>}`, Auth = **Bot-User-Token**
    /// (braucht `channel:moderate`-Scope). Speist den `BlacklistRaidGuard`, der
    /// manuelle Raids auf Blacklist-Ziele abbricht. Ist der Bot kein Moderator im
    /// Kanal, liefert Twitch 403 → `perm_failed` (kein Retry-Spam).
    pub async fn ensure_moderator_subscription(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        bot_token: &str,
        login: &str,
    ) -> bool {
        let condition = serde_json::json!({
            "broadcaster_user_id": broadcaster_id.trim(),
            "moderator_user_id": bot_user_id.trim(),
        });
        self.ensure_subscription_with_condition(
            "channel.moderate",
            "1",
            condition,
            broadcaster_id,
            login,
            Some(bot_token),
        )
        .await
    }

    /// Broadcaster-Telemetrie-Subs (Bits/Subs/Hype/Ads/Channel-Points) für einen
    /// Partner sicherstellen — Port von `eventsub_mixin.py` Schritt 3 (Z. 1599).
    /// `token` ist der Broadcaster-User-Token, `scopes` dessen DB-Scopes; ein
    /// Sub wird nur versucht, wenn der Scope vorhanden ist (oder die Scope-Liste
    /// leer/unbekannt ist — dann alle versuchen, wie Python). Liefert die Anzahl
    /// sichergestellter Subs (getrackt oder neu erstellt).
    pub async fn ensure_broadcaster_telemetry_subscriptions(
        &self,
        broadcaster_id: &str,
        login: &str,
        token: &str,
        scopes: &[String],
    ) -> usize {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() || token.trim().is_empty() {
            return 0;
        }
        let scope_set: HashSet<String> = scopes
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let condition = serde_json::json!({ "broadcaster_user_id": broadcaster_id });
        let mut ensured = 0;
        for (sub_type, version, required_scope) in BROADCASTER_TELEMETRY_SUBSCRIPTIONS {
            // Scope-Filter (Python: `if token_scopes and required_scope not in token_scopes`):
            // bei bekannter Scope-Liste fehlende Scopes überspringen.
            if !scope_set.is_empty() && !scope_set.contains(required_scope) {
                tracing::debug!(
                    sub_type,
                    login,
                    required_scope,
                    "Telemetrie-Sub übersprungen: Token-Scope fehlt"
                );
                continue;
            }
            if self
                .ensure_subscription_with_condition(
                    sub_type,
                    version,
                    condition.clone(),
                    broadcaster_id,
                    login,
                    Some(token),
                )
                .await
            {
                ensured += 1;
            }
        }
        ensured
    }

    /// `channel.chat.user_first_message`-Subscription für einen Partner (B5-01):
    /// erkennt, wenn ein Chatter zum ersten Mal überhaupt im Kanal schreibt
    /// (First-Message-Funnel). Condition `{broadcaster_user_id, user_id: <bot>}`,
    /// Auth = **Bot-User-Token** (braucht `user:read:chat`) — Port von
    /// `eventsub_mixin.py:2692`. Ohne Bot-Token/Bot-ID kein Versuch.
    pub async fn ensure_first_message_subscription(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        bot_token: &str,
        login: &str,
    ) -> bool {
        let broadcaster_id = broadcaster_id.trim();
        let bot_user_id = bot_user_id.trim();
        if broadcaster_id.is_empty() || bot_user_id.is_empty() || bot_token.trim().is_empty() {
            return false;
        }
        let condition = serde_json::json!({
            "broadcaster_user_id": broadcaster_id,
            "user_id": bot_user_id,
        });
        self.ensure_subscription_with_condition(
            "channel.chat.user_first_message",
            "1",
            condition,
            broadcaster_id,
            login,
            Some(bot_token),
        )
        .await
    }

    /// Moderator-Daten-Telemetrie-Subs (follow/ban/unban/shoutout) für einen
    /// Partner sicherstellen — Port von `eventsub_mixin.py` `moderator_subs`
    /// (Z. 1704, B5-02). `bot_token` ist der Moderator-User-Token (der Rust-Bot
    /// ist Moderator im Kanal), `bot_user_id` füllt `moderator_user_id` der
    /// Condition, `scopes` sind die Bot-Token-Scopes; ein Sub wird nur versucht,
    /// wenn der Scope vorhanden ist (oder die Scope-Liste leer/unbekannt ist).
    /// Ist der Bot kein Moderator im Kanal, liefert Twitch 403 → `perm_failed`
    /// (kein Retry-Spam). Liefert die Anzahl sichergestellter Subs.
    pub async fn ensure_moderator_telemetry_subscriptions(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        bot_token: &str,
        scopes: &[String],
        login: &str,
    ) -> usize {
        let broadcaster_id = broadcaster_id.trim();
        let bot_user_id = bot_user_id.trim();
        if broadcaster_id.is_empty() || bot_user_id.is_empty() || bot_token.trim().is_empty() {
            return 0;
        }
        let scope_set: HashSet<String> = scopes
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let condition = serde_json::json!({
            "broadcaster_user_id": broadcaster_id,
            "moderator_user_id": bot_user_id,
        });
        let mut ensured = 0;
        for (sub_type, version, required_scope) in MODERATOR_TELEMETRY_SUBSCRIPTIONS {
            // Scope-Filter (Python: `if required_scope not in bot_scopes`):
            // bei bekannter Scope-Liste fehlende Scopes überspringen.
            if !scope_set.is_empty() && !scope_set.contains(required_scope) {
                tracing::debug!(
                    sub_type,
                    login,
                    required_scope,
                    "Moderator-Telemetrie-Sub übersprungen: Bot-Token-Scope fehlt"
                );
                continue;
            }
            if self
                .ensure_subscription_with_condition(
                    sub_type,
                    version,
                    condition.clone(),
                    broadcaster_id,
                    login,
                    Some(bot_token),
                )
                .await
            {
                ensured += 1;
            }
        }
        ensured
    }

    async fn ensure_subscription_with_key(
        &self,
        sub_type: &str,
        version: &str,
        condition_key: &str,
        broadcaster_id: &str,
        login: &str,
    ) -> bool {
        let condition = serde_json::json!({ condition_key: broadcaster_id.trim() });
        self.ensure_subscription_with_condition(
            sub_type,
            version,
            condition,
            broadcaster_id,
            login,
            None,
        )
        .await
    }

    async fn ensure_subscription_with_condition(
        &self,
        sub_type: &str,
        version: &str,
        condition: serde_json::Value,
        broadcaster_id: &str,
        login: &str,
        bearer_override: Option<&str>,
    ) -> bool {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return false;
        }
        if self.is_tracked(sub_type, broadcaster_id) {
            tracing::debug!(sub_type, login, "EventSub: bereits subscribed, überspringe");
            return true;
        }
        if self
            .perm_failed
            .lock()
            .expect("perm_failed lock")
            .contains(&(sub_type.to_string(), broadcaster_id.to_string()))
        {
            tracing::debug!(sub_type, login, "EventSub: 403-gebannt, überspringe");
            return false;
        }
        match self
            .transport
            .create(
                sub_type,
                version,
                &condition,
                &self.config.callback_url,
                &self.config.secret,
                bearer_override,
            )
            .await
        {
            Ok(already_exists) => {
                self.track(sub_type, broadcaster_id);
                tracing::info!(
                    sub_type,
                    login,
                    "EventSub Webhook: Subscription {}",
                    if already_exists {
                        "bereits vorhanden"
                    } else {
                        "erstellt"
                    }
                );
                if sub_type == "stream.offline" {
                    self.record_capacity_snapshot("stream_offline_subscribed")
                        .await;
                }
                true
            }
            Err(error) => {
                let msg = error.to_string();
                if msg.contains("403") {
                    self.perm_failed
                        .lock()
                        .expect("perm_failed lock")
                        .insert((sub_type.to_string(), broadcaster_id.to_string()));
                    tracing::warn!(
                        sub_type,
                        login,
                        "EventSub 403: Bot gebannt oder Kanal gesperrt — \
                         kein weiterer Retry bis Neustart"
                    );
                } else if msg.contains("429") {
                    // Rate-Limit: transient, nächster Reconcile-Zyklus versucht erneut.
                    // debug! statt warn! — sonst gleicher Spam wie 403 (48 Kanäle × 30 min).
                    tracing::debug!(sub_type, login, "EventSub 429: Rate-Limit — Retry nächster Zyklus");
                } else if msg.contains("401") {
                    // App-Token abgelaufen/ungültig: TokenManager übernimmt Refresh.
                    // debug! — betrifft alle Kanäle gleichzeitig, würde sonst 48× spammen.
                    tracing::debug!(sub_type, login, "EventSub 401: App-Token temporär ungültig");
                } else if msg.contains("400") {
                    // Kanal für diesen Sub-Typ nicht berechtigt (z. B. hype_train
                    // braucht Affiliate/Partner-Tier) oder Scope-Edge-Case. Python
                    // fängt das in den broadcaster_subs still auf debug ab — nächster
                    // Reconcile-Zyklus versucht es erneut, falls sich die Lage ändert.
                    tracing::debug!(sub_type, login, "EventSub 400: Kanal nicht berechtigt — Retry nächster Zyklus");
                } else {
                    tracing::warn!(%error, sub_type, login, "EventSub: Subscription fehlgeschlagen");
                }
                false
            }
        }
    }

    /// Räumt verwaiste Subscriptions unserer Callback-URL ab
    /// (Python `_cleanup_old_eventsub_subscriptions`): Ziel-Broadcaster
    /// nicht mehr aktiv → löschen. Liefert die Anzahl gelöschter Subs.
    pub async fn cleanup_stale(&self, active_user_ids: &HashSet<String>) -> usize {
        let subs = match self.transport.list().await {
            Ok(subs) => subs,
            Err(error) => {
                tracing::debug!(%error, "EventSub-Cleanup: Liste nicht ladbar");
                return 0;
            }
        };
        let mut deleted = 0;
        for sub in subs {
            if sub.callback.as_deref() != Some(self.config.callback_url.as_str()) {
                continue;
            }
            let target = sub.broadcaster_user_id.as_deref().unwrap_or("");
            if !active_user_ids.is_empty() && !target.is_empty() && active_user_ids.contains(target)
            {
                continue;
            }
            match self.transport.delete(&sub.id).await {
                Ok(()) => {
                    deleted += 1;
                    self.tracked
                        .lock()
                        .expect("tracked lock")
                        .remove(&(sub.sub_type, target.to_string()));
                }
                Err(error) => {
                    tracing::debug!(%error, sub_id = %sub.id, "EventSub-Cleanup: Delete fehlgeschlagen");
                }
            }
        }
        if deleted > 0 {
            tracing::info!(
                deleted,
                "EventSub Webhook: veraltete Subscriptions gelöscht"
            );
        }
        deleted
    }

    /// Aktuell getrackte EventSub-Subscriptions als `(sub_type, broadcaster_id)`-
    /// Paare — Live-Quelle für die `current`-Sektion von `GET /stats` (Webhook-
    /// Modus; ersetzt Pythons WS-In-Process-State).
    pub fn tracked_pairs(&self) -> Vec<(String, String)> {
        self.tracked
            .lock()
            .expect("tracked lock")
            .iter()
            .cloned()
            .collect()
    }

    /// Kapazitäts-Snapshot fürs Admin-Dashboard. Webhook-Modus: keine
    /// WS-Listener — Listener-Felder 0, `used_slots` = getrackte Subs.
    pub async fn record_capacity_snapshot(&self, trigger: &str) {
        let used = self.tracked.lock().expect("tracked lock").len() as i32;
        if let Err(error) = self.capacity.record(trigger, used, Utc::now()).await {
            tracing::debug!(%error, trigger, "Capacity-Snapshot fehlgeschlagen");
        }
    }

    /// Periodischer Capacity-Snapshot im Poll-Tick (B5-08, Port von
    /// `eventsub_mixin.py:_record_eventsub_capacity_snapshot`). Schreibt höchstens
    /// alle `TWITCH_EVENTSUB_CAPACITY_SAMPLE_SECONDS` (Default 300s) eine Zeile,
    /// damit die Admin-Dashboard-Historie der EventSub-Kapazität auch ohne
    /// Subscription-Ereignisse als gleichmäßige Zeitreihe befüllt wird. Stündlich
    /// räumt er Zeilen jenseits des Retention-Fensters
    /// (`TWITCH_EVENTSUB_CAPACITY_RETENTION_DAYS`, Default 45) ab.
    ///
    /// Der Tick ruft das jedes Mal auf; die Drosselung passiert intern (monotone
    /// Uhr). `trigger` landet in `trigger_reason` (z. B. `"poll_tick"`).
    pub async fn record_capacity_snapshot_periodic(&self, trigger: &str) {
        let now_monotonic = (self.clock)();
        let interval = capacity_sample_interval_seconds() as f64;

        // Sample-Throttle: erster Aufruf schreibt immer, danach erst nach `interval`.
        let due = {
            let throttle = self
                .capacity_throttle
                .lock()
                .expect("capacity throttle lock");
            match throttle.last_snapshot {
                Some(last) => (now_monotonic - last) >= interval,
                None => true,
            }
        };
        if !due {
            return;
        }

        let used = self.tracked.lock().expect("tracked lock").len() as i32;
        if let Err(error) = self.capacity.record(trigger, used, Utc::now()).await {
            tracing::debug!(%error, trigger, "periodischer Capacity-Snapshot fehlgeschlagen");
            return;
        }

        // Throttle-Stempel setzen + Cleanup-Fälligkeit bestimmen — Guard wird vor
        // dem nächsten await freigegeben (kein MutexGuard über await-Punkt).
        let cleanup_due = {
            let mut throttle = self
                .capacity_throttle
                .lock()
                .expect("capacity throttle lock");
            throttle.last_snapshot = Some(now_monotonic);
            let due = throttle
                .last_cleanup
                .is_none_or(|last| (now_monotonic - last) >= CAPACITY_CLEANUP_INTERVAL_SECONDS);
            if due {
                throttle.last_cleanup = Some(now_monotonic);
            }
            due
        };

        // Retention-Cleanup höchstens stündlich.
        if cleanup_due {
            let cutoff = Utc::now() - Duration::days(capacity_retention_days());
            if let Err(error) = self.capacity.delete_older_than(cutoff).await {
                tracing::debug!(%error, "Capacity-Retention-Cleanup fehlgeschlagen");
            }
        }
    }
}

/// Schreibt `twitch_eventsub_capacity_snapshot` (Prod-Typen verifiziert).
#[derive(Clone)]
pub struct CapacitySnapshotStore {
    pool: PgPool,
}

impl CapacitySnapshotStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record(
        &self,
        trigger: &str,
        used_slots: i32,
        ts: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO twitch_eventsub_capacity_snapshot
                (ts_utc, trigger_reason, listener_count, ready_listeners,
                 failed_listeners, used_slots, total_slots, headroom_slots,
                 listeners_at_limit, utilization_pct, listeners_json)
            VALUES ($1, $2, 0, 0, 0, $3, 0, 0, 0, 0.0, '[]')
            "#,
        )
        .bind(ts)
        .bind(trigger)
        .bind(used_slots)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Löscht Zeitreihen-Zeilen älter als `cutoff` (Retention, B5-08). Liefert die
    /// Anzahl gelöschter Zeilen.
    pub async fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let rows = sqlx::query("DELETE FROM twitch_eventsub_capacity_snapshot WHERE ts_utc < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(rows)
    }
}
