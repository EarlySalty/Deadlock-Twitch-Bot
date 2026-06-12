//! Promo-Engine — Port von `bot/chat/promos.py` (1679 Z.) und
//! `bot/chat/targeted_promo.py` (282 Z.) nach dem Vertrag
//! `/tmp/welle-b-vertraege/promos.md`.
//!
//! # Drei Trigger-Pfade
//! 1. **per-Message** (`on_message`): jede eingehende Chat-Nachricht prüft
//!    Aktivitätsschwellen (promos.py:1406–1447).
//! 2. **60s-Loop** (`spawn_periodic_loop`): alle 60 Sekunden über Live-Kanäle
//!    (promos.py:1452–1587).
//! 3. **Viewer-Spike** (innerhalb des 60s-Loops): wenn Viewer-Zahl über
//!    Baseline springt (promos.py:1152, 1306).
//!
//! # Doppelsend-Lock (der Fix — promos.py:798)
//! Pro Kanal ein `tokio::sync::Mutex<()>` in einer `DashMap`. Beide Pfade
//! (per-Message und Periodik) halten den Lock während des gesamten
//! Gate-Check+Send-Blocks. Ohne diesen Lock kommt der TOCTOU-Doppelsend zurück.
//!
//! # Monotonic vs. Wall-Clock (promos.py:879–943)
//! In-Memory-Timeouts = `std::time::Instant` (Rust-Äquivalent zu
//! `time.monotonic`). DB-Persistenz = unix-epoch float (`wall_ts`).
//! Restore rekonstruiert Monotonic via Wall-Clock-Offset.
//!
//! # Typ-Disziplin (prod Postgres)
//! - `twitch_promo_cooldowns.wall_ts` = `double precision` → `f64`
//! - `twitch_promo_cooldowns.updated_at` = `TIMESTAMPTZ` → `DateTime<Utc>`
//! - `twitch_live_state.is_live` = `integer` → `i32`
//! - `twitch_live_state.active_session_id` = `bigint` → `Option<i64>`
//! - `streamer_plans.promo_disabled` = `integer` → `i32`
//! - `streamer_plans.lurker_tax_enabled` = `integer` → `i32`
//! - `twitch_session_chatters.seen_via_chatters_api` = `boolean` → `bool`
//! - `twitch_session_chatters.messages` = `integer` → `i32`

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rand::seq::SliceRandom;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::api::ChatApi;
use crate::types::ChatMessageEvent;

// ---------------------------------------------------------------------------
// Konstanten — exakt aus bot/chat/constants.py und targeted_promo.py
// ---------------------------------------------------------------------------

/// Fallback-Intervall Legacy-Pfad (constants.py: _PROMO_INTERVAL_MIN).
#[allow(dead_code)]
const PROMO_INTERVAL_MIN: u64 = 30;
/// Schleifentakt in Sekunden (constants.py: PROMO_LOOP_INTERVAL_SEC).
const PROMO_LOOP_INTERVAL_SEC: u64 = 60;
/// Aktivitätsfenster in Minuten (constants.py: PROMO_ACTIVITY_WINDOW_MIN).
const PROMO_ACTIVITY_WINDOW_MIN: u64 = 8;
/// Mindest-Messages im Aktivitätsfenster (constants.py: PROMO_ACTIVITY_MIN_MSGS).
const PROMO_ACTIVITY_MIN_MSGS: usize = 3;
/// Mindest-unique Chatter im Fenster (constants.py: PROMO_ACTIVITY_MIN_CHATTERS).
const PROMO_ACTIVITY_MIN_CHATTERS: usize = 1;
/// Roh-Nachrichten seit letzter Promo (constants.py: PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO).
const PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO: usize = 16;
/// Ziel-Messages/Minute für Cooldown-Interpolation (constants.py: PROMO_ACTIVITY_TARGET_MPM).
const PROMO_ACTIVITY_TARGET_MPM: f64 = 3.0;
/// Selber Chatter zählt max 1× alle 30s (constants.py: PROMO_ACTIVITY_CHATTER_DEDUP_SEC).
const PROMO_ACTIVITY_CHATTER_DEDUP_SEC: u64 = 30;
/// Minimaler Cooldown in Minuten (constants.py: _PROMO_COOLDOWN_MIN).
const PROMO_COOLDOWN_MIN_MIN: u64 = 45;
/// Maximaler Cooldown in Minuten (constants.py: _PROMO_COOLDOWN_MAX).
const PROMO_COOLDOWN_MAX_MIN: u64 = 180;
/// Absoluter Gesamt-Cooldown in Minuten (constants.py: PROMO_OVERALL_COOLDOWN_MIN).
const PROMO_OVERALL_COOLDOWN_MIN: u64 = 90;
/// Attempt-Lock-Cooldown in Minuten (constants.py: PROMO_ATTEMPT_COOLDOWN_MIN).
const PROMO_ATTEMPT_COOLDOWN_MIN: u64 = 10;
/// Mindest neue Chatter seit letzter Promo (constants.py: PROMO_NEW_CHATTERS_MIN).
const PROMO_NEW_CHATTERS_MIN: usize = 2;
/// Chatter gilt nach 2h wieder als neu (constants.py: PROMO_SEEN_CHATTER_MAX_AGE_SEC).
const PROMO_SEEN_CHATTER_MAX_AGE_SEC: u64 = 7200;
/// Viewer-Spike-Cooldown in Minuten (constants.py: PROMO_VIEWER_SPIKE_COOLDOWN_MIN).
const PROMO_VIEWER_SPIKE_COOLDOWN_MIN: u64 = 60;
/// Chat muss mind. 120s still sein für Spike-Promo (constants.py: PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC).
const PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC: u64 = 120;
/// Spike-Ratio ≥ 1.0 (constants.py: PROMO_VIEWER_SPIKE_MIN_RATIO, ≥1.0 erzwungen).
const PROMO_VIEWER_SPIKE_MIN_RATIO: f64 = 1.0;
/// Mindest historische Sessions für Baseline (constants.py: PROMO_VIEWER_SPIKE_MIN_SESSIONS).
const PROMO_VIEWER_SPIKE_MIN_SESSIONS: i64 = 3;
/// Letzten 20 Sessions für Baseline (constants.py: PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT).
const PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT: i64 = 20;
/// Letzten 240 Stats-Einträge (constants.py: PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT).
const PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT: i64 = 240;
/// Mindest Stats-Samples für Baseline (constants.py: PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES).
const PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES: i64 = 40;
/// Deque-Limit für Aktivitäts-Bucket (promos.py:66: _PROMO_ACTIVITY_BUCKET_MAXLEN).
const PROMO_ACTIVITY_BUCKET_MAXLEN: usize = 2048;
/// State-Verfall in Sekunden (promos.py:67: _PROMO_RUNTIME_STATE_MAX_AGE_SEC).
const PROMO_RUNTIME_STATE_MAX_AGE_SEC: u64 = 86400;
/// Prune-Takt in Sekunden (promos.py:68: _PROMO_RUNTIME_PRUNE_INTERVAL_SEC).
const PROMO_RUNTIME_PRUNE_INTERVAL_SEC: u64 = 60;
/// Fallback-Discord-Invite (constants.py: PROMO_DISCORD_INVITE).
const PROMO_DISCORD_INVITE: &str = "https://discord.gg/z5TfVHuQq2";
/// Scam-Warning-Cooldown in Minuten (constants.py: SCAM_WARNING_COOLDOWN_MIN).
const SCAM_WARNING_COOLDOWN_MIN: u64 = 120;
/// Scam-Warning-Anfangsverzögerung in Minuten (constants.py: SCAM_WARNING_INITIAL_DELAY_MIN).
const SCAM_WARNING_INITIAL_DELAY_MIN: u64 = 20;
/// Stammgast: mind. 10 Messages in 30 Tagen (targeted_promo.py:33–34).
const STAMMGAST_MIN_MESSAGES: i64 = 10;
const STAMMGAST_DAYS: i64 = 30;
/// Kanal-Targeted-Cooldown in Sekunden (targeted_promo.py:37: _CHANNEL_TARGETED_COOLDOWN_SEC).
const CHANNEL_TARGETED_COOLDOWN_SEC: u64 = 900;
/// User-Pitch-Cooldown in Sekunden (targeted_promo.py:36: _USER_PITCH_COOLDOWN_SEC).
const USER_PITCH_COOLDOWN_SEC: u64 = 86400;
/// Lurker-Tax-Freshness in Minuten (promos.py:62: _LURKER_TAX_FRESHNESS_MINUTES).
const LURKER_TAX_FRESHNESS_MINUTES: u64 = 5;
/// Lurker-Tax: mind. 3 frühere Sessions (promos.py:63).
const LURKER_TAX_MIN_PRIOR_SESSIONS: i64 = 3;
/// Lurker-Tax: mind. 240 min Watchtime (promos.py:64).
const LURKER_TAX_MIN_WATCHTIME_MINUTES: f64 = 240.0;
/// Lurker-Tax: max 2 @mentions (promos.py:65).
const LURKER_TAX_MAX_MENTIONS: usize = 2;
/// MiniMax-Timeout in Sekunden (targeted_promo.py:37: _MINIMAX_TIMEOUT_SEC).
const MINIMAX_TIMEOUT_SEC: u64 = 5;
/// Keine Promo in den ersten N Minuten nach Go-Live (constants.py: PROMO_STREAM_START_DELAY_MIN).
const PROMO_STREAM_START_DELAY_MIN: u64 = 10;

// ---------------------------------------------------------------------------
// Promo-Texte — exakt aus constants.py:114–152
// ---------------------------------------------------------------------------

/// Standard-Promo-Texte, kategorisiert. Placeholder `{invite}` wird ersetzt.
/// Port von PROMO_MESSAGES_CATEGORIZED (constants.py).
fn promo_messages_generic() -> Vec<&'static str> {
    vec![
        "Wir haben einen Discord! Komm vorbei, falls du dich mit anderen Deadlock-Spielern vernetzen willst: {invite}",
        "Falls du Deadlock mit anderen spielen willst oder einfach quatschen magst — wir haben eine Community: {invite}",
        "Unser Discord wächst gerade — falls du dabei sein willst: {invite}",
        "Community Discord für Deadlock-Spieler: {invite} — schau gern mal rein",
        "Falls ihr nach Mitspielern sucht oder einfach Deadlock-Content wollt: {invite}",
    ]
}

fn promo_messages_competitive() -> Vec<&'static str> {
    vec![
        "Solo-Queue-Grief nervig? Im Discord findet ihr Leute für Duo/Team-Queue: {invite}",
        "Ranked-Grind läuft besser mit guten Teammates — Discord: {invite}",
        "Meta-Talks, Tier-Listen und Guides gibt's bei uns auf Discord: {invite}",
        "Für Duo-Queue oder einfach um über den letzten Patch zu reden: {invite}",
        "Wir haben einen Competitive-Channel auf Discord — falls ihr Feedback wollt oder sucht: {invite}",
        "Kein Bock mehr auf Solo-Queue-Albtraum? Meld dich im Discord: {invite}",
    ]
}

fn promo_messages_community() -> Vec<&'static str> {
    vec![
        "Wir machen regelmäßig Inhouses — falls ihr mitspielen wollt: {invite}",
        "Nächstes Turnier kommt — Discord für Infos: {invite}",
        "Community-Events, Inhouses, Drafts — alles auf Discord: {invite}",
        "Falls ihr Deadlock-Mates sucht: {invite}",
        "Unsere Community wächst — kommt vorbei: {invite}",
    ]
}

fn promo_messages_growth() -> Vec<&'static str> {
    vec![
        "Neueinsteiger willkommen — Guides und Tipps auf Discord: {invite}",
        "Rank-Grind mit Unterstützung — Community Discord: {invite}",
        "Ob Einsteiger oder Veteran — unser Discord hat für alle was: {invite}",
    ]
}

fn promo_messages_hype() -> Vec<&'static str> {
    vec![
        "Heute so viele Viewer — willkommen alle Neuen! Falls ihr dabei bleiben wollt: {invite}",
        "Schön, so viele hier zu sehen! Community Discord: {invite}",
        "Willkommen, neue Gesichter! Wer mehr Deadlock-Content will: {invite}",
    ]
}

/// Alle Standard-Promo-Texte kombiniert (PROMO_MESSAGES, 22 Einträge gesamt).
fn all_promo_messages() -> Vec<&'static str> {
    let mut all = Vec::new();
    all.extend(promo_messages_generic());
    all.extend(promo_messages_competitive());
    all.extend(promo_messages_community());
    all.extend(promo_messages_growth());
    all.extend(promo_messages_hype());
    all
}

/// Texte für reason="chat_activity": competitive + community + growth.
fn activity_promo_messages() -> Vec<&'static str> {
    let mut pool = Vec::new();
    pool.extend(promo_messages_competitive());
    pool.extend(promo_messages_community());
    pool.extend(promo_messages_growth());
    pool
}

/// Scam-Warning-Texte (constants.py:236–246).
fn scam_warning_texts() -> Vec<&'static str> {
    vec![
        "⚠️ Achtung: „Deadlock Discord Deutschland\" und „Deadlock German Competitiv HUB\" sind NICHT unsere Server und könnten Fake/Scam sein. Unser einziger offizieller Discord: {invite}",
        "⚠️ Vorsicht vor „Deadlock Discord Deutschland\" und „Deadlock German Competitiv HUB\" – das sind nicht wir und könnte Scam sein. Offizieller Discord: {invite}",
    ]
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Abstrahiert die Outbound-Suppression (Mute-Guard). Wird vom Moderations-Modul
/// implementiert. (promos.py:1096: `timeout_guard.is_muted`).
#[async_trait]
pub trait OutboundSuppressionCheck: Send + Sync {
    /// True = Kanal ist aktuell stumm (Mute-Guard aktiv).
    async fn is_muted(&self, channel_login: &str) -> bool;
}

/// Invite-Auflösung pro Kanal. (promos.py:99: `_resolve_streamer_invite`).
/// Der Orchestrator verdrahtet die konkrete Implementierung.
#[async_trait]
pub trait InviteResolver: Send + Sync {
    /// Liefert (invite_url, is_specific). Fallback: PROMO_DISCORD_INVITE, false.
    async fn resolve_invite(&self, channel_login: &str) -> (String, bool);
}

/// Partner-Channel-Check für Chat-Tracking. (promos.py:1422: `is_partner_channel_for_chat_tracking`).
/// UNSICHER: interne Logik nicht aus Python gelesen.
#[async_trait]
pub trait PartnerChannelCheck: Send + Sync {
    async fn is_partner_channel_for_chat_tracking(&self, channel_login: &str) -> bool;
}

/// Preset-Picker für Targeted-Promos (MiniMax oder Random-Fallback).
/// (targeted_promo.py:91: `_pick_preset_with_minimax`).
#[async_trait]
pub trait PresetPicker: Send + Sync {
    /// Wählt das beste Preset aus `presets` anhand von `snippets` (User-Context) und
    /// `target_login`. Bei Timeout/Fehler: random Fallback.
    async fn pick_preset<'a>(
        &self,
        presets: &'a [PromoPreset],
        snippets: &[String],
        target_login: &str,
    ) -> &'a PromoPreset;
}

// ---------------------------------------------------------------------------
// Preset-Typen (promo_presets.py)
// ---------------------------------------------------------------------------

/// Typ eines Promo-Presets (targeted_promo.py: `type="global"` / `type="user"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetType {
    Global,
    User,
}

/// Ein Promo-Preset (promo_presets.py). Text enthält `{invite}` und optional `{login}`.
#[derive(Debug, Clone)]
pub struct PromoPreset {
    pub id: &'static str,
    pub preset_type: PresetType,
    pub text: &'static str,
    pub tags: &'static str,
}

/// 5 globale Presets (promo_presets.py: type="global").
pub fn global_presets() -> Vec<PromoPreset> {
    vec![
        PromoPreset {
            id: "global_community",
            preset_type: PresetType::Global,
            text: "Hier ist unsere Community für alle Deadlock-Spieler: {invite}",
            tags: "community, einladung",
        },
        PromoPreset {
            id: "global_competitive",
            preset_type: PresetType::Global,
            text: "Competitive Deadlock-Spieler gesucht — Discord: {invite}",
            tags: "competitive, ranked",
        },
        PromoPreset {
            id: "global_inhouse",
            preset_type: PresetType::Global,
            text: "Nächste Inhouse-Runde kommt — Infos auf Discord: {invite}",
            tags: "inhouse, event",
        },
        PromoPreset {
            id: "global_new_faces",
            preset_type: PresetType::Global,
            text: "Viele neue Gesichter heute — willkommen! Discord: {invite}",
            tags: "hype, willkommen",
        },
        PromoPreset {
            id: "global_guides",
            preset_type: PresetType::Global,
            text: "Guides und Meta-Diskussionen auf Discord: {invite}",
            tags: "guides, meta",
        },
    ]
}

/// 5 user-spezifische Presets (promo_presets.py: type="user"). Enthalten `{login}`.
pub fn user_presets() -> Vec<PromoPreset> {
    vec![
        PromoPreset {
            id: "user_duo",
            preset_type: PresetType::User,
            text: "@{login} du wirkst wie jemand der gerne Duo spielt — Discord: {invite}",
            tags: "duo, social",
        },
        PromoPreset {
            id: "user_ranked",
            preset_type: PresetType::User,
            text: "@{login} falls du rank pushen willst — bei uns sind gute Leute: {invite}",
            tags: "ranked, competitive",
        },
        PromoPreset {
            id: "user_welcome",
            preset_type: PresetType::User,
            text: "@{login} schön, dass du dabei bist! Community Discord: {invite}",
            tags: "willkommen, community",
        },
        PromoPreset {
            id: "user_event",
            preset_type: PresetType::User,
            text: "@{login} wir haben Turniere und Events — Discord: {invite}",
            tags: "turnier, event",
        },
        PromoPreset {
            id: "user_tips",
            preset_type: PresetType::User,
            text: "@{login} falls du Tipps oder Guides suchst: {invite}",
            tags: "guides, tipps",
        },
    ]
}

// ---------------------------------------------------------------------------
// Default-Implementierungen
// ---------------------------------------------------------------------------

/// Random-Fallback PresetPicker (kein MiniMax — für Tests und ohne KI-Setup).
/// Der Orchestrator injiziert die echte MiniMax-Implementierung.
pub struct RandomPresetPicker;

#[async_trait]
impl PresetPicker for RandomPresetPicker {
    async fn pick_preset<'a>(
        &self,
        presets: &'a [PromoPreset],
        _snippets: &[String],
        _target_login: &str,
    ) -> &'a PromoPreset {
        let mut rng = rand::thread_rng();
        presets.choose(&mut rng).unwrap_or(&presets[0])
    }
}

/// Fallback-InviteResolver: gibt immer den globalen Discord-Invite zurück.
pub struct StaticInviteResolver;

#[async_trait]
impl InviteResolver for StaticInviteResolver {
    async fn resolve_invite(&self, _channel_login: &str) -> (String, bool) {
        (PROMO_DISCORD_INVITE.to_string(), false)
    }
}

/// Fallback-OutboundSuppressionCheck: niemals stumm.
pub struct NoopSuppressionCheck;

#[async_trait]
impl OutboundSuppressionCheck for NoopSuppressionCheck {
    async fn is_muted(&self, _channel_login: &str) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// In-Memory-State pro Kanal
// ---------------------------------------------------------------------------

/// (timestamp, chatter_login) — Aktivitätsbucket-Eintrag (promos.py:730).
type ActivityEntry = (Instant, String);

/// Laufzeit-State eines Kanals.
struct ChannelState {
    /// Aktivitäts-Bucket (deque, maxlen 2048) — promos.py:730.
    activity: VecDeque<ActivityEntry>,
    /// Chatter-Dedup-Map: chatter_login → letzter dedup-Zeitstempel (30s) — promos.py:730.
    chatter_dedupe: HashMap<String, Instant>,
    /// Letzter gesendeter Promo-Text (Anti-Repeat) — promos.py:975.
    last_promo_text: Option<String>,
    /// Letzter gesendeter Scam-Warning-Text (Anti-Repeat) — promos.py:121.
    last_scam_warning_text: Option<String>,
    /// Monotonic-Timestamp letzte Promo — promos.py:1046.
    last_promo_sent: Option<Instant>,
    /// Monotonic-Timestamp letzter Attempt — promos.py:1046.
    last_promo_attempt: Option<Instant>,
    /// Monotonic-Timestamp letzter Viewer-Spike — promos.py:1046.
    last_promo_viewer_spike: Option<Instant>,
    /// Monotonic-Timestamp letzte Scam-Warning — promos.py:1046.
    last_scam_warning_sent: Option<Instant>,
    /// Roh-Nachrichten seit letzter Promo — promos.py:550.
    raw_msg_count_since_promo: usize,
    /// Letztes Chat-Event-Timestamp (für Spike-Silence-Check) — promos.py:550.
    last_raw_chat_message_ts: Option<Instant>,
    /// Gesehene Chatter (mit Timestamp; reset nach 2h) — promos.py:700.
    seen_chatters: HashMap<String, Instant>,
    /// Letzter Zugriff auf diesen State (für Prune) — promos.py:1452.
    last_accessed: Instant,
}

impl ChannelState {
    fn new() -> Self {
        Self {
            activity: VecDeque::with_capacity(64),
            chatter_dedupe: HashMap::new(),
            last_promo_text: None,
            last_scam_warning_text: None,
            last_promo_sent: None,
            last_promo_attempt: None,
            last_promo_viewer_spike: None,
            last_scam_warning_sent: None,
            raw_msg_count_since_promo: 0,
            last_raw_chat_message_ts: None,
            seen_chatters: HashMap::new(),
            last_accessed: Instant::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Targeted-Promo-State (module-level, In-Memory — kein DB-Persist, Reset bei Neustart)
// (targeted_promo.py: module-level dicts)
// ---------------------------------------------------------------------------

struct TargetedState {
    /// (channel_login, user_login) → monotonic ts — targeted_promo.py.
    user_last_pitched: HashMap<(String, String), Instant>,
    /// channel_login → monotonic ts — targeted_promo.py.
    channel_last_targeted: HashMap<String, Instant>,
    /// channel_login → "global" | "user" — Alternierung — targeted_promo.py.
    channel_last_type: HashMap<String, String>,
}

impl TargetedState {
    fn new() -> Self {
        Self {
            user_last_pitched: HashMap::new(),
            channel_last_targeted: HashMap::new(),
            channel_last_type: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// PromoEngine
// ---------------------------------------------------------------------------

/// Die zentrale Promo-Engine.
///
/// # Erzeugung
/// ```ignore
/// let engine = PromoEngine::new(pool, api, suppression);
/// // optional: engine.set_invite_resolver(...)
/// // optional: engine.set_partner_check(...)
/// // optional: engine.set_preset_picker(...)
/// let engine = Arc::new(engine);
/// engine.clone().spawn_periodic_loop();
/// ```
pub struct PromoEngine {
    pool: PgPool,
    api: Arc<dyn ChatApi>,
    suppression: Arc<dyn OutboundSuppressionCheck>,
    invite_resolver: Arc<dyn InviteResolver>,
    partner_check: Arc<dyn PartnerChannelCheck>,
    preset_picker: Arc<dyn PresetPicker>,
    /// Doppelsend-Lock pro Kanal (promos.py:798 — DER Fix).
    send_locks: DashMap<String, Arc<Mutex<()>>>,
    /// Kanal-State (Mutex pro Kanal).
    channel_states: DashMap<String, Mutex<ChannelState>>,
    /// Targeted-State (globales Mutex).
    targeted_state: Mutex<TargetedState>,
}

/// Fallback-PartnerChannelCheck: immer true (für Tests).
struct AlwaysPartner;

#[async_trait]
impl PartnerChannelCheck for AlwaysPartner {
    async fn is_partner_channel_for_chat_tracking(&self, _login: &str) -> bool {
        true
    }
}

/// Prüft ob ein Plan-Key das Entitlement `chat.lurker_tax` trägt (catalog.py:PLAN_ENTITLEMENTS_MAP).
fn plan_id_has_lurker_tax(plan_id: &str) -> bool {
    !matches!(
        plan_id.to_lowercase().as_str(),
        "raid_free" | "free" | "chat_quiet" | "werbefrei" | "quiet" | ""
    )
}

/// Prüft ob ein Plan-Key (canonical oder Legacy) das Entitlement chat.promos.disable trägt.
/// Port von catalog.py:PLAN_ENTITLEMENTS_MAP + LEGACY_PLAN_NAME_TO_ID_MAP.
fn plan_id_has_promos_disable(plan_id: &str) -> bool {
    matches!(
        plan_id.to_lowercase().as_str(),
        // Kanonische Plan-IDs mit chat.promos.disable (catalog.py).
        "chat_quiet"
        | "bundle_chat_quiet_raid_boost"
        | "bundle_analysis_raid_boost"
        | "bundle_werbefrei_analyse"
        | "bundle_komplett"
        // Legacy-Namen die auf diese Plans mappen (LEGACY_PLAN_NAME_TO_ID_MAP).
        | "werbefrei"
        | "quiet"
        | "chat_quiet_bundle"
        | "bundle"
    )
}

impl PromoEngine {
    /// Erzeugt eine neue PromoEngine. Default-Impls: StaticInviteResolver,
    /// AlwaysPartner, RandomPresetPicker — Orchestrator setzt produktive Impls
    /// via `set_*`-Methoden.
    pub fn new(
        pool: PgPool,
        api: Arc<dyn ChatApi>,
        suppression: Arc<dyn OutboundSuppressionCheck>,
    ) -> Self {
        Self {
            pool,
            api,
            suppression,
            invite_resolver: Arc::new(StaticInviteResolver),
            partner_check: Arc::new(AlwaysPartner),
            preset_picker: Arc::new(RandomPresetPicker),
            send_locks: DashMap::new(),
            channel_states: DashMap::new(),
            targeted_state: Mutex::new(TargetedState::new()),
        }
    }

    /// Setzt den InviteResolver (Default: StaticInviteResolver).
    pub fn set_invite_resolver(mut self, r: Arc<dyn InviteResolver>) -> Self {
        self.invite_resolver = r;
        self
    }

    /// Setzt den PartnerChannelCheck.
    pub fn set_partner_check(mut self, c: Arc<dyn PartnerChannelCheck>) -> Self {
        self.partner_check = c;
        self
    }

    /// Setzt den PresetPicker (Default: RandomPresetPicker).
    pub fn set_preset_picker(mut self, p: Arc<dyn PresetPicker>) -> Self {
        self.preset_picker = p;
        self
    }

    /// Per-Message-Pfad: wird für jede eingehende Chat-Nachricht aufgerufen.
    /// (promos.py:1406–1447: `_maybe_send_activity_promo`)
    pub async fn on_message(&self, event: &ChatMessageEvent) {
        let login = event.broadcaster_user_login.to_lowercase();
        let chatter = event.chatter_user_login.to_lowercase();
        let text = event.text();

        // Guard: PROMO_IGNORE_COMMANDS — Nachrichten mit "!" prefix nicht tracken.
        if text.starts_with('!') {
            return;
        }

        // Guard: Partner-Channel-Check (promos.py:1422).
        if !self.partner_check.is_partner_channel_for_chat_tracking(&login).await {
            return;
        }

        // Guard: Channel-Allowlist (leer = alle) — promos.py:78.
        if !self.promo_channel_allowed_db(&login).await {
            return;
        }

        let now = Instant::now();

        // Aktivität aufzeichnen (promos.py:730). Der Raw-Count läuft separat
        // über [`Self::record_raw_message`] — Python bumpt ihn in
        // `_track_chat_health` für ALLE partner-getrackten Nachrichten
        // (inkl. "!"-Commands), nicht nur im Promo-Pfad (moderation.py:2173).
        {
            let state_ref = self.channel_states.entry(login.clone()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_accessed = now;
            self.record_promo_activity_inner(&mut state, &chatter, now);
        }

        // Doppelsend-Lock (promos.py:798).
        let lock = self.get_send_lock(&login);
        let _guard = lock.lock().await;

        // maybe_send_promo_with_stats (promos.py:1281).
        let channel_id = event.broadcaster_user_id.clone();
        self.maybe_send_promo_with_stats(&login, &channel_id, now).await;
    }

    /// Raw-Chat-Aktivität aufzeichnen — Port von `_record_raw_chat_message`
    /// (promos.py:550, aufgerufen aus `_track_chat_health`, moderation.py:2173).
    ///
    /// Zählt JEDE partner-getrackte Nachricht inkl. "!"-Commands — der Zähler
    /// gated Promos gegen tote Chats (`raw_msg_count_since_promo`,
    /// `last_raw_chat_message_ts`). Die Pipeline ruft das im Track-Schritt auf,
    /// exakt an der Python-Stelle (nach Partner-Gate, vor Session-/Game-Gate).
    pub async fn record_raw_message(&self, broadcaster_login: &str) {
        let login = broadcaster_login.to_lowercase();
        if login.is_empty() {
            return;
        }
        let now = Instant::now();
        let state_ref = self
            .channel_states
            .entry(login)
            .or_insert_with(|| Mutex::new(ChannelState::new()));
        let mut state = state_ref.lock().await;
        state.last_accessed = now;
        state.raw_msg_count_since_promo += 1;
        state.last_raw_chat_message_ts = Some(now);
    }

    /// Startet den 60s-periodischen Loop (promos.py:1452).
    pub fn spawn_periodic_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            // Stale Einträge vor dem Laden bereinigen (promos.py:944).
            self.cleanup_stale_promo_cooldowns().await;
            // Cooldowns aus DB laden (promos.py:1452: _restore_promo_cooldowns).
            self.restore_promo_cooldowns().await;

            let mut tick = tokio::time::interval(Duration::from_secs(PROMO_LOOP_INTERVAL_SEC));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut prune_timer = Instant::now();

            loop {
                tick.tick().await;
                let now = Instant::now();

                // Prune alle 60s (promos.py:1452: _prune_promo_runtime_state).
                if now.duration_since(prune_timer).as_secs() >= PROMO_RUNTIME_PRUNE_INTERVAL_SEC {
                    self.prune_promo_runtime_state(now);
                    prune_timer = now;
                }

                self.send_promo_if_due(now).await;
            }
        });
    }

    // -----------------------------------------------------------------------
    // Interne Hilfsmethoden
    // -----------------------------------------------------------------------

    /// Liefert (oder erzeugt) den Doppelsend-Lock für einen Kanal.
    fn get_send_lock(&self, login: &str) -> Arc<Mutex<()>> {
        self.send_locks
            .entry(login.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Aktivitäts-Eintrag aufzeichnen (promos.py:730).
    fn record_promo_activity_inner(&self, state: &mut ChannelState, chatter: &str, now: Instant) {
        // Dedup: selber Chatter max 1× alle 30s.
        if let Some(&last) = state.chatter_dedupe.get(chatter) {
            if now.duration_since(last).as_secs() < PROMO_ACTIVITY_CHATTER_DEDUP_SEC {
                return;
            }
        }
        state.chatter_dedupe.insert(chatter.to_string(), now);

        // Fenster prunen.
        let window = Duration::from_secs(PROMO_ACTIVITY_WINDOW_MIN * 60);
        while state.activity.front().map(|(t, _)| now.duration_since(*t) > window).unwrap_or(false) {
            state.activity.pop_front();
        }

        // Eintrag hinzufügen (FIFO, maxlen 2048).
        if state.activity.len() >= PROMO_ACTIVITY_BUCKET_MAXLEN {
            state.activity.pop_front();
        }
        state.activity.push_back((now, chatter.to_string()));
    }

    /// Periodischer Haupt-Loop-Body (promos.py:1466: `_send_promo_if_due`).
    async fn send_promo_if_due(&self, now: Instant) {
        // Live-Kanäle holen (promos.py:1630).
        let live_channels = match self.get_live_channels_for_promo().await {
            Ok(v) => v,
            Err(e) => {
                warn!("get_live_channels_for_promo fehlgeschlagen: {e}");
                return;
            }
        };

        for (login, channel_id) in &live_channels {
            // Plan-Flag-Check (promos.py:1466: _promo_blocked_by_plan_or_flag).
            if self.promo_blocked_by_plan_or_flag(login).await {
                continue;
            }

            // Lurker-Tax (promos.py:1357 — eigener Pfad, vor Doppelsend-Lock).
            self.maybe_send_lurker_tax_reminder(login, channel_id, now).await;

            // Doppelsend-Lock (promos.py:1466).
            let lock = self.get_send_lock(login);
            let _guard = lock.lock().await;

            // Overall-Ready + Activity-Ready prüfen (promos.py:1466).
            let (overall_ready, activity_ready, invite_opt) = {
                let state_ref = self.channel_states.entry(login.clone()).or_insert_with(|| Mutex::new(ChannelState::new()));
                let state = state_ref.lock().await;
                let overall = self.overall_promo_ready_inner(&state, now);
                let activity = self.promo_activity_ready_inner(&state, now);
                let invite = self.cached_invite_or_none(); // Invite-Auflösung außerhalb des State-Lock
                (overall, activity, invite)
            };

            // Scam+Targeted nur im fälligen Slot (promos.py:1466: activity_ready Pflicht).
            if overall_ready && activity_ready && self.stream_start_delay_ok(login).await {
                let (invite, _is_specific) = self.invite_resolver.resolve_invite(login).await;

                // Scam-Warning-Slot (promos.py:1466).
                if self.maybe_send_scam_warning(login, channel_id, &invite, now, "promo").await {
                    continue;
                }

                // Targeted-Promo-Slot (targeted_promo.py:198).
                let active_chatters = self.get_active_chatters(login).await;
                if self.maybe_send_targeted_promo(login, channel_id, &invite, &active_chatters, now).await {
                    continue;
                }

                // Activity-Promo (promos.py:1466).
                let sent = self.maybe_send_promo_with_stats(login, channel_id, now).await;

                // Viewer-Spike (promos.py:1466).
                if !sent {
                    self.maybe_send_viewer_spike_promo(login, channel_id, now).await;
                }
            }

            let _ = invite_opt; // suppress unused warning
        }
    }

    /// Dummy-Rückgabe (Invite-Auflösung wird per async-Trait gemacht, nicht cached).
    fn cached_invite_or_none(&self) -> Option<()> {
        None
    }

    /// maybe_send_promo_with_stats (promos.py:1281).
    /// Gibt true zurück wenn gesendet.
    async fn maybe_send_promo_with_stats(&self, login: &str, channel_id: &str, now: Instant) -> bool {
        // Guard: Channel-Allowlist.
        if !self.promo_channel_allowed_db(login).await {
            return false;
        }

        // Guard: Stream-Start-Verzögerung (≥10 min nach Go-Live).
        if !self.stream_start_delay_ok(login).await {
            return false;
        }

        // Guard: Overall-Cooldown (≥90 min).
        let (overall_ready, activity_ready, attempt_allowed) = {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            let overall = self.overall_promo_ready_inner(&state, now);
            let activity = self.promo_activity_ready_inner(&state, now);
            let attempt = self.promo_attempt_allowed_inner(&state, now);
            (overall, activity, attempt)
        };

        if !overall_ready || !activity_ready || !attempt_allowed {
            return false;
        }

        // Attempt-Timestamp setzen.
        {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_attempt = Some(now);
        }
        // DB-Persist attempt (promos.py:879: save_promo_cooldown "attempt").
        self.save_promo_cooldown(login, "attempt", Utc::now().timestamp() as f64).await;

        self.send_promo_message(login, channel_id, now, "chat_activity").await
    }

    /// Kernfunktion Promo senden (promos.py:1096: `_send_promo_message`).
    async fn send_promo_message(&self, login: &str, channel_id: &str, now: Instant, reason: &str) -> bool {
        // Suppression-Check (promos.py:1096).
        if self.suppression.is_muted(login).await {
            return false;
        }
        // Plan-Flag (promos.py:1096).
        if self.promo_blocked_by_plan_or_flag(login).await {
            return false;
        }
        // Invite-Auflösung (promos.py:1096).
        let (invite, is_specific) = self.invite_resolver.resolve_invite(login).await;

        // Scam-Warning-Slot (promos.py:1096).
        if (reason == "promo" || reason == "chat_activity")
            && self.maybe_send_scam_warning(login, channel_id, &invite, now, reason).await {
                return true;
            }

        // Promo-Text bauen (promos.py:945).
        let text = self.build_promo_text(login, &invite, reason).await;

        // Announcement senden (promos.py:1096, color="purple").
        let sent = self.api.send_announcement(channel_id, &text, "purple").await.unwrap_or(false);
        if !sent {
            debug!(login, "Promo-Announcement nicht gesendet (Drop/Fehler)");
            // Cooldown NICHT verbrauchen bei Failed-Send (promos.py:1096: if not ok: return False).
            return false;
        }

        // Promo markieren (promos.py:879).
        self.mark_promo_sent(login, now, reason, Utc::now().timestamp() as f64).await;

        if is_specific {
            self.mark_streamer_invite_sent(login).await;
        }

        true
    }

    /// Promo-Text bauen (promos.py:945: `_build_promo_text`).
    async fn build_promo_text(&self, login: &str, invite: &str, reason: &str) -> String {
        // 1. Globalen Promo-Override aus DB prüfen (UNSICHER: promo_mode-Schema).
        if let Some(text) = self.load_global_promo_message(invite).await {
            return text;
        }

        // 2. Streamer-spezifische Promo aus DB (promos.py:945).
        if let Some(text) = self.load_streamer_promo_message(login, invite).await {
            return text;
        }

        // 3. Kategorie-Pool (hardcoded).
        let pool: Vec<&str> = match reason {
            "viewer_spike" => promo_messages_hype(),
            "chat_activity" => activity_promo_messages(),
            _ => all_promo_messages(),
        };

        // Anti-Repeat (promos.py:975).
        let last_text = {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            state.last_promo_text.clone()
        };
        let pool: Vec<&str> = if let Some(ref last) = last_text {
            let filtered: Vec<&str> = pool.iter().copied().filter(|t| *t != last.as_str()).collect();
            if filtered.is_empty() { pool } else { filtered }
        } else {
            pool
        };

        let template = {
            let mut rng = rand::thread_rng();
            pool.choose(&mut rng).copied().unwrap_or("{invite}")
        };

        // Anti-Repeat zurückschreiben (promos.py:975: last_map[login] = chosen).
        {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_text = Some(template.to_string());
        }

        template.replace("{invite}", invite)
    }

    /// Globalen Promo-Override laden (UNSICHER: Tabellen-Schema promo_mode nicht gelesen).
    async fn load_global_promo_message(&self, _invite: &str) -> Option<String> {
        // UNSICHER: Schema der promo_mode-Tabellen nicht aus Python gelesen.
        // Hier Stub: immer None (kein globaler Override).
        None
    }

    /// Streamer-spezifische Promo laden (promos.py:945, streamer_plans.promo_message).
    async fn load_streamer_promo_message(&self, login: &str, invite: &str) -> Option<String> {
        // streamer_plans.promo_message = text (prod schema)
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT promo_message FROM streamer_plans WHERE LOWER(COALESCE(twitch_login,'')) = $1 LIMIT 1",
        )
        .bind(login.to_lowercase())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        if let Some((Some(msg),)) = row {
            if !msg.trim().is_empty() {
                return Some(msg.replace("{invite}", invite));
            }
        }
        None
    }

    /// Promo gesendet markieren (promos.py:879: `_mark_promo_sent`).
    async fn mark_promo_sent(&self, login: &str, now: Instant, reason: &str, wall_ts: f64) {
        {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_sent = Some(now);
            state.raw_msg_count_since_promo = 0;
            self.update_seen_chatters_inner(&mut state, now);
            if reason == "viewer_spike" {
                state.last_promo_viewer_spike = Some(now);
            }
        }
        self.save_promo_cooldown(login, "sent", wall_ts).await;
        if reason == "viewer_spike" {
            self.save_promo_cooldown(login, "viewer_spike", wall_ts).await;
        }
    }

    /// Gesehene Chatter aktualisieren (promos.py:879: `_update_seen_chatters`).
    fn update_seen_chatters_inner(&self, state: &mut ChannelState, now: Instant) {
        for (_, ts) in &state.activity {
            // Chatter aus dem Aktivitäts-Bucket als gesehen markieren.
            let _ = ts;
        }
        let chatters: Vec<String> = state.activity.iter().map(|(_, c)| c.clone()).collect();
        for chatter in chatters {
            state.seen_chatters.insert(chatter, now);
        }
    }

    /// Streamer-Invite als gesendet markieren (promos.py:1096).
    async fn mark_streamer_invite_sent(&self, _login: &str) {
        // UNSICHER: konkrete Tabelle/Mechanismus nicht aus Python gelesen.
    }

    /// Overall-Cooldown-Check (≥90 min seit letzter Promo). (promos.py:1251).
    fn overall_promo_ready_inner(&self, state: &ChannelState, now: Instant) -> bool {
        match state.last_promo_sent {
            None => true,
            Some(last) => now.duration_since(last).as_secs() >= PROMO_OVERALL_COOLDOWN_MIN * 60,
        }
    }

    /// Aktivitätsschwellen-Check (promos.py:1251: `_promo_activity_ready`).
    fn promo_activity_ready_inner(&self, state: &ChannelState, now: Instant) -> bool {
        // 1. Roh-Nachrichten-Minimum.
        if state.raw_msg_count_since_promo < PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO {
            return false;
        }

        // 2. Aktivitätsfenster prüfen.
        let window = Duration::from_secs(PROMO_ACTIVITY_WINDOW_MIN * 60);
        let (msg_count, unique_chatters) = {
            let mut chatters = HashSet::new();
            let mut count = 0usize;
            for (ts, chatter) in &state.activity {
                if now.duration_since(*ts) <= window {
                    count += 1;
                    chatters.insert(chatter.as_str());
                }
            }
            (count, chatters.len())
        };

        if msg_count < PROMO_ACTIVITY_MIN_MSGS {
            return false;
        }
        if unique_chatters < PROMO_ACTIVITY_MIN_CHATTERS {
            return false;
        }

        // 3. Cooldown-Interpolation (promos.py:763–770).
        let window_secs = (PROMO_ACTIVITY_WINDOW_MIN * 60) as f64;
        let msgs_per_min = (msg_count as f64) / (window_secs / 60.0);
        let ratio = (msgs_per_min / PROMO_ACTIVITY_TARGET_MPM).min(1.0);
        let cooldown_sec = ((PROMO_COOLDOWN_MIN_MIN as f64)
            + (1.0 - ratio) * (PROMO_COOLDOWN_MAX_MIN as f64 - PROMO_COOLDOWN_MIN_MIN as f64))
            * 60.0;

        if let Some(last) = state.last_promo_sent {
            if now.duration_since(last).as_secs_f64() < cooldown_sec {
                return false;
            }
        }

        // 4. Neue Chatter ≥ 2 (wenn last_sent gesetzt).
        if state.last_promo_sent.is_some() {
            let new_chatters = self.get_new_chatters_in_window_inner(state, now);
            if new_chatters < PROMO_NEW_CHATTERS_MIN {
                return false;
            }
        }

        true
    }

    /// Neue Chatter im Fenster (promos.py:700: `_get_new_chatters_in_window`).
    fn get_new_chatters_in_window_inner(&self, state: &ChannelState, now: Instant) -> usize {
        let window = Duration::from_secs(PROMO_ACTIVITY_WINDOW_MIN * 60);
        let max_age = Duration::from_secs(PROMO_SEEN_CHATTER_MAX_AGE_SEC);

        let active: HashSet<&str> = state
            .activity
            .iter()
            .filter(|(ts, _)| now.duration_since(*ts) <= window)
            .map(|(_, c)| c.as_str())
            .collect();

        active
            .iter()
            .filter(|&&c| {
                match state.seen_chatters.get(c) {
                    None => true,
                    Some(&last) => now.duration_since(last) > max_age,
                }
            })
            .count()
    }

    /// Attempt-Cooldown-Check (≥10 min). (promos.py:1281).
    fn promo_attempt_allowed_inner(&self, state: &ChannelState, now: Instant) -> bool {
        match state.last_promo_attempt {
            None => true,
            Some(last) => now.duration_since(last).as_secs() >= PROMO_ATTEMPT_COOLDOWN_MIN * 60,
        }
    }

    /// Scam-Warning-Fälligkeit (promos.py:981: `_scam_warning_due`).
    fn scam_warning_due_inner(&self, state: &ChannelState, now: Instant, reason: &str) -> bool {
        // Nur bei "promo" oder "chat_activity".
        if reason != "promo" && reason != "chat_activity" {
            return false;
        }

        match state.last_scam_warning_sent {
            None => {
                // Allererster Aufruf: Seed setzen (Timer gesät, return false).
                // Effekt: Warnung wird fällig nach ≈20 Min (promos.py:981).
                // Wir geben false zurück — das Seed-Setzen passiert im Aufrufer.
                false
            }
            Some(last) => {
                now.duration_since(last).as_secs() >= SCAM_WARNING_COOLDOWN_MIN * 60
            }
        }
    }

    /// Scam-Warning senden (promos.py:1032: `_maybe_send_scam_warning`).
    /// Gibt true zurück wenn gesendet (Slot verbraucht).
    async fn maybe_send_scam_warning(
        &self,
        login: &str,
        channel_id: &str,
        invite: &str,
        now: Instant,
        reason: &str,
    ) -> bool {
        // Seed-Phase: wenn noch nie eine Scam-Warning gesendet wurde, Timer initialisieren
        // und in DB persistieren damit er Neustarts überlebt (promos.py:981: _persist_scam_warning_ts).
        let seed_wall_ts: Option<f64> = {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if state.last_scam_warning_sent.is_none() {
                let initial_delay = Duration::from_secs(SCAM_WARNING_COOLDOWN_MIN * 60)
                    - Duration::from_secs(SCAM_WARNING_INITIAL_DELAY_MIN * 60);
                state.last_scam_warning_sent = now.checked_sub(initial_delay);
                Some((Utc::now().timestamp() as f64) - initial_delay.as_secs_f64())
            } else {
                None
            }
        }; // DashMap-Ref und MutexGuard hier freigegeben — async-safe
        if let Some(wall_ts) = seed_wall_ts {
            self.save_promo_cooldown(login, "scam_warning", wall_ts).await;
            return false;
        }

        // Fälligkeit prüfen.
        let should_send = {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            self.scam_warning_due_inner(&state, now, reason)
        };

        if !should_send {
            return false;
        }

        // Text bauen (Anti-Repeat, promos.py:1032).
        let text = {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            let texts = scam_warning_texts();
            let pool: Vec<&str> = if let Some(ref last) = state.last_scam_warning_text {
                let filtered: Vec<&str> = texts.iter().copied().filter(|t| *t != last.as_str()).collect();
                if filtered.is_empty() { texts } else { filtered }
            } else {
                texts
            };
            let mut rng = rand::thread_rng();
            let template = pool.choose(&mut rng).copied().unwrap_or(scam_warning_texts()[0]);
            template.replace("{invite}", invite)
        };

        // Announcement senden (promos.py:1032, color="orange").
        let _ = self.api.send_announcement(channel_id, &text, "orange").await;

        let wall_ts = Utc::now().timestamp() as f64;

        // Markieren (promos.py:1032).
        self.mark_promo_sent(login, now, reason, wall_ts).await;
        {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_scam_warning_sent = Some(now);
            state.last_scam_warning_text = Some(text);
        }
        self.save_promo_cooldown(login, "scam_warning", wall_ts).await;

        info!(login, "Scam-Warning gesendet");
        true
    }

    /// Viewer-Spike-Promo (promos.py:1306: `_maybe_send_viewer_spike_promo`).
    async fn maybe_send_viewer_spike_promo(&self, login: &str, channel_id: &str, now: Instant) {
        // Guards (promos.py:1306).
        let (overall_ready, has_new_raw, chat_silent, spike_cd_ok, attempt_ok) = {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;

            let overall = self.overall_promo_ready_inner(&state, now);
            let has_raw = state.raw_msg_count_since_promo > 0;
            // Python: activity_age_sec is None → kein Chat → Silence gilt als OK (promos.py:1355).
            // Rust `is_some_and` würde None als false werten → geblockt. Korrekt: None → true.
            let silent = state.last_raw_chat_message_ts.map_or(true, |t| {
                now.duration_since(t).as_secs() >= PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC
            });
            let spike_ok = state.last_promo_viewer_spike.is_none_or(|t| {
                now.duration_since(t).as_secs() >= PROMO_VIEWER_SPIKE_COOLDOWN_MIN * 60
            });
            let attempt = self.promo_attempt_allowed_inner(&state, now);

            (overall, has_raw, silent, spike_ok, attempt)
        };

        if !overall_ready || !has_new_raw || !chat_silent || !spike_cd_ok || !attempt_ok {
            return;
        }

        // Channel-Allowlist.
        if !self.promo_channel_allowed_db(login).await {
            return;
        }

        // Guard: Stream-Start-Verzögerung (≥10 min nach Go-Live).
        if !self.stream_start_delay_ok(login).await {
            return;
        }

        // Spike-Kontext prüfen (promos.py:1152).
        let is_spike = self.get_viewer_spike_context(login).await;
        if !is_spike {
            return;
        }

        // Attempt-Timestamp.
        {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_attempt = Some(now);
        }
        self.save_promo_cooldown(login, "attempt", Utc::now().timestamp() as f64).await;

        self.send_promo_message(login, channel_id, now, "viewer_spike").await;
    }

    /// Viewer-Spike-Erkennung (promos.py:1152: `_get_viewer_spike_context`).
    async fn get_viewer_spike_context(&self, login: &str) -> bool {
        // SQL 1 — Session-Baseline (promos.py:1152).
        let session_baseline: Option<(Option<f64>, Option<i64>)> = sqlx::query_as(
            "SELECT AVG(avg_viewers), COUNT(*)::bigint
               FROM (
                 SELECT avg_viewers FROM twitch_stream_sessions
                  WHERE streamer_login = $1 AND ended_at IS NOT NULL AND avg_viewers > 0
                  ORDER BY started_at DESC LIMIT $2
               ) recent_sessions",
        )
        .bind(login)
        .bind(PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let baseline = if let Some((Some(avg), Some(cnt))) = session_baseline {
            if cnt >= PROMO_VIEWER_SPIKE_MIN_SESSIONS && avg > 0.0 {
                Some(avg)
            } else {
                None
            }
        } else {
            None
        };

        // SQL 2 — Stats-Baseline als Fallback (promos.py:1152).
        let baseline = if baseline.is_none() {
            let stats_baseline: Option<(Option<f64>, Option<i64>)> = sqlx::query_as(
                "SELECT AVG(viewer_count::float), COUNT(*)::bigint
                   FROM (
                     SELECT viewer_count FROM twitch_stats_tracked
                      WHERE LOWER(streamer) = $1 AND viewer_count > 0
                      ORDER BY ts_utc DESC LIMIT $2
                   ) recent_stats",
            )
            .bind(login.to_lowercase())
            .bind(PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            if let Some((Some(avg), Some(cnt))) = stats_baseline {
                if cnt >= PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES && avg > 0.0 {
                    Some(avg)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            baseline
        };

        let Some(baseline) = baseline else {
            return false;
        };

        // SQL 3 — Live-Viewer (promos.py:1152, twitch_live_state.last_viewer_count = integer).
        let live_viewers: Option<(Option<i32>,)> = sqlx::query_as(
            "SELECT last_viewer_count FROM twitch_live_state WHERE streamer_login = $1 AND is_live = 1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let current = match live_viewers {
            Some((Some(v),)) if v > 0 => v as f64,
            _ => return false,
        };

        // Schwelle: current ≥ baseline × 1.0 (promos.py:1152).
        let threshold = baseline * PROMO_VIEWER_SPIKE_MIN_RATIO;
        current >= threshold
    }

    /// Lurker-Tax-Erinnerung (promos.py:1357: `_maybe_send_lurker_tax_reminder`).
    async fn maybe_send_lurker_tax_reminder(&self, login: &str, channel_id: &str, now: Instant) {
        // Guard: Overall-Cooldown (promos.py:1357).
        let overall_ready = {
            let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            self.overall_promo_ready_inner(&state, now)
        };
        if !overall_ready {
            return;
        }

        // Lurker-Tax-Settings prüfen (promos.py:1357: _load_lurker_tax_settings).
        // streamer_plans: lurker_tax_enabled=integer, manual_plan_id=text, plan_name=text (prod schema)
        let settings: Option<(Option<i32>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT p.lurker_tax_enabled,
                    COALESCE(p.manual_plan_id, ''),
                    COALESCE(p.plan_name, '')
               FROM streamer_plans p
              WHERE LOWER(COALESCE(p.twitch_login,'')) = $1
              LIMIT 1",
        )
        .bind(login.to_lowercase())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let (enabled, manual_plan_id, plan_name) = match settings {
            Some((v, m, p)) => (v.unwrap_or(0) != 0, m.unwrap_or_default(), p.unwrap_or_default()),
            None => return,
        };
        if !enabled {
            return;
        }

        // is_paid_plan: Plan muss chat.lurker_tax-Entitlement haben (promos.py:1406).
        let effective_plan = if !manual_plan_id.is_empty() { &manual_plan_id } else { &plan_name };
        if !plan_id_has_lurker_tax(effective_plan) {
            return;
        }

        // has_moderator_read_chatters: Scope muss im Auth-Store vorliegen (promos.py:1410).
        // Prüft twitch_raid_auth.scopes für diesen Streamer.
        let auth_scopes: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT scopes FROM twitch_raid_auth
              WHERE LOWER(COALESCE(twitch_login,'')) = $1
              LIMIT 1",
        )
        .bind(login.to_lowercase())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let scopes_raw = auth_scopes.and_then(|(s,)| s).unwrap_or_default();
        let has_chatters_scope = scopes_raw
            .split_whitespace()
            .any(|s| s.eq_ignore_ascii_case("moderator:read:chatters"));
        if !has_chatters_scope {
            return;
        }

        // Kandidaten holen und Text bauen (promos.py:408).
        let candidates = self.get_lurker_tax_candidates(login).await;
        if candidates.is_empty() {
            return;
        }

        let text = self.build_lurker_tax_text(&candidates);
        let _ = self.api.send_message(channel_id, &text).await;

        // Promo-Slot belegen (promos.py:1357 — lurker_tax nutzt overall-Cooldown).
        self.mark_promo_sent(login, now, "lurker_tax", Utc::now().timestamp() as f64).await;
        info!(login, "Lurker-Tax-Erinnerung gesendet ({} Mentions)", candidates.len());
    }

    /// Lurker-Tax-Kandidaten aus DB (promos.py:408: `_get_lurker_tax_candidates`).
    async fn get_lurker_tax_candidates(&self, login: &str) -> Vec<String> {
        // twitch_session_chatters.seen_via_chatters_api = boolean (prod schema)
        // twitch_session_chatters.messages = integer (prod schema)
        let sql = format!(
            r#"WITH historical_lurks AS (
                SELECT sc.chatter_login,
                       COUNT(DISTINCT sc.session_id) AS prior_lurk_sessions,
                       SUM(EXTRACT(EPOCH FROM (sc.last_seen_at - sc.first_message_at)) / 60.0) AS estimated_lurk_minutes
                  FROM twitch_session_chatters sc
                  JOIN twitch_stream_sessions s ON s.id = sc.session_id
                 WHERE LOWER(sc.streamer_login) = LOWER($1)
                   AND s.ended_at IS NOT NULL
                   AND COALESCE(sc.messages, 0) = 0
                   AND sc.seen_via_chatters_api = TRUE
                 GROUP BY sc.chatter_login
               ),
               live_candidates AS (
                 SELECT sc.chatter_login
                   FROM twitch_session_chatters sc
                   JOIN twitch_live_state ls ON ls.streamer_login = $1 AND ls.active_session_id = sc.session_id
                  WHERE LOWER(sc.streamer_login) = LOWER($1)
                    AND sc.last_seen_at >= NOW() - INTERVAL '{freshness} minutes'
                    AND COALESCE(sc.messages, 0) = 0
                    AND sc.seen_via_chatters_api = TRUE
               )
               SELECT hl.chatter_login
                 FROM historical_lurks hl
                 JOIN live_candidates lc ON lc.chatter_login = hl.chatter_login
                WHERE hl.prior_lurk_sessions >= $2
                  AND hl.estimated_lurk_minutes >= $3
                ORDER BY hl.estimated_lurk_minutes DESC, LOWER(hl.chatter_login) ASC
                LIMIT $4"#,
            freshness = LURKER_TAX_FRESHNESS_MINUTES,
        );
        let rows: Vec<(String,)> = sqlx::query_as(&sql)
        .bind(login)
        .bind(LURKER_TAX_MIN_PRIOR_SESSIONS)
        .bind(LURKER_TAX_MIN_WATCHTIME_MINUTES)
        .bind(LURKER_TAX_MAX_MENTIONS as i64)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter().map(|(l,)| l).collect()
    }

    /// Lurker-Tax-Text bauen (promos.py:401: `_build_lurker_tax_text`).
    fn build_lurker_tax_text(&self, candidates: &[String]) -> String {
        let mentions: String = candidates
            .iter()
            .take(LURKER_TAX_MAX_MENTIONS)
            .map(|l| format!("@{l}"))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "Lurker Steuer: {mentions} falls ihr gerade entspannt mitlest, denkt gern an eure Channel-Points."
        )
    }

    /// Targeted-Promo (targeted_promo.py:198: `maybe_send_targeted_promo`).
    /// Gibt true zurück wenn gesendet.
    async fn maybe_send_targeted_promo(
        &self,
        login: &str,
        channel_id: &str,
        invite: &str,
        active_chatters: &[String],
        now: Instant,
    ) -> bool {
        // Kanal-Cooldown (targeted_promo.py:198).
        let (cd_ok, want_user) = {
            let ts_state = self.targeted_state.lock().await;
            let last = ts_state.channel_last_targeted.get(login).copied();
            let cd = last.is_none_or(|t| now.duration_since(t).as_secs() >= CHANNEL_TARGETED_COOLDOWN_SEC);
            let last_type = ts_state.channel_last_type.get(login).map(|s| s.as_str()).unwrap_or("global");
            let want = last_type == "global"; // alternieren (targeted_promo.py)
            (cd, want)
        };

        if !cd_ok {
            return false;
        }

        // User-Targeted-Pfad (targeted_promo.py:198).
        if want_user && !active_chatters.is_empty() {
            if let Some((target_login, target_id)) = self.pick_user_target(active_chatters, login, now).await {
                // User-Context laden (targeted_promo.py: _sync_user_context_snippets).
                let snippets = self.load_user_context_snippets(&target_id, login).await;
                let presets = user_presets();
                let preset = tokio::time::timeout(
                    Duration::from_secs(MINIMAX_TIMEOUT_SEC),
                    self.preset_picker.pick_preset(&presets, &snippets, &target_login),
                )
                .await
                .unwrap_or_else(|_| {
                    let mut rng = rand::thread_rng();
                    presets.choose(&mut rng).unwrap_or(&presets[0])
                });

                let text = preset.text
                    .replace("{invite}", invite)
                    .replace("{login}", &target_login);
                let _ = self.api.send_message(channel_id, &text).await;

                {
                    let mut ts_state = self.targeted_state.lock().await;
                    ts_state.channel_last_targeted.insert(login.to_string(), now);
                    ts_state.channel_last_type.insert(login.to_string(), "user".to_string());
                    ts_state.user_last_pitched.insert((login.to_string(), target_login), now);
                }
                // Promo-Slot belegen — Python: mark(channel_login, now, reason="targeted_promo").
                self.mark_promo_sent(login, now, "targeted_promo", Utc::now().timestamp() as f64).await;

                return true;
            }
        }

        // Global-Preset-Pfad (targeted_promo.py:198).
        let presets = global_presets();
        let preset = tokio::time::timeout(
            Duration::from_secs(MINIMAX_TIMEOUT_SEC),
            self.preset_picker.pick_preset(&presets, &[], ""),
        )
        .await
        .unwrap_or_else(|_| &presets[0]);

        let text = preset.text.replace("{invite}", invite).replace("{login}", "");
        // Global → Announcement, color="purple" (promo_presets.py).
        let _ = self.api.send_announcement(channel_id, &text, "purple").await;

        {
            let mut ts_state = self.targeted_state.lock().await;
            ts_state.channel_last_targeted.insert(login.to_string(), now);
            ts_state.channel_last_type.insert(login.to_string(), "global".to_string());
        }
        // Promo-Slot belegen — Python: mark(channel_login, now, reason="targeted_promo").
        self.mark_promo_sent(login, now, "targeted_promo", Utc::now().timestamp() as f64).await;

        true
    }

    /// User-Target für Targeted-Promo auswählen (targeted_promo.py: `_pick_user_target`).
    async fn pick_user_target(
        &self,
        active_chatters: &[String],
        channel_login: &str,
        now: Instant,
    ) -> Option<(String, String)> {
        let ts_state = self.targeted_state.lock().await;
        let mut candidates: Vec<&String> = active_chatters
            .iter()
            .filter(|c| {
                // Gepitchte User (< 24h) entfernen.
                let key = (channel_login.to_string(), (*c).clone());
                ts_state.user_last_pitched.get(&key).is_none_or(|&t| {
                    now.duration_since(t).as_secs() >= USER_PITCH_COOLDOWN_SEC
                })
            })
            .collect();
        drop(ts_state);

        // Shuffle, max 6 DB-Checks.
        {
            let mut rng = rand::thread_rng();
            candidates.shuffle(&mut rng);
        }
        candidates.truncate(6);

        for chatter in candidates {
            // chatter_id aus DB (targeted_promo.py: SELECT chatter_id FROM twitch_session_chatters).
            // twitch_session_chatters.chatter_id = text (prod schema)
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT chatter_id FROM twitch_session_chatters
                  WHERE LOWER(chatter_login) = LOWER($1)
                    AND LOWER(streamer_login) = LOWER($2)
                  ORDER BY last_seen_at DESC LIMIT 1",
            )
            .bind(chatter.as_str())
            .bind(channel_login)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            let Some((chatter_id,)) = row else { continue };

            // Stammgast-Check (targeted_promo.py: _sync_is_stammgast).
            // twitch_engagement_conversation: role=text, ts=timestamptz, twitch_user_id=text (prod schema)
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM twitch_engagement_conversation
                  WHERE channel_login = $1 AND twitch_user_id = $2 AND role = 'user'
                    AND ts > NOW() - ($3 || ' days')::INTERVAL",
            )
            .bind(channel_login)
            .bind(&chatter_id)
            .bind(STAMMGAST_DAYS)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

            if count >= STAMMGAST_MIN_MESSAGES {
                continue; // Stammgast → überspringen.
            }

            return Some((chatter.clone(), chatter_id));
        }
        None
    }

    /// User-Context-Snippets laden (targeted_promo.py: `_sync_user_context_snippets`).
    async fn load_user_context_snippets(&self, user_id: &str, channel_login: &str) -> Vec<String> {
        // twitch_engagement_conversation.content = text, ts = timestamptz (prod schema)
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT content FROM twitch_engagement_conversation
              WHERE channel_login = $1 AND twitch_user_id = $2 AND role = 'user'
              ORDER BY ts DESC LIMIT 5",
        )
        .bind(channel_login)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter().map(|(c,)| c).collect()
    }

    /// Aktive Chatter aus dem Aktivitäts-Bucket (promos.py:1466).
    async fn get_active_chatters(&self, login: &str) -> Vec<String> {
        let state_ref = self.channel_states.entry(login.to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
        let state = state_ref.lock().await;
        let now = Instant::now();
        let window = Duration::from_secs(PROMO_ACTIVITY_WINDOW_MIN * 60);
        state
            .activity
            .iter()
            .filter(|(ts, _)| now.duration_since(*ts) <= window)
            .map(|(_, c)| c.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }

    /// Stream-Start-Verzögerung prüfen (constants.py: PROMO_STREAM_START_DELAY_MIN = 10 min).
    /// Verhindert Promos direkt beim Go-Live. Fail-open (true) bei DB-Fehler oder fehlendem Eintrag.
    /// twitch_live_state.last_started_at = TEXT (RFC3339/ISO).
    async fn stream_start_delay_ok(&self, login: &str) -> bool {
        const DELAY_SECS: i64 = (PROMO_STREAM_START_DELAY_MIN * 60) as i64;
        if DELAY_SECS == 0 {
            return true;
        }
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT last_started_at FROM twitch_live_state
              WHERE LOWER(streamer_login) = LOWER($1)
                AND is_live = 1
              LIMIT 1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let Some((Some(started_at_str),)) = row else {
            return true; // fail-open
        };
        let normalized = started_at_str.replace('Z', "+00:00");
        let Ok(started_at) = chrono::DateTime::parse_from_rfc3339(&normalized) else {
            return true; // fail-open
        };
        let age_secs = (Utc::now() - started_at.with_timezone(&Utc)).num_seconds();
        age_secs >= DELAY_SECS
    }

    /// Kanal-Allowlist + Partner-State-Check (promos.py:78: `_promo_channel_allowed`).
    /// twitch_streamers_partner_state.is_partner_active = integer, archived_at = text (prod schema)
    async fn promo_channel_allowed_db(&self, login: &str) -> bool {
        if all_promo_messages().is_empty() {
            return false;
        }

        let row: Option<(Option<i32>, Option<String>)> = sqlx::query_as(
            "SELECT is_partner_active, archived_at
               FROM twitch_streamers_partner_state
              WHERE LOWER(twitch_login) = LOWER($1)
              LIMIT 1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        match row {
            None => false,
            Some((active, archived)) => {
                let is_active = active.unwrap_or(0) != 0;
                let not_archived = archived.is_none();
                is_active && not_archived
            }
        }
    }

    /// Plan-Flag-Check (promos.py:1061: `_promo_blocked_by_plan_or_flag`).
    /// Prüft promo_disabled-Spalte UND Plan-Entitlement chat.promos.disable
    /// (aus manual_plan_id / legacy plan_name, Parität mit Python).
    /// Fail-open bei DB-Fehler.
    async fn promo_blocked_by_plan_or_flag(&self, login: &str) -> bool {
        let row: Option<(Option<i32>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT COALESCE(promo_disabled, 0),
                    COALESCE(manual_plan_id, ''),
                    COALESCE(plan_name, '')
               FROM streamer_plans
              WHERE LOWER(COALESCE(twitch_login,'')) = $1
              LIMIT 1",
        )
        .bind(login.to_lowercase())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        match row {
            None => false, // Fail-open.
            Some((promo_disabled, manual_plan_id, plan_name)) => {
                if promo_disabled.unwrap_or(0) != 0 {
                    return true;
                }
                // Entitlement-Pfad: manual_plan_id hat Vorrang, Fallback auf plan_name.
                let effective_id = manual_plan_id
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or(plan_name.as_deref().filter(|s| !s.is_empty()))
                    .unwrap_or("");
                plan_id_has_promos_disable(effective_id)
            }
        }
    }

    /// Live-Kanäle für Promo-Loop laden (promos.py:1630: `_get_live_channels_for_promo`).
    /// twitch_live_state.is_live = integer, twitch_live_state.last_game = text (prod schema)
    async fn get_live_channels_for_promo(&self) -> Result<Vec<(String, String)>, String> {
        // SUBSCRIPTION_PLANS_ENABLED=True → mit promo_disabled-Filter.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT s.twitch_login, s.twitch_user_id
               FROM twitch_streamer_identities s
               JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
               LEFT JOIN streamer_plans p ON s.twitch_user_id = p.twitch_user_id
              WHERE l.is_live = 1
                AND LOWER(COALESCE(l.last_game, '')) = $1
                AND COALESCE(p.promo_disabled, 0) = 0",
        )
        .bind("deadlock")
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(rows)
    }

    /// Cooldowns aus DB laden (promos.py:1452: `_restore_promo_cooldowns`).
    /// twitch_promo_cooldowns.wall_ts = double precision, login = text, cooldown_type = text (prod schema).
    async fn restore_promo_cooldowns(&self) {
        let rows: Vec<(String, String, f64)> = sqlx::query_as(
            "SELECT login, cooldown_type, wall_ts FROM twitch_promo_cooldowns",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let wall_now = Utc::now().timestamp() as f64;
        let mono_now = Instant::now();

        for (login, cooldown_type, wall_ts) in rows {
            let age_secs = (wall_now - wall_ts).max(0.0) as u64;
            // Monotonic-Zeitstempel rekonstruieren (promos.py:903).
            let mono_restored = mono_now.checked_sub(Duration::from_secs(age_secs));

            let state_ref = self.channel_states.entry(login.clone()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;

            match cooldown_type.as_str() {
                "sent" => {
                    // setdefault: bereits gesetzter Wert bleibt (promos.py:903).
                    if state.last_promo_sent.is_none() {
                        state.last_promo_sent = mono_restored;
                    }
                }
                "attempt" => {
                    if state.last_promo_attempt.is_none() {
                        state.last_promo_attempt = mono_restored;
                    }
                }
                "viewer_spike" => {
                    if state.last_promo_viewer_spike.is_none() {
                        state.last_promo_viewer_spike = mono_restored;
                    }
                }
                "scam_warning"
                    if state.last_scam_warning_sent.is_none() => {
                        state.last_scam_warning_sent = mono_restored;
                    }
                _ => {}
            }
        }
    }

    /// Cooldown in DB speichern (promos.py:879: `save_promo_cooldown`).
    /// twitch_promo_cooldowns PRIMARY KEY (login, cooldown_type), wall_ts = double precision,
    /// updated_at = TIMESTAMPTZ (prod schema — DateTime<Utc> binden, nicht ISO-String).
    async fn save_promo_cooldown(&self, login: &str, cooldown_type: &str, wall_ts: f64) {
        let updated_at: DateTime<Utc> = Utc::now();
        if let Err(e) = sqlx::query(
            "INSERT INTO twitch_promo_cooldowns (login, cooldown_type, wall_ts, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (login, cooldown_type) DO UPDATE
             SET wall_ts = EXCLUDED.wall_ts, updated_at = EXCLUDED.updated_at",
        )
        .bind(login)
        .bind(cooldown_type)
        .bind(wall_ts)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        {
            warn!(login, cooldown_type, "save_promo_cooldown fehlgeschlagen: {e}");
        }
    }

    /// Alte Cooldown-Einträge bereinigen (promos.py: `cleanup_stale_promo_cooldowns(24)`).
    pub async fn cleanup_stale_promo_cooldowns(&self) {
        let cutoff = (Utc::now().timestamp() as f64) - 86400.0;
        if let Err(e) = sqlx::query(
            "DELETE FROM twitch_promo_cooldowns WHERE wall_ts < $1",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await
        {
            warn!("cleanup_stale_promo_cooldowns fehlgeschlagen: {e}");
        }
    }

    /// Stale State-Einträge bereinigen (promos.py:1452: `_prune_promo_runtime_state`).
    fn prune_promo_runtime_state(&self, now: Instant) {
        let max_age = Duration::from_secs(PROMO_RUNTIME_STATE_MAX_AGE_SEC);
        self.channel_states.retain(|_, v| {
            if let Ok(state) = v.try_lock() {
                now.duration_since(state.last_accessed) < max_age
            } else {
                true // Im Lock → behalten.
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SendOutcome;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex as TokioMutex;

    // -----------------------------------------------------------------------
    // Mock-ChatApi
    // -----------------------------------------------------------------------

    #[derive(Default)]
    pub(super) struct MockApi {
        announcements: TokioMutex<Vec<(String, String, String)>>, // (id, text, color)
        messages: TokioMutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, broadcaster_id: &str, message: &str) -> Result<SendOutcome, String> {
            self.messages.lock().await.push((broadcaster_id.to_string(), message.to_string()));
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(&self, broadcaster_id: &str, message: &str, color: &str) -> Result<bool, String> {
            self.announcements.lock().await.push((
                broadcaster_id.to_string(),
                message.to_string(),
                color.to_string(),
            ));
            Ok(true)
        }
        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<crate::api::BanOutcome, String> {
            Ok(crate::api::BanOutcome::Banned)
        }
        async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<crate::api::BanOutcome, String> {
            Ok(crate::api::BanOutcome::Banned)
        }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
            Ok(None)
        }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn bot_user_id(&self) -> String {
            "bot-id".to_string()
        }
    }

    // -----------------------------------------------------------------------
    // Cooldown-Interpolation (promos.py:763–770) — Formel exakt testen
    // -----------------------------------------------------------------------

    fn interpolated_cooldown_sec(msgs_per_min: f64) -> f64 {
        let ratio = (msgs_per_min / PROMO_ACTIVITY_TARGET_MPM).min(1.0);
        ((PROMO_COOLDOWN_MIN_MIN as f64)
            + (1.0 - ratio) * (PROMO_COOLDOWN_MAX_MIN as f64 - PROMO_COOLDOWN_MIN_MIN as f64))
            * 60.0
    }

    #[test]
    fn cooldown_interpolation_max_aktivitaet() {
        // 3.0+ MPM → 45 min (2700s)
        let cd = interpolated_cooldown_sec(3.0);
        assert!((cd - 45.0 * 60.0).abs() < 1.0, "3.0 MPM → 45 min, got {cd}");
    }

    #[test]
    fn cooldown_interpolation_null_aktivitaet() {
        // 0 MPM → 180 min (10800s)
        let cd = interpolated_cooldown_sec(0.0);
        assert!((cd - 180.0 * 60.0).abs() < 1.0, "0 MPM → 180 min, got {cd}");
    }

    #[test]
    fn cooldown_interpolation_mitte() {
        // 1.5 MPM → Mitte: (45 + 0.5*135) * 60 = (45+67.5)*60 = 6750s = 112.5 min
        let cd = interpolated_cooldown_sec(1.5);
        let expected = (45.0 + 0.5 * 135.0) * 60.0;
        assert!((cd - expected).abs() < 1.0, "1.5 MPM → {expected}s, got {cd}");
    }

    #[test]
    fn cooldown_interpolation_cap_ueber_target() {
        // 10.0 MPM → capped auf 3.0 → 45 min
        let cd = interpolated_cooldown_sec(10.0);
        assert!((cd - 45.0 * 60.0).abs() < 1.0, ">3.0 MPM → capped auf 45 min, got {cd}");
    }

    // -----------------------------------------------------------------------
    // Aktivitätsfenster-Logik
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn activity_ready_fehlschlag_bei_zu_wenig_msgs() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        // Nur 1 Eintrag, zu wenig.
        state.activity.push_back((Instant::now(), "user1".to_string()));
        state.raw_msg_count_since_promo = 20;

        let ready = engine.promo_activity_ready_inner(&state, Instant::now());
        assert!(!ready, "Zu wenige Msgs im Fenster → nicht ready");
    }

    #[tokio::test]
    async fn activity_ready_fehlschlag_bei_zu_wenig_raw_msgs() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        let now = Instant::now();
        for i in 0..5usize {
            state.activity.push_back((now, format!("user{i}")));
        }
        state.raw_msg_count_since_promo = 5; // < 16

        let ready = engine.promo_activity_ready_inner(&state, now);
        assert!(!ready, "Zu wenige Roh-Msgs → nicht ready");
    }

    #[tokio::test]
    async fn activity_ready_true_bei_allen_schwellen() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        let now = Instant::now();
        for i in 0..5usize {
            state.activity.push_back((now, format!("user{i}")));
        }
        state.raw_msg_count_since_promo = 20;
        // last_promo_sent = None → keine Cooldown-Prüfung nötig.

        let ready = engine.promo_activity_ready_inner(&state, now);
        assert!(ready, "Alle Schwellen OK → ready");
    }

    #[tokio::test]
    async fn overall_ready_false_wenn_cooldown_noch_aktiv() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        // last_promo_sent = jetzt → nicht ready (89 min < 90 min)
        let now = Instant::now();
        state.last_promo_sent = now.checked_sub(Duration::from_secs(89 * 60));

        let ready = engine.overall_promo_ready_inner(&state, now);
        assert!(!ready, "89 min < 90 min → nicht ready");
    }

    #[tokio::test]
    async fn overall_ready_true_nach_90_min() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        let now = Instant::now();
        state.last_promo_sent = now.checked_sub(Duration::from_secs(91 * 60));

        let ready = engine.overall_promo_ready_inner(&state, now);
        assert!(ready, "91 min ≥ 90 min → ready");
    }

    // -----------------------------------------------------------------------
    // Scam-Warning-Seeding und Cooldown
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn scam_warning_erster_aufruf_gibt_false() {
        let engine = make_engine_no_db();
        let state = ChannelState::new();
        let now = Instant::now();
        // Kein last_scam_warning_sent → Seed-Fall → false.
        let due = engine.scam_warning_due_inner(&state, now, "promo");
        assert!(!due, "Erster Aufruf → Seed, kein Senden");
    }

    #[tokio::test]
    async fn scam_warning_fällig_nach_120_min() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        let now = Instant::now();
        state.last_scam_warning_sent = now.checked_sub(Duration::from_secs(121 * 60));

        let due = engine.scam_warning_due_inner(&state, now, "promo");
        assert!(due, "121 min ≥ 120 min → fällig");
    }

    #[tokio::test]
    async fn scam_warning_nicht_fällig_für_viewer_spike() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        let now = Instant::now();
        state.last_scam_warning_sent = now.checked_sub(Duration::from_secs(200 * 60));

        let due = engine.scam_warning_due_inner(&state, now, "viewer_spike");
        assert!(!due, "viewer_spike → niemals Scam-Warning");
    }

    // -----------------------------------------------------------------------
    // Doppelsend-Lock (TOCTOU-Fix)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn doppelsend_lock_serialisiert_gleichzeitige_aufrufe() {
        let engine = make_engine_no_db();
        let counter = Arc::new(AtomicUsize::new(0));

        let lock = engine.get_send_lock("testkanal");
        let lock2 = engine.get_send_lock("testkanal");

        // Beide Locks sollten identisch sein (gleiche Arc-Instanz).
        assert!(Arc::ptr_eq(&lock, &lock2), "Lock für gleichen Kanal muss identisch sein");

        // Simulieren: zwei Tasks versuchen gleichzeitig zu senden.
        let c1 = counter.clone();
        let l1 = lock.clone();
        let t1 = tokio::spawn(async move {
            let _g = l1.lock().await;
            let v = c1.fetch_add(1, Ordering::SeqCst);
            assert_eq!(v, 0, "Erster Task muss Wert 0 sehen");
        });

        let c2 = counter.clone();
        let l2 = lock.clone();
        let t2 = tokio::spawn(async move {
            let _g = l2.lock().await;
            let v = c2.fetch_add(1, Ordering::SeqCst);
            assert!(v <= 1, "Zweiter Task muss serialisiert sein");
        });

        let _ = tokio::join!(t1, t2);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    // -----------------------------------------------------------------------
    // Lurker-Tax-Text
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn lurker_tax_text_format() {
        let engine = make_engine_no_db();
        let candidates = vec!["alice".to_string(), "bob".to_string()];
        let text = engine.build_lurker_tax_text(&candidates);
        assert!(text.contains("@alice"), "Mention alice fehlt");
        assert!(text.contains("@bob"), "Mention bob fehlt");
        assert!(text.contains("Channel-Points"), "Channel-Points fehlt");
    }

    #[tokio::test]
    async fn lurker_tax_text_max_2_mentions() {
        let engine = make_engine_no_db();
        let candidates = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let text = engine.build_lurker_tax_text(&candidates);
        assert!(!text.contains("@c"), "Mehr als 2 Mentions nicht erlaubt");
    }

    // -----------------------------------------------------------------------
    // Neue Chatter im Fenster
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn neue_chatter_erkennt_unbekannte() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        let now = Instant::now();

        state.activity.push_back((now, "alice".to_string()));
        state.activity.push_back((now, "bob".to_string()));
        // alice als gesehen markieren (frisch → zählt nicht als neu).
        state.seen_chatters.insert("alice".to_string(), now);

        let new_count = engine.get_new_chatters_in_window_inner(&state, now);
        assert_eq!(new_count, 1, "Nur bob ist neu");
    }

    #[tokio::test]
    async fn neue_chatter_erkennt_abgelaufene_als_neu() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        let now = Instant::now();

        state.activity.push_back((now, "alice".to_string()));
        // alice vor 3h gesehen → Alter > 2h → zählt als neu.
        state.seen_chatters.insert(
            "alice".to_string(),
            now.checked_sub(Duration::from_secs(3 * 3600)).unwrap(),
        );

        let new_count = engine.get_new_chatters_in_window_inner(&state, now);
        assert_eq!(new_count, 1, "Alice nach 3h wieder als neu");
    }

    // -----------------------------------------------------------------------
    // Anti-Repeat-Text
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn promo_text_rotiert_anti_repeat() {
        // Testet, dass build_promo_text nicht zweimal denselben Text zurückgibt
        // wenn der letzte gesetzt ist und Pool groß genug ist.
        let engine = make_engine_no_db();
        // Erster Aufruf: kein last_text.
        let text1 = engine.build_promo_text("kanal", PROMO_DISCORD_INVITE, "promo").await;
        // last_text setzen.
        {
            let state_ref = engine.channel_states.entry("kanal".to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_text = Some(text1.clone());
        }
        // Zweiter Aufruf: muss sich unterscheiden (bei Pool-Größe 22 fast sicher).
        // Wir wiederholen mehrmals um Flakiness zu minimieren.
        let mut different = false;
        for _ in 0..20 {
            let text2 = engine.build_promo_text("kanal", PROMO_DISCORD_INVITE, "promo").await;
            if text2 != text1 {
                different = true;
                break;
            }
        }
        assert!(different, "Anti-Repeat: nach 20 Versuchen sollte ein anderer Text kommen");
    }

    // -----------------------------------------------------------------------
    // Hilfsfunktion: Engine ohne DB
    // -----------------------------------------------------------------------

    fn make_engine_no_db() -> PromoEngine {
        use sqlx::postgres::PgPoolOptions;
        // Dummy-Pool — DB-Calls schlagen fehl, aber Unit-Tests brauchen keine DB.
        // connect_lazy → keine echte Verbindung; max_connections ≥ 1 (crossbeam-Queue).
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://localhost/nonexistent")
            .unwrap();
        PromoEngine::new(
            pool,
            Arc::new(MockApi::default()),
            Arc::new(NoopSuppressionCheck),
        )
    }
}

// ---------------------------------------------------------------------------
// DB-Tests (gegen TB_TEST_DATABASE_URL)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use super::*;
    use std::str::FromStr;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
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

        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        apply_ddl(&pool).await;
        pool
    }

    async fn apply_ddl(pool: &PgPool) {
        // prod-treues DDL für alle Tabellen die promos.rs anfasst.
        for ddl in [
            // twitch_promo_cooldowns — wall_ts=double precision, updated_at=TIMESTAMPTZ
            r#"CREATE TABLE twitch_promo_cooldowns (
                login TEXT NOT NULL,
                cooldown_type TEXT NOT NULL,
                wall_ts DOUBLE PRECISION NOT NULL,
                updated_at TIMESTAMPTZ,
                PRIMARY KEY (login, cooldown_type)
            )"#,
            // twitch_streamers_partner_state — is_partner_active=integer, archived_at=text
            r#"CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT NOT NULL,
                twitch_user_id TEXT,
                is_partner_active INTEGER DEFAULT 0,
                archived_at TEXT
            )"#,
            // streamer_plans — promo_disabled=integer, lurker_tax_enabled=integer, promo_message=text
            r#"CREATE TABLE streamer_plans (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                promo_disabled INTEGER DEFAULT 0,
                lurker_tax_enabled INTEGER DEFAULT 0,
                promo_message TEXT
            )"#,
            // twitch_streamer_identities — twitch_user_id=text, twitch_login=text
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT
            )"#,
            // twitch_live_state — is_live=integer, last_game=text, active_session_id=bigint, last_viewer_count=integer
            r#"CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                is_live INTEGER DEFAULT 0,
                last_game TEXT,
                active_session_id BIGINT,
                last_viewer_count INTEGER DEFAULT 0
            )"#,
            // twitch_stream_sessions — avg_viewers=double precision, started_at/ended_at TIMESTAMPTZ
            r#"CREATE TABLE twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                started_at TIMESTAMPTZ DEFAULT NOW(),
                ended_at TIMESTAMPTZ,
                avg_viewers DOUBLE PRECISION DEFAULT 0
            )"#,
            // twitch_stats_tracked — viewer_count=integer, ts_utc=TIMESTAMPTZ, streamer=text
            r#"CREATE TABLE twitch_stats_tracked (
                ts_utc TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                streamer TEXT NOT NULL,
                viewer_count INTEGER DEFAULT 0,
                is_partner BOOLEAN DEFAULT FALSE,
                game_name TEXT,
                stream_title TEXT
            )"#,
            // twitch_session_chatters — seen_via_chatters_api=boolean, messages=integer
            r#"CREATE TABLE twitch_session_chatters (
                session_id BIGINT NOT NULL,
                streamer_login TEXT NOT NULL,
                chatter_login TEXT NOT NULL,
                chatter_id TEXT,
                messages INTEGER DEFAULT 0,
                seen_via_chatters_api BOOLEAN DEFAULT FALSE,
                first_message_at TIMESTAMPTZ DEFAULT NOW(),
                last_seen_at TIMESTAMPTZ DEFAULT NOW()
            )"#,
            // twitch_engagement_conversation — role=text, ts=TIMESTAMPTZ, twitch_user_id=text
            r#"CREATE TABLE twitch_engagement_conversation (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                role TEXT NOT NULL,
                twitch_user_id TEXT NOT NULL,
                twitch_login TEXT,
                content TEXT,
                ts TIMESTAMPTZ DEFAULT NOW()
            )"#,
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    fn make_engine(pool: PgPool) -> PromoEngine {
        PromoEngine::new(
            pool,
            Arc::new(super::tests::MockApi::default()),
            Arc::new(NoopSuppressionCheck),
        )
    }

    // -----------------------------------------------------------------------
    // save/restore Cooldown
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn cooldown_save_und_restore() {
        let pool = pool_or_skip!("promo_cooldown_save");
        let engine = make_engine(pool.clone());

        let wall_ts = 1718000000.0_f64;
        engine.save_promo_cooldown("testkanal", "sent", wall_ts).await;

        let rows: Vec<(String, String, f64)> = sqlx::query_as(
            "SELECT login, cooldown_type, wall_ts FROM twitch_promo_cooldowns",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "testkanal");
        assert_eq!(rows[0].1, "sent");
        assert!((rows[0].2 - wall_ts).abs() < 0.001);
    }

    #[tokio::test]
    async fn cooldown_upsert_idempotent() {
        let pool = pool_or_skip!("promo_cooldown_upsert");
        let engine = make_engine(pool.clone());

        engine.save_promo_cooldown("kanal", "attempt", 1000.0).await;
        engine.save_promo_cooldown("kanal", "attempt", 2000.0).await;

        let wall_ts: f64 = sqlx::query_scalar(
            "SELECT wall_ts FROM twitch_promo_cooldowns WHERE login='kanal' AND cooldown_type='attempt'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!((wall_ts - 2000.0).abs() < 0.001, "Upsert muss neueren Wert schreiben");
    }

    #[tokio::test]
    async fn cooldown_restore_rekonstruiert_monotonic() {
        let pool = pool_or_skip!("promo_cooldown_restore");
        let engine = make_engine(pool.clone());

        // wall_ts = vor 60 Minuten.
        let wall_ts = (Utc::now().timestamp() as f64) - 3600.0;
        engine.save_promo_cooldown("kanal", "sent", wall_ts).await;

        engine.restore_promo_cooldowns().await;

        let state_ref = engine.channel_states.entry("kanal".to_string()).or_insert_with(|| Mutex::new(ChannelState::new()));
        let state = state_ref.lock().await;
        assert!(state.last_promo_sent.is_some(), "Restore muss last_promo_sent setzen");

        // Verify: das Instant liegt ca. 60 min in der Vergangenheit.
        let age = Instant::now().duration_since(state.last_promo_sent.unwrap());
        assert!(age.as_secs() > 3500 && age.as_secs() < 3700, "Age ~60 min, got {age:?}");
    }

    #[tokio::test]
    async fn cleanup_loescht_alte_eintraege() {
        let pool = pool_or_skip!("promo_cooldown_cleanup");
        let engine = make_engine(pool.clone());

        // Alter Eintrag (> 24h).
        let old_ts = (Utc::now().timestamp() as f64) - 90000.0;
        engine.save_promo_cooldown("altkanal", "sent", old_ts).await;

        // Neuer Eintrag.
        let new_ts = Utc::now().timestamp() as f64;
        engine.save_promo_cooldown("neukanal", "sent", new_ts).await;

        engine.cleanup_stale_promo_cooldowns().await;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_promo_cooldowns")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "Alter Eintrag gelöscht, neuer bleibt");

        let login: String = sqlx::query_scalar("SELECT login FROM twitch_promo_cooldowns")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(login, "neukanal");
    }

    // -----------------------------------------------------------------------
    // Channel-Allowlist DB-Check
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn channel_allowed_false_fuer_inaktiven_partner() {
        let pool = pool_or_skip!("promo_channel_allowed");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, archived_at)
             VALUES ('inaktiv', 0, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let allowed = engine.promo_channel_allowed_db("inaktiv").await;
        assert!(!allowed, "is_partner_active=0 → nicht erlaubt");
    }

    #[tokio::test]
    async fn channel_allowed_false_fuer_archivierten_partner() {
        let pool = pool_or_skip!("promo_channel_archived");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, archived_at)
             VALUES ('archiviert', 1, '2026-01-01T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let allowed = engine.promo_channel_allowed_db("archiviert").await;
        assert!(!allowed, "archived_at != NULL → nicht erlaubt");
    }

    #[tokio::test]
    async fn channel_allowed_true_fuer_aktiven_partner() {
        let pool = pool_or_skip!("promo_channel_active");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, archived_at)
             VALUES ('aktiv', 1, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let allowed = engine.promo_channel_allowed_db("aktiv").await;
        assert!(allowed, "is_partner_active=1 + archived_at=NULL → erlaubt");
    }

    // -----------------------------------------------------------------------
    // Plan-Flag-Block
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn plan_flag_blockiert_wenn_promo_disabled_gesetzt() {
        let pool = pool_or_skip!("promo_plan_flag");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_disabled)
             VALUES ('u1', 'blockiert', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let blocked = engine.promo_blocked_by_plan_or_flag("blockiert").await;
        assert!(blocked, "promo_disabled=1 → blockiert");
    }

    #[tokio::test]
    async fn plan_flag_nicht_blockiert_wenn_nicht_gesetzt() {
        let pool = pool_or_skip!("promo_plan_flag_off");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_disabled)
             VALUES ('u2', 'erlaubt', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let blocked = engine.promo_blocked_by_plan_or_flag("erlaubt").await;
        assert!(!blocked, "promo_disabled=0 → nicht blockiert");
    }

    // -----------------------------------------------------------------------
    // Viewer-Spike: Session-Baseline
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn viewer_spike_erkennt_spike_ueber_baseline() {
        let pool = pool_or_skip!("promo_spike_baseline");
        let engine = make_engine(pool.clone());

        // Genug historische Sessions mit avg_viewers=10.
        for i in 0i64..5 {
            sqlx::query(
                "INSERT INTO twitch_stream_sessions (streamer_login, ended_at, avg_viewers)
                 VALUES ($1, NOW() - ($2 || ' days')::INTERVAL, 10.0)",
            )
            .bind("spikekanal")
            .bind(i + 1)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Live: 15 Viewer (> 10 Baseline).
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_viewer_count)
             VALUES ('uid1', 'spikekanal', 1, 15)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let is_spike = engine.get_viewer_spike_context("spikekanal").await;
        assert!(is_spike, "15 > 10 baseline → Spike");
    }

    #[tokio::test]
    async fn viewer_spike_kein_spike_unter_baseline() {
        let pool = pool_or_skip!("promo_spike_no_spike");
        let engine = make_engine(pool.clone());

        for i in 0i64..5 {
            sqlx::query(
                "INSERT INTO twitch_stream_sessions (streamer_login, ended_at, avg_viewers)
                 VALUES ($1, NOW() - ($2 || ' days')::INTERVAL, 20.0)",
            )
            .bind("kein_spike")
            .bind(i + 1)
            .execute(&pool)
            .await
            .unwrap();
        }

        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_viewer_count)
             VALUES ('uid2', 'kein_spike', 1, 10)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let is_spike = engine.get_viewer_spike_context("kein_spike").await;
        assert!(!is_spike, "10 < 20 baseline → kein Spike");
    }

    // -----------------------------------------------------------------------
    // Streamer-Promo-Message aus DB
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn streamer_promo_message_aus_db() {
        let pool = pool_or_skip!("promo_streamer_msg");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_message)
             VALUES ('u3', 'msgkanal', 'Komm zu uns: {invite}')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let text = engine.load_streamer_promo_message("msgkanal", "http://example.com/invite").await;
        assert_eq!(text.as_deref(), Some("Komm zu uns: http://example.com/invite"));
    }

    #[tokio::test]
    async fn lurker_tax_kandidaten_filterung() {
        let pool = pool_or_skip!("promo_lurker");
        let engine = make_engine(pool.clone());

        // Live-Session anlegen (ended_at = NULL → ist live).
        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, avg_viewers)
             VALUES ('lurkerkanal', 10.0)
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Live-State verweist auf die laufende Session.
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('u4', 'lurkerkanal', 1, $1)",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();

        // 3 abgeschlossene historische Sessions für prior_lurk_sessions ≥ 3.
        for s in 0i64..3 {
            let sid: i64 = sqlx::query_scalar(
                "INSERT INTO twitch_stream_sessions (streamer_login, ended_at, avg_viewers)
                 VALUES ('lurkerkanal', NOW() - ($1 || ' hours')::INTERVAL, 5.0)
                 RETURNING id",
            )
            .bind(s + 2)
            .fetch_one(&pool)
            .await
            .unwrap();

            // Lurker-Eintrag in jeder abgeschlossenen Session (messages=0, seen_via_chatters_api=true).
            // estimated_lurk_minutes = (last_seen - first_message) / 60 = 3600/60 = 60 min → 3×60=180 < 240,
            // deswegen spreizen wir es auf 90 min je Session (3×90=270 ≥ 240).
            sqlx::query(
                "INSERT INTO twitch_session_chatters
                 (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
                  first_message_at, last_seen_at)
                 VALUES ($1, 'lurkerkanal', 'lurker1', 'uid-lurker1', 0, TRUE,
                  NOW() - INTERVAL '6 hours', NOW() - INTERVAL '4 hours 30 minutes')",
            )
            .bind(sid)
            .execute(&pool)
            .await
            .unwrap();
        }

        // Aktueller Eintrag (frisch in letzten 5 Min, in der live Session).
        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
              first_message_at, last_seen_at)
             VALUES ($1, 'lurkerkanal', 'lurker1', 'uid-lurker1', 0, TRUE,
              NOW() - INTERVAL '2 minutes', NOW() - INTERVAL '1 minute')",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();

        let candidates = engine.get_lurker_tax_candidates("lurkerkanal").await;
        assert!(!candidates.is_empty(), "Lurker-Kandidat sollte gefunden werden: {candidates:?}");
        assert!(candidates.contains(&"lurker1".to_string()));
    }
}
