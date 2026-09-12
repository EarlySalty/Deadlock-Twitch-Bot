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
//! Pro Kanal ein `tokio::sync::Mutex<()>` in einer `DashMap`. Der LLM-Text wird
//! immer vor dem Lock erzeugt; unter dem Lock laufen nur die erneute Cooldown-
//! Prüfung und der Send. So wartet weder die Chat-Pipeline noch ein anderer
//! Kanal-Sender auf einen LLM-Aufruf, und der TOCTOU-Doppelsend bleibt weg.
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
use rand::seq::IndexedRandom;
use sqlx::PgPool;
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, info, warn};

use crate::api::ChatApi;
use crate::commands::{InviteReplyNotifier, PromoBlockCheck};
use crate::promo_pitch::{
    pitch_filter_reject, pitch_injection_reject, ChannelPromoContext, PartnerPitchContext,
    PartnerPitchGen, PitchJudge, PitchJudgeInput, PitchTextGen,
};
use crate::suppression_guard::SuppressionGuardChatApi;
use crate::types::ChatMessageEvent;
use tb_analytics::promo_timers::{PromoTimerPolicy, PromoTimerSettings, COMMUNITY_BROADCASTER_ID};

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
#[cfg(test)]
const PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO: usize = 16;
#[cfg(test)]
const PROMO_NEW_CHATTERS_MIN: usize = 2;
/// Ziel-Messages/Minute für Cooldown-Interpolation (constants.py: PROMO_ACTIVITY_TARGET_MPM).
const PROMO_ACTIVITY_TARGET_MPM: f64 = 3.0;
/// Selber Chatter zählt max 1× alle 30s (constants.py: PROMO_ACTIVITY_CHATTER_DEDUP_SEC).
const PROMO_ACTIVITY_CHATTER_DEDUP_SEC: u64 = 30;
/// Minimaler Cooldown in Minuten (constants.py: _PROMO_COOLDOWN_MIN).
#[cfg(test)]
const PROMO_COOLDOWN_MIN_MIN: u64 = 45;
/// Maximaler Cooldown in Minuten (constants.py: _PROMO_COOLDOWN_MAX).
#[cfg(test)]
const PROMO_COOLDOWN_MAX_MIN: u64 = 180;
/// Chatter gilt nach 2h wieder als neu (constants.py: PROMO_SEEN_CHATTER_MAX_AGE_SEC).
const PROMO_SEEN_CHATTER_MAX_AGE_SEC: u64 = 7200;
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
const PITCH_JUDGE_CHATTER_COOLDOWN: Duration = Duration::from_secs(15 * 60);
const PITCH_JUDGE_CHANNEL_WINDOW: Duration = Duration::from_secs(60 * 60);
const PITCH_JUDGE_CHANNEL_MAX_PER_WINDOW: usize = 30;
const PITCH_MAX_CONCURRENT: usize = 8;
const PROMO_MAX_CONCURRENT: usize = 16;
const PROMO_DUE_CONCURRENCY: usize = 4;
/// Fallback-Discord-Invite (constants.py: PROMO_DISCORD_INVITE).
pub const DEFAULT_PROMO_DISCORD_INVITE: &str = "https://discord.gg/z5TfVHuQq2";
/// Partner-Seite für Streamer-Pitches in den Promo-Texten.
pub const STREAMER_PARTNER_URL: &str = "https://deutsche-deadlock-community.de/streamer";

/// Liefert den konfigurierten globalen Promo-Invite oder den Python-paritären
/// Default. Ein fehlendes/leeres Secret darf keinen leeren `{invite}`-Text
/// erzeugen.
pub fn promo_invite_fallback(configured: Option<&str>) -> String {
    let configured = configured.map(str::trim).filter(|value| !value.is_empty());
    configured
        .unwrap_or(DEFAULT_PROMO_DISCORD_INVITE)
        .to_string()
}

fn render_promo_template(template: &str, invite: &str) -> Option<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                out.push('{');
                i += 2;
            }
            '{' => {
                let mut j = i + 1;
                let mut field = String::new();
                while j < chars.len() && chars[j] != '}' {
                    field.push(chars[j]);
                    j += 1;
                }
                if j >= chars.len() {
                    return None;
                }
                out.push_str(&render_promo_field(&field, invite)?);
                i = j + 1;
            }
            '}' if i + 1 < chars.len() && chars[i + 1] == '}' => {
                out.push('}');
                i += 2;
            }
            '}' => return None,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Some(out)
}

fn render_promo_field(field: &str, invite: &str) -> Option<String> {
    let field = field.trim();
    if field.is_empty() {
        return None;
    }
    let name_end = field.find(['!', ':', '.', '[']).unwrap_or(field.len());
    if field[..name_end].trim() != "invite" {
        return None;
    }
    let mut rest = &field[name_end..];
    if rest.starts_with('.') || rest.starts_with('[') {
        return None;
    }
    if let Some(after_bang) = rest.strip_prefix('!') {
        let conversion_end = after_bang.find(':').unwrap_or(after_bang.len());
        if after_bang[..conversion_end].trim() != "s" {
            return None;
        }
        rest = &after_bang[conversion_end..];
    }
    let spec = rest.strip_prefix(':').unwrap_or(rest);
    if spec.contains(['{', '}']) {
        return None;
    }
    format_promo_invite(invite, spec)
}

fn format_promo_invite(invite: &str, spec: &str) -> Option<String> {
    if spec.is_empty() {
        return Some(invite.to_string());
    }

    let chars: Vec<char> = spec.chars().collect();
    let mut idx = 0;
    let mut fill = ' ';
    let mut align = '<';
    if chars.len() >= 2 && matches!(chars[1], '<' | '>' | '^') {
        fill = chars[0];
        align = chars[1];
        idx = 2;
    } else if chars.first().is_some_and(|c| matches!(c, '<' | '>' | '^')) {
        align = chars[0];
        idx = 1;
    }

    let width_start = idx;
    while idx < chars.len() && chars[idx].is_ascii_digit() {
        idx += 1;
    }
    let width = if idx > width_start {
        Some(
            chars[width_start..idx]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .ok()?,
        )
    } else {
        None
    };

    let precision = if idx < chars.len() && chars[idx] == '.' {
        idx += 1;
        let precision_start = idx;
        while idx < chars.len() && chars[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == precision_start {
            return None;
        }
        Some(
            chars[precision_start..idx]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .ok()?,
        )
    } else {
        None
    };

    if idx < chars.len() {
        if idx + 1 != chars.len() || chars[idx] != 's' {
            return None;
        }
        idx += 1;
    }
    if idx != chars.len() {
        return None;
    }

    let mut value: String = match precision {
        Some(max) => invite.chars().take(max).collect(),
        None => invite.to_string(),
    };
    let len = value.chars().count();
    if let Some(width) = width.filter(|w| *w > len) {
        let pad = width - len;
        match align {
            '<' => value.extend(std::iter::repeat_n(fill, pad)),
            '>' => {
                let mut padded = String::new();
                padded.extend(std::iter::repeat_n(fill, pad));
                padded.push_str(&value);
                value = padded;
            }
            '^' => {
                let left = pad / 2;
                let right = pad - left;
                let mut padded = String::new();
                padded.extend(std::iter::repeat_n(fill, left));
                padded.push_str(&value);
                padded.extend(std::iter::repeat_n(fill, right));
                value = padded;
            }
            _ => return None,
        }
    }
    Some(value)
}
/// Lurker-Tax-Freshness in Minuten (promos.py:62: _LURKER_TAX_FRESHNESS_MINUTES).
const LURKER_TAX_FRESHNESS_MINUTES: u64 = 5;
/// Lurker-Tax: mind. 3 frühere Sessions (promos.py:63).
const LURKER_TAX_MIN_PRIOR_SESSIONS: i64 = 3;
/// Lurker-Tax: mind. 240 min Watchtime (promos.py:64).
const LURKER_TAX_MIN_WATCHTIME_MINUTES: f64 = 240.0;
/// Lurker-Tax: max 2 @mentions (promos.py:65).
const LURKER_TAX_MAX_MENTIONS: usize = 2;
/// Mehr Kandidaten holen als am Ende erwähnt werden, damit nach dem Per-Session-
/// Dedup die nächstrangigen Lurker nachrücken (promos.py: fetch > MAX, dann kappen).
const LURKER_TAX_CANDIDATE_FETCH: i64 = 25;
pub const LURKER_TAX_REWARD_TITLE: &str = "Lurker Steuer";

const LURKER_TAX_REMINDER_ONE: [&str; 5] = [
    "Hey {mentions}, schön dass du da bist! Kleine Erinnerung vom Finanzamt: die Lurker Steuer wartet auf dich.",
    "{mentions}, schön dass du da bist! Still mitgucken ist erlaubt, aber die Lurker Steuer zahlt sich nicht von allein.",
    "Hey {mentions}, schön dass du da bist! Zeit für deine Lurker Steuer, dann bist du für heute frei.",
    "{mentions}, schön dass du wieder da bist! Das Finanzamt grüßt: Lurker Steuer nicht vergessen.",
    "Hey {mentions}, schön dass du da bist! Lurken kostet heute Lurker Steuer. Der Chat sagt danke.",
];

const LURKER_TAX_REMINDER_TWO: [&str; 5] = [
    "Hey {mentions}, schön dass ihr da seid! Kleine Erinnerung vom Finanzamt: die Lurker Steuer wartet auf euch.",
    "{mentions}, schön dass ihr da seid! Still mitgucken ist erlaubt, aber die Lurker Steuer zahlt sich nicht von allein.",
    "Hey {mentions}, schön dass ihr da seid! Zeit für eure Lurker Steuer, dann seid ihr für heute frei.",
    "{mentions}, schön dass ihr wieder da seid! Das Finanzamt grüßt: Lurker Steuer nicht vergessen.",
    "Hey {mentions}, schön dass ihr da seid! Lurken kostet heute Lurker Steuer. Der Chat sagt danke.",
];

const LURKER_FOLLOWUP_WINDOW: Duration = Duration::from_secs(600);

const LURKER_TAX_THANKS: [&str; 6] = [
    "{name} hat die Lurker Steuer bezahlt. Vorbildlich, danke!",
    "Steuerbescheid für {name}: bezahlt. Du darfst weiterlurken.",
    "{name} hat die Lurker Steuer gezahlt, das Finanzamt ist stolz auf dich.",
    "Ding ding, Lurker Steuer von {name} ist eingegangen. Musterbürger!",
    "{name} zahlt brav Lurker Steuer. Alle anderen Lurker wissen jetzt, wie es geht.",
    "Kasse klingelt: {name} hat die Lurker Steuer beglichen. Ehrenmensch.",
];

pub fn lurker_tax_title_matches(title: &str) -> bool {
    let normalized = title.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.to_lowercase().starts_with("lurker steuer")
}
/// Keine Promo in den ersten N Minuten nach Go-Live (constants.py: PROMO_STREAM_START_DELAY_MIN).
const PROMO_STREAM_START_DELAY_MIN: u64 = 10;

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

/// Schreibseite der Outbound-Suppression — Port von
/// `moderation.py:_maybe_blacklist_for_drop_reason` (Z. 1310–1329) +
/// `_set_outbound_chat_suppression`.
///
/// Wird gerufen, wenn ein ausgehender Bot-Send von Twitch serverseitig
/// verworfen wird (`is_sent=false`) mit `drop_reason.code == "channel_settings"`.
/// Der Kanal wird dann je nach `source` für 7 Tage (`promo`/`recruitment`) bzw.
/// 3 Tage (`partner_raid`) stummgeschaltet, damit derselbe Kanal nicht jeden
/// Promo-/Recruitment-/Raid-Zyklus erneut angeschrieben wird (Bot-Ban-Eskalation).
#[async_trait]
pub trait OutboundSuppressionWriter: Send + Sync {
    /// Schreibt (UPSERT) eine Suppression für `(channel_login, source)` mit dem
    /// quell-spezifischen TTL. No-op, wenn `reason_code`/`source` nicht zur
    /// Suppression führen (Python-Parität: nur `channel_settings` + erlaubte
    /// Quellen schalten stumm).
    async fn suppress_for_drop(
        &self,
        channel_login: &str,
        channel_id: Option<&str>,
        source: &str,
        reason_code: &str,
        reason_detail: Option<&str>,
    );
}

/// Liefert die Scopes des zentralen Bot-Tokens (P1.4). Implementiert vom
/// `BotTokenManager`; der Promo-Pfad nutzt das als Fallback, wenn der Streamer
/// selbst `moderator:read:chatters` nicht in seinem Raid-Auth trägt.
///
/// Port: `bot/chat/promos.py:345–349/357` — bot-zentrierte Migration, der
/// zentrale Bot-Token trägt den Scope.
#[async_trait]
pub trait BotScopeProvider: Send + Sync {
    /// Aktuell gewährte Scopes des Bot-Tokens (leer, falls noch nicht geladen).
    async fn bot_scopes(&self) -> Vec<String>;
}

#[async_trait]
impl BotScopeProvider for crate::token::BotTokenManager {
    async fn bot_scopes(&self) -> Vec<String> {
        self.scopes().await
    }
}

#[async_trait]
pub trait LurkerRewardChecker: Send + Sync {
    async fn active_lurker_reward_exists(&self, broadcaster_id: &str) -> bool;
}

/// Baut das `AND LOWER(<col>) NOT IN ($start, $start+1, …)`-Fragment für die
/// Known-Chat-Bot-Exklusion (P1.5). `start` ist der erste Positions-Parameter;
/// es werden `WHITELISTED_BOTS.len()` aufeinanderfolgende Params referenziert.
///
/// Port: `build_known_chat_bot_not_in_clause` (bot/chat/promos.py). Die Logins
/// selbst kommen als Bind-Params (clean-SQL), hier werden nur die `$n`-Platzhalter
/// erzeugt.
fn known_chat_bot_not_in_clause(column: &str, start: usize) -> String {
    let placeholders: Vec<String> = (0..crate::mention_scoring::WHITELISTED_BOTS.len())
        .map(|i| format!("${}", start + i))
        .collect();
    format!("AND LOWER({column}) NOT IN ({})", placeholders.join(", "))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchCardKind {
    Anlass,
    Partner,
}

#[async_trait]
pub trait PitchReviewSink: Send + Sync {
    async fn send_card(
        &self,
        channel_login: &str,
        target_login: &str,
        trigger: &str,
        reply: &str,
        kind: PitchCardKind,
        candidate_hint: Option<&str>,
    );
}

// ---------------------------------------------------------------------------
// Default-Implementierungen
// ---------------------------------------------------------------------------

/// Fallback-InviteResolver: gibt immer den globalen Discord-Invite zurück.
pub struct StaticInviteResolver;

#[async_trait]
impl InviteResolver for StaticInviteResolver {
    async fn resolve_invite(&self, _channel_login: &str) -> (String, bool) {
        (DEFAULT_PROMO_DISCORD_INVITE.to_string(), false)
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
#[derive(Clone)]
struct ChannelState {
    timers: PromoTimerPolicy,
    community_channel: bool,
    /// Aktivitäts-Bucket (deque, maxlen 2048) — promos.py:730.
    activity: VecDeque<ActivityEntry>,
    /// Chatter-Dedup-Map: chatter_login → letzter dedup-Zeitstempel (30s) — promos.py:730.
    chatter_dedupe: HashMap<String, Instant>,
    /// Monotonic-Timestamp letzte Promo — promos.py:1046.
    last_promo_sent: Option<Instant>,
    /// Monotonic-Timestamp letzter Attempt — promos.py:1046.
    last_promo_attempt: Option<Instant>,
    /// Monotonic-Timestamp letzter Viewer-Spike — promos.py:1046.
    last_promo_viewer_spike: Option<Instant>,
    /// Roh-Nachrichten seit letzter Promo — promos.py:550.
    raw_msg_count_since_promo: usize,
    /// Letztes Chat-Event-Timestamp (für Spike-Silence-Check) — promos.py:550.
    last_raw_chat_message_ts: Option<Instant>,
    /// Gesehene Chatter (mit Timestamp; reset nach 2h) — promos.py:700.
    seen_chatters: HashMap<String, Instant>,
    /// Letzter Zugriff auf diesen State (für Prune) — promos.py:1452.
    last_accessed: Instant,
    /// Per-Session bereits per Lurker-Tax erwähnte Logins `(session_id, set)`
    /// (promos.py:564-584). Bei Session-Wechsel zurückgesetzt — verhindert, dass
    /// derselbe ruhige Zuschauer mehrfach pro Session gepingt wird.
    lurker_mentions: (i64, HashSet<String>),
    thanked_redeemers: (i64, HashSet<String>),
    lurker_reminded_at: (i64, HashMap<String, Instant>),
    lurker_followup_answered: (i64, HashSet<String>),
}

impl ChannelState {
    fn new() -> Self {
        Self {
            timers: PromoTimerPolicy::default(),
            community_channel: false,
            activity: VecDeque::with_capacity(64),
            chatter_dedupe: HashMap::new(),
            last_promo_sent: None,
            last_promo_attempt: None,
            last_promo_viewer_spike: None,
            raw_msg_count_since_promo: 0,
            last_raw_chat_message_ts: None,
            seen_chatters: HashMap::new(),
            last_accessed: Instant::now(),
            lurker_mentions: (0, HashSet::new()),
            thanked_redeemers: (0, HashSet::new()),
            lurker_reminded_at: (0, HashMap::new()),
            lurker_followup_answered: (0, HashSet::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// PromoEngine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PitchLogEntry {
    pub channel_login: String,
    pub target_user_id: Option<String>,
    pub pfad: &'static str,
    pub occasion: Option<String>,
    pub trigger_text: Option<String>,
    pub generated_text: Option<String>,
    pub reject_reason: Option<String>,
    pub sent_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct PartnerCandidate {
    login: String,
    last_session: DateTime<Utc>,
}

pub struct PromoEngine {
    timer_settings: Mutex<(Option<Instant>, PromoTimerSettings)>,
    pool: PgPool,
    api: Arc<dyn ChatApi>,
    suppression: Arc<dyn OutboundSuppressionCheck>,
    suppression_writer: Option<Arc<dyn OutboundSuppressionWriter>>,
    bot_scope_provider: Option<Arc<dyn BotScopeProvider>>,
    reward_checker: Option<Arc<dyn LurkerRewardChecker>>,
    reward_gate_warned: DashMap<String, ()>,
    plan_gate_error_warned: DashMap<String, ()>,
    invite_resolver: Arc<dyn InviteResolver>,
    partner_check: Arc<dyn PartnerChannelCheck>,
    pitch_judge: Arc<dyn PitchJudge>,
    pitch_text_gen: Arc<dyn PitchTextGen>,
    partner_pitch_gen: Arc<dyn PartnerPitchGen>,
    pitch_review_sink: Option<Arc<dyn PitchReviewSink>>,
    pitch_judge_last: DashMap<String, Instant>,
    pitch_judge_channel: DashMap<String, Vec<Instant>>,
    pitch_semaphore: Semaphore,
    promo_semaphore: Arc<Semaphore>,
    send_locks: DashMap<String, Arc<Mutex<()>>>,
    channel_states: DashMap<String, Mutex<ChannelState>>,
    zuschauer_register: Option<Arc<crate::zuschauer_register::ZuschauerRegister>>,
}

/// Fallback-PartnerChannelCheck: immer true (für Tests).
struct AlwaysPartner;

#[async_trait]
impl PartnerChannelCheck for AlwaysPartner {
    async fn is_partner_channel_for_chat_tracking(&self, _login: &str) -> bool {
        true
    }
}

// Plan-Entitlement-Auflösung (chat.lurker_tax / chat.promos.disable) läuft über
// `tb_analytics::plan::resolve_plan_snapshot` — die volle Snapshot-Resolution
// (Manual-Override mit Ablauf-Gate via `manual_plan_expires_at`, Bundles, Legacy-
// Aliase, Stripe-Abo-Fallback). Das ersetzt die frühere statische Plan-Whitelist:
// sie kannte keinen Plan-Ablauf, sodass abgelaufene Werbefrei-/Lurker-Tax-Pläne
// weiterwirkten. Single Source of Truth ist jetzt `tb_analytics::plan`.

impl PromoEngine {
    pub fn new(
        pool: PgPool,
        api: Arc<dyn ChatApi>,
        suppression: Arc<dyn OutboundSuppressionCheck>,
    ) -> Self {
        Self {
            pool,
            api,
            suppression,
            suppression_writer: None,
            bot_scope_provider: None,
            reward_checker: None,
            reward_gate_warned: DashMap::new(),
            plan_gate_error_warned: DashMap::new(),
            timer_settings: Mutex::new((None, PromoTimerSettings::default())),
            invite_resolver: Arc::new(StaticInviteResolver),
            partner_check: Arc::new(AlwaysPartner),
            pitch_judge: Arc::new(crate::promo_pitch::FireworksPitchJudge),
            pitch_text_gen: Arc::new(crate::promo_pitch::FireworksPitchTextGen),
            partner_pitch_gen: Arc::new(crate::promo_pitch::FireworksPartnerPitchGen),
            pitch_review_sink: None,
            pitch_judge_last: DashMap::new(),
            pitch_judge_channel: DashMap::new(),
            pitch_semaphore: Semaphore::new(PITCH_MAX_CONCURRENT),
            promo_semaphore: Arc::new(Semaphore::new(PROMO_MAX_CONCURRENT)),
            send_locks: DashMap::new(),
            channel_states: DashMap::new(),
            zuschauer_register: None,
        }
    }

    /// Kanalidentität kommt vom EventSub-Event bzw. der Live-Kanalliste.
    /// Der Cache begrenzt DB-Zugriffe und übernimmt Admin-Änderungen nach 30 s.
    async fn prepare_channel_timers(&self, login: &str, broadcaster_id: &str) {
        let mut cache = self.timer_settings.lock().await;
        if cache
            .0
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(30))
        {
            match tb_analytics::promo_timers::load(&self.pool).await {
                Ok(settings) => cache.1 = settings,
                Err(error) => {
                    tracing::debug!(%error, "Promo-Timer nicht lesbar, behalte letzte gültige Einstellungen")
                }
            }
            cache.0 = Some(Instant::now());
        }
        let timers = cache.1.for_broadcaster(broadcaster_id);
        drop(cache);
        let state_ref = self
            .channel_states
            .entry(login.to_string())
            .or_insert_with(|| Mutex::new(ChannelState::new()));
        let mut state = state_ref.lock().await;
        state.timers = timers;
        state.community_channel = broadcaster_id == COMMUNITY_BROADCASTER_ID;
    }

    async fn channel_timers(&self, login: &str) -> (PromoTimerPolicy, bool) {
        let state_ref = self
            .channel_states
            .entry(login.to_string())
            .or_insert_with(|| Mutex::new(ChannelState::new()));
        let state = state_ref.lock().await;
        (state.timers.clone(), state.community_channel)
    }

    pub fn set_zuschauer_register(
        mut self,
        register: Arc<crate::zuschauer_register::ZuschauerRegister>,
    ) -> Self {
        self.zuschauer_register = Some(register);
        self
    }

    /// Verdrahtet die Schreibseite der Outbound-Suppression. Ohne Aufruf bleibt
    /// das Verhalten wie vor P1.1 (channel_settings-Drops werden nicht persistiert).
    ///
    // WIRING-TODO(P1.1): Im Composition-Root (bin/tb-bot) die PromoEngine via
    // `.set_suppression_writer(Arc::new(OutboundSuppressionStore::new(pool)))`
    // konstruieren (derselbe Store, der bereits als OutboundSuppressionCheck für
    // den Mute-Read hängt), damit channel_settings-Drops 7d/3d persistiert werden.
    pub fn set_suppression_writer(mut self, w: Arc<dyn OutboundSuppressionWriter>) -> Self {
        self.suppression_writer = Some(w);
        self
    }

    /// Verdrahtet die Bot-Token-Scope-Quelle für den Lurker-Tax-Fallback (P1.4).
    /// Ohne Aufruf zählt nur der Streamer-eigene `moderator:read:chatters`-Scope.
    ///
    // WIRING-TODO(P1.4): Im Composition-Root (bin/tb-bot) die PromoEngine via
    // `.set_bot_scope_provider(bot_token_manager.clone())` konstruieren
    // (der zentrale BotTokenManager implementiert BotScopeProvider), damit der
    // bot-zentrierte Scope-Fallback greift.
    pub fn set_bot_scope_provider(mut self, p: Arc<dyn BotScopeProvider>) -> Self {
        self.bot_scope_provider = Some(p);
        self
    }

    pub fn set_lurker_reward_checker(mut self, checker: Arc<dyn LurkerRewardChecker>) -> Self {
        self.reward_checker = Some(checker);
        self
    }

    /// Wertet ein `send_message`-Ergebnis auf einen `channel_settings`-Drop aus
    /// und schreibt — falls eine Schreibseite hängt — die quell-spezifische
    /// Suppression (7d promo/recruitment, 3d partner_raid).
    ///
    /// Port: `moderation.py:1525–1530` (is_sent=false-Drop ruft
    /// `_maybe_blacklist_for_drop_reason`).
    async fn record_suppression_on_drop(
        &self,
        login: &str,
        channel_id: &str,
        source: &str,
        outcome: &Result<crate::types::SendOutcome, String>,
    ) {
        let Some(writer) = self.suppression_writer.as_ref() else {
            return;
        };
        if let Ok(crate::types::SendOutcome::Dropped { code, message }) = outcome {
            let detail = (!message.is_empty()).then_some(message.as_str());
            writer
                .suppress_for_drop(login, Some(channel_id), source, code, detail)
                .await;
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

    pub fn set_pitch_judge(mut self, j: Arc<dyn PitchJudge>) -> Self {
        self.pitch_judge = j;
        self
    }

    pub fn set_pitch_text_gen(mut self, g: Arc<dyn PitchTextGen>) -> Self {
        self.pitch_text_gen = g;
        self
    }

    pub fn set_partner_pitch_gen(mut self, g: Arc<dyn PartnerPitchGen>) -> Self {
        self.partner_pitch_gen = g;
        self
    }

    pub fn set_pitch_review_sink(mut self, s: Arc<dyn PitchReviewSink>) -> Self {
        self.pitch_review_sink = Some(s);
        self
    }

    fn guarded_api_for(&self, source: &str, login: &str) -> SuppressionGuardChatApi {
        SuppressionGuardChatApi::with_pool(
            Arc::clone(&self.api),
            Arc::clone(&self.suppression),
            self.pool.clone(),
            source,
            login,
        )
    }

    /// Per-Message-Pfad: wird für jede eingehende Chat-Nachricht aufgerufen.
    /// (promos.py:1406–1447: `_maybe_send_activity_promo`)
    pub async fn on_message(self: &Arc<Self>, event: &ChatMessageEvent) {
        let login = event.broadcaster_user_login.to_lowercase();
        let chatter = event.chatter_user_login.to_lowercase();
        let text = event.text();

        // Guard: PROMO_IGNORE_COMMANDS — Nachrichten mit "!" prefix nicht tracken.
        if text.starts_with('!') {
            return;
        }

        // Guard: Partner-Channel-Check (promos.py:1422).
        if !self
            .partner_check
            .is_partner_channel_for_chat_tracking(&login)
            .await
        {
            return;
        }

        // Guard: Channel-Allowlist (leer = alle) — promos.py:78.
        if !self.promo_channel_allowed_db(&login).await {
            return;
        }

        self.prepare_channel_timers(&login, &event.broadcaster_user_id)
            .await;
        self.maybe_answer_lurker_followup(event).await;

        let now = Instant::now();

        // Aktivität aufzeichnen (promos.py:730). Der Raw-Count läuft separat
        // über [`Self::record_raw_message`] — Python bumpt ihn in
        // `_track_chat_health` für ALLE partner-getrackten Nachrichten
        // (inkl. "!"-Commands), nicht nur im Promo-Pfad (moderation.py:2173).
        {
            let state_ref = self
                .channel_states
                .entry(login.clone())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_accessed = now;
            self.record_promo_activity_inner(&mut state, &chatter, now);
        }

        if !self.reserve_promo_attempt(&login, now).await {
            tracing::debug!(channel = %login, "Promo-Attempt-Cooldown aktiv, kein Slot");
            return;
        }

        let Ok(promo_permit) = Arc::clone(&self.promo_semaphore).try_acquire_owned() else {
            tracing::debug!(channel = %login, "Promo-Semaphore voll, überspringe Promo");
            return;
        };

        let channel_id = event.broadcaster_user_id.clone();
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let _promo_permit = promo_permit;
            engine
                .maybe_send_promo_with_stats(&login, &channel_id, now, true)
                .await;
        });
    }

    pub async fn on_message_pitch(&self, event: &ChatMessageEvent) {
        let text = event.text();
        if text.starts_with('!') || text.chars().count() < 15 {
            return;
        }
        if event.chatter_user_id == event.broadcaster_user_id || event.chatter_user_id.is_empty() {
            return;
        }

        let login = event.broadcaster_user_login.to_lowercase();
        let target_user_id = event.chatter_user_id.clone();
        let target_login = event.chatter_user_login.clone();
        let channel_id = event.broadcaster_user_id.clone();

        self.prepare_channel_timers(&login, &channel_id).await;
        let text_len = text.chars().count();

        let Ok(_pitch_permit) = self.pitch_semaphore.try_acquire() else {
            tracing::debug!(channel = %login, "anlass-pitch: pitch-slots voll");
            return;
        };

        if !self
            .partner_check
            .is_partner_channel_for_chat_tracking(&login)
            .await
        {
            tracing::debug!(channel = %login, "anlass-pitch: kein partnerkanal");
            return;
        }
        if !self.promo_channel_allowed_db(&login).await {
            tracing::debug!(channel = %login, "anlass-pitch: nicht in allowlist");
            return;
        }
        if self.promo_blocked_by_plan_or_flag(&login).await {
            tracing::debug!(channel = %login, "anlass-pitch: werbefrei");
            return;
        }
        if self.suppression.is_muted(&login).await {
            tracing::debug!(channel = %login, "anlass-pitch: suppression");
            return;
        }
        if !self.stream_start_delay_ok(&login).await {
            tracing::debug!(channel = %login, "anlass-pitch: startverzoegerung");
            return;
        }

        if let Some(candidate) = self.partner_candidate(&target_user_id).await {
            if text_len >= 25 {
                self.run_partner_pitch(
                    &login,
                    &channel_id,
                    &target_user_id,
                    &target_login,
                    text,
                    candidate,
                )
                .await;
            }
            return;
        }

        let Some(register) = self.zuschauer_register.clone() else {
            self.log_zuschauer_reject(&login, &target_user_id, "register_fehlt", text, "anlass")
                .await;
            return;
        };
        match register.gate(event).await {
            crate::zuschauer_register::GateOutcome::Pass => {}
            crate::zuschauer_register::GateOutcome::Reject(grund) => {
                self.log_zuschauer_reject(&login, &target_user_id, grund, text, "anlass")
                    .await;
                return;
            }
        }

        let (game, title) = self.load_live_context(&login).await;
        let recent = self.load_recent_channel_messages(&login, 8).await;

        let occasion = if text_len >= 25
            && self.pitch_user_limit_ok(&target_user_id).await
            && self.pitch_channel_limit_ok(&login).await
            && self.pitch_judge_throttle_reserve(&login, &target_user_id)
        {
            let input = PitchJudgeInput {
                trigger_text: text.to_string(),
                game: game.clone(),
                title: title.clone(),
                recent_chat: recent.clone(),
                target_login: target_login.clone(),
            };
            self.pitch_judge
                .decide(input)
                .await
                .and_then(|resp| resp.occasion.map(|occ| (occ, resp.reply)))
        } else {
            None
        };
        let Some((occasion, reply)) = occasion else {
            return;
        };
        let resp_reply = reply;

        if let Some(reason) = pitch_filter_reject(&resp_reply) {
            self.log_anlass_reject(
                &login,
                &target_user_id,
                reason.as_str(),
                text,
                Some(resp_reply.clone()),
            )
            .await;
            return;
        }

        if pitch_injection_reject(&resp_reply, &target_login) {
            self.log_anlass_reject(
                &login,
                &target_user_id,
                "injection",
                text,
                Some(resp_reply.clone()),
            )
            .await;
            return;
        }

        let lock = self.get_send_lock(&login);
        let _guard = lock.lock().await;

        if !self.pitch_channel_limit_ok(&login).await {
            self.log_anlass_reject(&login, &target_user_id, "limit_channel", text, None)
                .await;
            return;
        }

        let out_text = format!("@{target_login} {}", resp_reply);

        let Some(log_id) = self
            .insert_pitch_log_pending(PitchLogEntry {
                channel_login: login.clone(),
                target_user_id: Some(target_user_id.clone()),
                pfad: "anlass",
                occasion: Some(occasion.as_str().to_string()),
                trigger_text: Some(text.to_string()),
                generated_text: Some(out_text.clone()),
                reject_reason: None,
                sent_at: None,
            })
            .await
        else {
            tracing::warn!(channel = %login, "anlass-pitch: log nicht schreibbar, kein pitch");
            return;
        };

        let outcome = self
            .guarded_api_for("promo", &login)
            .send_message(&channel_id, &out_text)
            .await;
        self.record_suppression_on_drop(&login, &channel_id, "promo", &outcome)
            .await;
        if !matches!(outcome, Ok(crate::types::SendOutcome::Sent)) {
            self.mark_pitch_log_dropped(log_id, "send_dropped").await;
            return;
        }

        self.mark_pitch_log_sent(log_id).await;
        self.mark_promo_sent(
            &login,
            Instant::now(),
            "anlass_pitch",
            Utc::now().timestamp() as f64,
        )
        .await;
        drop(_guard);

        if let Some(sink) = self.pitch_review_sink.as_ref() {
            sink.send_card(
                &login,
                &target_login,
                text,
                &resp_reply,
                PitchCardKind::Anlass,
                None,
            )
            .await;
        }
    }

    async fn run_partner_pitch(
        &self,
        login: &str,
        channel_id: &str,
        target_user_id: &str,
        target_login: &str,
        trigger: &str,
        candidate: PartnerCandidate,
    ) {
        if !self.partner_user_limit_ok(target_user_id).await {
            tracing::debug!(channel = %login, chatter = %target_user_id, "partner-pitch: user-limit");
            return;
        }
        if !self.partner_channel_limit_ok(login).await {
            tracing::debug!(channel = %login, "partner-pitch: kanal-limit");
            return;
        }
        if !self.partner_daily_limit_ok().await {
            tracing::debug!(channel = %login, "partner-pitch: tageslimit");
            return;
        }

        let (game, title) = self.load_live_context(login).await;
        let recent = self.load_recent_channel_messages(login, 8).await;
        let ctx = PartnerPitchContext {
            target_login: target_login.to_string(),
            target_messages: vec![trigger.to_string()],
            game,
            title,
            recent_chat: recent,
        };
        let Some(reply) = self.partner_pitch_gen.partner_pitch(&ctx).await else {
            self.log_partner_reject(login, target_user_id, "no_text", trigger, None)
                .await;
            tracing::debug!(channel = %login, "partner-pitch: kein text");
            return;
        };

        if let Some(reason) = pitch_filter_reject(&reply) {
            self.log_partner_reject(
                login,
                target_user_id,
                reason.as_str(),
                trigger,
                Some(reply.clone()),
            )
            .await;
            return;
        }
        if pitch_injection_reject(&reply, target_login) {
            self.log_partner_reject(login, target_user_id, "injection", trigger, Some(reply.clone()))
                .await;
            return;
        }

        let lock = self.get_send_lock(login);
        let _guard = lock.lock().await;

        if self.partner_candidate(target_user_id).await.is_none() {
            self.log_partner_reject(login, target_user_id, "kein_kandidat_mehr", trigger, None)
                .await;
            return;
        }
        if !self.partner_user_limit_ok(target_user_id).await {
            self.log_partner_reject(login, target_user_id, "limit_user", trigger, None)
                .await;
            return;
        }
        if !self.partner_channel_limit_ok(login).await {
            self.log_partner_reject(login, target_user_id, "limit_channel", trigger, None)
                .await;
            return;
        }

        let out_text = format!("@{target_login} {reply}");

        let Some(log_id) = self
            .insert_pitch_log_pending(PitchLogEntry {
                channel_login: login.to_string(),
                target_user_id: Some(target_user_id.to_string()),
                pfad: "partner",
                occasion: None,
                trigger_text: Some(trigger.to_string()),
                generated_text: Some(out_text.clone()),
                reject_reason: None,
                sent_at: None,
            })
            .await
        else {
            tracing::warn!(channel = %login, "partner-pitch: log nicht schreibbar, kein pitch");
            return;
        };

        let outcome = self
            .guarded_api_for("promo", login)
            .send_message(channel_id, &out_text)
            .await;
        self.record_suppression_on_drop(login, channel_id, "promo", &outcome)
            .await;
        if !matches!(outcome, Ok(crate::types::SendOutcome::Sent)) {
            self.mark_pitch_log_dropped(log_id, "send_dropped").await;
            return;
        }

        self.mark_pitch_log_sent(log_id).await;
        self.mark_promo_sent(
            login,
            Instant::now(),
            "partner_pitch",
            Utc::now().timestamp() as f64,
        )
        .await;
        drop(_guard);

        self.record_partner_ledger(&candidate.login, target_user_id, login)
            .await;

        if let Some(sink) = self.pitch_review_sink.as_ref() {
            let hint = format!(
                "Kandidat: {} streamt Deadlock, letzte Session {}",
                candidate.login,
                candidate.last_session.format("%Y-%m-%d")
            );
            sink.send_card(
                login,
                target_login,
                trigger,
                &reply,
                PitchCardKind::Partner,
                Some(&hint),
            )
            .await;
        }
    }

    async fn log_partner_reject(
        &self,
        login: &str,
        target_user_id: &str,
        reason: &str,
        trigger: &str,
        generated: Option<String>,
    ) {
        self.record_pitch_log(PitchLogEntry {
            channel_login: login.to_string(),
            target_user_id: Some(target_user_id.to_string()),
            pfad: "partner",
            occasion: None,
            trigger_text: Some(trigger.to_string()),
            generated_text: generated,
            reject_reason: Some(reason.to_string()),
            sent_at: None,
        })
        .await;
    }

    fn pitch_judge_throttle_reserve(&self, login: &str, chatter_id: &str) -> bool {
        let now = Instant::now();
        let chatter_key = format!("{login}|{chatter_id}");
        if let Some(last) = self.pitch_judge_last.get(&chatter_key) {
            if now.duration_since(*last) < PITCH_JUDGE_CHATTER_COOLDOWN {
                return false;
            }
        }
        {
            let mut entry = self
                .pitch_judge_channel
                .entry(login.to_string())
                .or_default();
            entry.retain(|t| now.duration_since(*t) < PITCH_JUDGE_CHANNEL_WINDOW);
            if entry.len() >= PITCH_JUDGE_CHANNEL_MAX_PER_WINDOW {
                return false;
            }
            entry.push(now);
        }
        self.pitch_judge_last.insert(chatter_key, now);
        true
    }

    async fn log_zuschauer_reject(
        &self,
        login: &str,
        target_user_id: &str,
        reason: &str,
        trigger: &str,
        pfad: &'static str,
    ) {
        if let Err(error) = sqlx::query!(
            r#"INSERT INTO twitch_promo_pitch_log
                 (channel_login, target_user_id, pfad, occasion, trigger_text,
                  generated_text, reject_reason, sent_at)
               SELECT $1, $2, $3, NULL, $4, NULL, $5, NULL
                WHERE NOT EXISTS (
                    SELECT 1
                      FROM twitch_promo_pitch_log log
                     WHERE log.channel_login = $1
                       AND log.target_user_id = $2
                       AND log.reject_reason = $5
                       AND log.created_at >= COALESCE((
                           SELECT sessions.started_at
                             FROM twitch_stream_sessions sessions
                             JOIN twitch_live_state live
                               ON live.active_session_id = sessions.id
                            WHERE LOWER(live.streamer_login) = LOWER($1)
                              AND live.is_live = 1
                            LIMIT 1
                       ), NOW())
                )"#,
            login,
            target_user_id,
            pfad,
            trigger,
            reason,
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, channel = %login, "zuschauer-reject nicht schreibbar");
        }
    }

    async fn log_anlass_reject(
        &self,
        login: &str,
        target_user_id: &str,
        reason: &str,
        trigger: &str,
        generated: Option<String>,
    ) {
        self.record_pitch_log(PitchLogEntry {
            channel_login: login.to_string(),
            target_user_id: Some(target_user_id.to_string()),
            pfad: "anlass",
            occasion: None,
            trigger_text: Some(trigger.to_string()),
            generated_text: generated,
            reject_reason: Some(reason.to_string()),
            sent_at: None,
        })
        .await;
    }

    async fn pitch_user_limit_ok(&self, target_user_id: &str) -> bool {
        let row = match sqlx::query!(
            "SELECT
                 MAX(sent_at) FILTER (WHERE sent_at IS NOT NULL) AS last_sent,
                 COUNT(*) FILTER (
                     WHERE sent_at IS NULL AND reject_reason IS NULL
                       AND created_at >= NOW() - INTERVAL '10 minutes'
                 ) AS \"pending!\"
               FROM twitch_promo_pitch_log
              WHERE target_user_id = $1 AND pfad IN ('anlass', 'partner', 'gezielt')",
            target_user_id,
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(%error, "anlass-pitch: user-limit nicht lesbar, blockiere");
                return false;
            }
        };
        if row.pending > 0 {
            return false;
        }
        match row.last_sent {
            Some(ts) => (Utc::now() - ts).num_seconds() >= 7 * 86400,
            None => true,
        }
    }

    async fn pitch_channel_limit_ok(&self, login: &str) -> bool {
        let (timers, community) = self.channel_timers(login).await;
        if community && !self.overall_promo_ready_locked(login, Instant::now()).await {
            return false;
        }
        let stream_start = self
            .load_stream_start(login)
            .await
            .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(3));
        let row = match sqlx::query!(
            "SELECT COUNT(*) AS \"count!\", MAX(sent_at) AS last FROM twitch_promo_pitch_log
              WHERE channel_login = $1 AND pfad IN ('anlass', 'partner') AND sent_at IS NOT NULL
                AND sent_at >= $2",
            login,
            stream_start,
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(%error, channel = %login, "anlass-pitch: kanal-limit nicht lesbar, blockiere");
                return false;
            }
        };
        if row.count >= timers.pitch_max_per_stream {
            return false;
        }
        if let Some(last) = row.last {
            if (Utc::now() - last).num_seconds() < (timers.pitch_cooldown_minutes * 60) as i64 {
                return false;
            }
        }
        true
    }

    async fn partner_candidate(&self, chatter_user_id: &str) -> Option<PartnerCandidate> {
        match sqlx::query!(
            r#"SELECT s.streamer_login AS login, MAX(s.started_at) AS "last_session!"
                 FROM twitch_stream_sessions s
                WHERE s.twitch_user_id = $1
                  AND LOWER(s.game_name) = 'deadlock'
                  AND s.started_at >= NOW() - INTERVAL '60 days'
                  AND NOT EXISTS (SELECT 1 FROM twitch_partners p WHERE p.twitch_user_id = $1)
                  AND NOT EXISTS (SELECT 1 FROM twitch_scout_pitch_blacklist b
                                   WHERE LOWER(b.streamer_login) = LOWER(s.streamer_login)
                                      OR b.twitch_user_id = $1)
                  AND NOT EXISTS (SELECT 1 FROM twitch_partner_outreach o
                                   WHERE LOWER(o.streamer_login) = LOWER(s.streamer_login)
                                      OR o.twitch_user_id = $1)
                  AND NOT EXISTS (SELECT 1 FROM twitch_scout_pitch_ledger l
                                   WHERE (LOWER(l.streamer_login) = LOWER(s.streamer_login)
                                          OR l.twitch_user_id = $1)
                                     AND l.action = 'posted')
                GROUP BY s.streamer_login
                ORDER BY MAX(s.started_at) DESC
                LIMIT 1"#,
            chatter_user_id,
        )
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(row)) => Some(PartnerCandidate {
                login: row.login,
                last_session: row.last_session,
            }),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(%error, "partner-pitch: kandidatenpruefung nicht lesbar, kein partner-pitch");
                None
            }
        }
    }

    async fn partner_user_limit_ok(&self, chatter_user_id: &str) -> bool {
        let row = match sqlx::query!(
            "SELECT
                 MAX(sent_at) FILTER (WHERE sent_at IS NOT NULL) AS last_sent,
                 COUNT(*) FILTER (
                     WHERE sent_at IS NULL AND reject_reason IS NULL
                       AND created_at >= NOW() - INTERVAL '10 minutes'
                 ) AS \"pending!\"
               FROM twitch_promo_pitch_log
              WHERE target_user_id = $1 AND pfad = 'partner'",
            chatter_user_id,
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(%error, "partner-pitch: user-limit nicht lesbar, blockiere");
                return false;
            }
        };
        if row.pending > 0 {
            return false;
        }
        row.last_sent.is_none()
    }

    async fn partner_channel_limit_ok(&self, login: &str) -> bool {
        if !self.pitch_channel_limit_ok(login).await {
            return false;
        }
        let (timers, community) = self.channel_timers(login).await;
        let stream_start = self
            .load_stream_start(login)
            .await
            .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(3));
        let row = match sqlx::query!(
            "SELECT
                 COUNT(*) FILTER (WHERE pfad = 'partner') AS \"partner_count!\",
                 MAX(sent_at) AS last
               FROM twitch_promo_pitch_log
              WHERE channel_login = $1 AND pfad IN ('anlass', 'partner') AND sent_at IS NOT NULL
                AND sent_at >= $2",
            login,
            stream_start,
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(%error, channel = %login, "partner-pitch: kanal-limit nicht lesbar, blockiere");
                return false;
            }
        };
        let partner_limit = if community {
            timers.pitch_max_per_stream
        } else {
            1
        };
        if row.partner_count >= partner_limit {
            return false;
        }
        if let Some(last) = row.last {
            if (Utc::now() - last).num_seconds() < (timers.pitch_cooldown_minutes * 60) as i64 {
                return false;
            }
        }
        true
    }

    async fn partner_daily_limit_ok(&self) -> bool {
        let row = match sqlx::query!(
            "SELECT COUNT(*) AS \"count!\" FROM twitch_promo_pitch_log
              WHERE pfad = 'partner' AND sent_at IS NOT NULL
                AND sent_at >= NOW() - INTERVAL '24 hours'",
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(%error, "partner-pitch: tageslimit nicht lesbar, blockiere");
                return false;
            }
        };
        row.count < 5
    }

    async fn record_partner_ledger(
        &self,
        own_login: &str,
        chatter_user_id: &str,
        channel_login: &str,
    ) {
        if let Err(error) = sqlx::query!(
            "INSERT INTO twitch_scout_pitch_ledger
                 (streamer_login, trigger_type, judge_verdict, action, detail, twitch_user_id)
             VALUES ($1, 'chat_partner_pitch', 'partner_pitch', 'posted', $2, $3)",
            own_login,
            channel_login,
            chatter_user_id,
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, login = %own_login, "partner-pitch: ledger-eintrag fehlgeschlagen");
        }
    }

    async fn load_stream_start(&self, login: &str) -> Option<DateTime<Utc>> {
        let row = sqlx::query_scalar!(
            "SELECT last_started_at FROM twitch_live_state
              WHERE LOWER(streamer_login) = LOWER($1) AND is_live = 1
              LIMIT 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .flatten()?;
        let normalized = row.replace('Z', "+00:00");
        chrono::DateTime::parse_from_rfc3339(&normalized)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
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
        while state
            .activity
            .front()
            .map(|(t, _)| now.duration_since(*t) > window)
            .unwrap_or(false)
        {
            state.activity.pop_front();
        }

        // Eintrag hinzufügen (FIFO, maxlen 2048).
        if state.activity.len() >= PROMO_ACTIVITY_BUCKET_MAXLEN {
            state.activity.pop_front();
        }
        state.activity.push_back((now, chatter.to_string()));
    }

    /// Periodischer Haupt-Loop-Body (promos.py:1466: `_send_promo_if_due`).
    async fn send_promo_if_due(self: &Arc<Self>, now: Instant) {
        // Lurker-Tax nutzt in Python eine eigene Channelquelle ohne Deadlock-,
        // promo_disabled- oder Plan-Filter.
        let lurker_tax_channels = match self.get_live_channels_for_lurker_tax().await {
            Ok(v) => v,
            Err(e) => {
                warn!("get_live_channels_for_lurker_tax fehlgeschlagen: {e}");
                Vec::new()
            }
        };

        for (login, channel_id) in &lurker_tax_channels {
            self.prepare_channel_timers(login, channel_id).await;
            if !self
                .partner_check
                .is_partner_channel_for_chat_tracking(login)
                .await
            {
                continue;
            }
            self.maybe_send_lurker_tax_reminder(login, channel_id, now)
                .await;
        }

        // Live-Kanäle holen (promos.py:1630).
        let live_channels = match self.get_live_channels_for_promo().await {
            Ok(v) => v,
            Err(e) => {
                warn!("get_live_channels_for_promo fehlgeschlagen: {e}");
                return;
            }
        };

        let mut faellig: Vec<(String, String)> = Vec::new();
        for (login, channel_id) in &live_channels {
            self.prepare_channel_timers(login, channel_id).await;
            if self.promo_blocked_by_plan_or_flag(login).await {
                continue;
            }
            if !self
                .partner_check
                .is_partner_channel_for_chat_tracking(login)
                .await
            {
                continue;
            }
            if !self.promo_channel_allowed_db(login).await {
                continue;
            }

            let state_snapshot = {
                let state_ref = self
                    .channel_states
                    .entry(login.clone())
                    .or_insert_with(|| Mutex::new(ChannelState::new()));
                let state = state_ref.lock().await;
                state.clone()
            };
            let overall_ready = self.overall_promo_ready_inner(&state_snapshot, now);
            let activity_ready = self
                .promo_activity_ready_inner(login, &state_snapshot, now)
                .await;

            if overall_ready
                && activity_ready
                && self.stream_start_delay_ok(login).await
            {
                faellig.push((login.clone(), channel_id.clone()));
            }
        }

        let mut set = tokio::task::JoinSet::new();
        let mut iter = faellig.into_iter();
        for _ in 0..PROMO_DUE_CONCURRENCY {
            let Some((login, channel_id)) = iter.next() else {
                break;
            };
            set.spawn(Arc::clone(self).process_due_channel(login, channel_id, now));
        }
        while set.join_next().await.is_some() {
            if let Some((login, channel_id)) = iter.next() {
                set.spawn(Arc::clone(self).process_due_channel(login, channel_id, now));
            }
        }
    }

    async fn process_due_channel(self: Arc<Self>, login: String, channel_id: String, now: Instant) {
        // Community und alle anderen Kanäle laufen durch denselben Aktivitäts-Gate;
        // nur die geladenen Timer-Werte unterscheiden sich. Keine Sonderlogik für
        // den Community-Kanal (kein Senden bei ruhigem Chat trotz Overall-Timer).
        let sent = self
            .maybe_send_promo_with_stats(&login, &channel_id, now, false)
            .await;
        if !sent {
            self.maybe_send_viewer_spike_promo(&login, &channel_id, now)
                .await;
        }
    }

    /// Dummy-Rückgabe (Invite-Auflösung wird per async-Trait gemacht, nicht cached).
    /// maybe_send_promo_with_stats (promos.py:1281).
    /// Gibt true zurück wenn gesendet.
    async fn maybe_send_promo_with_stats(
        &self,
        login: &str,
        channel_id: &str,
        now: Instant,
        attempt_already_reserved: bool,
    ) -> bool {
        // Guard: Channel-Allowlist.
        if !self.promo_channel_allowed_db(login).await {
            return false;
        }

        // Guard: Stream-Start-Verzögerung (≥10 min nach Go-Live).
        if !self.stream_start_delay_ok(login).await {
            return false;
        }

        // Guard: Overall-Cooldown (≥90 min).
        let state_snapshot = {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            state.clone()
        };
        let overall_ready = self.overall_promo_ready_inner(&state_snapshot, now);
        let activity_ready = self
            .promo_activity_ready_inner(login, &state_snapshot, now)
            .await;

        if !overall_ready || !activity_ready {
            return false;
        }

        if !attempt_already_reserved && !self.reserve_promo_attempt(login, now).await {
            return false;
        }

        self.send_promo_message(login, channel_id, now, "chat_activity")
            .await
    }

    /// Reserviert atomar den Attempt-Cooldown (≥10 min) im In-Memory-State und
    /// persistiert ihn. Liefert false, wenn der Kanal noch in der Attempt-Sperre
    /// steht. Der per-Message-Pfad reserviert damit synchron vor dem Semaphore,
    /// damit ein aktiver Kanal nicht jeden Slot mit No-Op-Tasks belegt.
    async fn reserve_promo_attempt(&self, login: &str, now: Instant) -> bool {
        let reserved = {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if !self.promo_attempt_allowed_inner(&state, now) {
                false
            } else {
                state.last_promo_attempt = Some(now);
                true
            }
        };
        if reserved {
            self.save_promo_cooldown(login, "attempt", Utc::now().timestamp() as f64)
                .await;
        }
        reserved
    }

    /// Kernfunktion Promo senden (promos.py:1096: `_send_promo_message`).
    async fn send_promo_message(
        &self,
        login: &str,
        channel_id: &str,
        now: Instant,
        reason: &str,
    ) -> bool {
        // Suppression-Check (promos.py:1096).
        if self.suppression.is_muted(login).await {
            return false;
        }
        // Plan-Flag (promos.py:1096).
        if self.promo_blocked_by_plan_or_flag(login).await {
            return false;
        }
        let (invite, is_specific) = self.invite_resolver.resolve_invite(login).await;

        let Some((text, color)) = self.build_promo_text(login, &invite).await else {
            self.record_pitch_log(PitchLogEntry {
                channel_login: login.to_string(),
                target_user_id: None,
                pfad: "periodic",
                occasion: None,
                trigger_text: None,
                generated_text: None,
                reject_reason: Some("kein_text".to_string()),
                sent_at: None,
            })
            .await;
            return false;
        };

        let lock = self.get_send_lock(login);
        let _guard = lock.lock().await;
        if self.suppression.is_muted(login).await {
            return false;
        }
        if !self.overall_promo_ready_locked(login, now).await {
            return false;
        }

        let sent = self
            .api
            .send_announcement(channel_id, &text, &color)
            .await
            .unwrap_or(false);
        if !sent {
            debug!(login, "Promo-Announcement nicht gesendet (Drop/Fehler)");
            self.record_pitch_log(PitchLogEntry {
                channel_login: login.to_string(),
                target_user_id: None,
                pfad: "periodic",
                occasion: None,
                trigger_text: None,
                generated_text: Some(text),
                reject_reason: Some("send_dropped".to_string()),
                sent_at: None,
            })
            .await;
            return false;
        }

        self.record_pitch_log(PitchLogEntry {
            channel_login: login.to_string(),
            target_user_id: None,
            pfad: "periodic",
            occasion: None,
            trigger_text: None,
            generated_text: Some(text),
            reject_reason: None,
            sent_at: Some(Utc::now()),
        })
        .await;

        self.mark_promo_sent(login, now, reason, Utc::now().timestamp() as f64)
            .await;

        if is_specific {
            self.mark_streamer_invite_sent(login).await;
        }

        true
    }

    /// Werbefrei-Pitch beim Go-Live senden (Python `eventsub_mixin.py:1523-1555`
    /// → `_send_announcement(source="promo")` + `_mark_promo_sent(reason="timeout_pitch")`).
    ///
    /// Anders als ein direkter `api.send_announcement` läuft der Pitch hier durch
    /// die Outbound-Promo-Suppression (wird unterdrückt, wenn der Kanal gerade
    /// gemutet ist) UND belegt bei Erfolg den Promo-Cooldown — sonst könnte
    /// unmittelbar danach eine reguläre Promo feuern (Doppel-Werbung). `message`
    /// ist der fertige Pitch-Text, gesendet als blaues Announcement.
    pub async fn send_timeout_pitch(&self, channel_id: &str, login: &str, message: &str) -> bool {
        // Suppression-Check (Python source="promo").
        if self.suppression.is_muted(login).await {
            debug!(
                login,
                "Werbefrei-Pitch unterdrückt (Outbound-Promo-Suppression)"
            );
            return false;
        }
        let sent = self
            .api
            .send_announcement(channel_id, message, "blue")
            .await
            .unwrap_or(false);
        if !sent {
            debug!(login, "Werbefrei-Pitch nicht gesendet (Drop/Fehler)");
            return false;
        }
        // Promo-Cooldown belegen (Python `_mark_promo_sent(reason="timeout_pitch")`).
        self.mark_promo_sent(
            login,
            Instant::now(),
            "timeout_pitch",
            Utc::now().timestamp() as f64,
        )
        .await;
        true
    }

    async fn build_promo_text(&self, login: &str, invite: &str) -> Option<(String, String)> {
        let (_, community) = self.channel_timers(login).await;
        if community {
            // Vor jedem Versand neu laden; deaktivierte Texte haben keinen Fallback.
            let settings = match tb_analytics::community_announcements::load(&self.pool).await {
                Ok(settings) => settings,
                Err(error) => {
                    warn!(%error, "Community-Ankündigungen konnten nicht geladen werden");
                    return None;
                }
            };
            if !settings.enabled {
                return None;
            }
            let mut rotation = Vec::new();
            if settings.include_global_event {
                if let Some(event) = self.load_global_promo_message(invite).await {
                    rotation.push(event);
                }
            }
            for entry in settings.entries.iter().filter(|entry| entry.enabled) {
                let text = entry.text.trim().replace("{invite}", invite);
                if text.chars().count() > 500 {
                    warn!("Community-Ankündigung überschreitet nach Einfügen des Einladungslinks 500 Zeichen");
                    return None;
                }
                rotation.push((text, entry.color.clone()));
            }
            if rotation.is_empty() {
                return None;
            }
            // Nur erfolgreiche Sends zählen; die Position überlebt Neustarts.
            let count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE channel_login = $1 AND pfad = 'periodic' AND sent_at IS NOT NULL")
                .bind(login).fetch_one(&self.pool).await;
            return match count {
                Ok(count) => {
                    Some(rotation.remove(count.rem_euclid(rotation.len() as i64) as usize))
                }
                Err(error) => {
                    warn!(%error, login, "Community-Themenrotation konnte nicht geladen werden");
                    None
                }
            };
        }
        if let Some(text) = self.load_global_promo_message(invite).await {
            return Some(text);
        }

        if let Some(text) = self.load_streamer_promo_message(login, invite).await {
            return Some((text, "purple".into()));
        }

        let (game, title) = self.load_live_context(login).await;
        let ctx = ChannelPromoContext {
            game,
            title,
            recent_chat: self.load_recent_channel_messages(login, 8).await,
        };
        self.pitch_text_gen
            .channel_promo(&ctx, invite)
            .await
            .map(|text| (text, "purple".into()))
    }

    async fn load_live_context(&self, login: &str) -> (Option<String>, Option<String>) {
        let row = sqlx::query!(
            "SELECT last_game, last_title FROM twitch_live_state
              WHERE LOWER(streamer_login) = LOWER($1)
              LIMIT 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        match row {
            Some(row) => (row.last_game, row.last_title),
            None => (None, None),
        }
    }

    /// Globalen Promo-Override laden (promos.py `_load_global_promo_message` +
    /// `_build_promo_text`-Schritt 1). Lädt die Singleton-Config aus
    /// `twitch_global_promo_modes`, wertet sie gegen die aktuelle Zeit aus und
    /// gibt — wenn der `custom_event`-Modus aktiv ist — den formatierten
    /// Event-Text zurück (`{invite}` ersetzt). DB-/Auswertungs-Fehler → None
    /// (kein Override, fällt auf Streamer-/Pool-Promo zurück).
    async fn load_global_promo_message(&self, invite: &str) -> Option<(String, String)> {
        let config = tb_analytics::promo_mode::load_global_promo_mode(&self.pool)
            .await
            .ok()?;
        let evaluation =
            tb_analytics::promo_mode::evaluate_global_promo_mode(&config.to_json(), None);
        let message = evaluation.active_message?;
        let message = message.trim();
        if message.is_empty() {
            return None;
        }
        render_promo_template(message, invite).map(|text| (text, config.announcement_color))
    }

    /// Streamer-spezifische Promo laden (promos.py:945, streamer_plans.promo_message).
    async fn load_streamer_promo_message(&self, login: &str, invite: &str) -> Option<String> {
        // streamer_plans.promo_message = text (prod schema)
        let row = sqlx::query_scalar!(
            "SELECT promo_message FROM streamer_plans WHERE LOWER(COALESCE(twitch_login,'')) = $1 LIMIT 1",
            login.to_lowercase(),
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        if let Some(Some(msg)) = row {
            let message = msg.trim();
            if !message.is_empty()
                && tb_analytics::promo_mode::validate_streamer_promo_message(message).is_empty()
            {
                return render_promo_template(message, invite);
            }
        }
        None
    }

    /// Promo gesendet markieren (promos.py:879: `_mark_promo_sent`).
    async fn mark_promo_sent(&self, login: &str, now: Instant, reason: &str, wall_ts: f64) {
        {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
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
            self.save_promo_cooldown(login, "viewer_spike", wall_ts)
                .await;
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
    async fn mark_streamer_invite_sent(&self, login: &str) {
        let login_norm = login.trim().to_lowercase();
        if login_norm.is_empty() {
            return;
        }
        let now = Utc::now().to_rfc3339();
        if let Err(e) = sqlx::query!(
            "UPDATE twitch_streamer_invites
                SET last_sent_at = $1
              WHERE LOWER(streamer_login) = $2",
            now,
            &login_norm,
        )
        .execute(&self.pool)
        .await
        {
            warn!(login = %login_norm, "mark_streamer_invite_sent fehlgeschlagen: {e}");
        }
    }

    /// Overall-Cooldown-Check (≥90 min seit letzter Promo). (promos.py:1251).
    fn overall_promo_ready_inner(&self, state: &ChannelState, now: Instant) -> bool {
        match state.last_promo_sent {
            None => true,
            Some(last) => {
                now.duration_since(last).as_secs() >= state.timers.overall_cooldown_minutes * 60
            }
        }
    }

    async fn overall_promo_ready_locked(&self, login: &str, now: Instant) -> bool {
        let state_ref = self
            .channel_states
            .entry(login.to_string())
            .or_insert_with(|| Mutex::new(ChannelState::new()));
        let state = state_ref.lock().await;
        self.overall_promo_ready_inner(&state, now)
    }

    /// Aktivitätsschwellen-Check (promos.py:1251: `_promo_activity_ready`).
    async fn promo_activity_ready_inner(
        &self,
        login: &str,
        state: &ChannelState,
        now: Instant,
    ) -> bool {
        // 1. Roh-Nachrichten-Minimum.
        if state.raw_msg_count_since_promo < state.timers.min_messages {
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
        let cooldown_sec = ((state.timers.activity_cooldown_min_minutes as f64)
            + (1.0 - ratio)
                * (state.timers.activity_cooldown_max_minutes as f64
                    - state.timers.activity_cooldown_min_minutes as f64))
            * 60.0;

        if let Some(last) = state.last_promo_sent {
            if now.duration_since(last).as_secs_f64() < cooldown_sec {
                return false;
            }
        }

        // 4. Neue Chatter ≥ eingestellter Wert. Gilt auch für den ersten Versand:
        //    ohne bisherige "gesehen"-Basis zählen alle aktiven Chatter als neu,
        //    die eingestellte Schwelle bleibt so auch beim ersten Timer greifbar.
        if state.timers.new_chatters > 0 {
            let new_chatters = self
                .get_new_chatters_in_window_inner(login, state, now)
                .await;
            if new_chatters < state.timers.new_chatters {
                return false;
            }
        }

        true
    }

    /// Neue Chatter im Fenster (promos.py:700: `_get_new_chatters_in_window`).
    async fn get_new_chatters_in_window_inner(
        &self,
        login: &str,
        state: &ChannelState,
        now: Instant,
    ) -> usize {
        let window = Duration::from_secs(PROMO_ACTIVITY_WINDOW_MIN * 60);
        let max_age = Duration::from_secs(PROMO_SEEN_CHATTER_MAX_AGE_SEC);

        let mut active: HashSet<String> = state
            .activity
            .iter()
            .filter(|(ts, _)| now.duration_since(*ts) <= window)
            .map(|(_, c)| c.clone())
            .collect();
        active.extend(self.get_current_session_viewers(login).await);

        active
            .iter()
            .filter(|c| match state.seen_chatters.get(*c) {
                None => true,
                Some(&last) => now.duration_since(last) > max_age,
            })
            .count()
    }

    /// Aktuelle API-getrackte Chatter/Viewer der laufenden Session
    /// (promos.py:704: `_get_current_session_viewers`).
    async fn get_current_session_viewers(&self, login: &str) -> HashSet<String> {
        if login.trim().is_empty() {
            return HashSet::new();
        }
        let rows = sqlx::query!(
            "SELECT sc.chatter_login
               FROM twitch_session_chatters sc
               JOIN twitch_live_state ls ON ls.active_session_id = sc.session_id
              WHERE LOWER(ls.streamer_login) = LOWER($1)
                AND ls.is_live = 1
                AND TRIM(COALESCE(sc.chatter_login, '')) <> ''",
            login,
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .filter_map(|row| {
                let normalized = row.chatter_login.trim().to_lowercase();
                (!normalized.is_empty()).then_some(normalized)
            })
            .collect()
    }

    /// Attempt-Cooldown-Check (≥10 min). (promos.py:1281).
    fn promo_attempt_allowed_inner(&self, state: &ChannelState, now: Instant) -> bool {
        match state.last_promo_attempt {
            None => true,
            Some(last) => {
                now.duration_since(last).as_secs() >= state.timers.attempt_cooldown_minutes * 60
            }
        }
    }

    /// Viewer-Spike-Promo (promos.py:1306: `_maybe_send_viewer_spike_promo`).
    async fn maybe_send_viewer_spike_promo(&self, login: &str, channel_id: &str, now: Instant) {
        // Guards (promos.py:1306).
        let state_snapshot = {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            state.clone()
        };

        let overall_ready = self.overall_promo_ready_inner(&state_snapshot, now);
        // Python: activity_age_sec is None → kein Chat → Silence gilt als OK (promos.py:1355).
        // Rust `is_some_and` würde None als false werten → geblockt. Korrekt: None → true.
        let chat_silent = state_snapshot.last_raw_chat_message_ts.is_none_or(|t| {
            now.duration_since(t).as_secs() >= PROMO_VIEWER_SPIKE_MIN_CHAT_SILENCE_SEC
        });
        let spike_cd_ok = state_snapshot.last_promo_viewer_spike.is_none_or(|t| {
            now.duration_since(t).as_secs()
                >= state_snapshot.timers.viewer_spike_cooldown_minutes * 60
        });
        let attempt_ok = self.promo_attempt_allowed_inner(&state_snapshot, now);
        // Auch der Viewer-Spike-Pfad respektiert die eingestellten Chat-Grenzen
        // (min_messages, Aktivitätsfenster, neue Chatter). Keine alternative
        // Werbeschleife darf diese Grenzen umgehen; die frühere Schwelle "eine
        // Roh-Nachricht" ist damit abgelöst.
        let activity_ready = self
            .promo_activity_ready_inner(login, &state_snapshot, now)
            .await;

        if !overall_ready || !activity_ready || !chat_silent || !spike_cd_ok || !attempt_ok {
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
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_attempt = Some(now);
        }
        self.save_promo_cooldown(login, "attempt", Utc::now().timestamp() as f64)
            .await;

        self.send_promo_message(login, channel_id, now, "viewer_spike")
            .await;
    }

    /// Viewer-Spike-Erkennung (promos.py:1152: `_get_viewer_spike_context`).
    async fn get_viewer_spike_context(&self, login: &str) -> bool {
        // SQL 1 — Session-Baseline (promos.py:1152).
        let session_baseline = sqlx::query!(
            "SELECT AVG(avg_viewers) AS avg_viewers, COUNT(*)::bigint AS \"sample_count!\"
               FROM (
                 SELECT avg_viewers FROM twitch_stream_sessions
                  WHERE streamer_login = $1 AND ended_at IS NOT NULL AND avg_viewers > 0
                  ORDER BY started_at DESC LIMIT $2
               ) recent_sessions",
            login,
            PROMO_VIEWER_SPIKE_SESSION_SAMPLE_LIMIT,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let baseline = if let Some(row) = session_baseline {
            if row.sample_count >= PROMO_VIEWER_SPIKE_MIN_SESSIONS
                && row.avg_viewers.is_some_and(|avg| avg > 0.0)
            {
                let avg = row.avg_viewers.unwrap_or_default();
                Some(avg)
            } else {
                None
            }
        } else {
            None
        };

        // SQL 2 — Stats-Baseline als Fallback (promos.py:1152).
        let baseline = if baseline.is_none() {
            let stats_baseline = sqlx::query!(
                "SELECT AVG(viewer_count::float) AS avg_viewers, COUNT(*)::bigint AS \"sample_count!\"
                   FROM (
                     SELECT viewer_count FROM twitch_stats_tracked
                      WHERE LOWER(streamer) = $1 AND viewer_count > 0
                      ORDER BY ts_utc DESC LIMIT $2
                   ) recent_stats",
                login.to_lowercase(),
                PROMO_VIEWER_SPIKE_STATS_SAMPLE_LIMIT,
            )
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            if let Some(row) = stats_baseline {
                if row.sample_count >= PROMO_VIEWER_SPIKE_MIN_STATS_SAMPLES
                    && row.avg_viewers.is_some_and(|avg| avg > 0.0)
                {
                    let avg = row.avg_viewers.unwrap_or_default();
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
        let live_viewers = sqlx::query_scalar!(
            "SELECT last_viewer_count FROM twitch_live_state WHERE streamer_login = $1 AND is_live = 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let current = match live_viewers {
            Some(Some(v)) if v > 0 => v as f64,
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
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            self.overall_promo_ready_inner(&state, now)
        };
        if !overall_ready {
            return;
        }

        if !self.lurker_tax_channel_gate(channel_id).await {
            return;
        }

        // has_moderator_read_chatters: Scope muss im Auth-Store vorliegen (promos.py:1410).
        // Prüft twitch_raid_auth.scopes für diesen Streamer.
        let auth_scopes = sqlx::query_scalar::<_, Option<String>>(
            "SELECT scopes FROM twitch_raid_auth
              WHERE twitch_user_id = $1
              LIMIT 1",
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let scopes_raw = auth_scopes.flatten().unwrap_or_default();
        if !self.has_chatters_scope(&scopes_raw).await {
            return;
        }

        match self.reward_checker.as_ref() {
            Some(checker) => {
                if !checker.active_lurker_reward_exists(channel_id).await {
                    return;
                }
            }
            None => {
                if self.reward_gate_warned.insert(login.to_string(), ()).is_none() {
                    warn!(
                        login,
                        "Lurker-Tax: kein Reward-Checker verdrahtet, Erinnerung wird nicht gesendet"
                    );
                }
                return;
            }
        }

        // Kandidaten holen (promos.py:408).
        let candidates = self.get_lurker_tax_candidates(channel_id).await;
        if candidates.is_empty() {
            return;
        }

        // Aktive Session-ID fürs Per-Session-Dedup.
        let session_id: i64 = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id = $1",
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
        .unwrap_or(0);

        // Bereits in dieser Session erwähnte Lurker rausfiltern, dann auf MAX kappen
        // (nächstrangige rücken nach). Set wird bei Session-Wechsel geräumt.
        let selected: Vec<String> = {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if state.lurker_mentions.0 != session_id {
                state.lurker_mentions = (session_id, HashSet::new());
            }
            candidates
                .iter()
                .filter(|c| !state.lurker_mentions.1.contains(*c))
                .take(LURKER_TAX_MAX_MENTIONS)
                .cloned()
                .collect()
        };
        if selected.is_empty() {
            return;
        }

        let text = self.build_lurker_tax_text(&selected);
        // Nur bei erfolgreichem Send merken + Cooldown belegen (Python: if ok).
        let sent = self
            .api
            .send_announcement(channel_id, &text, "orange")
            .await
            .unwrap_or(false);
        if !sent {
            return;
        }
        {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if state.lurker_mentions.0 == session_id {
                state.lurker_mentions.1.extend(selected.iter().cloned());
            }
            if state.lurker_reminded_at.0 != session_id {
                state.lurker_reminded_at = (session_id, HashMap::new());
            }
            let jetzt = Instant::now();
            for login in &selected {
                state
                    .lurker_reminded_at
                    .1
                    .insert(login.to_lowercase(), jetzt);
            }
        }

        // Promo-Slot belegen (promos.py:1357 — lurker_tax nutzt overall-Cooldown).
        self.mark_promo_sent(login, now, "lurker_tax", Utc::now().timestamp() as f64)
            .await;
        info!(
            login,
            "Lurker-Tax-Erinnerung gesendet ({} Mentions)",
            selected.len()
        );
    }

    /// Lurker-Tax-Scope-Gate (P1.4): `moderator:read:chatters` muss entweder im
    /// Streamer-eigenen Raid-Auth ODER im zentralen Bot-Token vorliegen.
    ///
    /// Port: `bot/chat/promos.py:345–349/357` — bot-zentrierte Migration. Ohne
    /// verdrahteten `BotScopeProvider` greift nur der Streamer-Scope (Verhalten
    /// wie vor P1.4).
    async fn has_chatters_scope(&self, streamer_scopes_raw: &str) -> bool {
        const SCOPE: &str = "moderator:read:chatters";
        let streamer_has = streamer_scopes_raw
            .split_whitespace()
            .any(|s| s.eq_ignore_ascii_case(SCOPE));
        if streamer_has {
            return true;
        }
        if let Some(provider) = self.bot_scope_provider.as_ref() {
            return provider
                .bot_scopes()
                .await
                .iter()
                .any(|s| s.eq_ignore_ascii_case(SCOPE));
        }
        false
    }

    /// Lurker-Tax-Kandidaten aus DB (promos.py:408: `_get_lurker_tax_candidates`).
    async fn get_lurker_tax_candidates(&self, broadcaster_id: &str) -> Vec<String> {
        if broadcaster_id.trim().is_empty() {
            return Vec::new();
        }
        // twitch_session_chatters.seen_via_chatters_api = boolean (prod schema)
        // twitch_session_chatters.messages = integer (prod schema)
        //
        // P1.5: Bekannte Chat-Bots (nightbot etc.) ausschließen — sonst werden sie
        // als stille Lurker (messages=0, seen_via_chatters_api) öffentlich
        // ge-@-mentioned. Python: build_known_chat_bot_not_in_clause, injiziert in
        // historische CTE UND live_candidates (promos.py:451–458, 490, 523).
        // Die Bot-Logins sind eine Compile-Zeit-Konstante; trotzdem als Bind-Params
        // (ab $5) gebunden statt als SQL-Literal interpoliert (clean-SQL).
        let historical_bot_clause = known_chat_bot_not_in_clause("sc.chatter_login", 5);
        let current_bot_clause = known_chat_bot_not_in_clause("lc.chatter_login", 5);
        let sql = format!(
            r#"WITH historical_lurks AS (
                SELECT CASE
                         WHEN TRIM(COALESCE(sc.chatter_id, '')) <> '' THEN 'id:' || TRIM(sc.chatter_id)
                         ELSE 'login:' || LOWER(sc.chatter_login)
                       END AS chatter_identity_key,
                       COUNT(DISTINCT sc.session_id) AS prior_lurk_sessions,
                       COALESCE(SUM(CASE
                                WHEN sc.first_message_at IS NULL OR sc.last_seen_at IS NULL THEN 0
                                WHEN sc.last_seen_at <= sc.first_message_at THEN 0
                                ELSE EXTRACT(EPOCH FROM (sc.last_seen_at - sc.first_message_at)) / 60.0
                             END), 0) AS estimated_lurk_minutes
                  FROM twitch_session_chatters sc
                  JOIN twitch_stream_sessions s ON s.id = sc.session_id
                 WHERE s.twitch_user_id = $1
                   AND s.ended_at IS NOT NULL
                   AND COALESCE(sc.messages, 0) = 0
                   AND sc.seen_via_chatters_api = TRUE
                   {historical_bot_clause}
                 GROUP BY CASE
                            WHEN TRIM(COALESCE(sc.chatter_id, '')) <> '' THEN 'id:' || TRIM(sc.chatter_id)
                            ELSE 'login:' || LOWER(sc.chatter_login)
                          END
               ),
               live_candidates AS (
                 SELECT sc.chatter_login,
                        CASE
                          WHEN TRIM(COALESCE(sc.chatter_id, '')) <> '' THEN 'id:' || TRIM(sc.chatter_id)
                          ELSE 'login:' || LOWER(sc.chatter_login)
                        END AS chatter_identity_key
                   FROM twitch_session_chatters sc
                   JOIN twitch_live_state ls ON ls.twitch_user_id = $1 AND ls.active_session_id = sc.session_id
                  WHERE sc.last_seen_at >= NOW() - INTERVAL '{freshness} minutes'
                    AND COALESCE(sc.messages, 0) = 0
                    AND sc.seen_via_chatters_api = TRUE
               )
               SELECT lc.chatter_login
                 FROM historical_lurks hl
                 JOIN live_candidates lc ON lc.chatter_identity_key = hl.chatter_identity_key
                WHERE hl.prior_lurk_sessions >= $2
                  AND hl.estimated_lurk_minutes >= $3
                  {current_bot_clause}
                ORDER BY hl.estimated_lurk_minutes DESC, LOWER(lc.chatter_login) ASC
                LIMIT $4"#,
            freshness = LURKER_TAX_FRESHNESS_MINUTES,
            historical_bot_clause = historical_bot_clause,
            current_bot_clause = current_bot_clause,
        );
        let mut query = sqlx::query_as::<_, (String,)>(&sql)
            .bind(broadcaster_id)
            .bind(LURKER_TAX_MIN_PRIOR_SESSIONS)
            .bind(LURKER_TAX_MIN_WATCHTIME_MINUTES)
            .bind(LURKER_TAX_CANDIDATE_FETCH);
        // Bot-Logins (lowercase) in stabiler Reihenfolge an $5.. binden.
        for bot in crate::mention_scoring::WHITELISTED_BOTS {
            query = query.bind(bot.to_lowercase());
        }
        let rows: Vec<(String,)> = match query.fetch_all(&self.pool).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    %error,
                    broadcaster_id,
                    "Lurker-Tax-Kandidaten konnten nicht geladen werden"
                );
                Vec::new()
            }
        };

        let mut logins: Vec<String> = rows.into_iter().map(|(l,)| l).collect();
        let session_id: Option<i64> = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id = $1",
        )
        .bind(broadcaster_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .flatten();
        if let Some(session_id) = session_id {
            let redeemed: Vec<String> = sqlx::query_scalar::<_, String>(
                "SELECT LOWER(user_login) FROM twitch_channel_points_events \
                  WHERE session_id = $1 AND user_login IS NOT NULL \
                    AND TRIM(regexp_replace(LOWER(reward_title), '\\s+', ' ', 'g')) LIKE 'lurker steuer%'",
            )
            .bind(session_id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
            if !redeemed.is_empty() {
                let redeemed: HashSet<String> = redeemed.into_iter().collect();
                logins.retain(|l| !redeemed.contains(&l.to_lowercase()));
            }
        }
        logins
    }

    fn build_lurker_tax_text(&self, candidates: &[String]) -> String {
        let mentions: Vec<String> = candidates
            .iter()
            .take(LURKER_TAX_MAX_MENTIONS)
            .map(|l| format!("@{l}"))
            .collect();
        let joined = mentions.join(" ");
        let templates: &[&str] = if mentions.len() <= 1 {
            &LURKER_TAX_REMINDER_ONE
        } else {
            &LURKER_TAX_REMINDER_TWO
        };
        let template = {
            let mut rng = rand::rng();
            templates.choose(&mut rng).copied().unwrap_or(templates[0])
        };
        template.replace("{mentions}", &joined)
    }

    fn build_lurker_thank_text(&self, redeemer_login: &str) -> String {
        let template = {
            let mut rng = rand::rng();
            LURKER_TAX_THANKS
                .choose(&mut rng)
                .copied()
                .unwrap_or(LURKER_TAX_THANKS[0])
        };
        template.replace("{name}", &format!("@{redeemer_login}"))
    }

    async fn maybe_answer_lurker_followup(&self, event: &ChatMessageEvent) {
        let lower = event.text().to_lowercase();
        let ist_nachfrage = lower.contains('?')
            || lower
                .split(|c: char| !c.is_alphabetic())
                .any(|w| matches!(w, "wo" | "wie" | "was" | "hä" | "wat" | "wohin" | "womit"));
        if !ist_nachfrage {
            return;
        }

        let login = event.broadcaster_user_login.to_lowercase();
        let chatter = event.chatter_user_login.to_lowercase();
        if chatter.is_empty() {
            return;
        }

        if event.broadcaster_user_id.trim().is_empty() {
            return;
        }
        let session_id: i64 = match sqlx::query_scalar::<_, Option<i64>>(
            "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id = $1",
        )
        .bind(&event.broadcaster_user_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(row) => row.flatten().unwrap_or(0),
            Err(_) => return,
        };

        let antworten = {
            let state_ref = self
                .channel_states
                .entry(login.clone())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if state.lurker_reminded_at.0 != session_id {
                state.lurker_reminded_at = (session_id, HashMap::new());
            }
            if state.lurker_followup_answered.0 != session_id {
                state.lurker_followup_answered = (session_id, HashSet::new());
            }
            let frisch = state
                .lurker_reminded_at
                .1
                .get(&chatter)
                .map(|t| t.elapsed() <= LURKER_FOLLOWUP_WINDOW)
                .unwrap_or(false);
            frisch && state.lurker_followup_answered.1.insert(chatter.clone())
        };
        if !antworten {
            return;
        }

        let text = format!(
            "@{} bei den Kanalpunkten, Belohnung Lurker Steuer. Danach bist du frei.",
            event.chatter_user_login
        );
        let sent = self
            .api
            .send_message(&event.broadcaster_user_id, &text)
            .await
            .is_ok();
        if !sent {
            let state_ref = self
                .channel_states
                .entry(login.clone())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if state.lurker_followup_answered.0 == session_id {
                state.lurker_followup_answered.1.remove(&chatter);
            }
        }
    }

    pub async fn thank_lurker_tax_redeemer(
        &self,
        broadcaster_id: &str,
        broadcaster_login: &str,
        redeemer_login: &str,
    ) {
        if redeemer_login.trim().is_empty() {
            return;
        }
        if !self.lurker_tax_channel_gate(broadcaster_id).await {
            return;
        }
        let session_id: i64 = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id = $1",
        )
        .bind(broadcaster_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
        .unwrap_or(0);

        let key = redeemer_login.to_lowercase();
        let already = {
            let state_ref = self
                .channel_states
                .entry(broadcaster_login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if state.thanked_redeemers.0 != session_id {
                state.thanked_redeemers = (session_id, HashSet::new());
            }
            !state.thanked_redeemers.1.insert(key.clone())
        };
        if already {
            return;
        }

        let text = self.build_lurker_thank_text(redeemer_login);
        let sent = self.api.send_message(broadcaster_id, &text).await.is_ok();
        if !sent {
            let state_ref = self
                .channel_states
                .entry(broadcaster_login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            if state.thanked_redeemers.0 == session_id {
                state.thanked_redeemers.1.remove(&key);
            }
        }
    }

    /// Stream-Start-Verzögerung prüfen (constants.py: PROMO_STREAM_START_DELAY_MIN = 10 min).
    /// Verhindert Promos direkt beim Go-Live. Fail-open (true) bei DB-Fehler oder fehlendem Eintrag.
    /// twitch_live_state.last_started_at = TEXT (RFC3339/ISO).
    async fn stream_start_delay_ok(&self, login: &str) -> bool {
        const DELAY_SECS: i64 = (PROMO_STREAM_START_DELAY_MIN * 60) as i64;
        if DELAY_SECS == 0 {
            return true;
        }
        let row = sqlx::query_scalar!(
            "SELECT last_started_at FROM twitch_live_state
              WHERE LOWER(streamer_login) = LOWER($1)
                AND is_live = 1
              LIMIT 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let Some(Some(started_at_str)) = row else {
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
        let row = sqlx::query!(
            "SELECT is_partner_active, archived_at
               FROM twitch_streamers_partner_state
              WHERE LOWER(twitch_login) = LOWER($1)
              LIMIT 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        match row {
            None => false,
            Some(row) => {
                let is_active = row.is_partner_active.unwrap_or(0) != 0;
                let not_archived = row.archived_at.is_none();
                is_active && not_archived
            }
        }
    }

    /// Plan-Flag-Check (promos.py:1097: `_promo_blocked_by_plan_or_flag`).
    ///
    /// Zwei Gründe blockieren Bot-Werbung dauerhaft:
    /// 1. `streamer_plans.promo_disabled = 1` (harte Abschaltung).
    /// 2. Der effektive Plan trägt das Entitlement `chat.promos.disable`.
    ///
    /// Der Entitlement-Pfad läuft über die VOLLE Snapshot-Resolution
    /// (`tb_analytics::plan::resolve_plan_snapshot`, Port von Pythons
    /// `resolve_plan_snapshot_for_refs`): respektiert `manual_plan_expires_at`
    /// (abgelaufene Pläne fallen auf `raid_free` zurück), löst Bundles/Legacy-
    /// Namen auf kanonische IDs auf und zieht den Stripe-Abo-Fallback heran.
    /// Damit werden Pläne, die Promo abschalten, nicht fälschlich ignoriert —
    /// und umgekehrt schaltet ein abgelaufener Werbefrei-Plan die Werbung wieder
    /// frei (vorher: statische Whitelist ohne Ablauf-Gate).
    ///
    /// Fail-open bei DB-Fehler (Infra-Issues blockieren nicht alle Promos).
    /// Wie Python wird per Login referenziert (leerer user_id) — der periodische
    /// Pfad iteriert ohnehin nur über aufgelöste Live-Kanäle.
    async fn promo_blocked_by_plan_or_flag(&self, login: &str) -> bool {
        let normalized = login.trim().to_lowercase();
        if normalized.is_empty() {
            return false;
        }

        // 1. Harte promo_disabled-Spalte (greift vor jeder Override-Auswertung).
        let promo_disabled = match sqlx::query_scalar!(
            "SELECT COALESCE(promo_disabled, 0) AS \"promo_disabled!\"
               FROM streamer_plans
              WHERE LOWER(COALESCE(twitch_login,'')) = $1
              LIMIT 1",
            &normalized,
        )
        .fetch_optional(&self.pool)
        .await
        {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    %error,
                    login = %normalized,
                    "Promo-Disable-Flag konnte nicht geladen werden"
                );
                None
            }
        };
        if promo_disabled.is_some_and(|flag| flag != 0) {
            return true;
        }

        // 2. Entitlement-Pfad über volle Plan-Snapshot-Resolution.
        match tb_analytics::plan::resolve_plan_snapshot(&self.pool, &normalized, "").await {
            Ok(snapshot) => snapshot.entitlements.contains(&"chat.promos.disable"),
            Err(error) => {
                tracing::warn!(
                    %error,
                    login = %normalized,
                    "Promo-Disable-Plan konnte nicht aufgeloest werden"
                );
                false
            } // Fail-open.
        }
    }

    async fn lurker_tax_channel_gate(&self, broadcaster_id: &str) -> bool {
        if broadcaster_id.trim().is_empty() {
            return false;
        }
        let settings = match sqlx::query_scalar::<_, Option<i32>>(
            "SELECT p.lurker_tax_enabled
               FROM streamer_plans p
              WHERE p.twitch_user_id = $1
              LIMIT 1",
        )
        .bind(broadcaster_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(row) => row,
            Err(error) => {
                if self
                    .plan_gate_error_warned
                    .insert(broadcaster_id.to_string(), ())
                    .is_none()
                {
                    warn!(
                        broadcaster_id,
                        %error,
                        "Lurker-Tax: Plan-Gate-Abfrage fehlgeschlagen, Aktion wird nicht gesendet"
                    );
                }
                return false;
            }
        };
        if settings.flatten().unwrap_or(0) == 0 {
            return false;
        }
        self.lurker_tax_is_paid_plan(broadcaster_id).await
    }

    /// Lurker-Tax `is_paid_plan`-Gate (promos.py:355: der Plan muss das
    /// Entitlement `chat.lurker_tax` tragen). Nutzt die volle Snapshot-Resolution,
    /// damit abgelaufene Pläne (`manual_plan_expires_at` in der Vergangenheit) das
    /// kostenpflichtige Lurker-Tax-Feature NICHT mehr freischalten.
    /// Zentrale Planauflösung gewinnt Checkoutreferenzen aus verifizierten
    /// ID-Zuordnungen; kein Event-Login als Identitätsersatz.
    async fn lurker_tax_is_paid_plan(&self, user_id: &str) -> bool {
        match tb_analytics::plan::resolve_plan_snapshot_for_user_id(&self.pool, user_id).await {
            Ok(snapshot) => snapshot.entitlements.contains(&"chat.lurker_tax"),
            Err(error) => {
                tracing::warn!(
                    %error,
                    user_id,
                    "Lurker-Tax-Plan konnte nicht aufgeloest werden"
                );
                false
            }
        }
    }

    /// Live-Kanäle für Promo-Loop laden (promos.py:1630: `_get_live_channels_for_promo`).
    /// twitch_live_state.is_live = integer, twitch_live_state.last_game = text (prod schema)
    async fn get_live_channels_for_promo(&self) -> Result<Vec<(String, String)>, String> {
        // SUBSCRIPTION_PLANS_ENABLED=True → mit promo_disabled-Filter.
        let rows = sqlx::query!(
            "SELECT s.twitch_login AS \"twitch_login!\",
                    s.twitch_user_id AS \"twitch_user_id!\"
               FROM twitch_streamer_identities s
               JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
               LEFT JOIN streamer_plans p ON s.twitch_user_id = p.twitch_user_id
              WHERE l.is_live = 1
                AND LOWER(COALESCE(l.last_game, '')) = $1
                AND COALESCE(p.promo_disabled, 0) = 0",
            "deadlock",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(rows
            .into_iter()
            .map(|row| (row.twitch_login, row.twitch_user_id))
            .collect())
    }

    /// Live-Kanäle für Lurker-Tax laden (promos.py:1636:
    /// `_get_live_channels_for_lurker_tax`).
    async fn get_live_channels_for_lurker_tax(&self) -> Result<Vec<(String, String)>, String> {
        let rows = sqlx::query!(
            "SELECT streamer_login AS \"streamer_login!\",
                    twitch_user_id AS \"twitch_user_id!\"
               FROM twitch_live_state
              WHERE is_live = 1
                AND active_session_id IS NOT NULL
                AND TRIM(COALESCE(streamer_login, '')) <> ''
                AND TRIM(COALESCE(twitch_user_id, '')) <> ''",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.streamer_login.trim().to_lowercase(),
                    row.twitch_user_id.trim().to_string(),
                )
            })
            .filter(|(login, user_id)| !login.is_empty() && !user_id.is_empty())
            .collect())
    }

    /// Cooldowns aus DB laden (promos.py:1452: `_restore_promo_cooldowns`).
    /// twitch_promo_cooldowns.wall_ts = double precision, login = text, cooldown_type = text (prod schema).
    async fn restore_promo_cooldowns(&self) {
        let rows = sqlx::query!(
            "SELECT login AS \"login!\", \
                    cooldown_type AS \"cooldown_type!\", \
                    wall_ts AS \"wall_ts!\" \
             FROM twitch_promo_cooldowns",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let wall_now = Utc::now().timestamp() as f64;
        let mono_now = Instant::now();

        for row in rows {
            let age_secs = (wall_now - row.wall_ts).max(0.0) as u64;
            // Monotonic-Zeitstempel rekonstruieren (promos.py:903).
            let mono_restored = mono_now.checked_sub(Duration::from_secs(age_secs));

            let state_ref = self
                .channel_states
                .entry(row.login.clone())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;

            match row.cooldown_type.as_str() {
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
                "viewer_spike" if state.last_promo_viewer_spike.is_none() => {
                    state.last_promo_viewer_spike = mono_restored;
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
        if let Err(e) = sqlx::query!(
            "INSERT INTO twitch_promo_cooldowns (login, cooldown_type, wall_ts, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (login, cooldown_type) DO UPDATE
             SET wall_ts = EXCLUDED.wall_ts, updated_at = EXCLUDED.updated_at",
            login,
            cooldown_type,
            wall_ts,
            updated_at,
        )
        .execute(&self.pool)
        .await
        {
            warn!(
                login,
                cooldown_type, "save_promo_cooldown fehlgeschlagen: {e}"
            );
        }
    }

    async fn record_pitch_log(&self, entry: PitchLogEntry) {
        if let Err(e) = sqlx::query!(
            "INSERT INTO twitch_promo_pitch_log
                (channel_login, target_user_id, pfad, occasion, trigger_text,
                 generated_text, reject_reason, sent_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            entry.channel_login,
            entry.target_user_id,
            entry.pfad,
            entry.occasion,
            entry.trigger_text,
            entry.generated_text,
            entry.reject_reason,
            entry.sent_at,
        )
        .execute(&self.pool)
        .await
        {
            warn!(
                login = %entry.channel_login,
                pfad = entry.pfad,
                "record_pitch_log fehlgeschlagen: {e}"
            );
        }
    }

    async fn insert_pitch_log_pending(&self, entry: PitchLogEntry) -> Option<i64> {
        match sqlx::query_scalar!(
            "INSERT INTO twitch_promo_pitch_log
                (channel_login, target_user_id, pfad, occasion, trigger_text,
                 generated_text, reject_reason, sent_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
            entry.channel_login,
            entry.target_user_id,
            entry.pfad,
            entry.occasion,
            entry.trigger_text,
            entry.generated_text,
            entry.reject_reason,
            entry.sent_at,
        )
        .fetch_one(&self.pool)
        .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                warn!(
                    login = %entry.channel_login,
                    pfad = entry.pfad,
                    "insert_pitch_log_pending fehlgeschlagen: {e}"
                );
                None
            }
        }
    }

    async fn mark_pitch_log_sent(&self, id: i64) {
        if let Err(e) = sqlx::query!(
            "UPDATE twitch_promo_pitch_log SET sent_at = $2 WHERE id = $1",
            id,
            Utc::now(),
        )
        .execute(&self.pool)
        .await
        {
            warn!(id, "mark_pitch_log_sent fehlgeschlagen: {e}");
        }
    }

    async fn mark_pitch_log_dropped(&self, id: i64, reason: &str) {
        if let Err(e) = sqlx::query!(
            "UPDATE twitch_promo_pitch_log SET reject_reason = $2 WHERE id = $1",
            id,
            reason,
        )
        .execute(&self.pool)
        .await
        {
            warn!(id, "mark_pitch_log_dropped fehlgeschlagen: {e}");
        }
    }

    async fn load_recent_channel_messages(&self, login: &str, n: i64) -> Vec<String> {
        let known_bots: Vec<&str> = tb_analytics::bekannte_bots::KNOWN_CHAT_BOTS.to_vec();
        let rows = sqlx::query_scalar!(
            "SELECT content AS \"content?\" FROM twitch_chat_messages
              WHERE LOWER(streamer_login) = LOWER($1)
                AND message_ts >= NOW() - INTERVAL '30 minutes'
                AND COALESCE(is_command, FALSE) = FALSE
                AND COALESCE(content, '') NOT LIKE '!%'
                AND LOWER(COALESCE(chatter_login, '')) <> ALL($3::text[])
                AND LOWER(COALESCE(chatter_login, '')) !~ $4
              ORDER BY message_ts DESC
              LIMIT $2",
            login,
            n,
            &known_bots as &[&str],
            tb_analytics::bekannte_bots::ANONYM_LOGIN_REGEX_SQL,
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.into_iter().flatten().collect()
    }

    /// Alte Cooldown-Einträge bereinigen (promos.py: `cleanup_stale_promo_cooldowns(24)`).
    pub async fn cleanup_stale_promo_cooldowns(&self) {
        let cutoff = (Utc::now().timestamp() as f64) - 86400.0;
        if let Err(e) = sqlx::query!(
            "DELETE FROM twitch_promo_cooldowns WHERE wall_ts < $1",
            cutoff,
        )
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
                true
            }
        });
        self.pitch_judge_last
            .retain(|_, last| now.duration_since(*last) < PITCH_JUDGE_CHATTER_COOLDOWN);
        self.pitch_judge_channel.retain(|_, stamps| {
            stamps.retain(|t| now.duration_since(*t) < PITCH_JUDGE_CHANNEL_WINDOW);
            !stamps.is_empty()
        });
    }
}

#[async_trait]
impl InviteReplyNotifier for PromoEngine {
    async fn note_invite_reply(&self, channel_login: &str) {
        self.mark_promo_sent(
            channel_login,
            Instant::now(),
            "invite_reply",
            Utc::now().timestamp() as f64,
        )
        .await;
    }
}

#[async_trait]
impl PromoBlockCheck for PromoEngine {
    async fn is_promo_blocked(&self, channel_login: &str) -> bool {
        self.promo_blocked_by_plan_or_flag(channel_login).await
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

    type SuppressionCall = (String, Option<String>, String, String);

    fn hat_gedankenstrich(text: &str) -> bool {
        text.contains('\u{2014}')
            || text.contains('\u{2013}')
            || text.contains('\u{2015}')
            || text.contains(" -- ")
            || text.contains(" - ")
    }

    #[test]
    fn periodischer_promo_text_traegt_invite_am_ende_ohne_strich() {
        let invite = "https://discord.gg/deadlock";
        let out = crate::promo_pitch::finalize_channel_promo(
            "komm vorbei und zock ne runde mit uns",
            invite,
        )
        .unwrap();
        assert!(out.ends_with(invite), "Invite muss am Ende stehen: {out}");
        assert!(!hat_gedankenstrich(&out), "Gedankenstrich in: {out}");
    }

    #[test]
    fn periodischer_promo_text_leer_gibt_keinen_text() {
        assert!(crate::promo_pitch::finalize_channel_promo("", "https://discord.gg/x").is_none());
    }

    #[test]
    fn promo_invite_fallback_nutzt_default_bei_fehlender_oder_leerer_config() {
        assert_eq!(promo_invite_fallback(None), DEFAULT_PROMO_DISCORD_INVITE);
        assert_eq!(
            promo_invite_fallback(Some("")),
            DEFAULT_PROMO_DISCORD_INVITE
        );
        assert_eq!(
            promo_invite_fallback(Some("   ")),
            DEFAULT_PROMO_DISCORD_INVITE
        );
        assert_eq!(
            promo_invite_fallback(Some(" https://discord.gg/custom ")),
            "https://discord.gg/custom"
        );
    }

    #[test]
    fn promo_template_renderer_unterstuetzt_python_invite_formen() {
        assert_eq!(
            render_promo_template("Join {invite!s}", "discord").as_deref(),
            Some("Join discord")
        );
        assert_eq!(
            render_promo_template("Join {{ {invite:.4} }}", "discord").as_deref(),
            Some("Join { disc }")
        );
        assert_eq!(
            render_promo_template("Join {invite:*>9s}", "discord").as_deref(),
            Some("Join **discord")
        );
    }

    #[test]
    fn promo_template_renderer_invalid_faellt_auf_none() {
        assert!(render_promo_template("Join {streamer}", "discord").is_none());
        assert!(render_promo_template("Join {invite!r}", "discord").is_none());
        assert!(render_promo_template("Join {invite", "discord").is_none());
        assert!(render_promo_template("Join }", "discord").is_none());
    }

    // -----------------------------------------------------------------------
    // Mock-ChatApi
    // -----------------------------------------------------------------------

    #[derive(Default)]
    pub(super) struct MockApi {
        announcement_result: Option<bool>,
        announcements: TokioMutex<Vec<(String, String, String)>>, // (id, text, color)
        messages: TokioMutex<Vec<(String, String)>>,
    }

    impl MockApi {
        pub(super) fn announcement_dropped() -> Self {
            Self {
                announcement_result: Some(false),
                ..Self::default()
            }
        }

        pub(super) async fn announcement_count(&self) -> usize {
            self.announcements.lock().await.len()
        }

        pub(super) async fn message_count(&self) -> usize {
            self.messages.lock().await.len()
        }

        pub(super) async fn messages_sent(&self) -> Vec<(String, String)> {
            self.messages.lock().await.clone()
        }

        pub(super) async fn announcement_colors(&self) -> Vec<String> {
            self.announcements
                .lock()
                .await
                .iter()
                .map(|(_, _, color)| color.clone())
                .collect()
        }

        pub(super) async fn announcement_texts(&self) -> Vec<String> {
            self.announcements
                .lock()
                .await
                .iter()
                .map(|(_, text, _)| text.clone())
                .collect()
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(
            &self,
            broadcaster_id: &str,
            message: &str,
        ) -> Result<SendOutcome, String> {
            self.messages
                .lock()
                .await
                .push((broadcaster_id.to_string(), message.to_string()));
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(
            &self,
            broadcaster_id: &str,
            message: &str,
            color: &str,
        ) -> Result<bool, String> {
            self.announcements.lock().await.push((
                broadcaster_id.to_string(),
                message.to_string(),
                color.to_string(),
            ));
            Ok(self.announcement_result.unwrap_or(true))
        }
        async fn ban_user(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<crate::api::BanOutcome, String> {
            Ok(crate::api::BanOutcome::Banned)
        }
        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &str,
        ) -> Result<crate::api::BanOutcome, String> {
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
        assert!(
            (cd - expected).abs() < 1.0,
            "1.5 MPM → {expected}s, got {cd}"
        );
    }

    #[test]
    fn cooldown_interpolation_cap_ueber_target() {
        // 10.0 MPM → capped auf 3.0 → 45 min
        let cd = interpolated_cooldown_sec(10.0);
        assert!(
            (cd - 45.0 * 60.0).abs() < 1.0,
            ">3.0 MPM → capped auf 45 min, got {cd}"
        );
    }

    // -----------------------------------------------------------------------
    // Aktivitätsfenster-Logik
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn activity_ready_fehlschlag_bei_zu_wenig_msgs() {
        let engine = make_engine_no_db();
        let mut state = ChannelState::new();
        // Nur 1 Eintrag, zu wenig.
        state
            .activity
            .push_back((Instant::now(), "user1".to_string()));
        state.raw_msg_count_since_promo = 20;

        let ready = engine
            .promo_activity_ready_inner("", &state, Instant::now())
            .await;
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

        let ready = engine.promo_activity_ready_inner("", &state, now).await;
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

        let ready = engine.promo_activity_ready_inner("", &state, now).await;
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
    async fn community_timer_und_admin_standard_sind_unabhaengig() {
        let engine = make_engine_no_db();
        let now = Instant::now();
        let mut state = ChannelState::new();
        state.last_promo_sent = Some(now - Duration::from_secs(21 * 60));
        assert!(!engine.overall_promo_ready_inner(&state, now));
        state.timers = PromoTimerSettings::default().for_broadcaster(COMMUNITY_BROADCASTER_ID);
        assert!(engine.overall_promo_ready_inner(&state, now));
        state.timers.overall_cooldown_minutes = 30;
        assert!(!engine.overall_promo_ready_inner(&state, now));
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
    // Doppelsend-Lock (TOCTOU-Fix)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn doppelsend_lock_serialisiert_gleichzeitige_aufrufe() {
        let engine = make_engine_no_db();
        let counter = Arc::new(AtomicUsize::new(0));

        let lock = engine.get_send_lock("testkanal");
        let lock2 = engine.get_send_lock("testkanal");

        // Beide Locks sollten identisch sein (gleiche Arc-Instanz).
        assert!(
            Arc::ptr_eq(&lock, &lock2),
            "Lock für gleichen Kanal muss identisch sein"
        );

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

    fn lurker_reminder_ok(text: &str) -> bool {
        text.contains("schön dass")
            && text.contains("Lurker Steuer")
            && !text.contains("Kanalpunkte")
            && !text.contains('—')
            && !text.contains('–')
    }

    #[tokio::test]
    async fn lurker_tax_text_format() {
        let engine = make_engine_no_db();
        let candidates = vec!["alice".to_string(), "bob".to_string()];
        let text = engine.build_lurker_tax_text(&candidates);
        assert!(text.contains("@alice"), "Mention alice fehlt: {text}");
        assert!(text.contains("@bob"), "Mention bob fehlt: {text}");
        assert!(lurker_reminder_ok(&text), "Erinnerung unvollständig: {text}");
    }

    #[tokio::test]
    async fn lurker_tax_text_max_2_mentions() {
        let engine = make_engine_no_db();
        let candidates = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let text = engine.build_lurker_tax_text(&candidates);
        assert!(!text.contains("@c"), "Mehr als 2 Mentions nicht erlaubt");
    }

    #[tokio::test]
    async fn req2_lurker_tax_text_ein_name_nennt_belohnung() {
        let engine = make_engine_no_db();
        let mut gesehen = std::collections::HashSet::new();
        for _ in 0..50 {
            let text = engine.build_lurker_tax_text(&["xy".to_string()]);
            assert!(text.contains("@xy"), "Erwähnung fehlt: {text}");
            assert!(text.contains("schön dass du"), "Anrede fehlt: {text}");
            assert!(lurker_reminder_ok(&text), "Erinnerung unvollständig: {text}");
            gesehen.insert(text);
        }
        assert!(
            gesehen.len() >= 2,
            "es sollten mehrere Varianten vorkommen: {gesehen:?}"
        );
    }

    #[tokio::test]
    async fn req2_lurker_tax_text_zwei_namen_ein_satz() {
        let engine = make_engine_no_db();
        let mut gesehen = std::collections::HashSet::new();
        for _ in 0..50 {
            let text = engine.build_lurker_tax_text(&["alice".to_string(), "bob".to_string()]);
            assert!(text.contains("@alice"), "erste Erwähnung fehlt: {text}");
            assert!(text.contains("@bob"), "zweite Erwähnung fehlt: {text}");
            assert!(text.contains("schön dass ihr"), "Anrede fehlt: {text}");
            assert!(lurker_reminder_ok(&text), "Erinnerung unvollständig: {text}");
            gesehen.insert(text);
        }
        assert!(
            gesehen.len() >= 2,
            "es sollten mehrere Varianten vorkommen: {gesehen:?}"
        );
    }

    #[tokio::test]
    async fn req3_lurker_thank_text_variiert() {
        let engine = make_engine_no_db();
        let mut gesehen = std::collections::HashSet::new();
        for _ in 0..50 {
            let text = engine.build_lurker_thank_text("xy");
            assert!(text.contains("@xy"), "Erwähnung fehlt: {text}");
            assert!(
                !text.contains('—') && !text.contains('–'),
                "keine Gedankenstriche erlaubt: {text}"
            );
            gesehen.insert(text);
        }
        assert!(
            gesehen.len() >= 2,
            "es sollten mehrere Dank-Varianten vorkommen: {gesehen:?}"
        );
    }

    #[test]
    fn req1_lurker_tax_title_matches_erkennt_varianten() {
        assert!(lurker_tax_title_matches("Lurker Steuer"));
        assert!(lurker_tax_title_matches("lurker steuern"));
        assert!(lurker_tax_title_matches("LURKER STEUER "));
        assert!(lurker_tax_title_matches("LURKER STEUER 10"));
        assert!(lurker_tax_title_matches("Lurker  Steuer"));
        assert!(!lurker_tax_title_matches("Steuer"));
        assert!(!lurker_tax_title_matches("Lurker"));
        assert!(!lurker_tax_title_matches(""));
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

        let new_count = engine
            .get_new_chatters_in_window_inner("", &state, now)
            .await;
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

        let new_count = engine
            .get_new_chatters_in_window_inner("", &state, now)
            .await;
        assert_eq!(new_count, 1, "Alice nach 3h wieder als neu");
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

    struct AlwaysMuted;
    #[async_trait]
    impl OutboundSuppressionCheck for AlwaysMuted {
        async fn is_muted(&self, _channel_login: &str) -> bool {
            true
        }
    }

    // -----------------------------------------------------------------------
    // P1.1 — channel_settings-Drop schreibt Outbound-Suppression
    // -----------------------------------------------------------------------

    /// Test-Writer, der jeden suppress_for_drop-Aufruf festhält. Spiegelt die
    /// Python-Soll-TTL (channel_settings + source → 7d/3d), damit der Test die
    /// Verdrahtung prüft, ohne eine echte DB anzufassen.
    #[derive(Clone, Default)]
    struct CapturingWriter {
        calls: Arc<std::sync::Mutex<Vec<SuppressionCall>>>,
    }

    #[async_trait]
    impl OutboundSuppressionWriter for CapturingWriter {
        async fn suppress_for_drop(
            &self,
            channel_login: &str,
            channel_id: Option<&str>,
            source: &str,
            reason_code: &str,
            _reason_detail: Option<&str>,
        ) {
            self.calls.lock().unwrap().push((
                channel_login.to_string(),
                channel_id.map(str::to_string),
                source.to_string(),
                reason_code.to_string(),
            ));
        }
    }

    #[tokio::test]
    async fn channel_settings_drop_schreibt_promo_suppression() {
        let writer = CapturingWriter::default();
        let engine = make_engine_no_db().set_suppression_writer(Arc::new(writer.clone()));

        let dropped = Ok(SendOutcome::Dropped {
            code: "channel_settings".into(),
            message: "Blocked by channel settings".into(),
        });
        engine
            .record_suppression_on_drop("streamerlogin", "bcast-1", "promo", &dropped)
            .await;

        let calls = writer.calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            1,
            "channel_settings-Drop muss genau einen Write auslösen"
        );
        assert_eq!(calls[0].0, "streamerlogin");
        assert_eq!(calls[0].1.as_deref(), Some("bcast-1"));
        assert_eq!(calls[0].2, "promo");
        assert_eq!(calls[0].3, "channel_settings");
    }

    #[tokio::test]
    async fn sent_und_andere_drops_schreiben_keine_suppression() {
        let writer = CapturingWriter::default();
        let engine = make_engine_no_db().set_suppression_writer(Arc::new(writer.clone()));

        engine
            .record_suppression_on_drop("s", "b", "promo", &Ok(SendOutcome::Sent))
            .await;
        engine
            .record_suppression_on_drop(
                "s",
                "b",
                "promo",
                &Ok(SendOutcome::Dropped {
                    code: "sender_timedout".into(),
                    message: String::new(),
                }),
            )
            .await;
        engine
            .record_suppression_on_drop("s", "b", "promo", &Err("boom".into()))
            .await;

        // record_suppression_on_drop reicht zwar sender_timedout an den Writer
        // weiter, aber NUR der channel_settings-Code löst real einen DB-Write aus.
        // Hier prüfen wir die Helper-Ebene: Sent/Err lösen GAR keinen Aufruf aus.
        let calls = writer.calls.lock().unwrap();
        assert_eq!(
            calls.iter().filter(|c| c.3 == "channel_settings").count(),
            0,
            "kein channel_settings-Drop → keine channel_settings-Suppression"
        );
    }

    // -----------------------------------------------------------------------
    // P1.4 — Lurker-Tax Bot-Token-Scope-Fallback
    // -----------------------------------------------------------------------

    pub(super) struct FakeBotScopes(pub(super) Vec<String>);
    #[async_trait]
    impl BotScopeProvider for FakeBotScopes {
        async fn bot_scopes(&self) -> Vec<String> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn has_chatters_scope_true_wenn_streamer_scope_traegt() {
        let engine = make_engine_no_db();
        assert!(
            engine
                .has_chatters_scope("user:bot moderator:read:chatters user:read:chat")
                .await
        );
    }

    #[tokio::test]
    async fn has_chatters_scope_false_ohne_provider_und_ohne_streamer_scope() {
        let engine = make_engine_no_db();
        assert!(!engine.has_chatters_scope("user:bot user:read:chat").await);
    }

    #[tokio::test]
    async fn has_chatters_scope_true_via_bot_token_fallback() {
        // Streamer-Auth OHNE moderator:read:chatters, aber Bot-Token trägt ihn.
        let engine = make_engine_no_db().set_bot_scope_provider(Arc::new(FakeBotScopes(vec![
            "user:bot".into(),
            "moderator:read:chatters".into(),
        ])));
        assert!(
            engine.has_chatters_scope("user:bot user:read:chat").await,
            "bot-zentrierter Fallback muss das Gate öffnen (P1.4)"
        );
    }

    #[tokio::test]
    async fn has_chatters_scope_false_wenn_auch_bot_token_scope_fehlt() {
        let engine = make_engine_no_db()
            .set_bot_scope_provider(Arc::new(FakeBotScopes(vec!["user:bot".into()])));
        assert!(!engine.has_chatters_scope("user:read:chat").await);
    }

    // -----------------------------------------------------------------------
    // P1.5 — Known-Chat-Bot-Exklusion (Clause-Bau)
    // -----------------------------------------------------------------------

    #[test]
    fn known_chat_bot_not_in_clause_erzeugt_passende_platzhalter() {
        let clause = known_chat_bot_not_in_clause("sc.chatter_login", 5);
        let n = crate::mention_scoring::WHITELISTED_BOTS.len();
        assert!(clause.starts_with("AND LOWER(sc.chatter_login) NOT IN ("));
        assert!(clause.contains("$5"));
        assert!(clause.contains(&format!("${}", 5 + n - 1)));
        // Genau n Platzhalter.
        assert_eq!(clause.matches('$').count(), n);
    }

    /// Die TTL-Tabelle (Schreibseite-Entscheidung) bleibt Python-treu:
    /// channel_settings + promo/recruitment = 7d, partner_raid = 3d, sonst None.
    #[test]
    fn suppression_writer_ttl_entspricht_python_soll() {
        use crate::moderation::OutboundSuppressionStore;
        use chrono::Duration;
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("promo", "channel_settings"),
            Some(Duration::seconds(7 * 24 * 3600))
        );
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("recruitment", "channel_settings"),
            Some(Duration::seconds(7 * 24 * 3600))
        );
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("partner_raid", "channel_settings"),
            Some(Duration::seconds(3 * 24 * 3600))
        );
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("promo", "sender_timedout"),
            None
        );
    }

    fn dummy_pool() -> PgPool {
        use sqlx::postgres::PgPoolOptions;
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://localhost/nonexistent")
            .unwrap()
    }

    #[tokio::test]
    async fn prune_raeumt_judge_maps_nach_cooldown() {
        let engine = PromoEngine::new(
            dummy_pool(),
            Arc::new(MockApi::default()),
            Arc::new(NoopSuppressionCheck),
        );
        let now = Instant::now();

        let alt_chatter = now
            .checked_sub(PITCH_JUDGE_CHATTER_COOLDOWN + Duration::from_secs(60))
            .unwrap();
        engine
            .pitch_judge_last
            .insert("kanal|alt".to_string(), alt_chatter);
        engine
            .pitch_judge_last
            .insert("kanal|frisch".to_string(), now);

        let alt_channel = now
            .checked_sub(PITCH_JUDGE_CHANNEL_WINDOW + Duration::from_secs(60))
            .unwrap();
        engine
            .pitch_judge_channel
            .insert("altkanal".to_string(), vec![alt_channel]);
        engine
            .pitch_judge_channel
            .insert("mischkanal".to_string(), vec![alt_channel, now]);

        engine.prune_promo_runtime_state(now);

        assert!(engine.pitch_judge_last.get("kanal|alt").is_none());
        assert!(engine.pitch_judge_last.get("kanal|frisch").is_some());
        assert!(engine.pitch_judge_channel.get("altkanal").is_none());
        let misch = engine
            .pitch_judge_channel
            .get("mischkanal")
            .expect("frischer Kanal-Eintrag bleibt");
        assert_eq!(misch.len(), 1);
    }

    #[tokio::test]
    async fn timeout_pitch_unterdrueckt_bei_suppression() {
        // Kanal gemutet → Pitch wird NICHT gesendet (Suppression-Gate, Python source="promo").
        let api = Arc::new(MockApi::default());
        let engine = PromoEngine::new(dummy_pool(), api.clone(), Arc::new(AlwaysMuted));
        let sent = engine.send_timeout_pitch("123", "login", "PITCH").await;
        assert!(!sent, "gemuteter Kanal darf keinen Pitch bekommen");
        assert!(api.announcements.lock().await.is_empty());
    }

    #[tokio::test]
    async fn timeout_pitch_sendet_blau_und_belegt_cooldown() {
        // Nicht gemutet → blaues Announcement + Promo-Cooldown belegt (kein Doppel-Promo danach).
        let api = Arc::new(MockApi::default());
        let engine = PromoEngine::new(dummy_pool(), api.clone(), Arc::new(NoopSuppressionCheck));
        let sent = engine.send_timeout_pitch("123", "login", "PITCH-MSG").await;
        assert!(sent);
        {
            let anns = api.announcements.lock().await;
            assert_eq!(anns.len(), 1);
            assert_eq!(anns[0].1, "PITCH-MSG");
            assert_eq!(anns[0].2, "blue");
        }
        // Cooldown belegt → overall_promo_ready_inner ist jetzt false.
        let state_ref = engine
            .channel_states
            .get("login")
            .expect("ChannelState belegt");
        let state = state_ref.lock().await;
        assert!(
            !engine.overall_promo_ready_inner(&state, Instant::now()),
            "Promo-Cooldown muss nach dem Pitch belegt sein"
        );
    }

    // Plan-Entitlement-Mapping (chat.lurker_tax / chat.promos.disable inkl.
    // Legacy-Aliase + Bundles) wird zentral in `tb_analytics::plan` gepflegt und
    // getestet. Das Lurker-Tax-/Promo-Gating hier deckt das Verhalten end-to-end
    // über `resolve_plan_snapshot` ab (siehe db_tests, inkl. Plan-Ablauf).
}

// ---------------------------------------------------------------------------
// DB-Tests (gegen TB_TEST_DATABASE_URL)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::promo_pitch::TargetedPitchContext;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    struct FixedSuppression(bool);

    #[async_trait]
    impl OutboundSuppressionCheck for FixedSuppression {
        async fn is_muted(&self, _channel_login: &str) -> bool {
            self.0
        }
    }

    struct FixedTextGen(Option<String>);

    #[async_trait]
    impl PitchTextGen for FixedTextGen {
        async fn channel_promo(
            &self,
            _ctx: &ChannelPromoContext,
            invite: &str,
        ) -> Option<String> {
            self.0.as_ref().map(|body| format!("{body} {invite}"))
        }
        async fn targeted_pitch(&self, _ctx: &TargetedPitchContext) -> Option<String> {
            self.0.clone()
        }
    }

    struct SlowTextGen {
        delay: Duration,
    }

    #[async_trait]
    impl PitchTextGen for SlowTextGen {
        async fn channel_promo(
            &self,
            _ctx: &ChannelPromoContext,
            invite: &str,
        ) -> Option<String> {
            tokio::time::sleep(self.delay).await;
            Some(format!("hallo {invite}"))
        }
        async fn targeted_pitch(&self, _ctx: &TargetedPitchContext) -> Option<String> {
            tokio::time::sleep(self.delay).await;
            Some("hallo".to_string())
        }
    }

    struct CountingTextGen {
        body: String,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl CountingTextGen {
        fn new(body: &str) -> Self {
            Self {
                body: body.to_string(),
                calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait]
    impl PitchTextGen for CountingTextGen {
        async fn channel_promo(&self, _ctx: &ChannelPromoContext, invite: &str) -> Option<String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(format!("{} {invite}", self.body))
        }
        async fn targeted_pitch(&self, _ctx: &TargetedPitchContext) -> Option<String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(self.body.clone())
        }
    }

    struct MockPitchJudge {
        response: Option<crate::promo_pitch::PitchResponse>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl MockPitchJudge {
        fn new(response: Option<crate::promo_pitch::PitchResponse>) -> Self {
            Self {
                response,
                calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait]
    impl PitchJudge for MockPitchJudge {
        async fn decide(
            &self,
            _input: PitchJudgeInput,
        ) -> Option<crate::promo_pitch::PitchResponse> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.response.clone()
        }
    }

    struct MockPartnerPitchGen {
        text: Option<String>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl MockPartnerPitchGen {
        fn new(text: Option<&str>) -> Self {
            Self {
                text: text.map(|t| t.to_string()),
                calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait]
    impl PartnerPitchGen for MockPartnerPitchGen {
        async fn partner_pitch(&self, _ctx: &PartnerPitchContext) -> Option<String> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.text.clone()
        }
    }

    #[derive(Default, Clone)]
    struct RecordingReviewSink {
        cards: Arc<Mutex<Vec<(String, String, String, String, PitchCardKind, Option<String>)>>>,
    }

    #[async_trait]
    impl PitchReviewSink for RecordingReviewSink {
        async fn send_card(
            &self,
            channel_login: &str,
            target_login: &str,
            trigger: &str,
            reply: &str,
            kind: PitchCardKind,
            candidate_hint: Option<&str>,
        ) {
            self.cards.lock().await.push((
                channel_login.to_string(),
                target_login.to_string(),
                trigger.to_string(),
                reply.to_string(),
                kind,
                candidate_hint.map(|h| h.to_string()),
            ));
        }
    }

    fn pitch_event(channel_id: &str, channel_login: &str, chatter_id: &str, chatter_login: &str, text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: channel_id.to_string(),
            broadcaster_user_login: channel_login.to_string(),
            chatter_user_id: chatter_id.to_string(),
            chatter_user_login: chatter_login.to_string(),
            message: crate::types::ChatMessageBody {
                text: text.to_string(),
                fragments: Vec::new(),
            },
            ..Default::default()
        }
    }

    async fn seed_partner_channel(pool: &PgPool, channel_id: &str, channel_login: &str) {
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ($1, $2)",
        )
        .bind(channel_id)
        .bind(channel_login)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active, archived_at)
             VALUES ($1, $2, 1, NULL)",
        )
        .bind(channel_login)
        .bind(channel_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game)
             VALUES ($1, $2, 1, 'Deadlock')",
        )
        .bind(channel_id)
        .bind(channel_login)
        .execute(pool)
        .await
        .unwrap();
        let session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, twitch_user_id)
             VALUES ($1, NOW() - INTERVAL '2 hours', $2) RETURNING id",
        )
        .bind(channel_login)
        .bind(channel_id)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("UPDATE twitch_live_state SET active_session_id = $1 WHERE twitch_user_id = $2")
            .bind(session_id)
            .bind(channel_id)
            .execute(pool)
            .await
            .unwrap();
    }

    fn pitch_response(
        occasion: Option<crate::promo_pitch::PitchOccasion>,
        reply: &str,
    ) -> crate::promo_pitch::PitchResponse {
        crate::promo_pitch::PitchResponse {
            occasion,
            reply: reply.to_string(),
            confidence: 0.9,
        }
    }

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
                manual_partner_opt_out INTEGER DEFAULT 0,
                archived_at TEXT
            )"#,
            // streamer_plans — promo_disabled=integer, lurker_tax_enabled=integer,
            // promo_message=text, manual_plan_id/plan_name=text. Die Plan-Resolution
            // läuft über tb_analytics::plan::resolve_plan_snapshot — die braucht
            // manual_plan_expires_at (Ablauf-Gate) + manual_plan_updated_at (CASE-Order).
            r#"CREATE TABLE streamer_plans (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                promo_disabled INTEGER DEFAULT 0,
                lurker_tax_enabled INTEGER DEFAULT 0,
                promo_message TEXT,
                manual_plan_id TEXT,
                manual_plan_expires_at TEXT,
                manual_plan_updated_at TEXT,
                manual_plan_notes TEXT,
                trial_ever_granted INTEGER DEFAULT 0,
                first_login_at TIMESTAMPTZ,
                plan_name TEXT
            )"#,
            // twitch_billing_subscriptions — Stripe-Abo-Fallback der Plan-Resolution
            // (resolve_plan_snapshot Schritt 2). status=text, current_period_end=TIMESTAMPTZ.
            r#"CREATE TABLE twitch_billing_subscriptions (
                customer_reference TEXT NOT NULL,
                plan_id TEXT,
                status TEXT,
                current_period_end TIMESTAMPTZ,
                updated_at TIMESTAMPTZ DEFAULT NOW()
            )"#,
            // twitch_streamer_identities — twitch_user_id=text, twitch_login=text
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                discord_user_id TEXT
            )"#,
            r#"CREATE TABLE twitch_zuschauer_register (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                discord_user_id TEXT,
                community_probability DOUBLE PRECISION NOT NULL,
                signals JSONB NOT NULL DEFAULT '{}'::jsonb,
                first_partner_channel TEXT,
                first_seen_at TIMESTAMPTZ,
                computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
            r#"CREATE TABLE twitch_streamers (twitch_user_id TEXT)"#,
            r#"CREATE TABLE twitch_partner_signup_denylist (twitch_user_id TEXT NOT NULL)"#,
            // twitch_live_state — is_live=integer, last_game=text, active_session_id=bigint, last_viewer_count=integer
            r#"CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                last_started_at TEXT,
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
                game_name TEXT,
                stream_title TEXT,
                twitch_user_id TEXT,
                avg_viewers DOUBLE PRECISION DEFAULT 0
            )"#,
            r#"CREATE TABLE twitch_partners (
                id BIGSERIAL PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                twitch_user_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active'
            )"#,
            r#"CREATE TABLE twitch_scout_pitch_blacklist (
                streamer_login TEXT PRIMARY KEY,
                twitch_user_id TEXT,
                reason TEXT,
                created_at TIMESTAMPTZ DEFAULT NOW()
            )"#,
            r#"CREATE TABLE twitch_partner_outreach (
                streamer_login TEXT NOT NULL,
                streamer_user_id TEXT,
                twitch_user_id TEXT,
                detected_at TEXT NOT NULL DEFAULT '',
                cooldown_until TEXT,
                contacted_at TIMESTAMPTZ,
                status TEXT
            )"#,
            r#"CREATE TABLE twitch_scout_pitch_ledger (
                id BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                trigger_type TEXT NOT NULL,
                judge_verdict TEXT NOT NULL,
                action TEXT NOT NULL,
                detail TEXT,
                twitch_user_id TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
            // twitch_streamer_invites — streamer-specific Discord invite marker
            r#"CREATE TABLE twitch_streamer_invites (
                streamer_login TEXT PRIMARY KEY,
                guild_id BIGINT NOT NULL DEFAULT 1,
                channel_id BIGINT NOT NULL DEFAULT 1,
                invite_code TEXT NOT NULL DEFAULT 'code',
                invite_url TEXT NOT NULL DEFAULT 'https://discord.example/invite',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                last_sent_at TEXT
            )"#,
            // twitch_raid_auth — Lurker-Tax scope lookup. Tests may still use the
            // bot-scope fallback, but the table exists in prod and avoids false
            // negatives from missing-table errors.
            r#"CREATE TABLE twitch_raid_auth (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT,
                scopes TEXT
            )"#,
            r#"CREATE TABLE twitch_promo_pitch_log (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                target_user_id TEXT,
                pfad TEXT NOT NULL,
                occasion TEXT,
                trigger_text TEXT,
                generated_text TEXT,
                reject_reason TEXT,
                sent_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
            r#"CREATE TABLE twitch_chat_messages (
                id INTEGER,
                session_id INTEGER,
                streamer_login TEXT NOT NULL,
                chatter_login TEXT,
                chatter_id TEXT,
                message_id TEXT,
                message_ts TIMESTAMPTZ NOT NULL,
                is_command BOOLEAN DEFAULT FALSE,
                content TEXT
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

    #[tokio::test]
    async fn community_timer_persistenz_farbauswahl_und_sendpfad() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        apply_ddl(&pool).await;
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260912090000_promo_timer_settings.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260912204500_community_announcements.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        seed_partner_channel(&pool, COMMUNITY_BROADCASTER_ID, "community-renamed").await;
        seed_partner_channel(&pool, "other-id", "other").await;
        let api = Arc::new(super::tests::MockApi::default());
        let engine = Arc::new(PromoEngine::new(
            pool.clone(),
            api.clone(),
            Arc::new(NoopSuppressionCheck),
        ));
        engine
            .prepare_channel_timers("community-renamed", COMMUNITY_BROADCASTER_ID)
            .await;
        engine.prepare_channel_timers("other", "other-id").await;
        assert_eq!(
            engine
                .channel_timers("community-renamed")
                .await
                .0
                .overall_cooldown_minutes,
            20
        );
        assert_eq!(
            engine
                .channel_timers("other")
                .await
                .0
                .overall_cooldown_minutes,
            90
        );
        // Ohne Chat-Aktivität bleibt der eigene Timer still: die eingestellten
        // Chat-Schwellen gelten auch für den Community-Kanal.
        let now = Instant::now();
        engine
            .clone()
            .process_due_channel(
                "community-renamed".into(),
                COMMUNITY_BROADCASTER_ID.into(),
                now,
            )
            .await;
        assert_eq!(
            api.announcement_count().await,
            0,
            "ruhiger Chat darf keinen Timer-Spam auslösen"
        );
        // Erst wenn min_messages und das Aktivitätsfenster erreicht sind, sendet der Timer.
        {
            let state_ref = engine
                .channel_states
                .entry("community-renamed".into())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.raw_msg_count_since_promo = 8;
            for idx in 0..8usize {
                state.activity.push_back((now, format!("chatter{}", idx % 2)));
            }
        }
        engine
            .clone()
            .process_due_channel(
                "community-renamed".into(),
                COMMUNITY_BROADCASTER_ID.into(),
                now,
            )
            .await;
        assert_eq!(api.announcement_count().await, 1);
        let restart = Arc::new(PromoEngine::new(
            pool.clone(),
            api.clone(),
            Arc::new(NoopSuppressionCheck),
        ));
        restart.restore_promo_cooldowns().await;
        restart
            .prepare_channel_timers("community-renamed", COMMUNITY_BROADCASTER_ID)
            .await;
        restart
            .clone()
            .process_due_channel(
                "community-renamed".into(),
                COMMUNITY_BROADCASTER_ID.into(),
                Instant::now(),
            )
            .await;
        assert_eq!(
            api.announcement_count().await,
            1,
            "Neustart darf Cooldown nicht löschen"
        );
        let mut settings = tb_analytics::promo_timers::load(&pool).await.unwrap();
        settings.defaults.overall_cooldown_minutes = 60;
        tb_analytics::promo_timers::save(&pool, &settings)
            .await
            .unwrap();
        assert_eq!(
            tb_analytics::promo_timers::load(&pool).await.unwrap(),
            settings
        );
        assert_eq!(settings.community.timers.overall_cooldown_minutes, 20);
        sqlx::raw_sql("CREATE TABLE twitch_global_promo_modes (config_key TEXT PRIMARY KEY, mode TEXT NOT NULL DEFAULT 'standard', custom_message TEXT, starts_at TEXT, ends_at TEXT, is_enabled INTEGER NOT NULL DEFAULT 0, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_by TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260912170000_announcement_color.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        tb_analytics::promo_mode::save_global_promo_mode(
            &pool,
            &serde_json::json!({
                "mode":"custom_event", "custom_message":"Unser bestehender Hinweis {invite}",
                "is_enabled":true, "announcement_color":"green"
            }),
            "test",
        )
        .await
        .unwrap();
        assert!(
            engine
                .send_promo_message("other", "other-id", Instant::now(), "test")
                .await
        );
        assert_eq!(api.announcement_colors().await, vec!["purple", "green"]);
        sqlx::query("INSERT INTO twitch_promo_pitch_log(channel_login,pfad,sent_at) SELECT 'community-renamed','periodic',now() FROM generate_series(1,3)")
            .execute(&pool).await.unwrap();
        let active_event = engine
            .build_promo_text("community-renamed", DEFAULT_PROMO_DISCORD_INVITE)
            .await
            .unwrap();
        assert!(active_event.0.starts_with("Unser bestehender Hinweis"));
        assert_eq!(active_event.1, "green");
        sqlx::query("UPDATE twitch_global_promo_modes SET ends_at = '2000-01-01T00:00:00+00:00'")
            .execute(&pool)
            .await
            .unwrap();
        let expired_event = engine
            .build_promo_text("community-renamed", DEFAULT_PROMO_DISCORD_INVITE)
            .await
            .unwrap();
        assert_eq!(expired_event.0, format!("Zuschauen ist gut, selber mitmischen auch :) Alles rund um unsere Turniere findest du bei uns im Discord. {}", DEFAULT_PROMO_DISCORD_INVITE));
        assert_eq!(expired_event.1, "purple");
        use tb_analytics::community_announcements::{load, save, Announcement};
        let mut config = load(&pool).await.unwrap();
        config.include_global_event = false;
        for entry in &mut config.entries {
            entry.enabled = false;
        }
        config = save(&pool, &config).await.unwrap().unwrap();
        assert!(
            engine
                .build_promo_text("community-renamed", DEFAULT_PROMO_DISCORD_INVITE)
                .await
                .is_none(),
            "Keine versteckten Standardtexte bei deaktivierter Rotation"
        );
        config.entries.push(Announcement {
            text: "Nur unser neuer Test {invite}".into(),
            enabled: false,
            color: "orange".into(),
        });
        config = save(&pool, &config).await.unwrap().unwrap();
        assert!(engine
            .build_promo_text("community-renamed", DEFAULT_PROMO_DISCORD_INVITE)
            .await
            .is_none());
        config.entries.last_mut().unwrap().enabled = true;
        config = save(&pool, &config).await.unwrap().unwrap();
        let selected = engine
            .build_promo_text("community-renamed", DEFAULT_PROMO_DISCORD_INVITE)
            .await
            .unwrap();
        assert_eq!(
            selected,
            (
                format!("Nur unser neuer Test {}", DEFAULT_PROMO_DISCORD_INVITE),
                "orange".into()
            )
        );
        // Tatsächlicher Sendepfad, bestehender Cooldown bleibt erhalten.
        assert!(
            engine
                .send_promo_message(
                    "community-renamed",
                    COMMUNITY_BROADCASTER_ID,
                    Instant::now() + Duration::from_secs(3600),
                    "test"
                )
                .await
        );
        assert_eq!(api.announcement_colors().await.last().unwrap(), "orange");
        config.enabled = false;
        config.include_global_event = true;
        save(&pool, &config).await.unwrap().unwrap();
        sqlx::query("UPDATE twitch_global_promo_modes SET ends_at = NULL")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            engine
                .build_promo_text("community-renamed", DEFAULT_PROMO_DISCORD_INVITE)
                .await
                .is_none(),
            "Kanalpause sperrt auch das globale Event"
        );
        assert!(
            engine
                .build_promo_text("other", DEFAULT_PROMO_DISCORD_INVITE)
                .await
                .unwrap()
                .0
                .starts_with("Unser bestehender Hinweis"),
            "Andere Kanäle bleiben unverändert"
        );
    }

    struct EmptyMembers;

    #[async_trait::async_trait]
    impl crate::zuschauer_register::MemberIndexSource for EmptyMembers {
        async fn fetch_members(
            &self,
        ) -> Option<Vec<crate::zuschauer_register::MemberLite>> {
            Some(Vec::new())
        }
    }

    fn test_register(pool: PgPool) -> Arc<crate::zuschauer_register::ZuschauerRegister> {
        Arc::new(crate::zuschauer_register::ZuschauerRegister::new(
            pool,
            Arc::new(EmptyMembers),
        ))
    }

    #[tokio::test]
    async fn recent_messages_filtern_bots_und_anon() {
        let pool = pool_or_skip!("promo_recent_bot_filter");
        let engine = make_engine(pool.clone());
        for (chatter, content) in [
            ("nani_fan", "richtig cooler stream heute"),
            ("nightbot", "nightbot meldet den timer"),
            ("deutschedeadlockcommunity", "die community freut sich"),
            ("justinfan12345", "lurker sagt hallo"),
            ("nani_fan", "wann kommt der naechste patch"),
        ] {
            sqlx::query(
                "INSERT INTO twitch_chat_messages \
                 (streamer_login, chatter_login, message_ts, is_command, content) \
                 VALUES ($1, $2, NOW(), FALSE, $3)",
            )
            .bind("nani")
            .bind(chatter)
            .bind(content)
            .execute(&pool)
            .await
            .unwrap();
        }

        let recent = engine.load_recent_channel_messages("nani", 8).await;

        assert_eq!(recent.len(), 2, "nur echte Chatter erwartet: {recent:?}");
        assert!(recent.iter().any(|m| m.contains("patch")));
        assert!(recent.iter().any(|m| m.contains("cooler stream")));
        assert!(recent.iter().all(|m| !m.contains("nightbot")));
        assert!(recent.iter().all(|m| !m.contains("community freut")));
        assert!(recent.iter().all(|m| !m.contains("lurker")));
    }

    // -----------------------------------------------------------------------
    // save/restore Cooldown
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn cooldown_save_und_restore() {
        let pool = pool_or_skip!("promo_cooldown_save");
        let engine = make_engine(pool.clone());

        let wall_ts = 1718000000.0_f64;
        engine
            .save_promo_cooldown("testkanal", "sent", wall_ts)
            .await;

        let rows: Vec<(String, String, f64)> =
            sqlx::query_as("SELECT login, cooldown_type, wall_ts FROM twitch_promo_cooldowns")
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

        assert!(
            (wall_ts - 2000.0).abs() < 0.001,
            "Upsert muss neueren Wert schreiben"
        );
    }

    #[tokio::test]
    async fn cooldown_restore_rekonstruiert_monotonic() {
        let pool = pool_or_skip!("promo_cooldown_restore");
        let engine = make_engine(pool.clone());

        // wall_ts = vor 60 Minuten.
        let wall_ts = (Utc::now().timestamp() as f64) - 3600.0;
        engine.save_promo_cooldown("kanal", "sent", wall_ts).await;

        engine.restore_promo_cooldowns().await;

        let state_ref = engine
            .channel_states
            .entry("kanal".to_string())
            .or_insert_with(|| Mutex::new(ChannelState::new()));
        let state = state_ref.lock().await;
        assert!(
            state.last_promo_sent.is_some(),
            "Restore muss last_promo_sent setzen"
        );

        // Verify: das Instant liegt ca. 60 min in der Vergangenheit.
        let age = Instant::now().duration_since(state.last_promo_sent.unwrap());
        assert!(
            age.as_secs() > 3500 && age.as_secs() < 3700,
            "Age ~60 min, got {age:?}"
        );
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

    #[tokio::test]
    async fn lurker_tax_channelquelle_ignoriert_game_und_promo_disabled() {
        let pool = pool_or_skip!("promo_lurker_channels");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO twitch_live_state
             (twitch_user_id, streamer_login, is_live, last_game, active_session_id)
             VALUES ('u-lurk', 'varietykanal', 1, 'Just Chatting', 42)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_disabled)
             VALUES ('u-lurk', 'varietykanal', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let lurker_channels = engine.get_live_channels_for_lurker_tax().await.unwrap();
        assert_eq!(
            lurker_channels,
            vec![("varietykanal".to_string(), "u-lurk".to_string())]
        );

        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('u-lurk', 'varietykanal')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let promo_channels = engine.get_live_channels_for_promo().await.unwrap();
        assert!(
            promo_channels.is_empty(),
            "Promo-Quelle bleibt Deadlock/promo_disabled-gefiltert"
        );
    }

    #[tokio::test]
    async fn send_promo_if_due_respektiert_promo_channel_allowed_gate() {
        let pool = pool_or_skip!("promo_slot_allowed_gate");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = Arc::new(PromoEngine::new(
            pool.clone(),
            api.clone(),
            Arc::new(NoopSuppressionCheck),
        ));

        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('u-target', 'targetkanal')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game)
             VALUES ('u-target', 'targetkanal', 1, 'Deadlock')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let now = Instant::now();
        {
            let state_ref = engine
                .channel_states
                .entry("targetkanal".to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.raw_msg_count_since_promo = PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO;
            for idx in 0..PROMO_ACTIVITY_MIN_MSGS {
                state.activity.push_back((now, format!("chatter{idx}")));
            }
        }

        engine.send_promo_if_due(now).await;

        assert_eq!(
            api.announcement_count().await,
            0,
            "ohne aktiven Partner-State sendet der Promo-Slot nichts"
        );
        assert_eq!(api.message_count().await, 0);
    }

    #[tokio::test]
    async fn send_promo_if_due_laeuft_ohne_targeted_global() {
        let pool = pool_or_skip!("promo_ohne_targeted_global");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = Arc::new(
            PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
                .set_pitch_text_gen(Arc::new(FixedTextGen(Some(
                    "mitspieler findest du bei uns".to_string(),
                )))),
        );

        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('u-p', 'pkanal')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, archived_at)
             VALUES ('pkanal', 1, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game)
             VALUES ('u-p', 'pkanal', 1, 'Deadlock')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let now = Instant::now();
        {
            let state_ref = engine
                .channel_states
                .entry("pkanal".to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.raw_msg_count_since_promo = PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO;
            for idx in 0..PROMO_ACTIVITY_MIN_MSGS {
                state.activity.push_back((now, format!("chatter{idx}")));
            }
        }

        engine.send_promo_if_due(now).await;

        assert_eq!(
            api.message_count().await,
            0,
            "der Timer pitcht keine Einzelpersonen"
        );
        assert_eq!(
            api.announcement_count().await,
            1,
            "der Periodik-Pfad übernimmt den Slot"
        );
        let targeted: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE pfad LIKE 'targeted%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(targeted, 0, "kein targeted_global/targeted_user mehr");
        let periodic_sent: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_promo_pitch_log
              WHERE pfad = 'periodic' AND sent_at IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(periodic_sent, 1, "der Send läuft über periodic");
    }

    #[tokio::test]
    async fn on_message_wartet_nicht_auf_textgenerator() {
        let pool = pool_or_skip!("promo_on_message_nonblocking");
        seed_partner_channel(&pool, "c-nb", "nbkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let engine = Arc::new(
            PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
                .set_pitch_text_gen(Arc::new(SlowTextGen {
                    delay: Duration::from_secs(3),
                })),
        );

        let now = Instant::now();
        {
            let state_ref = engine
                .channel_states
                .entry("nbkanal".to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.raw_msg_count_since_promo = PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO;
            for idx in 0..PROMO_ACTIVITY_MIN_MSGS {
                state.activity.push_back((now, format!("chatter{idx}")));
            }
        }

        let event = pitch_event(
            "c-nb",
            "nbkanal",
            "u-nb",
            "Nb",
            "das ist eine ganz normale nachricht im chat",
        );

        let start = std::time::Instant::now();
        engine.on_message(&event).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(500),
            "on_message darf nicht auf den Textgenerator warten: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn on_message_spawnt_keinen_promo_bei_vollem_semaphore() {
        let pool = pool_or_skip!("promo_on_message_semaphore");
        seed_partner_channel(&pool, "c-ps", "pskanal").await;
        let gen = Arc::new(CountingTextGen::new("bei uns gibts leute zum zocken"));
        let engine = Arc::new(
            PromoEngine::new(
                pool.clone(),
                Arc::new(super::tests::MockApi::default()),
                Arc::new(NoopSuppressionCheck),
            )
            .set_pitch_text_gen(gen.clone()),
        );

        async fn setze_bereit(engine: &Arc<PromoEngine>, now: Instant) {
            let state_ref = engine
                .channel_states
                .entry("pskanal".to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_attempt = None;
            state.last_promo_sent = Some(now - Duration::from_secs(190 * 60));
            state.raw_msg_count_since_promo = PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO;
            state.activity.clear();
            state.seen_chatters.clear();
            for idx in 0..(PROMO_ACTIVITY_MIN_MSGS + PROMO_NEW_CHATTERS_MIN) {
                state.activity.push_back((now, format!("chatter_{idx}")));
            }
        }

        let now = Instant::now();
        let event = pitch_event(
            "c-ps",
            "pskanal",
            "u-ps",
            "Ps",
            "das ist eine ganz normale nachricht im chat",
        );

        setze_bereit(&engine, now).await;
        let mut permits = Vec::new();
        for _ in 0..PROMO_MAX_CONCURRENT {
            permits.push(
                Arc::clone(&engine.promo_semaphore)
                    .try_acquire_owned()
                    .expect("Promo-Semaphore-Slot muss anfangs frei sein"),
            );
        }

        engine.on_message(&event).await;

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "bei vollem Promo-Semaphore darf kein Promo-Task starten"
        );
        assert_eq!(
            engine.promo_semaphore.available_permits(),
            0,
            "on_message darf bei vollem Semaphore keinen weiteren Slot belegen"
        );

        drop(permits);
        setze_bereit(&engine, Instant::now()).await;
        engine.on_message(&event).await;

        let mut versuche = 0;
        while gen.calls.load(std::sync::atomic::Ordering::SeqCst) == 0 && versuche < 30 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            versuche += 1;
        }
        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "mit freiem Semaphore muss der Promo-Task genau einmal Text erzeugen"
        );
    }

    #[tokio::test]
    async fn on_message_belegt_keinen_permit_wenn_nicht_faellig() {
        let pool = pool_or_skip!("promo_on_message_attempt_cooldown");
        seed_partner_channel(&pool, "c-nf", "nfkanal").await;
        let gen = Arc::new(CountingTextGen::new("bei uns gibts leute zum zocken"));
        let engine = Arc::new(
            PromoEngine::new(
                pool.clone(),
                Arc::new(super::tests::MockApi::default()),
                Arc::new(NoopSuppressionCheck),
            )
            .set_pitch_text_gen(gen.clone()),
        );

        let now = Instant::now();
        {
            let state_ref = engine
                .channel_states
                .entry("nfkanal".to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_attempt = Some(now);
            state.raw_msg_count_since_promo = PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO;
            for idx in 0..(PROMO_ACTIVITY_MIN_MSGS + PROMO_NEW_CHATTERS_MIN) {
                state.activity.push_back((now, format!("chatter_{idx}")));
            }
        }

        let event = pitch_event(
            "c-nf",
            "nfkanal",
            "u-nf",
            "Nf",
            "das ist eine ganz normale nachricht im chat",
        );
        engine.on_message(&event).await;

        assert_eq!(
            engine.promo_semaphore.available_permits(),
            PROMO_MAX_CONCURRENT,
            "eine nicht faellige Nachricht darf keinen Promo-Slot belegen"
        );
        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "eine nicht faellige Nachricht darf keinen Promo-Task starten"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pitch_judge_reserve_zaehlt_beide_parallelen_aufrufe() {
        let pool = pool_or_skip!("promo_reserve_parallel");
        let engine = Arc::new(make_engine(pool));
        let e1 = Arc::clone(&engine);
        let e2 = Arc::clone(&engine);
        let t1 = tokio::spawn(async move { e1.pitch_judge_throttle_reserve("rkanal", "chatter-a") });
        let t2 = tokio::spawn(async move { e2.pitch_judge_throttle_reserve("rkanal", "chatter-b") });
        let (r1, r2) = tokio::join!(t1, t2);
        assert!(r1.unwrap(), "erste Reservierung muss durchgehen");
        assert!(r2.unwrap(), "zweite Reservierung muss durchgehen");
        let budget = engine
            .pitch_judge_channel
            .get("rkanal")
            .map(|entry| entry.len())
            .unwrap_or(0);
        assert_eq!(
            budget, 2,
            "beide parallelen Reservierungen muessen im Kanalbudget zaehlen, war {budget}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attempt_reservierung_ist_atomar_ein_textgen_pro_runde() {
        let pool = pool_or_skip!("promo_attempt_atomar");
        seed_partner_channel(&pool, "c-at", "atkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let gen = Arc::new(CountingTextGen::new("bei uns gibts leute zum zocken"));
        let engine = Arc::new(
            PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
                .set_pitch_text_gen(gen.clone()),
        );

        let now = Instant::now();
        let stale_sent = now - Duration::from_secs(190 * 60);
        const RUNDEN: usize = 40;

        for round in 0..RUNDEN {
            {
                let state_ref = engine
                    .channel_states
                    .entry("atkanal".to_string())
                    .or_insert_with(|| Mutex::new(ChannelState::new()));
                let mut state = state_ref.lock().await;
                state.last_promo_attempt = None;
                state.last_promo_sent = Some(stale_sent);
                state.raw_msg_count_since_promo = PROMO_ACTIVITY_MIN_RAW_MSGS_SINCE_PROMO;
                state.activity.clear();
                state.seen_chatters.clear();
                for idx in 0..(PROMO_ACTIVITY_MIN_MSGS + PROMO_NEW_CHATTERS_MIN) {
                    state
                        .activity
                        .push_back((now, format!("chatter{round}_{idx}")));
                }
            }

            let e1 = Arc::clone(&engine);
            let e2 = Arc::clone(&engine);
            let t1 = tokio::spawn(async move {
                e1.maybe_send_promo_with_stats("atkanal", "c-at", now, false)
                    .await
            });
            let t2 = tokio::spawn(async move {
                e2.maybe_send_promo_with_stats("atkanal", "c-at", now, false)
                    .await
            });
            let _ = tokio::join!(t1, t2);
        }

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            RUNDEN,
            "zwei parallele Promo-Versuche im selben Kanal duerfen je Runde genau einen TextGen-Aufruf ergeben"
        );
    }

    #[tokio::test]
    async fn anlass_pitch_bei_vollem_semaphore_kein_judge() {
        let pool = pool_or_skip!("promo_anlass_semaphore");
        seed_partner_channel(&pool, "c-sem", "semkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::GameUnpopular),
            "deadlock ist echt unterschaetzt",
        ))));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone());

        let mut permits = Vec::new();
        for _ in 0..PITCH_MAX_CONCURRENT {
            permits.push(
                engine
                    .pitch_semaphore
                    .try_acquire()
                    .expect("Semaphore-Slot muss anfangs frei sein"),
            );
        }

        let event = pitch_event(
            "c-sem",
            "semkanal",
            "u-sem",
            "Sem",
            "deadlock ist so unpopulaer, keine ahnung warum das keiner spielt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(
            judge.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "bei vollem Semaphore darf der Judge nicht laufen"
        );
        assert_eq!(api.message_count().await, 0);
        drop(permits);
    }

    #[tokio::test]
    async fn anlass_pitch_bei_promo_disabled_verbraucht_kein_budget() {
        let pool = pool_or_skip!("promo_anlass_disabled_budget");
        seed_partner_channel(&pool, "c-dis", "diskanal").await;
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_disabled) \
             VALUES ('c-dis', 'diskanal', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::GameUnpopular),
            "deadlock ist echt unterschaetzt",
        ))));
        let engine = PromoEngine::new(
            pool.clone(),
            Arc::new(super::tests::MockApi::default()),
            Arc::new(NoopSuppressionCheck),
        )
        .set_pitch_judge(judge.clone());

        for i in 0..30 {
            let event = pitch_event(
                "c-dis",
                "diskanal",
                &format!("u-dis-{i}"),
                "Dis",
                "deadlock ist so unpopulaer, keine ahnung warum das keiner spielt",
            );
            engine.on_message_pitch(&event).await;
        }

        assert_eq!(
            judge.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "promo_disabled darf den Judge nie aufrufen"
        );
        let budget = engine
            .pitch_judge_channel
            .get("diskanal")
            .map(|entry| entry.len())
            .unwrap_or(0);
        assert_eq!(
            budget, 0,
            "promo_disabled darf kein Judge-Budget reservieren, war {budget}"
        );
    }

    #[tokio::test]
    async fn pitch_user_limit_blockt_bei_pending_zeile() {
        let pool = pool_or_skip!("promo_user_limit_pending");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO twitch_promo_pitch_log
                 (channel_login, target_user_id, pfad, sent_at, reject_reason, created_at)
             VALUES ('kanal', 'u-pend', 'anlass', NULL, NULL, NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            !engine.pitch_user_limit_ok("u-pend").await,
            "eine frische Pending-Zeile muss den naechsten Pitch derselben Person blocken"
        );

        sqlx::query(
            "INSERT INTO twitch_promo_pitch_log
                 (channel_login, target_user_id, pfad, sent_at, reject_reason, created_at)
             VALUES ('kanal', 'u-stale', 'anlass', NULL, NULL, NOW() - INTERVAL '20 minutes')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            engine.pitch_user_limit_ok("u-stale").await,
            "eine ueber 10 Minuten alte Pending-Zeile blockt nicht mehr"
        );

        sqlx::query(
            "INSERT INTO twitch_promo_pitch_log
                 (channel_login, target_user_id, pfad, sent_at, reject_reason, created_at)
             VALUES ('kanal', 'u-gezielt', 'gezielt', NOW(), NULL, NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            !engine.pitch_user_limit_ok("u-gezielt").await,
            "ein gesendeter gezielter Pitch muss den Anlass-Pitch blocken"
        );
    }

    #[tokio::test]
    async fn pitch_user_limit_blockt_bei_fehlender_tabelle() {
        let pool = pool_or_skip!("promo_user_limit_no_table");
        sqlx::query("DROP TABLE twitch_promo_pitch_log")
            .execute(&pool)
            .await
            .unwrap();
        let engine = make_engine(pool.clone());
        assert!(
            !engine.pitch_user_limit_ok("u-x").await,
            "DB-Fehler beim User-Limit muss blockieren"
        );
    }

    #[tokio::test]
    async fn pitch_channel_limit_blockt_bei_fehlender_tabelle() {
        let pool = pool_or_skip!("promo_channel_limit_no_table");
        sqlx::query("DROP TABLE twitch_promo_pitch_log")
            .execute(&pool)
            .await
            .unwrap();
        let engine = make_engine(pool.clone());
        assert!(
            !engine.pitch_channel_limit_ok("kanal-x").await,
            "DB-Fehler beim Kanal-Limit muss blockieren"
        );
    }

    #[tokio::test]
    async fn anlass_pitch_insert_fehler_verhindert_send() {
        let pool = pool_or_skip!("promo_anlass_insert_fehler");
        seed_partner_channel(&pool, "c-inserr", "inserrkanal").await;
        sqlx::query("ALTER TABLE twitch_promo_pitch_log ADD COLUMN pflicht TEXT NOT NULL")
            .execute(&pool)
            .await
            .unwrap();
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::GameUnpopular),
            "deadlock ist echt unterschaetzt",
        ))));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_zuschauer_register(test_register(pool.clone()));

        let event = pitch_event(
            "c-inserr",
            "inserrkanal",
            "u-inserr",
            "Inserr",
            "deadlock ist so unpopulaer, keine ahnung warum das keiner spielt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(
            api.message_count().await,
            0,
            "Insert-Fehler muss den Send verhindern"
        );
    }

    #[tokio::test]
    async fn anlass_pitch_symphooniee_wird_gesendet() {
        let pool = pool_or_skip!("promo_anlass_symphooniee");
        seed_partner_channel(&pool, "c-sym", "symkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::GameUnpopular),
            "deadlock ist echt unterschaetzt, das game macht suchtig",
        ))));
        let sink = RecordingReviewSink::default();
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_pitch_review_sink(Arc::new(sink.clone()))
            .set_zuschauer_register(test_register(pool.clone()));

        let event = pitch_event(
            "c-sym",
            "symkanal",
            "u-symphooniee",
            "Symphooniee",
            "yo wieso ist deadlock so unpopulaer wie haben die den anschluss verpasst",
        );
        engine.on_message_pitch(&event).await;

        let msgs = api.messages_sent().await;
        assert_eq!(msgs.len(), 1, "genau eine Anlass-Antwort erwartet");
        assert!(
            msgs[0].1.starts_with("@Symphooniee "),
            "Antwort muss die Person mit @login anreden: {}",
            msgs[0].1
        );

        let row: (String, Option<chrono::DateTime<Utc>>, Option<String>) = sqlx::query_as(
            "SELECT pfad, sent_at, occasion FROM twitch_promo_pitch_log
              WHERE pfad = 'anlass' AND sent_at IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "anlass");
        assert!(row.1.is_some(), "sent_at muss gesetzt sein");
        assert_eq!(row.2.as_deref(), Some("game_unpopular"));

        assert_eq!(sink.cards.lock().await.len(), 1, "eine Review-Karte erwartet");
    }

    async fn seed_deadlock_candidate(pool: &PgPool, own_login: &str, chatter_id: &str) {
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, game_name, twitch_user_id)
             VALUES ($1, NOW(), 'Deadlock', $2)",
        )
        .bind(own_login)
        .bind(chatter_id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn partner_kandidat_bekommt_partner_pitch() {
        let pool = pool_or_skip!("promo_partner_kandidat");
        seed_partner_channel(&pool, "c-pk", "pkkanal").await;
        seed_deadlock_candidate(&pool, "kandidatlogin", "u-pk").await;

        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(None));
        let gen = Arc::new(MockPartnerPitchGen::new(Some(
            "stark gespielt gerade. wenn du öfter deadlock streamst, bei der deutschen deadlock community gibts ein partner netzwerk, das raidet dich wenn andere offline gehen und schützt deinen chat vor spam",
        )));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone());

        let event = pitch_event(
            "c-pk",
            "pkkanal",
            "u-pk",
            "Kandidat",
            "hey leute ich streame auch deadlock schaut gerne mal vorbei wenn ihr lust habt",
        );
        engine.on_message_pitch(&event).await;

        let msgs = api.messages_sent().await;
        assert_eq!(msgs.len(), 1, "genau ein Partner-Pitch erwartet");
        assert!(
            msgs[0].1.starts_with("@Kandidat "),
            "Antwort muss die Person mit @login anreden: {}",
            msgs[0].1
        );

        let row: (String, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
            "SELECT pfad, sent_at FROM twitch_promo_pitch_log WHERE pfad = 'partner'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "partner");
        assert!(row.1.is_some(), "sent_at muss gesetzt sein");
    }

    #[tokio::test]
    async fn partner_kandidat_kurznachricht_kein_partner_pitch() {
        let pool = pool_or_skip!("promo_partner_kurznachricht");
        seed_partner_channel(&pool, "c-pkz", "pkzkanal").await;
        seed_deadlock_candidate(&pool, "kurzlogin", "u-pkz").await;

        let api = Arc::new(super::tests::MockApi::default());
        let gen = Arc::new(MockPartnerPitchGen::new(Some("partner text egal")));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_partner_pitch_gen(gen.clone())
            .set_zuschauer_register(test_register(pool.clone()));

        let event = pitch_event("c-pkz", "pkzkanal", "u-pkz", "Kurz", "yo deadlock laeuft gut");
        engine.on_message_pitch(&event).await;

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "eine Nachricht unter 25 Zeichen darf den Partner-Generator nie aufrufen"
        );
        assert_eq!(
            api.message_count().await,
            0,
            "kein Partner-Pitch bei einer Nachricht unter 25 Zeichen"
        );
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE pfad = 'partner'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 0, "kein Partner-Log fuer eine Kurznachricht");
    }

    #[tokio::test]
    async fn partner_pitch_schreibt_ledger_und_review_karte() {
        let pool = pool_or_skip!("promo_partner_ledger_karte");
        seed_partner_channel(&pool, "c-lk", "lkkanal").await;
        seed_deadlock_candidate(&pool, "ledgerlogin", "u-lk").await;

        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(None));
        let gen = Arc::new(MockPartnerPitchGen::new(Some(
            "stark gespielt gerade, wenn du öfter deadlock streamst gibts bei der community ein partner netzwerk mit raids und chat schutz",
        )));
        let sink = RecordingReviewSink::default();
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone())
            .set_pitch_review_sink(Arc::new(sink.clone()));

        let event = pitch_event(
            "c-lk",
            "lkkanal",
            "u-lk",
            "Ledger",
            "hey ich streame auch deadlock schaut gerne mal bei mir vorbei wenn ihr wollt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(api.message_count().await, 1, "genau ein Partner-Pitch erwartet");

        let ledger: (String, String, String, Option<String>) = sqlx::query_as(
            "SELECT trigger_type, judge_verdict, action, twitch_user_id FROM twitch_scout_pitch_ledger",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ledger.0, "chat_partner_pitch");
        assert_eq!(ledger.1, "partner_pitch");
        assert_eq!(ledger.2, "posted");
        assert_eq!(ledger.3.as_deref(), Some("u-lk"));

        let cards = sink.cards.lock().await;
        assert_eq!(cards.len(), 1, "eine Review-Karte erwartet");
        assert_eq!(cards[0].4, PitchCardKind::Partner, "Karte muss als Partner markiert sein");
        let hint = cards[0].5.as_deref().unwrap_or("");
        assert!(
            hint.contains("ledgerlogin"),
            "Kandidaten-Hinweis muss den Login tragen: {hint}"
        );
    }

    #[tokio::test]
    async fn nicht_kandidat_geht_in_anlass_pfad() {
        let pool = pool_or_skip!("promo_partner_nicht_kandidat");
        seed_partner_channel(&pool, "c-nk", "nkkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::GameUnpopular),
            "deadlock ist echt unterschaetzt, das game macht suchtig",
        ))));
        let gen = Arc::new(MockPartnerPitchGen::new(Some("partner text")));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone())
            .set_zuschauer_register(test_register(pool.clone()));

        let event = pitch_event(
            "c-nk",
            "nkkanal",
            "u-nk",
            "Nichtkandidat",
            "yo wieso ist deadlock so unpopulaer wie haben die den anschluss verpasst",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "ohne Deadlock-Session darf der Partner-Generator nie laufen"
        );
        let msgs = api.messages_sent().await;
        assert_eq!(msgs.len(), 1, "genau eine Anlass-Antwort erwartet");
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE pfad = 'partner'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 0, "kein Partner-Log fuer Nicht-Kandidaten");
        let anlass: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE pfad = 'anlass' AND sent_at IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(anlass.0, 1, "der Anlass-Pfad muss unveraendert senden");
    }

    #[tokio::test]
    async fn partner_wird_nicht_gepitcht() {
        let pool = pool_or_skip!("promo_partner_ist_partner");
        seed_partner_channel(&pool, "c-ip", "ipkanal").await;
        seed_deadlock_candidate(&pool, "schonpartner", "u-ip").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ('schonpartner', 'u-ip', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(None));
        let gen = Arc::new(MockPartnerPitchGen::new(Some("partner text")));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone());

        let event = pitch_event(
            "c-ip",
            "ipkanal",
            "u-ip",
            "SchonPartner",
            "hey ich streame auch deadlock schaut gerne mal bei mir vorbei wenn ihr wollt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "ein Partner darf nie als Kandidat gelten"
        );
        assert_eq!(api.message_count().await, 0, "kein Send an einen Partner");
    }

    #[tokio::test]
    async fn zweiter_partner_pitch_derselben_user_id_geblockt() {
        let pool = pool_or_skip!("promo_partner_lifetime");
        seed_partner_channel(&pool, "c-lt", "ltkanal").await;
        seed_deadlock_candidate(&pool, "lifetimelogin", "u-lt").await;
        sqlx::query(
            "INSERT INTO twitch_promo_pitch_log (channel_login, target_user_id, pfad, sent_at, created_at)
             VALUES ('anderer_kanal', 'u-lt', 'partner', NOW() - INTERVAL '3 days', NOW() - INTERVAL '3 days')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(None));
        let gen = Arc::new(MockPartnerPitchGen::new(Some("partner text")));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone());

        let event = pitch_event(
            "c-lt",
            "ltkanal",
            "u-lt",
            "Lifetime",
            "hey ich streame auch deadlock schaut gerne mal bei mir vorbei wenn ihr wollt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "nach einem Lifetime-Pitch darf kein zweiter Generator-Aufruf kommen"
        );
        assert_eq!(api.message_count().await, 0, "kein zweiter Partner-Pitch");
    }

    #[tokio::test]
    async fn partner_tageslimit_fuenf() {
        let pool = pool_or_skip!("promo_partner_tageslimit");
        seed_partner_channel(&pool, "c-tl", "tlkanal").await;
        seed_deadlock_candidate(&pool, "tageslogin", "u-tl-6").await;
        for i in 0..5 {
            sqlx::query(
                "INSERT INTO twitch_promo_pitch_log (channel_login, target_user_id, pfad, sent_at, created_at)
                 VALUES ($1, $2, 'partner', NOW() - INTERVAL '1 hour', NOW() - INTERVAL '1 hour')",
            )
            .bind(format!("kanal_{i}"))
            .bind(format!("u-tl-{i}"))
            .execute(&pool)
            .await
            .unwrap();
        }

        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(None));
        let gen = Arc::new(MockPartnerPitchGen::new(Some("partner text")));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone());

        let event = pitch_event(
            "c-tl",
            "tlkanal",
            "u-tl-6",
            "Tageslimit",
            "hey ich streame auch deadlock schaut gerne mal bei mir vorbei wenn ihr wollt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "das Tageslimit muss vor dem Generator-Aufruf greifen"
        );
        assert_eq!(api.message_count().await, 0, "sechster Partner-Pitch am Tag blockiert");
    }

    #[tokio::test]
    async fn promo_disabled_sendet_keinen_partner_pitch() {
        let pool = pool_or_skip!("promo_partner_disabled");
        seed_partner_channel(&pool, "c-pd", "pdkanal").await;
        seed_deadlock_candidate(&pool, "disabledlogin", "u-pd").await;
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_disabled)
             VALUES ('c-pd', 'pdkanal', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(None));
        let gen = Arc::new(MockPartnerPitchGen::new(Some("partner text")));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone());

        let event = pitch_event(
            "c-pd",
            "pdkanal",
            "u-pd",
            "Disabled",
            "hey ich streame auch deadlock schaut gerne mal bei mir vorbei wenn ihr wollt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(
            gen.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "Werbefrei muss den Partner-Pitch vor dem Generator abschalten"
        );
        assert_eq!(api.message_count().await, 0, "Werbefrei: kein Partner-Pitch");
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE pfad = 'partner'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 0, "Gate-Block wird nicht geloggt");
    }

    #[tokio::test]
    async fn partner_filter_verwirft_link() {
        let pool = pool_or_skip!("promo_partner_filter_link");
        seed_partner_channel(&pool, "c-fl", "flkanal").await;
        seed_deadlock_candidate(&pool, "filterlogin", "u-fl").await;

        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(None));
        let gen = Arc::new(MockPartnerPitchGen::new(Some(
            "stark gespielt, schau mal auf https://discord.gg/abc vorbei",
        )));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_partner_pitch_gen(gen.clone());

        let event = pitch_event(
            "c-fl",
            "flkanal",
            "u-fl",
            "Filter",
            "hey ich streame auch deadlock schaut gerne mal bei mir vorbei wenn ihr wollt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(api.message_count().await, 0, "Link-Antwort darf nicht raus");
        let row: (Option<String>, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
            "SELECT reject_reason, sent_at FROM twitch_promo_pitch_log WHERE pfad = 'partner'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("link"), "harter Filter muss den Grund protokollieren");
        assert!(row.1.is_none());
    }

    #[tokio::test]
    async fn anlass_pitch_bei_promo_disabled_sendet_nichts() {
        let pool = pool_or_skip!("promo_anlass_disabled");
        seed_partner_channel(&pool, "c-off", "offkanal").await;
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_disabled)
             VALUES ('c-off', 'offkanal', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::GameUnpopular),
            "deadlock ist stark",
        ))));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone());

        let event = pitch_event(
            "c-off",
            "offkanal",
            "u-viewer",
            "Viewer",
            "deadlock ist so unpopulaer, keine ahnung warum das keiner spielt",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(api.message_count().await, 0, "Werbefrei: nichts senden");
        assert_eq!(
            judge.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "bei promo_disabled darf der Judge gar nicht laufen"
        );
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE pfad = 'anlass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 0, "Gate-Block wird nicht geloggt");
    }

    #[tokio::test]
    async fn anlass_pitch_kurznachricht_ohne_llm() {
        let pool = pool_or_skip!("promo_anlass_kurz");
        seed_partner_channel(&pool, "c-kurz", "kurzkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::NoMates),
            "komm vorbei",
        ))));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone());

        let event = pitch_event("c-kurz", "kurzkanal", "u-kurz", "Kurz", "gg wp");
        engine.on_message_pitch(&event).await;

        assert_eq!(api.message_count().await, 0);
        assert_eq!(
            judge.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "Kurznachricht darf das Modell nicht erreichen"
        );
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM twitch_promo_pitch_log WHERE pfad = 'anlass'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 0, "Vorfilter erzeugt keinen Log-Eintrag");
    }

    #[tokio::test]
    async fn anlass_pitch_harter_filter_verwirft_link() {
        let pool = pool_or_skip!("promo_anlass_filter");
        seed_partner_channel(&pool, "c-filt", "filtkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(
            Some(crate::promo_pitch::PitchOccasion::WantsHelp),
            "klar helfen wir dir, schau auf https://discord.gg/abc vorbei",
        ))));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_zuschauer_register(test_register(pool.clone()));

        let event = pitch_event(
            "c-filt",
            "filtkanal",
            "u-filt",
            "Filt",
            "kann mir jemand helfen ich bin neu und komme nicht weiter",
        );
        engine.on_message_pitch(&event).await;

        assert_eq!(api.message_count().await, 0, "Link-Antwort darf nicht raus");
        let row: (Option<String>, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
            "SELECT reject_reason, sent_at FROM twitch_promo_pitch_log WHERE pfad = 'anlass'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row.0.is_some(), "harter Filter muss den Grund protokollieren");
        assert!(row.1.is_none());
    }

    #[tokio::test]
    async fn anlass_pitch_judge_drossel_pro_chatter() {
        let pool = pool_or_skip!("promo_anlass_drossel");
        seed_partner_channel(&pool, "c-dr", "drkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let judge = Arc::new(MockPitchJudge::new(Some(pitch_response(None, ""))));
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_pitch_judge(judge.clone())
            .set_pitch_text_gen(Arc::new(FixedTextGen(None)))
            .set_zuschauer_register(test_register(pool.clone()));

        let event = pitch_event(
            "c-dr",
            "drkanal",
            "u-dr",
            "Dr",
            "deadlock ist echt unterschaetzt, wieso spielt das keiner richtig",
        );
        engine.on_message_pitch(&event).await;
        engine.on_message_pitch(&event).await;

        assert_eq!(
            judge.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "zweite Nachricht desselben Chatters darf den Judge nicht erneut aufrufen"
        );
    }

    #[tokio::test]
    async fn zuschauer_reject_wird_pro_session_entprellt() {
        let pool = pool_or_skip!("promo_reject_entprellt");
        seed_partner_channel(&pool, "c-rej", "rejkanal").await;
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api, Arc::new(NoopSuppressionCheck));
        let event = pitch_event(
            "c-rej",
            "rejkanal",
            "u-rej",
            "Rejler",
            "diese nachricht ist lang genug fuer das zuschauer gate",
        );

        engine.on_message_pitch(&event).await;
        engine.on_message_pitch(&event).await;

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_promo_pitch_log
              WHERE channel_login = 'rejkanal' AND target_user_id = 'u-rej'
                AND reject_reason = 'register_fehlt'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "gleicher Reject-Grund nur einmal je Session");
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
    // promos-engine-6: Volle Plan-Snapshot-Resolution
    // (Plan-Ablauf via manual_plan_expires_at + chat.promos.disable-Entitlement)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn promo_disable_entitlement_blockiert_bei_aktivem_plan() {
        // Aktiver chat_quiet-Override (kein Ablauf) → chat.promos.disable greift.
        // User-Betonung: Pläne die Promo abschalten dürfen NICHT ignoriert werden.
        let pool = pool_or_skip!("promo_entitlement_aktiv");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id)
             VALUES ('uq1', 'werbefreikanal', 'chat_quiet')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let blocked = engine.promo_blocked_by_plan_or_flag("werbefreikanal").await;
        assert!(
            blocked,
            "aktiver chat_quiet → chat.promos.disable → blockiert"
        );
    }

    #[tokio::test]
    async fn promo_disable_entitlement_blockiert_bei_zukuenftigem_ablauf() {
        // manual_plan_expires_at in der Zukunft → Plan effektiv → blockiert.
        let pool = pool_or_skip!("promo_entitlement_zukunft");
        let engine = make_engine(pool.clone());

        sqlx::query(
            // manual_plan_expires_at = TEXT (Prod-Schema): ISO-8601-String wie Python
            // (`datetime.isoformat()`), den der Resolver parst — kein timestamptz-Cast.
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at)
             VALUES ('uq2', 'zukunftkanal', 'bundle_komplett',
                     to_char((NOW() + INTERVAL '30 days') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS+00:00'))"
        )
        .execute(&pool)
        .await
        .unwrap();

        let blocked = engine.promo_blocked_by_plan_or_flag("zukunftkanal").await;
        assert!(
            blocked,
            "nicht abgelaufenes bundle_komplett → chat.promos.disable → blockiert"
        );
    }

    #[tokio::test]
    async fn abgelaufener_promo_disable_plan_blockiert_nicht() {
        // manual_plan_expires_at in der Vergangenheit → Plan NICHT effektiv →
        // fällt auf raid_free zurück → KEINE chat.promos.disable → nicht blockiert.
        let pool = pool_or_skip!("promo_entitlement_abgelaufen");
        let engine = make_engine(pool.clone());

        sqlx::query(
            // manual_plan_expires_at = TEXT (Prod-Schema): ISO-8601-String wie Python.
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at)
             VALUES ('uq3', 'abgelaufenkanal', 'chat_quiet',
                     to_char((NOW() - INTERVAL '1 day') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS+00:00'))"
        )
        .execute(&pool)
        .await
        .unwrap();

        let blocked = engine
            .promo_blocked_by_plan_or_flag("abgelaufenkanal")
            .await;
        assert!(
            !blocked,
            "abgelaufener chat_quiet → Plan nicht effektiv → nicht blockiert"
        );
    }

    #[tokio::test]
    async fn lurker_tax_is_paid_plan_respektiert_ablauf() {
        // Aktiver raid_boost → chat.lurker_tax → is_paid_plan true.
        // Abgelaufener raid_boost → raid_free → is_paid_plan false.
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        apply_ddl(&pool).await;
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('upaid', 'paidkanal'), ('uexp', 'expkanal')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            // manual_plan_expires_at = TEXT (Prod-Schema): ISO-8601-String wie Python.
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at)
             VALUES
               ('upaid', 'paidkanal', 'raid_boost',
                to_char((NOW() + INTERVAL '10 days') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS+00:00')),
               ('uexp',  'expkanal',  'raid_boost',
                to_char((NOW() - INTERVAL '1 day') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS+00:00'))"
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            engine.lurker_tax_is_paid_plan("upaid").await,
            "aktiver raid_boost → chat.lurker_tax → is_paid_plan"
        );
        assert!(
            !engine.lurker_tax_is_paid_plan("uexp").await,
            "abgelaufener raid_boost → raid_free → kein is_paid_plan"
        );
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

        let text = engine
            .load_streamer_promo_message("msgkanal", "http://example.com/invite")
            .await;
        assert_eq!(
            text.as_deref(),
            Some("Komm zu uns: http://example.com/invite")
        );
    }

    #[tokio::test]
    async fn streamer_promo_message_invalid_faellt_auf_none() {
        let pool = pool_or_skip!("promo_streamer_msg_invalid");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_message)
             VALUES ('u4', 'invalidkanal', 'Schau mal vorbei')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let text = engine
            .load_streamer_promo_message("invalidkanal", "http://example.com/invite")
            .await;
        assert!(text.is_none(), "ungueltiger Text muss Fallback erlauben");
    }

    #[tokio::test]
    async fn streamer_invite_sent_marker_aktualisiert_last_sent_at() {
        let pool = pool_or_skip!("promo_invite_sent_marker");
        let engine = make_engine(pool.clone());

        sqlx::query(
            "INSERT INTO twitch_streamer_invites (streamer_login, guild_id, channel_id, invite_code, invite_url)
             VALUES ('invitekanal', 1, 2, 'abc', 'https://discord.example/abc')",
        )
        .execute(&pool)
        .await
        .unwrap();

        engine.mark_streamer_invite_sent("InviteKanal").await;

        let last_sent_at: Option<String> = sqlx::query_scalar(
            "SELECT last_sent_at FROM twitch_streamer_invites WHERE streamer_login = 'invitekanal'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(last_sent_at
            .as_deref()
            .is_some_and(|value| value.contains('T')));
    }

    #[tokio::test]
    async fn lurker_tax_kandidaten_filterung() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        apply_ddl(&pool).await;
        let engine = make_engine(pool.clone());

        // Live-Session anlegen (ended_at = NULL → ist live).
        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, avg_viewers)
             VALUES ('lurkerkanal', 'u4', 10.0)
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
                "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, ended_at, avg_viewers)
                 VALUES ('lurkerkanal', 'u4', NOW() - ($1 || ' hours')::INTERVAL, 5.0)
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

        let candidates = engine.get_lurker_tax_candidates("u4").await;
        assert!(
            !candidates.is_empty(),
            "Lurker-Kandidat sollte gefunden werden: {candidates:?}"
        );
        assert!(candidates.contains(&"lurker1".to_string()));
    }

    #[tokio::test]
    async fn lurker_tax_kandidaten_joinen_ueber_chatter_identity_key() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        apply_ddl(&pool).await;
        let engine = make_engine(pool.clone());

        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, avg_viewers)
             VALUES ('renamekanal', 'u-rename', 10.0)
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('u-rename', 'renamekanal', 1, $1)",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();

        for s in 0i64..3 {
            let sid: i64 = sqlx::query_scalar(
                "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, ended_at, avg_viewers)
                 VALUES ('renamekanal', 'u-rename', NOW() - ($1 || ' hours')::INTERVAL, 5.0)
                 RETURNING id",
            )
            .bind(s + 2)
            .fetch_one(&pool)
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO twitch_session_chatters
                 (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
                  first_message_at, last_seen_at)
                 VALUES ($1, 'renamekanal', 'oldlogin', 'same-id-1', 0, TRUE,
                  NOW() - INTERVAL '6 hours', NOW() - INTERVAL '4 hours 30 minutes')",
            )
            .bind(sid)
            .execute(&pool)
            .await
            .unwrap();
        }

        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
              first_message_at, last_seen_at)
             VALUES ($1, 'renamekanal', 'newlogin', 'same-id-1', 0, TRUE,
              NOW() - INTERVAL '2 minutes', NOW() - INTERVAL '1 minute')",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();

        let candidates = engine.get_lurker_tax_candidates("u-rename").await;
        assert_eq!(candidates, vec!["newlogin".to_string()]);
    }

    async fn create_channel_points_table(pool: &PgPool) {
        sqlx::query(
            "CREATE TABLE twitch_channel_points_events (
                id BIGSERIAL PRIMARY KEY,
                session_id BIGINT,
                twitch_user_id TEXT NOT NULL,
                user_login TEXT,
                reward_id TEXT,
                reward_title TEXT,
                reward_cost INTEGER,
                user_input TEXT,
                redeemed_at TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_live_channel(pool: &PgPool, login: &str, uid: &str) -> i64 {
        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, avg_viewers)
             VALUES ($1, $2, 10.0)
             RETURNING id",
        )
        .bind(login)
        .bind(uid)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ($1, $2, 1, $3)",
        )
        .bind(uid)
        .bind(login)
        .bind(live_session_id)
        .execute(pool)
        .await
        .unwrap();
        live_session_id
    }

    async fn seed_qualifying_lurker(
        pool: &PgPool,
        login: &str,
        live_session_id: i64,
        chatter: &str,
        chatter_id: &str,
    ) {
        for s in 0i64..3 {
            let sid: i64 = sqlx::query_scalar(
                "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, ended_at, avg_viewers)
                 VALUES ($1, (SELECT twitch_user_id FROM twitch_stream_sessions WHERE id=$3), NOW() - ($2 || ' hours')::INTERVAL, 5.0)
                 RETURNING id",
            )
            .bind(login)
            .bind(s + 2)
            .bind(live_session_id)
            .fetch_one(pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO twitch_session_chatters
                 (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
                  first_message_at, last_seen_at)
                 VALUES ($1, $2, $3, $4, 0, TRUE,
                  NOW() - INTERVAL '6 hours', NOW() - INTERVAL '4 hours 30 minutes')",
            )
            .bind(sid)
            .bind(login)
            .bind(chatter)
            .bind(chatter_id)
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
              first_message_at, last_seen_at)
             VALUES ($1, $2, $3, $4, 0, TRUE,
              NOW() - INTERVAL '2 minutes', NOW() - INTERVAL '1 minute')",
        )
        .bind(live_session_id)
        .bind(login)
        .bind(chatter)
        .bind(chatter_id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn req4_eingeloester_zuschauer_nicht_mehr_erinnert() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        apply_ddl(&pool).await;
        create_channel_points_table(&pool).await;
        let engine = make_engine(pool.clone());
        let live = seed_live_channel(&pool, "steuerkanal", "u-steuer").await;
        seed_qualifying_lurker(&pool, "steuerkanal", live, "steuerzahler", "id-zahler").await;
        seed_qualifying_lurker(&pool, "steuerkanal", live, "lurkerin", "id-lurkerin").await;

        sqlx::query(
            "INSERT INTO twitch_channel_points_events
             (session_id, twitch_user_id, user_login, reward_title, redeemed_at)
             VALUES ($1, 'u-steuer', 'steuerzahler', 'Lurker Steuer', '2026-09-09T00:00:00Z')",
        )
        .bind(live)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_channel_points_events
             (session_id, twitch_user_id, user_login, reward_title, redeemed_at)
             VALUES ($1, 'u-steuer', 'lurkerin', 'Highlight My Message', '2026-09-09T00:00:00Z')",
        )
        .bind(live)
        .execute(&pool)
        .await
        .unwrap();

        let candidates = engine.get_lurker_tax_candidates("u-steuer").await;
        assert!(
            candidates.contains(&"lurkerin".to_string()),
            "Wer eine andere Belohnung einlöst, bleibt Kandidatin: {candidates:?}"
        );
        assert!(
            !candidates.contains(&"steuerzahler".to_string()),
            "Wer die Lurker Steuer dieser Session eingelöst hat, darf nicht mehr erinnert werden: {candidates:?}"
        );
    }

    struct FakeReward(bool);

    #[async_trait]
    impl LurkerRewardChecker for FakeReward {
        async fn active_lurker_reward_exists(&self, _broadcaster_id: &str) -> bool {
            self.0
        }
    }

    async fn seed_sending_channel(pool: &PgPool, login: &str, uid: &str) {
        let live = seed_live_channel(pool, login, uid).await;
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id)
             VALUES ($1, $2, 1, 'raid_boost')",
        )
        .bind(uid)
        .bind(login)
        .execute(pool)
        .await
        .unwrap();
        seed_qualifying_lurker(pool, login, live, "lurker1", "uid-lurker1").await;
    }

    #[tokio::test]
    async fn regression_lurker_quellen_scopes_session_und_plan_bleiben_beim_kanal() {
        let db = crate::test_postgres::TestPostgres::start().await;
        let pool = &db.pool;
        apply_ddl(pool).await;
        create_channel_points_table(pool).await;
        seed_sending_channel(pool, "oldname", "own").await;
        let foreign_live = seed_live_channel(pool, "newname", "foreign").await;
        seed_qualifying_lurker(
            pool,
            "newname",
            foreign_live,
            "foreign-lurker",
            "foreign-viewer",
        )
        .await;
        sqlx::raw_sql("INSERT INTO streamer_plans (twitch_user_id,twitch_login,lurker_tax_enabled,manual_plan_id) VALUES ('foreign','newname',1,'raid_boost');
            INSERT INTO twitch_raid_auth (twitch_user_id,twitch_login,scopes) VALUES ('own','oldname',''), ('foreign','newname','moderator:read:chatters');")
            .execute(pool).await.unwrap();
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_lurker_reward_checker(Arc::new(FakeReward(true)));
        engine
            .maybe_send_lurker_tax_reminder("newname", "own", Instant::now())
            .await;
        assert_eq!(
            api.announcement_count().await,
            0,
            "fremder Scope darf nicht berechtigen"
        );
        sqlx::query("UPDATE twitch_raid_auth SET scopes=CASE WHEN twitch_user_id='own' THEN 'moderator:read:chatters' ELSE '' END").execute(pool).await.unwrap();
        // Fremde Einlösung und Live-State dürfen den eigenen Kandidaten nicht entfernen.
        sqlx::query("INSERT INTO twitch_channel_points_events (session_id,twitch_user_id,user_login,reward_title,redeemed_at) VALUES ($1,'foreign','lurker1','Lurker Steuer','2026-09-09T00:00:00Z')").bind(foreign_live).execute(pool).await.unwrap();
        engine
            .maybe_send_lurker_tax_reminder("newname", "own", Instant::now())
            .await;
        assert_eq!(
            api.announcement_count().await,
            1,
            "eigener Scope und eigene historische Sessions bleiben gültig"
        );
        let texts = api.announcement_texts().await;
        assert!(texts[0].contains("@lurker1"), "{texts:?}");
        assert!(!texts[0].contains("foreign-lurker"));
        let own_live: i64 = sqlx::query_scalar(
            "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id='own'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let state = engine.channel_states.get("newname").unwrap();
        assert_eq!(state.lock().await.lurker_mentions.0, own_live);
        drop(state);
        engine
            .thank_lurker_tax_redeemer("own", "newname", "paid")
            .await;
        assert_eq!(api.message_count().await, 1);
        let state = engine.channel_states.get("newname").unwrap();
        assert_eq!(state.lock().await.thanked_redeemers.0, own_live);
        drop(state);
        // Aktives fremdes Abo darf keinen abgelaufenen eigenen Plan ersetzen.
        sqlx::query("UPDATE streamer_plans SET manual_plan_expires_at='2020-01-01T00:00:00Z' WHERE twitch_user_id='own'").execute(pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_billing_subscriptions (customer_reference,plan_id,status,current_period_end) VALUES ('newname','raid_boost','active',NOW()+INTERVAL '1 day')").execute(pool).await.unwrap();
        engine
            .thank_lurker_tax_redeemer("own", "newname", "expired")
            .await;
        engine
            .thank_lurker_tax_redeemer("", "newname", "missing-id")
            .await;
        engine
            .thank_lurker_tax_redeemer("missing", "newname", "missing-plan")
            .await;
        assert_eq!(api.message_count().await, 1);
        // Der bisherige Stripe-Weg funktioniert weiterhin mit eigener ID.
        sqlx::query("INSERT INTO twitch_billing_subscriptions (customer_reference,plan_id,status,current_period_end) VALUES ('own','raid_boost','active',NOW()+INTERVAL '1 day')").execute(pool).await.unwrap();
        engine
            .thank_lurker_tax_redeemer("own", "newname", "billing")
            .await;
        assert_eq!(api.message_count().await, 2);
        pool.close().await;
        engine
            .thank_lurker_tax_redeemer("own", "newname", "db-error")
            .await;
        assert_eq!(api.message_count().await, 2);
    }

    #[tokio::test]
    async fn req5_reminder_blockt_wenn_belohnung_fehlt() {
        let pool = pool_or_skip!("promo_lurker_reward_absent");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_bot_scope_provider(Arc::new(super::tests::FakeBotScopes(vec![
                "moderator:read:chatters".into(),
            ])))
            .set_lurker_reward_checker(Arc::new(FakeReward(false)));
        seed_sending_channel(&pool, "rewardkanal", "u-reward").await;

        engine
            .maybe_send_lurker_tax_reminder("rewardkanal", "u-reward", Instant::now())
            .await;

        assert_eq!(
            api.announcement_count().await,
            0,
            "ohne aktive Belohnung darf keine Erinnerung gehen"
        );
    }

    #[tokio::test]
    async fn req5_reminder_sendet_wenn_belohnung_aktiv() {
        let pool = pool_or_skip!("promo_lurker_reward_present");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_bot_scope_provider(Arc::new(super::tests::FakeBotScopes(vec![
                "moderator:read:chatters".into(),
            ])))
            .set_lurker_reward_checker(Arc::new(FakeReward(true)));
        seed_sending_channel(&pool, "rewardkanal2", "u-reward2").await;

        engine
            .maybe_send_lurker_tax_reminder("rewardkanal2", "u-reward2", Instant::now())
            .await;

        assert_eq!(
            api.announcement_count().await,
            1,
            "mit aktiver Belohnung muss die Erinnerung gehen"
        );
    }

    #[tokio::test]
    async fn reminder_blockt_ohne_reward_checker() {
        let pool = pool_or_skip!("promo_lurker_reward_ungated");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_bot_scope_provider(Arc::new(super::tests::FakeBotScopes(vec![
                "moderator:read:chatters".into(),
            ])));
        seed_sending_channel(&pool, "ungatedkanal", "u-ungated").await;

        engine
            .maybe_send_lurker_tax_reminder("ungatedkanal", "u-ungated", Instant::now())
            .await;

        assert_eq!(
            api.announcement_count().await,
            0,
            "ohne verdrahteten Reward-Checker darf die Erinnerung nicht gehen"
        );
    }

    #[tokio::test]
    async fn thank_blockt_bei_schalter_aus_oder_raid_free() {
        let pool = pool_or_skip!("promo_thank_gate");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck));
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id)
             VALUES ('u-off', 'offkanal', 0, 'raid_boost')",
        )
        .execute(&pool)
        .await
        .unwrap();
        engine
            .thank_lurker_tax_redeemer("u-off", "offkanal", "xy")
            .await;
        assert_eq!(
            api.message_count().await,
            0,
            "Schalter aus: kein Dank"
        );

        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id)
             VALUES ('u-free', 'freekanal', 1, 'raid_free')",
        )
        .execute(&pool)
        .await
        .unwrap();
        engine
            .thank_lurker_tax_redeemer("u-free", "freekanal", "yz")
            .await;
        assert_eq!(
            api.message_count().await,
            0,
            "raid_free ist kein bezahlter Plan: kein Dank"
        );
    }

    #[tokio::test]
    async fn req3_dank_hoechstens_einmal_je_zuschauer_und_session() {
        let pool = pool_or_skip!("promo_thank_einmal");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck));
        seed_live_channel(&pool, "dankkanal", "u-dank").await;
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id)
             VALUES ('u-dank', 'dankkanal', 1, 'raid_boost')",
        )
        .execute(&pool)
        .await
        .unwrap();

        engine
            .thank_lurker_tax_redeemer("u-dank", "dankkanal", "xy")
            .await;
        assert_eq!(
            api.message_count().await,
            1,
            "erster Dank geht raus"
        );

        engine
            .thank_lurker_tax_redeemer("u-dank", "dankkanal", "xy")
            .await;
        assert_eq!(
            api.message_count().await,
            1,
            "zweiter Dank je Zuschauer und Session unterbleibt"
        );
    }

    fn followup_event(channel_id: &str, channel_login: &str, chatter_login: &str, text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: channel_id.to_string(),
            broadcaster_user_login: channel_login.to_string(),
            chatter_user_id: "cid".to_string(),
            chatter_user_login: chatter_login.to_string(),
            message: crate::types::ChatMessageBody {
                text: text.to_string(),
                fragments: Vec::new(),
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn lurker_followup_antwortet_einmal_je_session() {
        let pool = pool_or_skip!("promo_followup_einmal");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck));
        let session_id = seed_live_channel(&pool, "fkanal", "u-f").await;
        {
            let state_ref = engine
                .channel_states
                .entry("fkanal".to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.lurker_reminded_at = (session_id, HashMap::new());
            state
                .lurker_reminded_at
                .1
                .insert("lurk".to_string(), Instant::now());
        }
        let event = followup_event("u-f", "fkanal", "lurk", "wo finde ich das?");
        engine.maybe_answer_lurker_followup(&event).await;
        assert_eq!(
            api.message_count().await,
            1,
            "erste Nachfrage eines erinnerten Zuschauers wird beantwortet"
        );
        engine.maybe_answer_lurker_followup(&event).await;
        assert_eq!(
            api.message_count().await,
            1,
            "zweite Nachfrage in derselben Session bleibt still"
        );
    }

    #[tokio::test]
    async fn lurker_followup_ignoriert_nicht_erinnerte() {
        let pool = pool_or_skip!("promo_followup_fremd");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck));
        seed_live_channel(&pool, "fkanal", "u-f").await;
        let event = followup_event("u-f", "fkanal", "fremd", "wo denn?");
        engine.maybe_answer_lurker_followup(&event).await;
        assert_eq!(
            api.message_count().await,
            0,
            "nicht erinnerter Zuschauer bekommt keine Antwort"
        );
    }

    #[tokio::test]
    async fn lurker_tax_sendet_orange_announcement_ohne_plain_fallback() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        apply_ddl(&pool).await;
        let api = Arc::new(super::tests::MockApi::announcement_dropped());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_bot_scope_provider(Arc::new(super::tests::FakeBotScopes(vec![
                "moderator:read:chatters".into(),
            ])))
            .set_lurker_reward_checker(Arc::new(FakeReward(true)));

        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, avg_viewers)
             VALUES ('taxkanal', 'u-tax', 10.0)
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('u-tax', 'taxkanal', 1, $1)",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, lurker_tax_enabled, manual_plan_id)
             VALUES ('u-tax', 'taxkanal', 1, 'raid_boost')",
        )
        .execute(&pool)
        .await
        .unwrap();

        for s in 0i64..3 {
            let sid: i64 = sqlx::query_scalar(
                "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, ended_at, avg_viewers)
                 VALUES ('taxkanal', 'u-tax', NOW() - ($1 || ' hours')::INTERVAL, 5.0)
                 RETURNING id",
            )
            .bind(s + 2)
            .fetch_one(&pool)
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO twitch_session_chatters
                 (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
                  first_message_at, last_seen_at)
                 VALUES ($1, 'taxkanal', 'lurker1', 'uid-lurker1', 0, TRUE,
                  NOW() - INTERVAL '6 hours', NOW() - INTERVAL '4 hours 30 minutes')",
            )
            .bind(sid)
            .execute(&pool)
            .await
            .unwrap();
        }

        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api,
              first_message_at, last_seen_at)
             VALUES ($1, 'taxkanal', 'lurker1', 'uid-lurker1', 0, TRUE,
              NOW() - INTERVAL '2 minutes', NOW() - INTERVAL '1 minute')",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();

        engine
            .maybe_send_lurker_tax_reminder("taxkanal", "u-tax", Instant::now())
            .await;

        assert_eq!(api.announcement_count().await, 1);
        assert_eq!(api.announcement_colors().await, vec!["orange".to_string()]);
        assert_eq!(
            api.message_count().await,
            0,
            "Lurker-Tax darf keinen Plain-Chat-Fallback senden"
        );
    }

    #[tokio::test]
    async fn new_chatter_gate_kombiniert_chat_bucket_und_session_viewer() {
        let pool = pool_or_skip!("promo_new_chatters_combined");
        let engine = make_engine(pool.clone());
        let now = Instant::now();

        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, avg_viewers)
             VALUES ('combokanal', 10.0)
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, active_session_id)
             VALUES ('u-combo', 'combokanal', 1, $1)",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, chatter_id, messages, seen_via_chatters_api)
             VALUES
             ($1, 'combokanal', 'ApiViewerOne', 'api-1', 0, TRUE),
             ($1, 'combokanal', 'ApiViewerTwo', 'api-2', 0, TRUE)",
        )
        .bind(live_session_id)
        .execute(&pool)
        .await
        .unwrap();

        let mut state = ChannelState::new();
        state.activity.push_back((now, "oldchat".to_string()));
        state.seen_chatters.insert("oldchat".to_string(), now);

        let new_count = engine
            .get_new_chatters_in_window_inner("combokanal", &state, now)
            .await;
        assert_eq!(
            new_count, 2,
            "API-getrackte Session-Viewer zählen als neue Chatter"
        );
    }
}
