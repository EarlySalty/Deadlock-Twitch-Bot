//! EventSub-Subscription-Lifecycle (Webhook-Transport, ADR 0004).
//!
//! Rust verwaltet die **Core-Subscriptions** (stream.online/offline,
//! channel.update) mit App-Token selbst — das ist alles, was das Monitoring
//! braucht. Die Broadcaster-/Moderator-Telemetrie-Subs (Bits/Subs/Bans/…)
//! brauchen User-Tokens aus dem verschlüsselten `twitch_raid_auth`-Store
//! (Raid-Subsystem, Phase 6) und bleiben bis dahin bei Python; bei Twitch
//! bestehende Subscriptions liefern unabhängig vom Ersteller weiter an
//! dieselbe Callback-URL (Cutover-Kopplung, siehe Plan-Doc).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tb_chat::{is_passive_lurker_channel, PASSIVE_LURKER_DETAIL, PASSIVE_LURKER_STATE};
use thiserror::Error;

use crate::inbox_runtime::{epoch_clock, ClockFn};
use crate::poller::source::SourceError;

/// Core-Subscription-Typen (Python `_EVENTSUB_WEBHOOK_CORE_SUB_TYPES`): bei
/// Revocation dieser Typen heilt der Reconcile-Loop die Live-Event-Zustellung
/// (Go-Live/Offline/Title) durch Resubscribe; nicht-Core-Typen werden ebenfalls
/// untracked, hängen aber nicht am Go-Live-Pfad.
pub const EVENTSUB_CORE_SUB_TYPES: &[&str] = &["stream.online", "stream.offline", "channel.update"];

/// Chat-Subscription-Typen, deren 403 sich durch Re-Modding des Bots heilen
/// lässt (Python `_subscribe_missing_chat_subscriptions`). Nur für diese Typen
/// läuft der 403-Mod-Retry; andere 403 (z. B. externe Telemetrie) bleiben perm.
const CHAT_MOD_RETRY_SUB_TYPES: &[&str] = &[
    "channel.chat.message",
    "channel.chat.notification",
    "channel.chat.user_first_message",
];

/// Cooldown nach fehlgeschlagenem 403-Mod-Retry (Python `_mod_retry_cooldown`,
/// 10 Minuten). Solange aktiv wird der Subscribe-Versuch übersprungen; nach
/// Ablauf läuft er automatisch erneut (laufzeit-clearbar statt Neustart-Pflicht).
const MOD_RETRY_COOLDOWN_SECONDS: f64 = 600.0;

/// Dauer, nach der ein nicht-chat-spezifischer 403 erneut versucht werden darf.
/// Das verhindert den alten Neustart-Wedge, bleibt aber konservativ genug, um
/// bei echter fehlender Berechtigung nicht dauerhaft gegen Twitch zu feuern.
const PERMISSION_RETRY_COOLDOWN_SECONDS: f64 = 30.0 * 60.0;

/// Maximalanzahl Create-Versuche inkl. Erstversuch für transiente Fehler.
const CREATE_RETRY_MAX_ATTEMPTS: u32 = 3;
const CREATE_RETRY_BASE_DELAY_SECONDS: u64 = 5;
const CREATE_RETRY_MAX_DELAY_SECONDS: u64 = 60;
const CREATE_RETRY_AFTER_MAX_SECONDS: u64 = 60;
const CREATE_RETRY_JITTER_MS: u64 = 750;

/// Webhook EventSub-Limit aus Twitch/Python-Pfad.
const WEBHOOK_EVENTSUB_TOTAL_SLOTS: i64 = 10_000;

#[derive(Debug, Clone, Copy)]
struct SubscriptionRetryConfig {
    max_attempts: u32,
    base_delay: StdDuration,
    max_delay: StdDuration,
    max_retry_after: StdDuration,
    jitter_ms: u64,
}

impl Default for SubscriptionRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: CREATE_RETRY_MAX_ATTEMPTS,
            base_delay: StdDuration::from_secs(CREATE_RETRY_BASE_DELAY_SECONDS),
            max_delay: StdDuration::from_secs(CREATE_RETRY_MAX_DELAY_SECONDS),
            max_retry_after: StdDuration::from_secs(CREATE_RETRY_AFTER_MAX_SECONDS),
            jitter_ms: CREATE_RETRY_JITTER_MS,
        }
    }
}

/// Capacity-Felder im Webhook-Modus.
#[derive(Debug, Clone, Copy)]
pub struct EventSubCapacityValues {
    pub used_slots: i64,
    pub total_slots: i64,
    pub headroom_slots: i64,
    pub listeners_at_limit: i64,
    pub utilization_pct: f64,
}

pub fn eventsub_webhook_capacity_values(used_slots: i64) -> EventSubCapacityValues {
    let used_slots = used_slots.max(0);
    let total_slots = WEBHOOK_EVENTSUB_TOTAL_SLOTS;
    let headroom_slots = (total_slots - used_slots).max(0);
    let utilization_pct = if total_slots > 0 {
        ((used_slots as f64 / total_slots as f64) * 10_000.0).round() / 100.0
    } else {
        0.0
    };
    EventSubCapacityValues {
        used_slots,
        total_slots,
        headroom_slots,
        listeners_at_limit: i64::from(headroom_slots == 0),
        utilization_pct,
    }
}

fn saturating_i32(value: i64) -> i32 {
    match i32::try_from(value) {
        Ok(value) => value,
        Err(_) if value < 0 => i32::MIN,
        Err(_) => i32::MAX,
    }
}

/// Typisierter Create-Fehler auf dem Monitoring-Port. Der Transport-Adapter
/// mappt Helix-Status, Retry-After und Body hierauf, damit der Manager nicht
/// mehr per String-Matching entscheiden muss.
#[derive(Debug, Clone, Error)]
pub enum SubscriptionCreateError {
    #[error("EventSub-Create HTTP {status}")]
    HttpStatus {
        status: u16,
        retry_after: Option<StdDuration>,
        body: Option<String>,
    },
    #[error("EventSub-Create Transportfehler: {message}")]
    Transport { message: String },
}

impl SubscriptionCreateError {
    pub fn http_status(
        status: u16,
        retry_after: Option<StdDuration>,
        body: Option<String>,
    ) -> Self {
        Self::HttpStatus {
            status,
            retry_after,
            body,
        }
    }

    pub fn transport(error: impl std::fmt::Display) -> Self {
        Self::Transport {
            message: error.to_string(),
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::HttpStatus { status, .. } => Some(*status),
            Self::Transport { .. } => None,
        }
    }

    pub fn retry_after(&self) -> Option<StdDuration> {
        match self {
            Self::HttpStatus { retry_after, .. } => *retry_after,
            Self::Transport { .. } => None,
        }
    }

    fn body(&self) -> Option<&str> {
        match self {
            Self::HttpStatus { body, .. } => body.as_deref(),
            Self::Transport { .. } => None,
        }
    }

    fn reason(&self) -> &'static str {
        match self.status() {
            Some(401) => "auth_unauthorized",
            Some(403) => "permission_failed",
            Some(429) if self.is_hard_quota_or_cost_limit() => "rate_limit_quota",
            Some(429) => "rate_limited",
            Some(400) => "bad_request",
            Some(_) => "http_status",
            None => "transport_error",
        }
    }

    fn is_hard_quota_or_cost_limit(&self) -> bool {
        let Some(body) = self.body() else {
            return false;
        };
        let body = normalized_error_message(body);
        [
            "maximum total cost",
            "total cost exceeded",
            "subscription limit",
            "quota exceeded",
            "too many subscriptions",
        ]
        .into_iter()
        .any(|phrase| body.contains(phrase))
    }
}

fn normalized_error_message(body: &str) -> String {
    let body = body.trim();
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return message.to_ascii_lowercase();
        }
    }
    body.to_ascii_lowercase()
}

/// Sichtbarer Status eines fehlgeschlagenen Ensures.
#[derive(Debug, Clone)]
pub struct SubscriptionFailureStatus {
    pub sub_type: String,
    pub broadcaster_id: String,
    pub login: String,
    pub reason: String,
    pub http_status: Option<u16>,
    pub retry_after_seconds: Option<u64>,
    pub attempts: u32,
    pub total_failures: u64,
    pub first_failed_at: f64,
    pub last_failed_at: f64,
    pub next_retry_at: Option<f64>,
    pub hard_failure: bool,
}

#[derive(Debug, Clone)]
pub struct SubscriptionFailureCounter {
    pub sub_type: String,
    pub broadcaster_id: String,
    pub reason: String,
    pub count: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SubscriptionEnsureReport {
    pub attempted: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failures: Vec<SubscriptionFailureStatus>,
}

impl SubscriptionEnsureReport {
    pub fn failed(&self) -> usize {
        self.failures.len()
    }
}

#[derive(Debug, Clone)]
struct SubscriptionFailureState {
    login: String,
    reason: String,
    http_status: Option<u16>,
    retry_after_seconds: Option<u64>,
    attempts: u32,
    total_failures: u64,
    first_failed_at: f64,
    last_failed_at: f64,
    next_retry_at: Option<f64>,
    hard_failure: bool,
}

struct SubscriptionCreateContext<'a> {
    sub_type: &'a str,
    version: &'a str,
    condition: &'a serde_json::Value,
    broadcaster_id: &'a str,
    login: &'a str,
    bearer_override: Option<&'a str>,
}

/// Stellt den Bot zur Laufzeit als Moderator eines Kanals wieder her (Port von
/// Pythons `_ensure_bot_is_mod`). Implementierung lebt in `tb-transport-twitch`
/// (`HelixClient::add_channel_moderator`, Broadcaster-Token) und wird von außen
/// injiziert, damit tb-monitoring nicht aufs Transport-Crate verweisen muss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeratorProvisionOutcome {
    Ready,
    RetryLater,
    BotBanned,
}

#[async_trait::async_trait]
pub trait ModeratorProvisioner: Send + Sync {
    /// `true`, wenn der Bot (wieder) Mod im Kanal ist. `broadcaster_id` = Ziel-
    /// Kanal, `login` = dessen Login (für Logging/Token-Auflösung).
    async fn ensure_bot_is_mod(&self, broadcaster_id: &str, login: &str) -> bool;

    async fn ensure_bot_is_mod_outcome(
        &self,
        broadcaster_id: &str,
        login: &str,
    ) -> ModeratorProvisionOutcome {
        if self.ensure_bot_is_mod(broadcaster_id, login).await {
            ModeratorProvisionOutcome::Ready
        } else {
            ModeratorProvisionOutcome::RetryLater
        }
    }
}

/// User-Token für EventSub-Subscribe-Versuche. Kein `Debug`, damit Tokens nicht
/// versehentlich in Logs/Testfehlern landen.
#[derive(Clone)]
pub struct EventSubUserToken {
    token: String,
    scopes: Vec<String>,
}

impl EventSubUserToken {
    pub fn new(token: impl Into<String>, scopes: Vec<String>) -> Self {
        Self {
            token: token.into(),
            scopes,
        }
    }

    fn token(&self) -> &str {
        self.token.as_str()
    }

    fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

/// Liefert den Broadcaster-User-Token für EventSub-Fallbacks. Die konkrete
/// Auflösung lebt im Composition-Root, weil dort der verschlüsselte
/// `twitch_raid_auth`-Store und Refresh-Kontext verfügbar sind.
#[async_trait::async_trait]
pub trait BroadcasterEventSubTokenProvider: Send + Sync {
    async fn eventsub_broadcaster_token(
        &self,
        broadcaster_id: &str,
        login: &str,
    ) -> Option<EventSubUserToken>;
}

/// Senke für Webhook-Revocations: entkoppelt den `WebhookReceiver` vom
/// `SubscriptionManager`, ohne dass tb-monitoring auf das Binary verweisen muss.
/// Der Empfänger ruft [`RevocationSink::on_revocation`], die Implementierung
/// (der `SubscriptionManager`) untrackt die Sub, damit der nächste Reconcile-
/// Zyklus sie neu anlegt (Selbstheilung statt stillem Event-Verlust).
pub trait RevocationSink: Send + Sync {
    /// Reagiert auf eine widerrufene Subscription. `true`, wenn ein Tracking-
    /// Eintrag entfernt wurde (→ Resubscribe beim nächsten Reconcile fällig).
    fn on_revocation(&self, sub_type: &str, broadcaster_id: &str) -> bool;
}

impl RevocationSink for SubscriptionManager {
    fn on_revocation(&self, sub_type: &str, broadcaster_id: &str) -> bool {
        self.untrack(sub_type, broadcaster_id)
    }
}

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
    T: std::str::FromStr + Ord + std::fmt::Display + Copy,
{
    match std::env::var(key) {
        Ok(raw) => match raw.trim().parse::<T>() {
            Ok(value) => {
                let clamped = value.clamp(min, max);
                if clamped != value {
                    tracing::warn!(
                        setting = key,
                        value = %value,
                        minimum = %min,
                        maximum = %max,
                        "Optionaler EventSub-Capacity-Env-Wert ausserhalb des Bereichs; Clamp wird verwendet"
                    );
                }
                clamped
            }
            Err(_) => {
                tracing::warn!(
                    setting = key,
                    value = %raw,
                    default = %default,
                    "Ungültiger optionaler EventSub-Capacity-Env-Wert; Default wird verwendet"
                );
                default
            }
        },
        Err(_) => default,
    }
}

/// Normalisiert einen Kanal-Login wie Pythons `_record_chat_subscription_state`:
/// trimmen, Kleinschreibung, führendes `#` entfernen.
fn normalize_login(login: &str) -> String {
    login
        .trim()
        .to_lowercase()
        .trim_start_matches('#')
        .to_string()
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
    (
        "channel.hype_train.progress",
        "1",
        "channel:read:hype_train",
    ),
    ("channel.hype_train.end", "1", "channel:read:hype_train"),
    ("channel.subscribe", "1", "channel:read:subscriptions"),
    (
        "channel.subscription.gift",
        "1",
        "channel:read:subscriptions",
    ),
    (
        "channel.subscription.message",
        "1",
        "channel:read:subscriptions",
    ),
    (
        "channel.subscription.end",
        "1",
        "channel:read:subscriptions",
    ),
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
    (
        "channel.shoutout.receive",
        "1",
        "moderator:manage:shoutouts",
    ),
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
    ) -> Result<bool, SubscriptionCreateError>;
    /// Erzwingt einen frischen Auth-Kontext vor einem gezielten Retry. Adapter
    /// mit App-Token können den Token-Cache invalidieren; User-Token-Pfade
    /// dürfen hier no-op bleiben und beim nächsten Reconcile neu auflösen.
    async fn refresh_auth(
        &self,
        _broadcaster_id: &str,
        _login: &str,
        _bearer_override: Option<&str>,
    ) -> Result<(), SourceError> {
        Ok(())
    }
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

/// Persistierter Subscription-State-Eintrag eines Kanals (Port von Pythons
/// `_channel_subscription_state[login][sub_type]`): aktuell nur der
/// Passive-Lurker-Marker, der den Subscribe-Versuch ersetzt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SubscriptionState {
    state: &'static str,
    detail: Option<&'static str>,
}

pub struct SubscriptionManager {
    transport: Arc<dyn SubscriptionTransport>,
    config: SubscriptionConfig,
    capacity: CapacitySnapshotStore,
    /// DB-Pool für DB-gestützte Reconcile-Entscheidungen (Passive-Lurker-Gate,
    /// B8-07). Geteilt mit dem `CapacitySnapshotStore` — derselbe Prod-Pool.
    pool: PgPool,
    /// In-Memory-Tracking (Typ, broadcaster_user_id) — wie Pythons
    /// `_eventsub_has_sub`, beim Start via [`Self::rehydrate`] gefüllt.
    tracked: Mutex<HashSet<(String, String)>>,
    /// Laufzeit-403-Cooldown (Typ, broadcaster_user_id): verhindert Retry-Spam,
    /// läuft aber aus, damit Reauth/Reconcile ohne Neustart erneut versuchen.
    perm_failed: Mutex<HashMap<(String, String), f64>>,
    /// Fehlgeschlagene Ensures mit letztem Status und Retry-Zeitpunkt.
    failed_subscriptions: Mutex<HashMap<(String, String), SubscriptionFailureState>>,
    /// In-Memory-Counter je Sub/Reason. Dient als lokale Metrik ohne neue
    /// Observability-Abhängigkeit im Monitoring-Crate.
    failure_counters: Mutex<HashMap<(String, String, String), u64>>,
    /// Subscription-State pro Kanal (login → sub_type → State) — Port von Pythons
    /// `_channel_subscription_state`. Hält den Passive-Lurker-Marker für die
    /// Join-Diagnose, ohne einen Subscribe-Versuch zu starten (B8-07).
    subscription_state: Mutex<HashMap<String, HashMap<String, SubscriptionState>>>,
    /// Drosselung der periodischen Capacity-Zeitreihe (B5-08).
    capacity_throttle: Mutex<CapacityThrottle>,
    /// Monotone Uhr (Epoch-Sek.) für die Throttle-Fenster — in Tests injizierbar.
    clock: ClockFn,
    /// Optionaler Mod-Provisioner für die 403-Selbstheilung im Chat-Pfad
    /// (Python `_ensure_bot_is_mod`). `None` → 403-Cooldown statt Re-Mod.
    moderator_provisioner: Option<Arc<dyn ModeratorProvisioner>>,
    /// Optionaler Broadcaster-Token-Provider für Moderator-Telemetrie-Fallbacks
    /// (Python `auth_attempts = [bot-token, broadcaster-token]`).
    broadcaster_token_provider: Option<Arc<dyn BroadcasterEventSubTokenProvider>>,
    /// 403-Mod-Retry-Cooldown je (sub_type, broadcaster_id) als Ablauf-Epoch-Sek.
    /// (Python `_mod_retry_cooldown`). Ersetzt den permanenten perm_failed-Eintrag
    /// im Chat-Pfad: nach Ablauf wird automatisch erneut versucht.
    mod_retry_cooldown: Mutex<HashMap<(String, String), f64>>,
    retry_config: SubscriptionRetryConfig,
}

impl SubscriptionManager {
    pub fn new(
        transport: Arc<dyn SubscriptionTransport>,
        config: SubscriptionConfig,
        capacity: CapacitySnapshotStore,
    ) -> Self {
        let pool = capacity.pool().clone();
        Self {
            transport,
            config,
            capacity,
            pool,
            tracked: Mutex::new(HashSet::new()),
            perm_failed: Mutex::new(HashMap::new()),
            failed_subscriptions: Mutex::new(HashMap::new()),
            failure_counters: Mutex::new(HashMap::new()),
            subscription_state: Mutex::new(HashMap::new()),
            capacity_throttle: Mutex::new(CapacityThrottle::default()),
            clock: Arc::new(epoch_clock),
            moderator_provisioner: None,
            broadcaster_token_provider: None,
            mod_retry_cooldown: Mutex::new(HashMap::new()),
            retry_config: SubscriptionRetryConfig::default(),
        }
    }

    /// Ersetzt die Throttle-Uhr (Tests). Default ist die System-Epoch-Uhr.
    #[must_use]
    pub fn with_clock(mut self, clock: ClockFn) -> Self {
        self.clock = clock;
        self
    }

    #[cfg(test)]
    fn with_retry_config(mut self, retry_config: SubscriptionRetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    /// Verdrahtet den Mod-Provisioner für die 403-Selbstheilung im Chat-Pfad
    /// (Python `_ensure_bot_is_mod`). Ohne ihn bleibt es beim Alt-Verhalten
    /// (403 → auslaufender Cooldown statt Re-Mod).
    /// Live verdrahtet in `bin/tb-bot` (`with_moderator_provisioner`, main.rs).
    #[must_use]
    pub fn with_moderator_provisioner(
        mut self,
        provisioner: Arc<dyn ModeratorProvisioner>,
    ) -> Self {
        self.moderator_provisioner = Some(provisioner);
        self
    }

    /// Verdrahtet den Broadcaster-Token-Fallback für Moderator-Telemetrie-Subs.
    ///
    /// Live verdrahtet in `bin/tb-bot` (`with_broadcaster_eventsub_token_provider`
    /// im `manager_builder`, main.rs): reicht den bestehenden Broadcaster-/Raid-
    /// Auth-Tokenpfad als [`BroadcasterEventSubTokenProvider`] durch. Ohne
    /// Krypto-Key (DB_MASTER_KEY_V1) bleibt der Fallback aus.
    #[must_use]
    pub fn with_broadcaster_eventsub_token_provider(
        mut self,
        provider: Arc<dyn BroadcasterEventSubTokenProvider>,
    ) -> Self {
        self.broadcaster_token_provider = Some(provider);
        self
    }

    /// `true`, wenn für (sub_type, broadcaster_id) ein noch nicht abgelaufener
    /// Mod-Retry-Cooldown aktiv ist. Abgelaufene Einträge werden entfernt
    /// (laufzeit-clearbar — kein Neustart nötig).
    fn mod_retry_cooldown_active(&self, sub_type: &str, broadcaster_id: &str) -> bool {
        let key = (sub_type.to_string(), broadcaster_id.to_string());
        let now = (self.clock)();
        let mut cd = self
            .mod_retry_cooldown
            .lock()
            .expect("mod_retry_cooldown lock");
        match cd.get(&key) {
            Some(&until) if until > now => true,
            Some(_) => {
                cd.remove(&key);
                false
            }
            None => false,
        }
    }

    /// Setzt den 10-Minuten-Cooldown (Python `_mod_retry_cooldown`).
    fn set_mod_retry_cooldown(&self, sub_type: &str, broadcaster_id: &str) {
        let until = (self.clock)() + MOD_RETRY_COOLDOWN_SECONDS;
        self.mod_retry_cooldown
            .lock()
            .expect("mod_retry_cooldown lock")
            .insert((sub_type.to_string(), broadcaster_id.to_string()), until);
    }

    fn is_tracked(&self, sub_type: &str, broadcaster_id: &str) -> bool {
        self.tracked
            .lock()
            .expect("tracked lock")
            .contains(&(sub_type.to_string(), broadcaster_id.to_string()))
    }

    fn is_perm_failed(&self, sub_type: &str, broadcaster_id: &str) -> bool {
        let key = (sub_type.to_string(), broadcaster_id.to_string());
        let now = (self.clock)();
        let Ok(mut failures) = self.perm_failed.lock() else {
            tracing::warn!("EventSub: perm_failed-Cooldown-State nicht lesbar");
            return false;
        };
        match failures.get(&key).copied() {
            Some(until) if until > now => true,
            Some(_) => {
                failures.remove(&key);
                false
            }
            None => false,
        }
    }

    fn mark_perm_failed(&self, sub_type: &str, broadcaster_id: &str) {
        let until = (self.clock)() + PERMISSION_RETRY_COOLDOWN_SECONDS;
        if let Ok(mut failures) = self.perm_failed.lock() {
            failures.insert((sub_type.to_string(), broadcaster_id.to_string()), until);
        } else {
            tracing::warn!("EventSub: perm_failed-Cooldown-State nicht schreibbar");
        }
    }

    fn track(&self, sub_type: &str, broadcaster_id: &str) {
        self.tracked
            .lock()
            .expect("tracked lock")
            .insert((sub_type.to_string(), broadcaster_id.to_string()));
        self.clear_subscription_failure(sub_type, broadcaster_id);
    }

    fn failure_status_for(
        sub_type: String,
        broadcaster_id: String,
        state: SubscriptionFailureState,
    ) -> SubscriptionFailureStatus {
        SubscriptionFailureStatus {
            sub_type,
            broadcaster_id,
            login: state.login,
            reason: state.reason,
            http_status: state.http_status,
            retry_after_seconds: state.retry_after_seconds,
            attempts: state.attempts,
            total_failures: state.total_failures,
            first_failed_at: state.first_failed_at,
            last_failed_at: state.last_failed_at,
            next_retry_at: state.next_retry_at,
            hard_failure: state.hard_failure,
        }
    }

    pub fn failed_subscription_statuses(&self) -> Vec<SubscriptionFailureStatus> {
        let Ok(failures) = self.failed_subscriptions.lock() else {
            tracing::warn!("EventSub: Failed-Subscription-State nicht lesbar");
            return Vec::new();
        };
        let mut out: Vec<SubscriptionFailureStatus> = failures
            .iter()
            .map(|((sub_type, broadcaster_id), state)| {
                Self::failure_status_for(sub_type.clone(), broadcaster_id.clone(), state.clone())
            })
            .collect();
        out.sort_by(|a, b| {
            a.sub_type
                .cmp(&b.sub_type)
                .then_with(|| a.broadcaster_id.cmp(&b.broadcaster_id))
        });
        out
    }

    pub fn subscription_failure_counters(&self) -> Vec<SubscriptionFailureCounter> {
        let Ok(counters) = self.failure_counters.lock() else {
            tracing::warn!("EventSub: Subscription-Failure-Counter nicht lesbar");
            return Vec::new();
        };
        let mut out: Vec<SubscriptionFailureCounter> = counters
            .iter()
            .map(
                |((sub_type, broadcaster_id, reason), count)| SubscriptionFailureCounter {
                    sub_type: sub_type.clone(),
                    broadcaster_id: broadcaster_id.clone(),
                    reason: reason.clone(),
                    count: *count,
                },
            )
            .collect();
        out.sort_by(|a, b| {
            a.sub_type
                .cmp(&b.sub_type)
                .then_with(|| a.broadcaster_id.cmp(&b.broadcaster_id))
                .then_with(|| a.reason.cmp(&b.reason))
        });
        out
    }

    fn subscription_failure_status(
        &self,
        sub_type: &str,
        broadcaster_id: &str,
    ) -> Option<SubscriptionFailureStatus> {
        let Ok(failures) = self.failed_subscriptions.lock() else {
            tracing::warn!("EventSub: Failed-Subscription-State nicht lesbar");
            return None;
        };
        failures
            .get(&(sub_type.to_string(), broadcaster_id.to_string()))
            .cloned()
            .map(|state| {
                Self::failure_status_for(sub_type.to_string(), broadcaster_id.to_string(), state)
            })
    }

    fn clear_subscription_failure(&self, sub_type: &str, broadcaster_id: &str) {
        if let Ok(mut failures) = self.failed_subscriptions.lock() {
            failures.remove(&(sub_type.to_string(), broadcaster_id.to_string()));
        } else {
            tracing::warn!("EventSub: Failed-Subscription-State nicht schreibbar");
        }
    }

    fn increment_failure_counter(&self, sub_type: &str, broadcaster_id: &str, reason: &str) -> u64 {
        let Ok(mut counters) = self.failure_counters.lock() else {
            tracing::warn!("EventSub: Subscription-Failure-Counter nicht schreibbar");
            return 0;
        };
        let key = (
            sub_type.to_string(),
            broadcaster_id.to_string(),
            reason.to_string(),
        );
        let entry = counters.entry(key).or_insert(0);
        *entry += 1;
        *entry
    }

    fn record_subscription_failure(
        &self,
        sub_type: &str,
        broadcaster_id: &str,
        login: &str,
        error: &SubscriptionCreateError,
        attempts: u32,
        next_retry_at: Option<f64>,
    ) -> u64 {
        let reason = error.reason();
        let total_failures = self.increment_failure_counter(sub_type, broadcaster_id, reason);
        let now = (self.clock)();
        let state = SubscriptionFailureState {
            login: normalize_login(login),
            reason: reason.to_string(),
            http_status: error.status(),
            retry_after_seconds: error.retry_after().map(|d| d.as_secs()),
            attempts,
            total_failures,
            first_failed_at: now,
            last_failed_at: now,
            next_retry_at,
            hard_failure: error.is_hard_quota_or_cost_limit(),
        };
        let Ok(mut failures) = self.failed_subscriptions.lock() else {
            tracing::warn!("EventSub: Failed-Subscription-State nicht schreibbar");
            return total_failures;
        };
        failures
            .entry((sub_type.to_string(), broadcaster_id.to_string()))
            .and_modify(|existing| {
                existing.login = state.login.clone();
                existing.reason = state.reason.clone();
                existing.http_status = state.http_status;
                existing.retry_after_seconds = state.retry_after_seconds;
                existing.attempts = state.attempts;
                existing.total_failures = state.total_failures;
                existing.last_failed_at = state.last_failed_at;
                existing.next_retry_at = state.next_retry_at;
                existing.hard_failure = state.hard_failure;
            })
            .or_insert(state);
        total_failures
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
    pub async fn ensure_core_subscriptions(
        &self,
        broadcaster_id: &str,
        login: &str,
    ) -> SubscriptionEnsureReport {
        let mut report = SubscriptionEnsureReport::default();
        for (sub_type, version) in CORE_SUBSCRIPTIONS {
            report.attempted += 1;
            if self
                .ensure_subscription(sub_type, version, broadcaster_id, login)
                .await
            {
                report.succeeded += 1;
            } else if let Some(failure) = self.subscription_failure_status(sub_type, broadcaster_id)
            {
                report.failures.push(failure);
            } else {
                report.skipped += 1;
            }
        }
        if !report.failures.is_empty() {
            tracing::warn!(
                login,
                broadcaster_id,
                attempted = report.attempted,
                succeeded = report.succeeded,
                failed = report.failures.len(),
                "EventSub Core-Ensure unvollständig"
            );
        }
        report
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
        const CHAT_SUB_TYPES: [&str; 2] = ["channel.chat.message", "channel.chat.notification"];

        // Passive-Lurker-Gate (B8-07, Python `connection.py:1237`/`:1532`):
        // monitored-only Kanäle ohne Partner-State und ohne Raid-Auth sind ein
        // Endzustand — kein Subscribe-Versuch, stattdessen den Lurker-State
        // schreiben (sonst pro Reconcile-Zyklus wiederkehrende Fehlversuche).
        if self.is_passive_lurker(broadcaster_id, login).await {
            for sub_type in CHAT_SUB_TYPES {
                self.record_subscription_state(
                    login,
                    sub_type,
                    PASSIVE_LURKER_STATE,
                    Some(PASSIVE_LURKER_DETAIL),
                );
            }
            tracing::debug!(
                login,
                "Chat-Reconcile: passiver Lurker — Subscribe übersprungen"
            );
            return false;
        }

        let mut ok = true;
        for sub_type in CHAT_SUB_TYPES {
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

    pub fn chat_subscriptions_permanently_blocked(&self, broadcaster_id: &str) -> bool {
        let broadcaster_id = broadcaster_id.trim();
        ["channel.chat.message", "channel.chat.notification"]
            .into_iter()
            .any(|sub_type| self.is_perm_failed(sub_type, broadcaster_id))
    }

    /// `true`, wenn der Kanal ein passiver Lurker ist — monitored-only **und**
    /// kein aktiver Partner **und** ohne Raid-Auth. Lädt die drei Flags aus
    /// `twitch_streamers`, `twitch_streamers_partner_state` und `twitch_raid_auth`
    /// (gematcht über `twitch_user_id` ODER `LOWER(twitch_login)`, wie Pythons
    /// `_load_chat_join_channel_state`) und wertet [`is_passive_lurker_channel`]
    /// aus. DB-Fehler werden defensiv als „kein Lurker" behandelt (Python fängt
    /// dort still ab und versucht zu joinen).
    async fn is_passive_lurker(&self, broadcaster_id: &str, login: &str) -> bool {
        let target_id = broadcaster_id.trim();
        let normalized_login = normalize_login(login);
        if target_id.is_empty() && normalized_login.is_empty() {
            return false;
        }

        let is_monitored_only: bool = sqlx::query_scalar!(
            "SELECT TRUE AS \"is_monitored_only!\" FROM twitch_streamers s \
             WHERE (($1 <> '' AND s.twitch_user_id = $1) \
                OR ($2 <> '' AND LOWER(s.twitch_login) = $2)) \
               AND NOT EXISTS ( \
                   SELECT 1 FROM twitch_partners p \
                   WHERE p.twitch_user_id = s.twitch_user_id \
                      OR LOWER(p.twitch_login) = LOWER(s.twitch_login) \
               ) \
             LIMIT 1",
            target_id,
            &normalized_login,
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, login, "Lurker-Gate: is_monitored_only nicht ladbar");
            None
        })
        .unwrap_or(false);

        // Kein monitored-only → niemals Lurker; die beiden Folge-Queries sparen.
        if !is_monitored_only {
            return false;
        }

        let is_partner_active: bool = sqlx::query_scalar!(
            "SELECT COALESCE(is_partner_active, 0) <> 0 AS \"is_partner_active!\" \
             FROM twitch_streamers_partner_state \
             WHERE ($1 <> '' AND twitch_user_id = $1) \
                OR ($2 <> '' AND LOWER(twitch_login) = $2) \
             ORDER BY is_partner_active DESC LIMIT 1",
            target_id,
            &normalized_login,
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, login, "Lurker-Gate: is_partner_active nicht ladbar");
            None
        })
        .unwrap_or(false);

        let has_raid_auth: bool = sqlx::query_scalar!(
            "SELECT EXISTS( \
                SELECT 1 FROM twitch_raid_auth \
                WHERE ($1 <> '' AND twitch_user_id = $1) \
                   OR ($2 <> '' AND LOWER(twitch_login) = $2) \
             ) AS \"has_raid_auth!\"",
            target_id,
            &normalized_login,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, login, "Lurker-Gate: has_raid_auth nicht ladbar");
            false
        });

        is_passive_lurker_channel(is_monitored_only, is_partner_active, has_raid_auth)
    }

    /// Schreibt den Subscription-State eines Kanals (Port von Pythons
    /// `_record_chat_subscription_state`). Leerer Login/Sub-Typ → No-op.
    fn record_subscription_state(
        &self,
        login: &str,
        sub_type: &str,
        state: &'static str,
        detail: Option<&'static str>,
    ) {
        let normalized_login = normalize_login(login);
        if normalized_login.is_empty() || sub_type.is_empty() {
            return;
        }
        self.subscription_state
            .lock()
            .expect("subscription_state lock")
            .entry(normalized_login)
            .or_default()
            .insert(sub_type.to_string(), SubscriptionState { state, detail });
    }

    /// Entfernt die in Rust vorhandenen lokalen Chat-/Subscription-Zustände für
    /// einen stale/removed Kanal (Port von Pythons `_purge_local_channel_state`):
    /// hier sind das `subscription_state`, `perm_failed` und ein evtl. laufender
    /// Mod-Retry-Cooldown. Persistente Twitch-/Partner-Tabellen bleiben
    /// unangetastet.
    fn purge_local_channel_state(&self, broadcaster_id: &str, login: &str) {
        let normalized_login = normalize_login(login);
        if !normalized_login.is_empty() {
            self.subscription_state
                .lock()
                .expect("subscription_state lock")
                .remove(&normalized_login);
        }
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return;
        }
        if let Ok(mut perm_failed) = self.perm_failed.lock() {
            perm_failed.retain(|(_, bid), _| bid != broadcaster_id);
        }
        if let Ok(mut failed_subscriptions) = self.failed_subscriptions.lock() {
            failed_subscriptions.retain(|(_, bid), _| bid != broadcaster_id);
        }
        self.mod_retry_cooldown
            .lock()
            .expect("mod_retry_cooldown lock")
            .retain(|(_, bid), _| bid != broadcaster_id);
    }

    fn has_local_channel_state(&self, broadcaster_id: &str, login: &str) -> bool {
        let normalized_login = normalize_login(login);
        if !normalized_login.is_empty()
            && self
                .subscription_state
                .lock()
                .expect("subscription_state lock")
                .contains_key(&normalized_login)
        {
            return true;
        }
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return false;
        }
        let now = (self.clock)();
        self.perm_failed
            .lock()
            .map(|failures| {
                failures
                    .iter()
                    .any(|((_, bid), until)| bid == broadcaster_id && *until > now)
            })
            .unwrap_or(false)
    }

    /// Subscription-States eines Kanals als `(sub_type, state, detail)`-Tripel —
    /// Diagnose-Quelle für die Join-Entscheidung (Port von Pythons
    /// `get_channel_subscription_state`).
    pub fn chat_subscription_states(&self, login: &str) -> Vec<(String, String, Option<String>)> {
        let normalized_login = normalize_login(login);
        self.subscription_state
            .lock()
            .expect("subscription_state lock")
            .get(&normalized_login)
            .map(|states| {
                states
                    .iter()
                    .map(|(sub_type, entry)| {
                        (
                            sub_type.clone(),
                            entry.state.to_string(),
                            entry.detail.map(str::to_string),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `channel.moderate`-Subscription für einen Partner-Kanal, in dem der Bot
    /// Moderator ist (Port von `eventsub_mixin.py:1711`). Condition
    /// `{broadcaster_user_id, moderator_user_id: <bot>}`, Auth = **Bot-User-Token**
    /// (braucht `channel:moderate`-Scope). Speist den `BlacklistRaidGuard`, der
    /// manuelle Raids auf Blacklist-Ziele abbricht. Ist der Bot kein Moderator im
    /// Kanal, liefert Twitch 403 → auslaufender Cooldown (kein Retry-Spam).
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

    fn scopes_allow(scopes: &[String], required_scope: &str) -> bool {
        scopes.is_empty()
            || scopes
                .iter()
                .map(|scope| scope.trim())
                .any(|scope| scope.eq_ignore_ascii_case(required_scope))
    }

    /// Moderator-Daten-Telemetrie-Subs (follow/ban/unban/shoutout) für einen
    /// Partner sicherstellen — Port von `eventsub_mixin.py` `moderator_subs`
    /// (Z. 1704, B5-02). `bot_token` ist der Moderator-User-Token (der Rust-Bot
    /// ist Moderator im Kanal), `bot_user_id` füllt `moderator_user_id` der
    /// Condition, `scopes` sind die Bot-Token-Scopes. Bei 403/Scope-Lücke wird
    /// zusätzlich ein injizierter Broadcaster-Token versucht (P2.56); dessen
    /// Condition trägt `moderator_user_id = broadcaster_id`, wie Python.
    /// Liefert die Anzahl sichergestellter Subs.
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
        let bot_token = bot_token.trim();
        if broadcaster_id.is_empty() {
            return 0;
        }
        let broadcaster_auth = match self.broadcaster_token_provider.as_ref() {
            Some(provider) => provider
                .eventsub_broadcaster_token(broadcaster_id, login)
                .await
                .filter(|auth| !auth.token().trim().is_empty()),
            None => None,
        };
        let mut ensured = 0;
        for (sub_type, version, required_scope) in MODERATOR_TELEMETRY_SUBSCRIPTIONS {
            let mut attempts: Vec<(&'static str, serde_json::Value, &str)> = Vec::new();
            if !bot_token.is_empty()
                && !bot_user_id.is_empty()
                && Self::scopes_allow(scopes, required_scope)
            {
                attempts.push((
                    "bot",
                    serde_json::json!({
                        "broadcaster_user_id": broadcaster_id,
                        "moderator_user_id": bot_user_id,
                    }),
                    bot_token,
                ));
            } else if !scopes.is_empty() && !Self::scopes_allow(scopes, required_scope) {
                tracing::debug!(
                    sub_type,
                    login,
                    required_scope,
                    "Moderator-Telemetrie-Sub: Bot-Token-Scope fehlt, prüfe Broadcaster-Fallback"
                );
            }

            if let Some(auth) = broadcaster_auth.as_ref() {
                if Self::scopes_allow(auth.scopes(), required_scope) {
                    attempts.push((
                        "broadcaster",
                        serde_json::json!({
                            "broadcaster_user_id": broadcaster_id,
                            "moderator_user_id": broadcaster_id,
                        }),
                        auth.token().trim(),
                    ));
                }
            }

            if attempts.is_empty() {
                tracing::debug!(
                    sub_type,
                    login,
                    required_scope,
                    "Moderator-Telemetrie-Sub übersprungen: kein passender Token/Scope"
                );
                continue;
            }
            if self
                .ensure_subscription_with_auth_attempts(
                    sub_type,
                    version,
                    broadcaster_id,
                    login,
                    attempts,
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
        if self.is_perm_failed(sub_type, broadcaster_id) {
            tracing::debug!(sub_type, login, "EventSub: 403-Cooldown aktiv, überspringe");
            return false;
        }
        // Chat-Pfad-Selbstheilung (P1.2): während des 10-Min-Cooldowns nach
        // einem fehlgeschlagenen Mod-Retry wird der Versuch übersprungen, nach
        // Ablauf aber automatisch erneut gestartet (kein Neustart nötig).
        if self.mod_retry_cooldown_active(sub_type, broadcaster_id) {
            tracing::debug!(
                sub_type,
                login,
                "EventSub: Mod-Retry-Cooldown aktiv — überspringe (Retry nach Ablauf)"
            );
            return false;
        }
        match self
            .create_subscription_with_retries(
                sub_type,
                version,
                &condition,
                broadcaster_id,
                login,
                bearer_override,
            )
            .await
        {
            Ok(_) => true,
            Err(error) => {
                let context = SubscriptionCreateContext {
                    sub_type,
                    version,
                    condition: &condition,
                    broadcaster_id,
                    login,
                    bearer_override,
                };
                self.handle_subscription_create_error(error, &context).await
            }
        }
    }

    async fn create_subscription_once(
        &self,
        sub_type: &str,
        version: &str,
        condition: &serde_json::Value,
        broadcaster_id: &str,
        login: &str,
        bearer_override: Option<&str>,
    ) -> Result<bool, SubscriptionCreateError> {
        let already_exists = self
            .transport
            .create(
                sub_type,
                version,
                condition,
                &self.config.callback_url,
                &self.config.secret,
                bearer_override,
            )
            .await?;
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
        Ok(already_exists)
    }

    async fn create_subscription_with_retries(
        &self,
        sub_type: &str,
        version: &str,
        condition: &serde_json::Value,
        broadcaster_id: &str,
        login: &str,
        bearer_override: Option<&str>,
    ) -> Result<bool, SubscriptionCreateError> {
        let max_attempts = self.retry_config.max_attempts.max(1);
        let mut attempt = 1;
        loop {
            match self
                .create_subscription_once(
                    sub_type,
                    version,
                    condition,
                    broadcaster_id,
                    login,
                    bearer_override,
                )
                .await
            {
                Ok(already_exists) => return Ok(already_exists),
                Err(error) => {
                    let retryable =
                        self.should_retry_create(&error, attempt, max_attempts, bearer_override);
                    let delay = if retryable {
                        Some(self.create_retry_delay(&error, attempt, sub_type, broadcaster_id))
                    } else {
                        None
                    };
                    let next_retry_at = delay.map(|d| (self.clock)() + d.as_secs_f64());
                    let failure_count = self.record_subscription_failure(
                        sub_type,
                        broadcaster_id,
                        login,
                        &error,
                        attempt,
                        next_retry_at,
                    );

                    if !retryable {
                        return Err(error);
                    }

                    if error.status() == Some(401) {
                        if let Err(refresh_error) = self
                            .transport
                            .refresh_auth(broadcaster_id, login, bearer_override)
                            .await
                        {
                            tracing::warn!(
                                %refresh_error,
                                sub_type,
                                login,
                                broadcaster_id,
                                "EventSub 401: Auth-Refresh vor Retry fehlgeschlagen"
                            );
                        }
                    }

                    let Some(delay) = delay else {
                        return Err(error);
                    };
                    tracing::warn!(
                        sub_type,
                        login,
                        broadcaster_id,
                        status = error.status(),
                        reason = error.reason(),
                        attempt,
                        max_attempts,
                        failure_count,
                        retry_delay_ms = delay.as_millis() as u64,
                        "EventSub: Subscription-Create gezielt erneut geplant"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    fn should_retry_create(
        &self,
        error: &SubscriptionCreateError,
        attempt: u32,
        max_attempts: u32,
        bearer_override: Option<&str>,
    ) -> bool {
        if attempt >= max_attempts {
            return false;
        }
        match error.status() {
            // App-Token lässt sich im Transport invalidieren. User-Token werden
            // beim nächsten Provider/Reconcile neu geholt, nicht mit demselben
            // Bearer aggressiv wiederholt.
            Some(401) => bearer_override.is_none(),
            Some(429) => !error.is_hard_quota_or_cost_limit(),
            _ => false,
        }
    }

    fn create_retry_delay(
        &self,
        error: &SubscriptionCreateError,
        attempt: u32,
        sub_type: &str,
        broadcaster_id: &str,
    ) -> StdDuration {
        if let Some(retry_after) = error.retry_after() {
            return retry_after.min(self.retry_config.max_retry_after);
        }
        let exponent = attempt.saturating_sub(1).min(5);
        let multiplier = 1u64 << exponent;
        let base_ms = self.retry_config.base_delay.as_millis() as u64;
        let backoff = StdDuration::from_millis(base_ms.saturating_mul(multiplier))
            .min(self.retry_config.max_delay);
        backoff + StdDuration::from_millis(self.retry_jitter_ms(sub_type, broadcaster_id, attempt))
    }

    fn retry_jitter_ms(&self, sub_type: &str, broadcaster_id: &str, attempt: u32) -> u64 {
        if self.retry_config.jitter_ms == 0 {
            return 0;
        }
        let mut hash = 0xcbf29ce484222325u64;
        for byte in sub_type
            .bytes()
            .chain(broadcaster_id.bytes())
            .chain(attempt.to_le_bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash % self.retry_config.jitter_ms
    }

    async fn handle_subscription_create_error(
        &self,
        error: SubscriptionCreateError,
        context: &SubscriptionCreateContext<'_>,
    ) -> bool {
        match error.status() {
            Some(403) => {
                if CHAT_MOD_RETRY_SUB_TYPES.contains(&context.sub_type)
                    && self
                        .is_stale_removed_channel_after_403(context.broadcaster_id, context.login)
                        .await
                    && (self.has_local_channel_state(context.broadcaster_id, context.login)
                        || self.moderator_provisioner.is_none())
                {
                    self.record_subscription_state(
                        context.login,
                        context.sub_type,
                        "stale_removed_channel",
                        Some("channel no longer tracked locally or authorized"),
                    );
                    self.purge_local_channel_state(context.broadcaster_id, context.login);
                    tracing::info!(
                        sub_type = context.sub_type,
                        login = context.login,
                        "EventSub 403: stale/removed channel erkannt — lokaler Subscription-State gepurged"
                    );
                    return false;
                }
                // P1.2: Chat-Pfad heilt einen Laufzeit-403 (Bot demoddet)
                // selbst: Bot re-modden, 1s warten, ein Re-Subscribe.
                // Gelingt das, ist der Kanal sofort wieder live; scheitert
                // es, greift ein 10-Min-Cooldown (clearbar) STATT eines
                // permanenten perm_failed-Eintrags.
                if CHAT_MOD_RETRY_SUB_TYPES.contains(&context.sub_type)
                    && self.moderator_provisioner.is_some()
                {
                    return self
                        .retry_chat_subscription_after_mod(
                            context.sub_type,
                            context.version,
                            context.condition,
                            context.broadcaster_id,
                            context.login,
                            context.bearer_override,
                        )
                        .await;
                }
                self.mark_perm_failed(context.sub_type, context.broadcaster_id);
                if context.sub_type == "channel.moderate" {
                    tracing::info!(
                        status = 403u16,
                        sub_type = context.sub_type,
                        login = context.login,
                        broadcaster_id = context.broadcaster_id,
                        cooldown_seconds = PERMISSION_RETRY_COOLDOWN_SECONDS as u64,
                        "EventSub 403: Moderator-Guard nicht autorisiert — Retry nach Cooldown/Reauth möglich"
                    );
                } else {
                    tracing::warn!(
                        status = 403u16,
                        sub_type = context.sub_type,
                        login = context.login,
                        broadcaster_id = context.broadcaster_id,
                        cooldown_seconds = PERMISSION_RETRY_COOLDOWN_SECONDS as u64,
                        "EventSub 403: Berechtigung fehlt — Retry nach Cooldown/Reauth möglich"
                    );
                }
            }
            Some(429) if error.is_hard_quota_or_cost_limit() => {
                tracing::error!(
                    status = 429u16,
                    sub_type = context.sub_type,
                    login = context.login,
                    broadcaster_id = context.broadcaster_id,
                    "EventSub 429: harte Quota-/Cost-Grenze beim Subscription-Create"
                );
            }
            Some(429) => {
                tracing::warn!(
                    status = 429u16,
                    retry_after_seconds = error.retry_after().map(|d| d.as_secs()),
                    sub_type = context.sub_type,
                    login = context.login,
                    broadcaster_id = context.broadcaster_id,
                    "EventSub 429: Rate-Limit nach begrenztem Retry weiter aktiv"
                );
            }
            Some(401) => {
                tracing::warn!(
                    status = 401u16,
                    sub_type = context.sub_type,
                    login = context.login,
                    broadcaster_id = context.broadcaster_id,
                    "EventSub 401: Auth-Fehler beim Subscription-Create nach begrenztem Retry"
                );
            }
            Some(400) => {
                // Kanal für diesen Sub-Typ nicht berechtigt (z. B. hype_train
                // braucht Affiliate/Partner-Tier) oder Scope-Edge-Case. Python
                // fängt das in den broadcaster_subs still auf debug ab — nächster
                // Reconcile-Zyklus versucht es erneut, falls sich die Lage ändert.
                tracing::debug!(
                    status = 400u16,
                    sub_type = context.sub_type,
                    login = context.login,
                    broadcaster_id = context.broadcaster_id,
                    "EventSub 400: Kanal nicht berechtigt — Retry nächster Zyklus"
                );
            }
            _ => {
                tracing::warn!(
                    %error,
                    sub_type = context.sub_type,
                    login = context.login,
                    broadcaster_id = context.broadcaster_id,
                    "EventSub: Subscription fehlgeschlagen"
                );
            }
        }
        false
    }

    async fn ensure_subscription_with_auth_attempts(
        &self,
        sub_type: &str,
        version: &str,
        broadcaster_id: &str,
        login: &str,
        attempts: Vec<(&'static str, serde_json::Value, &str)>,
    ) -> bool {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return false;
        }
        if self.is_tracked(sub_type, broadcaster_id) {
            tracing::debug!(sub_type, login, "EventSub: bereits subscribed, überspringe");
            return true;
        }
        if self.is_perm_failed(sub_type, broadcaster_id) {
            tracing::debug!(sub_type, login, "EventSub: 403-Cooldown aktiv, überspringe");
            return false;
        }

        let mut saw_403 = false;
        for (auth_label, condition, token) in attempts {
            match self
                .create_subscription_with_retries(
                    sub_type,
                    version,
                    &condition,
                    broadcaster_id,
                    login,
                    Some(token),
                )
                .await
            {
                Ok(_) => {
                    if auth_label == "broadcaster" {
                        tracing::warn!(
                            sub_type,
                            login,
                            "EventSub Webhook: Moderator-Telemetrie via Broadcaster-Fallback erstellt"
                        );
                    } else {
                        tracing::debug!(
                            sub_type,
                            login,
                            auth_label,
                            "EventSub Webhook: Moderator-Telemetrie via Token-Pfad erstellt"
                        );
                    }
                    return true;
                }
                Err(error) => {
                    if error.status() == Some(403) {
                        saw_403 = true;
                    }
                    tracing::debug!(
                        %error,
                        sub_type,
                        login,
                        auth_label,
                        "EventSub Webhook: Moderator-Telemetrie-Auth-Versuch fehlgeschlagen"
                    );
                }
            }
        }

        if saw_403 {
            self.mark_perm_failed(sub_type, broadcaster_id);
            tracing::debug!(
                status = 403u16,
                sub_type,
                login,
                broadcaster_id,
                cooldown_seconds = PERMISSION_RETRY_COOLDOWN_SECONDS as u64,
                "EventSub 403: Moderator-Telemetrie nicht autorisiert — Retry nach Cooldown/Reauth möglich"
            );
        }
        false
    }

    async fn is_stale_removed_channel_after_403(&self, broadcaster_id: &str, login: &str) -> bool {
        let target_id = broadcaster_id.trim();
        let normalized_login = normalize_login(login);
        if target_id.is_empty() && normalized_login.is_empty() {
            return false;
        }

        let exists_in_streamers: bool = sqlx::query_scalar!(
            "SELECT EXISTS( \
                SELECT 1 FROM twitch_streamers \
                WHERE ($1 <> '' AND twitch_user_id = $1) \
                   OR ($2 <> '' AND LOWER(twitch_login) = $2) \
             ) AS \"exists_in_streamers!\"",
            target_id,
            &normalized_login,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, login, "403-Stale-Purge: twitch_streamers nicht ladbar");
            true
        });

        let is_partner_active: bool = sqlx::query_scalar!(
            "SELECT COALESCE(MAX(COALESCE(is_partner_active, 0)), 0) <> 0 AS \"is_partner_active!\" \
             FROM twitch_streamers_partner_state \
             WHERE ($1 <> '' AND twitch_user_id = $1) \
                OR ($2 <> '' AND LOWER(twitch_login) = $2)",
            target_id,
            &normalized_login,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, login, "403-Stale-Purge: Partner-State nicht ladbar");
            true
        });

        let has_raid_auth: bool = sqlx::query_scalar!(
            "SELECT EXISTS( \
                SELECT 1 FROM twitch_raid_auth \
                WHERE ($1 <> '' AND twitch_user_id = $1) \
                   OR ($2 <> '' AND LOWER(twitch_login) = $2) \
             ) AS \"has_raid_auth!\"",
            target_id,
            &normalized_login,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, login, "403-Stale-Purge: Raid-Auth nicht ladbar");
            true
        });

        !exists_in_streamers && !is_partner_active && !has_raid_auth
    }

    /// 403-Selbstheilung im Chat-Pfad (Python `connection.py:1558-1624`): Bot
    /// als Mod setzen, 1s auf die Twitch-Propagation warten, einmal
    /// re-subscriben. Bei Erfolg wird die Sub getrackt und `true` geliefert;
    /// scheitert Re-Mod ODER der Re-Subscribe, wird ein 10-Min-Cooldown gesetzt
    /// (clearbar, kein permanenter perm_failed) und `false` geliefert.
    async fn retry_chat_subscription_after_mod(
        &self,
        sub_type: &str,
        version: &str,
        condition: &serde_json::Value,
        broadcaster_id: &str,
        login: &str,
        bearer_override: Option<&str>,
    ) -> bool {
        let Some(provisioner) = self.moderator_provisioner.as_ref() else {
            // Defensive: ohne Provisioner gibt es keinen Mod-Retry — Cooldown
            // statt perm_failed, damit der nächste Zyklus es erneut versucht.
            self.set_mod_retry_cooldown(sub_type, broadcaster_id);
            return false;
        };
        tracing::info!(
            sub_type,
            login,
            "EventSub 403: versuche Bot automatisch als Mod zu setzen"
        );
        match provisioner
            .ensure_bot_is_mod_outcome(broadcaster_id, login)
            .await
        {
            ModeratorProvisionOutcome::Ready => {}
            ModeratorProvisionOutcome::RetryLater => {
                self.set_mod_retry_cooldown(sub_type, broadcaster_id);
                tracing::warn!(
                    sub_type,
                    login,
                    "EventSub 403: Re-Mod fehlgeschlagen — 10-Min-Cooldown (Retry danach)"
                );
                return false;
            }
            ModeratorProvisionOutcome::BotBanned => {
                self.mark_perm_failed(sub_type, broadcaster_id);
                tracing::info!(
                    sub_type,
                    login,
                    cooldown_seconds = PERMISSION_RETRY_COOLDOWN_SECONDS as u64,
                    "EventSub 403: Bot ist im Kanal gebannt — Re-Mod bis Unban/Reauth ausgesetzt"
                );
                return false;
            }
        }
        // Kurze Pause, damit Twitch den Mod-Status propagiert (Python: sleep 1s).
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        match self
            .transport
            .create(
                sub_type,
                version,
                condition,
                &self.config.callback_url,
                &self.config.secret,
                bearer_override,
            )
            .await
        {
            Ok(_) => {
                self.track(sub_type, broadcaster_id);
                self.mod_retry_cooldown
                    .lock()
                    .expect("mod_retry_cooldown lock")
                    .remove(&(sub_type.to_string(), broadcaster_id.to_string()));
                tracing::info!(
                    sub_type,
                    login,
                    "EventSub: Re-Subscribe nach Mod-Autorisierung erfolgreich"
                );
                true
            }
            Err(error) => {
                self.record_subscription_failure(
                    sub_type,
                    broadcaster_id,
                    login,
                    &error,
                    1,
                    Some((self.clock)() + MOD_RETRY_COOLDOWN_SECONDS),
                );
                self.set_mod_retry_cooldown(sub_type, broadcaster_id);
                tracing::warn!(
                    %error,
                    sub_type,
                    login,
                    "EventSub 403: Re-Subscribe nach Re-Mod fehlgeschlagen — 10-Min-Cooldown"
                );
                false
            }
        }
    }

    /// Räumt verwaiste Subscriptions unserer Callback-URL ab
    /// (Python `_cleanup_old_eventsub_subscriptions`): Ziel-Broadcaster
    /// nicht mehr aktiv → löschen. Liefert die Anzahl gelöschter Subs.
    ///
    /// WIRING-TODO(P2.10): In `bin/tb-bot/src/main.rs` oder der passenden
    /// Chat/EventSub-Composition alle ~300s mit der aktiven Broadcaster-ID-Menge
    /// spawnen. Dieses Crate enthält nur die purge-/deletefähige Logik.
    pub async fn cleanup_stale(&self, active_user_ids: &HashSet<String>) -> usize {
        if active_user_ids.is_empty() {
            tracing::warn!(
                "EventSub-Cleanup: aktives Broadcaster-Set leer, Cleanup fail-open übersprungen"
            );
            return 0;
        }

        let subs = match self.transport.list().await {
            Ok(subs) => subs,
            Err(error) => {
                tracing::debug!(%error, "EventSub-Cleanup: Liste nicht ladbar");
                return 0;
            }
        };
        let mut deleted = 0;
        for sub in subs {
            let callback = sub.callback.as_deref().map(str::trim).unwrap_or("");
            if callback.is_empty() {
                continue;
            }
            let target = sub.broadcaster_user_id.as_deref().unwrap_or("");
            let current_callback = callback == self.config.callback_url.as_str();
            let active_target = !target.is_empty() && active_user_ids.contains(target);
            if current_callback && active_target {
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

    /// Entfernt eine Subscription aus dem In-Memory-Tracking (und löscht einen
    /// etwaigen `perm_failed`-Eintrag), sodass der nächste Reconcile-Zyklus
    /// (`ensure_core_subscriptions`/`ensure_chat_subscriptions`) sie neu anlegt
    /// statt sie wegen `is_tracked` zu überspringen.
    ///
    /// Port des Python-`_eventsub_untrack_sub` (eventsub_mixin.py:1318-1344):
    /// reine Laufzeit-Selbstheilung bei Webhook-Revocation — ohne dieses Untrack
    /// bliebe eine von Twitch widerrufene Sub bis zum Prozess-Neustart als
    /// „subscribed" markiert und Events gingen still verloren. Liefert `true`,
    /// wenn ein Tracking-Eintrag tatsächlich entfernt wurde.
    pub fn untrack(&self, sub_type: &str, broadcaster_id: &str) -> bool {
        let key = (sub_type.to_string(), broadcaster_id.to_string());
        // 403-Bann zurücksetzen: nach einer Revocation ist die alte Sperre
        // hinfällig, der Re-Subscribe-Versuch soll wieder laufen dürfen.
        if let Ok(mut perm_failed) = self.perm_failed.lock() {
            perm_failed.remove(&key);
        }
        self.clear_subscription_failure(sub_type, broadcaster_id);
        let removed = self.tracked.lock().expect("tracked lock").remove(&key);
        if removed {
            tracing::info!(
                sub_type,
                broadcaster_id,
                "EventSub: Subscription nach Revocation untracked → Resubscribe beim nächsten Reconcile"
            );
        }
        removed
    }

    /// Kapazitäts-Snapshot fürs Admin-Dashboard. Webhook-Modus: keine
    /// WS-Listener — Listener-Felder 0, `used_slots` = getrackte Subs.
    pub async fn record_capacity_snapshot(&self, trigger: &str) {
        let (used, subscriptions_json) = self.capacity_snapshot_payload();
        if let Err(error) = self
            .capacity
            .record(trigger, used, &subscriptions_json, Utc::now())
            .await
        {
            tracing::debug!(%error, trigger, "Capacity-Snapshot fehlgeschlagen");
        }
    }

    fn capacity_snapshot_payload(&self) -> (i32, String) {
        let mut pairs: Vec<(String, String)> = self
            .tracked
            .lock()
            .expect("tracked lock")
            .iter()
            .cloned()
            .collect();
        pairs.sort();
        let subscriptions: Vec<Value> = pairs
            .iter()
            .map(|(sub_type, broadcaster_id)| {
                serde_json::json!({
                    "id": format!("{sub_type}:{broadcaster_id}"),
                    "type": sub_type,
                    "status": "enabled",
                    "transport": "webhook",
                    "condition": {
                        "broadcaster_user_id": broadcaster_id,
                    },
                })
            })
            .collect();
        let json = serde_json::to_string(&subscriptions).unwrap_or_else(|error| {
            tracing::debug!(%error, "EventSub-Snapshot-JSON konnte nicht serialisiert werden");
            "[]".to_string()
        });
        (saturating_i32(pairs.len() as i64), json)
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

        let (used, subscriptions_json) = self.capacity_snapshot_payload();
        if let Err(error) = self
            .capacity
            .record(trigger, used, &subscriptions_json, Utc::now())
            .await
        {
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

    /// Pool-Handle für andere DB-gestützte Helfer im selben Crate
    /// (z. B. das Passive-Lurker-Gate des [`SubscriptionManager`]).
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn record(
        &self,
        trigger: &str,
        used_slots: i32,
        subscriptions_json: &str,
        ts: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let capacity = eventsub_webhook_capacity_values(i64::from(used_slots));
        let total_slots = saturating_i32(capacity.total_slots);
        let headroom_slots = saturating_i32(capacity.headroom_slots);
        let listeners_at_limit = saturating_i32(capacity.listeners_at_limit);
        sqlx::query(
            r#"
            INSERT INTO twitch_eventsub_capacity_snapshot
                (ts_utc, trigger_reason, listener_count, ready_listeners,
                 failed_listeners, used_slots, total_slots, headroom_slots,
                 listeners_at_limit, utilization_pct, listeners_json)
            VALUES ($1, $2, 0, 0, 0, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(ts)
        .bind(trigger)
        .bind(used_slots)
        .bind(total_slots)
        .bind(headroom_slots)
        .bind(listeners_at_limit)
        .bind(capacity.utilization_pct)
        .bind(subscriptions_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Löscht Zeitreihen-Zeilen älter als `cutoff` (Retention, B5-08). Liefert die
    /// Anzahl gelöschter Zeilen.
    pub async fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let rows = sqlx::query!(
            "DELETE FROM twitch_eventsub_capacity_snapshot WHERE ts_utc < $1",
            cutoff,
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::{HashSet, VecDeque};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    async fn pool_in_schema(schema: &str) -> Option<PgPool> {
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return None;
        };
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("admin connect");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("drop schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create schema");
        admin.close().await;

        let opts = PgConnectOptions::from_str(&dsn)
            .expect("dsn parse")
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .expect("connect schema");
        for ddl in [
            "CREATE TABLE twitch_streamers (
                twitch_login TEXT,
                twitch_user_id TEXT
            )",
            "CREATE TABLE twitch_partners (
                twitch_login TEXT,
                twitch_user_id TEXT,
                status TEXT
            )",
            "CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT,
                twitch_user_id TEXT,
                is_partner_active INTEGER DEFAULT 0
            )",
            "CREATE TABLE twitch_raid_auth (
                twitch_login TEXT,
                twitch_user_id TEXT
            )",
            "CREATE TABLE twitch_eventsub_capacity_snapshot (
                ts_utc TIMESTAMPTZ,
                trigger_reason TEXT,
                listener_count INTEGER,
                ready_listeners INTEGER,
                failed_listeners INTEGER,
                used_slots INTEGER,
                total_slots INTEGER,
                headroom_slots INTEGER,
                listeners_at_limit INTEGER,
                utilization_pct DOUBLE PRECISION,
                listeners_json TEXT
            )",
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("test ddl");
        }
        Some(pool)
    }

    #[derive(Debug, Clone)]
    struct CreateCall {
        sub_type: String,
        condition: Value,
        bearer: Option<String>,
    }

    #[derive(Default)]
    struct UnitTransport {
        creates: Mutex<Vec<CreateCall>>,
        fail_all_403: bool,
        fail_bearers_403: Mutex<HashSet<String>>,
        failures: Mutex<VecDeque<SubscriptionCreateError>>,
        auth_refreshes: AtomicU64,
    }

    #[async_trait::async_trait]
    impl SubscriptionTransport for UnitTransport {
        async fn create(
            &self,
            sub_type: &str,
            _version: &str,
            condition: &Value,
            _callback: &str,
            _secret: &str,
            bearer_override: Option<&str>,
        ) -> Result<bool, SubscriptionCreateError> {
            self.creates.lock().expect("creates lock").push(CreateCall {
                sub_type: sub_type.to_string(),
                condition: condition.clone(),
                bearer: bearer_override.map(str::to_string),
            });
            if let Some(error) = self.failures.lock().expect("failures lock").pop_front() {
                return Err(error);
            }
            if self.fail_all_403
                || bearer_override.is_some_and(|bearer| {
                    self.fail_bearers_403
                        .lock()
                        .expect("fail_bearers_403 lock")
                        .contains(bearer)
                })
            {
                return Err(SubscriptionCreateError::http_status(
                    403,
                    None,
                    Some("subscription missing proper authorization".to_string()),
                ));
            }
            Ok(false)
        }

        async fn refresh_auth(
            &self,
            _broadcaster_id: &str,
            _login: &str,
            _bearer_override: Option<&str>,
        ) -> Result<(), SourceError> {
            self.auth_refreshes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn list(&self) -> Result<Vec<RemoteSubscription>, SourceError> {
            Ok(Vec::new())
        }

        async fn delete(&self, _id: &str) -> Result<(), SourceError> {
            Ok(())
        }
    }

    struct UnitBroadcasterTokenProvider {
        calls: AtomicU64,
    }

    #[async_trait::async_trait]
    impl BroadcasterEventSubTokenProvider for UnitBroadcasterTokenProvider {
        async fn eventsub_broadcaster_token(
            &self,
            _broadcaster_id: &str,
            _login: &str,
        ) -> Option<EventSubUserToken> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Some(EventSubUserToken::new(
                "BROADCASTER_TOKEN",
                vec!["moderator:read:followers".to_string()],
            ))
        }
    }

    fn lazy_test_pool() -> PgPool {
        sqlx::PgPool::connect_lazy("postgres://invalid:invalid@127.0.0.1:1/unused")
            .expect("lazy pool")
    }

    fn zero_retry_config(max_attempts: u32) -> SubscriptionRetryConfig {
        SubscriptionRetryConfig {
            max_attempts,
            base_delay: StdDuration::ZERO,
            max_delay: StdDuration::ZERO,
            max_retry_after: StdDuration::ZERO,
            jitter_ms: 0,
        }
    }

    #[tokio::test]
    async fn eventsub_capacity_values_fuellen_total_headroom_und_utilization() {
        let values = eventsub_webhook_capacity_values(25);
        assert_eq!(values.used_slots, 25);
        assert_eq!(values.total_slots, 10_000);
        assert_eq!(values.headroom_slots, 9_975);
        assert_eq!(values.listeners_at_limit, 0);
        assert_eq!(values.utilization_pct, 0.25);
    }

    #[tokio::test]
    async fn create_401_invalidiert_auth_retryt_und_zaehlt_failure() {
        let transport = Arc::new(UnitTransport::default());
        transport.failures.lock().expect("failures lock").push_back(
            SubscriptionCreateError::http_status(
                401,
                None,
                Some("invalid oauth token".to_string()),
            ),
        );
        let manager = SubscriptionManager::new(
            transport.clone(),
            SubscriptionConfig {
                callback_url: "https://cb/test".to_string(),
                secret: "secret".to_string(),
            },
            CapacitySnapshotStore::new(lazy_test_pool()),
        )
        .with_retry_config(zero_retry_config(2));

        assert!(
            manager
                .ensure_subscription("stream.online", "1", "42", "drag")
                .await
        );
        assert_eq!(transport.auth_refreshes.load(Ordering::SeqCst), 1);
        let creates = transport.creates.lock().expect("creates lock").clone();
        assert_eq!(
            creates
                .iter()
                .filter(|call| call.sub_type == "stream.online")
                .count(),
            2,
            "nur die fehlgeschlagene stream.online-Sub wird direkt erneut versucht"
        );
        assert!(manager.failed_subscription_statuses().is_empty());
        let counters = manager.subscription_failure_counters();
        assert!(counters.iter().any(|counter| {
            counter.sub_type == "stream.online"
                && counter.broadcaster_id == "42"
                && counter.reason == "auth_unauthorized"
                && counter.count == 1
        }));
    }

    #[tokio::test]
    async fn create_429_quota_wird_hart_markiert_ohne_retry() {
        let transport = Arc::new(UnitTransport::default());
        transport.failures.lock().expect("failures lock").push_back(
            SubscriptionCreateError::http_status(
                429,
                Some(StdDuration::from_secs(30)),
                Some("maximum total cost exceeded".to_string()),
            ),
        );
        let manager = SubscriptionManager::new(
            transport.clone(),
            SubscriptionConfig {
                callback_url: "https://cb/test".to_string(),
                secret: "secret".to_string(),
            },
            CapacitySnapshotStore::new(lazy_test_pool()),
        )
        .with_retry_config(zero_retry_config(3));

        assert!(!manager.ensure_raid_subscription("55", "target").await);
        let creates = transport.creates.lock().expect("creates lock").clone();
        assert_eq!(creates.len(), 1, "harte Quota wird nicht erneut versucht");
        let statuses = manager.failed_subscription_statuses();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].reason, "rate_limit_quota");
        assert_eq!(statuses[0].http_status, Some(429));
        assert_eq!(statuses[0].retry_after_seconds, Some(30));
        assert!(statuses[0].hard_failure);
    }

    #[tokio::test]
    async fn create_429_maximum_request_rate_bleibt_retrybar() {
        let transport = Arc::new(UnitTransport::default());
        transport.failures.lock().expect("failures lock").push_back(
            SubscriptionCreateError::http_status(
                429,
                None,
                Some("maximum request rate exceeded".to_string()),
            ),
        );
        let manager = SubscriptionManager::new(
            transport.clone(),
            SubscriptionConfig {
                callback_url: "https://cb/test".to_string(),
                secret: "secret".to_string(),
            },
            CapacitySnapshotStore::new(lazy_test_pool()),
        )
        .with_retry_config(zero_retry_config(2));

        assert!(manager.ensure_raid_subscription("55", "target").await);
        let creates = transport.creates.lock().expect("creates lock").clone();
        assert_eq!(
            creates.len(),
            2,
            "transientes Rate-Limit wird erneut versucht"
        );
        assert!(manager.failed_subscription_statuses().is_empty());
        assert!(manager
            .subscription_failure_counters()
            .iter()
            .any(|counter| counter.sub_type == "channel.raid"
                && counter.broadcaster_id == "55"
                && counter.reason == "rate_limited"
                && counter.count == 1));
    }

    #[tokio::test]
    async fn core_ensure_report_meldet_fehlgeschlagene_subscriptions() {
        let transport = Arc::new(UnitTransport::default());
        {
            let mut failures = transport.failures.lock().expect("failures lock");
            for _ in 0..CORE_SUBSCRIPTIONS.len() {
                failures.push_back(SubscriptionCreateError::http_status(
                    403,
                    None,
                    Some("missing authorization".to_string()),
                ));
            }
        }
        let manager = SubscriptionManager::new(
            transport,
            SubscriptionConfig {
                callback_url: "https://cb/test".to_string(),
                secret: "secret".to_string(),
            },
            CapacitySnapshotStore::new(lazy_test_pool()),
        )
        .with_retry_config(zero_retry_config(1));

        let report = manager.ensure_core_subscriptions("66", "core").await;
        assert_eq!(report.attempted, 3);
        assert_eq!(report.succeeded, 0);
        assert_eq!(report.failed(), 3);
        let reasons: HashSet<&str> = report
            .failures
            .iter()
            .map(|failure| failure.reason.as_str())
            .collect();
        assert_eq!(reasons, HashSet::from(["permission_failed"]));
    }

    #[tokio::test]
    async fn permission_403_cooldown_laeuft_aus_und_reconcile_versucht_neu() {
        let now = Arc::new(AtomicU64::new(1_000));
        let now_clk = now.clone();
        let transport = Arc::new(UnitTransport::default());
        transport.failures.lock().expect("failures lock").push_back(
            SubscriptionCreateError::http_status(
                403,
                None,
                Some("missing authorization".to_string()),
            ),
        );
        let manager = SubscriptionManager::new(
            transport.clone(),
            SubscriptionConfig {
                callback_url: "https://cb/test".to_string(),
                secret: "secret".to_string(),
            },
            CapacitySnapshotStore::new(lazy_test_pool()),
        )
        .with_retry_config(zero_retry_config(1))
        .with_clock(Arc::new(move || now_clk.load(Ordering::SeqCst) as f64));

        assert!(!manager.ensure_raid_subscription("77", "target").await);
        assert!(!manager.failed_subscription_statuses().is_empty());
        let creates_after_403 = transport.creates.lock().expect("creates lock").len();
        assert!(!manager.ensure_raid_subscription("77", "target").await);
        assert_eq!(
            transport.creates.lock().expect("creates lock").len(),
            creates_after_403,
            "während 403-Cooldown kein weiterer Create"
        );

        now.store(
            1_000 + PERMISSION_RETRY_COOLDOWN_SECONDS as u64 + 1,
            Ordering::SeqCst,
        );
        assert!(manager.ensure_raid_subscription("77", "target").await);
        assert!(manager.failed_subscription_statuses().is_empty());
    }

    #[tokio::test]
    async fn stale_removed_chat_403_purges_local_state_instead_of_perm_failed() {
        let Some(pool) = pool_in_schema("unit_subs_stale_403").await else {
            return;
        };
        let transport = Arc::new(UnitTransport {
            fail_all_403: true,
            ..Default::default()
        });
        let manager = SubscriptionManager::new(
            transport,
            SubscriptionConfig {
                callback_url: "https://cb/test".to_string(),
                secret: "secret".to_string(),
            },
            CapacitySnapshotStore::new(pool),
        );
        manager.record_subscription_state(
            "Removed",
            "channel.chat.message",
            PASSIVE_LURKER_STATE,
            Some(PASSIVE_LURKER_DETAIL),
        );

        assert!(
            !manager
                .ensure_chat_subscriptions("900", "BOTID", "Removed")
                .await
        );
        assert!(
            !manager.chat_subscriptions_permanently_blocked("900"),
            "stale/removed 403 darf keinen dauerhaften perm_failed-Eintrag behalten"
        );
        assert!(
            manager.chat_subscription_states("removed").is_empty(),
            "stale/removed 403 muss lokalen Subscription-State purgen"
        );
    }

    #[tokio::test]
    async fn moderator_telemetry_403_versucht_broadcaster_token_fallback() {
        let Some(pool) = pool_in_schema("unit_subs_mod_fallback").await else {
            return;
        };
        let transport = Arc::new(UnitTransport::default());
        transport
            .fail_bearers_403
            .lock()
            .expect("fail_bearers_403 lock")
            .insert("BOT_TOKEN".to_string());
        let provider = Arc::new(UnitBroadcasterTokenProvider {
            calls: AtomicU64::new(0),
        });
        let manager = SubscriptionManager::new(
            transport.clone(),
            SubscriptionConfig {
                callback_url: "https://cb/test".to_string(),
                secret: "secret".to_string(),
            },
            CapacitySnapshotStore::new(pool),
        )
        .with_broadcaster_eventsub_token_provider(provider.clone());

        let ensured = manager
            .ensure_moderator_telemetry_subscriptions(
                "555",
                "BOTID",
                "BOT_TOKEN",
                &["moderator:read:followers".to_string()],
                "partner",
            )
            .await;
        assert_eq!(ensured, 1);
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "Broadcaster-Token wird einmal pro Reconcile-Aufruf geladen"
        );

        let creates = transport.creates.lock().expect("creates lock").clone();
        assert_eq!(creates.len(), 2, "Bot-Versuch plus Broadcaster-Fallback");
        assert_eq!(creates[0].sub_type, "channel.follow");
        assert_eq!(creates[0].bearer.as_deref(), Some("BOT_TOKEN"));
        assert_eq!(
            creates[0].condition,
            serde_json::json!({
                "broadcaster_user_id": "555",
                "moderator_user_id": "BOTID",
            })
        );
        let call = &creates[1];
        assert_eq!(call.sub_type, "channel.follow");
        assert_eq!(call.bearer.as_deref(), Some("BROADCASTER_TOKEN"));
        assert_eq!(
            call.condition,
            serde_json::json!({
                "broadcaster_user_id": "555",
                "moderator_user_id": "555",
            })
        );
        assert!(manager
            .tracked_pairs()
            .iter()
            .any(|(sub_type, bid)| { sub_type == "channel.follow" && bid == "555" }));
    }
}
