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
use crate::commands::{InviteReplyNotifier, PromoBlockCheck};
use crate::suppression_guard::SuppressionGuardChatApi;
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
pub const DEFAULT_PROMO_DISCORD_INVITE: &str = "https://discord.gg/z5TfVHuQq2";

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
/// Mehr Kandidaten holen als am Ende erwähnt werden, damit nach dem Per-Session-
/// Dedup die nächstrangigen Lurker nachrücken (promos.py: fetch > MAX, dann kappen).
const LURKER_TAX_CANDIDATE_FETCH: i64 = 25;
/// MiniMax-Timeout in Sekunden (targeted_promo.py:37: _MINIMAX_TIMEOUT_SEC).
const MINIMAX_TIMEOUT_SEC: u64 = 5;
/// Keine Promo in den ersten N Minuten nach Go-Live (constants.py: PROMO_STREAM_START_DELAY_MIN).
const PROMO_STREAM_START_DELAY_MIN: u64 = 10;

// ---------------------------------------------------------------------------
// Promo-Texte — Ursprung constants.py:114–152, seitdem erweitert
// ---------------------------------------------------------------------------

/// Standard-Promo-Texte, kategorisiert. Placeholder `{invite}` wird ersetzt.
/// Port von PROMO_MESSAGES_CATEGORIZED (constants.py).
///
/// Kategorie = Routing, nicht nur Ordnung: `generic`/`hype` fehlen im
/// `chat_activity`-Pool (siehe [`activity_promo_messages`]).
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
        "Sei nicht nur während des Streams Teil der Community, komm auf unseren Discord: {invite}",
        "Der Stream geht irgendwann offline, die Community nicht: {invite}",
        "Community ist mehr als der Chat hier. Wir sind auch zwischen den Streams da: {invite}",
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

/// Alle Standard-Promo-Texte kombiniert (PROMO_MESSAGES, 25 Einträge gesamt).
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

/// 5 globale Presets — 1:1 zu `bot/chat/promo_presets.py` (IDs/Texte/Tags stabil,
/// damit die MiniMax-Auswahl dieselben Schlagwörter sieht).
pub fn global_presets() -> Vec<PromoPreset> {
    vec![
        PromoPreset {
            id: "g_competitive",
            preset_type: PresetType::Global,
            text: "Kein Bock mehr auf Solo-Queue-Grief? Such dir feste Mates in unserer deutschen Deadlock-Community! {invite} 🔫",
            tags: "ranked, mmr, solo_queue, competitive, mates",
        },
        PromoPreset {
            id: "g_community",
            preset_type: PresetType::Global,
            text: "Bock auf Inhouses oder kleine Turniere? Wir organisieren regelmäßig Events – schau gerne vorbei: {invite} 🏆",
            tags: "inhouse, events, tournament, community, fun",
        },
        PromoPreset {
            id: "g_new_to_deadlock",
            preset_type: PresetType::Global,
            text: "Neu in Deadlock? Unsere Community hat Guides, Tipps und Leute die gerne helfen: {invite} 📚",
            tags: "new_player, beginner, guide, help, learning",
        },
        PromoPreset {
            id: "g_meta",
            preset_type: PresetType::Global,
            text: "Patch-Diskussionen, Tier-Listen, Meta-Talks – alles bei uns auf Discord: {invite}",
            tags: "meta, patch, tierlist, discussion, build",
        },
        PromoPreset {
            id: "g_chill",
            preset_type: PresetType::Global,
            text: "Wer nach dem Stream noch Deadlock zockt und ne Community sucht – wir sind auf Discord: {invite} 👀",
            tags: "chill, after_stream, looking_for_group, casual",
        },
    ]
}

/// 5 user-spezifische Presets — 1:1 zu `promo_presets.py` (type="user"). Enthalten `{login}`.
pub fn user_presets() -> Vec<PromoPreset> {
    vec![
        PromoPreset {
            id: "u_welcome",
            preset_type: PresetType::User,
            text: "@{login} Willkommen! Falls du noch eine deutsche Deadlock-Community suchst – hier entlang: {invite} 🎮",
            tags: "new, first_time, welcome, lurker",
        },
        PromoPreset {
            id: "u_mates",
            preset_type: PresetType::User,
            text: "@{login} Falls du Mates zum Zocken suchst, in unserer Community wirst du fündig: {invite}",
            tags: "looking_for_group, mates, team, duo, party",
        },
        PromoPreset {
            id: "u_lurker_viewer",
            preset_type: PresetType::User,
            text: "@{login} Regelmäßig dabei? 👀 Falls du über Deadlock reden willst, komm gerne auf unseren Discord: {invite}",
            tags: "lurker, regular_viewer, silent, watcher",
        },
        PromoPreset {
            id: "u_ranked_grind",
            preset_type: PresetType::User,
            text: "@{login} Ranked-Grind solo macht keinen Spaß – bei uns findest du Leute für den Duo-Queue: {invite} 🔫",
            tags: "ranked, competitive, grind, duo, elo",
        },
        PromoPreset {
            id: "u_new_player",
            preset_type: PresetType::User,
            text: "@{login} Neu in Deadlock? Unsere Community hilft gerne beim Einstieg – schau mal vorbei: {invite} 📚",
            tags: "new_player, beginner, help, learning, guide",
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
    /// Aktivitäts-Bucket (deque, maxlen 2048) — promos.py:730.
    activity: VecDeque<ActivityEntry>,
    /// Chatter-Dedup-Map: chatter_login → letzter dedup-Zeitstempel (30s) — promos.py:730.
    chatter_dedupe: HashMap<String, Instant>,
    /// Letzter gesendeter Promo-Text (Anti-Repeat) — promos.py:975.
    last_promo_text: Option<String>,
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
}

impl ChannelState {
    fn new() -> Self {
        Self {
            activity: VecDeque::with_capacity(64),
            chatter_dedupe: HashMap::new(),
            last_promo_text: None,
            last_promo_sent: None,
            last_promo_attempt: None,
            last_promo_viewer_spike: None,
            raw_msg_count_since_promo: 0,
            last_raw_chat_message_ts: None,
            seen_chatters: HashMap::new(),
            last_accessed: Instant::now(),
            lurker_mentions: (0, HashSet::new()),
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
    /// Schreibseite der Outbound-Suppression (channel_settings-Drop → Mute).
    /// `None` = Schreiben deaktiviert (Verhalten wie vor P1.1).
    suppression_writer: Option<Arc<dyn OutboundSuppressionWriter>>,
    /// Scope-Quelle des zentralen Bot-Tokens (P1.4). `None` = nur Streamer-Scope
    /// zählt (Verhalten wie vor P1.4).
    bot_scope_provider: Option<Arc<dyn BotScopeProvider>>,
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

// Plan-Entitlement-Auflösung (chat.lurker_tax / chat.promos.disable) läuft über
// `tb_analytics::plan::resolve_plan_snapshot` — die volle Snapshot-Resolution
// (Manual-Override mit Ablauf-Gate via `manual_plan_expires_at`, Bundles, Legacy-
// Aliase, Stripe-Abo-Fallback). Das ersetzt die frühere statische Plan-Whitelist:
// sie kannte keinen Plan-Ablauf, sodass abgelaufene Werbefrei-/Lurker-Tax-Pläne
// weiterwirkten. Single Source of Truth ist jetzt `tb_analytics::plan`.

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
            suppression_writer: None,
            bot_scope_provider: None,
            invite_resolver: Arc::new(StaticInviteResolver),
            partner_check: Arc::new(AlwaysPartner),
            preset_picker: Arc::new(RandomPresetPicker),
            send_locks: DashMap::new(),
            channel_states: DashMap::new(),
            targeted_state: Mutex::new(TargetedState::new()),
        }
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

    /// Setzt den PresetPicker (Default: RandomPresetPicker).
    pub fn set_preset_picker(mut self, p: Arc<dyn PresetPicker>) -> Self {
        self.preset_picker = p;
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
    pub async fn on_message(&self, event: &ChatMessageEvent) {
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

        // Doppelsend-Lock (promos.py:798).
        let lock = self.get_send_lock(&login);
        let _guard = lock.lock().await;

        // maybe_send_promo_with_stats (promos.py:1281).
        let channel_id = event.broadcaster_user_id.clone();
        self.maybe_send_promo_with_stats(&login, &channel_id, now)
            .await;
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
    async fn send_promo_if_due(&self, now: Instant) {
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

        for (login, channel_id) in &live_channels {
            // Plan-Flag-Check (promos.py:1466: _promo_blocked_by_plan_or_flag).
            if self.promo_blocked_by_plan_or_flag(login).await {
                continue;
            }

            // Partner-Gate (promos.py:1524/1533: is_partner_channel_for_chat_tracking).
            // Gilt für die periodische Promo-Iteration:
            // Kanäle, die zwar live + promo_disabled=0 sind, im Chat-Tracking aber nicht als
            // Partner gelten, dürfen KEINE periodischen Promos erhalten.
            if !self
                .partner_check
                .is_partner_channel_for_chat_tracking(login)
                .await
            {
                continue;
            }

            // Channel-Allowlist gilt für die gesamte Promo-Iteration inklusive
            // Targeted-Promo, nicht aber für den separaten Lurker-Tax-Pfad.
            if !self.promo_channel_allowed_db(login).await {
                continue;
            }

            // Doppelsend-Lock (promos.py:1466).
            let lock = self.get_send_lock(login);
            let _guard = lock.lock().await;

            // Overall-Ready + Activity-Ready prüfen (promos.py:1466).
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

            // Scam+Targeted nur im fälligen Slot (promos.py:1466: activity_ready Pflicht).
            if overall_ready && activity_ready && self.stream_start_delay_ok(login).await {
                let (invite, _is_specific) = self.invite_resolver.resolve_invite(login).await;

                // Targeted-Promo-Slot (targeted_promo.py:198).
                let active_chatters = self.get_active_chatters(login).await;
                if self
                    .maybe_send_targeted_promo(login, channel_id, &invite, &active_chatters, now)
                    .await
                {
                    continue;
                }

                // Activity-Promo (promos.py:1466).
                let sent = self
                    .maybe_send_promo_with_stats(login, channel_id, now)
                    .await;

                // Viewer-Spike (promos.py:1466).
                if !sent {
                    self.maybe_send_viewer_spike_promo(login, channel_id, now)
                        .await;
                }
            }
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
        let attempt_allowed = self.promo_attempt_allowed_inner(&state_snapshot, now);

        if !overall_ready || !activity_ready || !attempt_allowed {
            return false;
        }

        // Attempt-Timestamp setzen.
        {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_attempt = Some(now);
        }
        // DB-Persist attempt (promos.py:879: save_promo_cooldown "attempt").
        self.save_promo_cooldown(login, "attempt", Utc::now().timestamp() as f64)
            .await;

        self.send_promo_message(login, channel_id, now, "chat_activity")
            .await
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
        // Invite-Auflösung (promos.py:1096).
        let (invite, is_specific) = self.invite_resolver.resolve_invite(login).await;

        // Promo-Text bauen (promos.py:945).
        let text = self.build_promo_text(login, &invite, reason).await;

        // Announcement senden (promos.py:1096, color="purple").
        let sent = self
            .api
            .send_announcement(channel_id, &text, "purple")
            .await
            .unwrap_or(false);
        if !sent {
            debug!(login, "Promo-Announcement nicht gesendet (Drop/Fehler)");
            // Cooldown NICHT verbrauchen bei Failed-Send (promos.py:1096: if not ok: return False).
            return false;
        }

        // Promo markieren (promos.py:879).
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
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;
            state.last_promo_text.clone()
        };
        let pool: Vec<&str> = if let Some(ref last) = last_text {
            let filtered: Vec<&str> = pool
                .iter()
                .copied()
                .filter(|t| *t != last.as_str())
                .collect();
            if filtered.is_empty() {
                pool
            } else {
                filtered
            }
        } else {
            pool
        };

        let template = {
            let mut rng = rand::thread_rng();
            pool.choose(&mut rng).copied().unwrap_or("{invite}")
        };

        // Anti-Repeat zurückschreiben (promos.py:975: last_map[login] = chosen).
        {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_text = Some(template.to_string());
        }

        render_promo_template(template, invite)
            .unwrap_or_else(|| template.replace("{invite}", invite))
    }

    /// Globalen Promo-Override laden (promos.py `_load_global_promo_message` +
    /// `_build_promo_text`-Schritt 1). Lädt die Singleton-Config aus
    /// `twitch_global_promo_modes`, wertet sie gegen die aktuelle Zeit aus und
    /// gibt — wenn der `custom_event`-Modus aktiv ist — den formatierten
    /// Event-Text zurück (`{invite}` ersetzt). DB-/Auswertungs-Fehler → None
    /// (kein Override, fällt auf Streamer-/Pool-Promo zurück).
    async fn load_global_promo_message(&self, invite: &str) -> Option<String> {
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
        render_promo_template(message, invite)
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
            Some(last) => now.duration_since(last).as_secs() >= PROMO_OVERALL_COOLDOWN_MIN * 60,
        }
    }

    /// Aktivitätsschwellen-Check (promos.py:1251: `_promo_activity_ready`).
    async fn promo_activity_ready_inner(
        &self,
        login: &str,
        state: &ChannelState,
        now: Instant,
    ) -> bool {
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
            let new_chatters = self
                .get_new_chatters_in_window_inner(login, state, now)
                .await;
            if new_chatters < PROMO_NEW_CHATTERS_MIN {
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
            Some(last) => now.duration_since(last).as_secs() >= PROMO_ATTEMPT_COOLDOWN_MIN * 60,
        }
    }

    /// Viewer-Spike-Promo (promos.py:1306: `_maybe_send_viewer_spike_promo`).
    async fn maybe_send_viewer_spike_promo(&self, login: &str, channel_id: &str, now: Instant) {
        // Guards (promos.py:1306).
        let (overall_ready, has_new_raw, chat_silent, spike_cd_ok, attempt_ok) = {
            let state_ref = self
                .channel_states
                .entry(login.to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let state = state_ref.lock().await;

            let overall = self.overall_promo_ready_inner(&state, now);
            let has_raw = state.raw_msg_count_since_promo > 0;
            // Python: activity_age_sec is None → kein Chat → Silence gilt als OK (promos.py:1355).
            // Rust `is_some_and` würde None als false werten → geblockt. Korrekt: None → true.
            let silent = state.last_raw_chat_message_ts.is_none_or(|t| {
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

        // Lurker-Tax-Settings prüfen (promos.py:193: _load_lurker_tax_settings).
        // streamer_plans.lurker_tax_enabled = integer (Opt-in-Flag, default 0).
        let settings = sqlx::query!(
            "SELECT p.lurker_tax_enabled AS \"lurker_tax_enabled?\",
                    COALESCE(p.twitch_user_id, '') AS \"twitch_user_id!\"
               FROM streamer_plans p
              WHERE LOWER(COALESCE(p.twitch_login,'')) = $1
              LIMIT 1",
            login.to_lowercase(),
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let (enabled, plan_user_id) = match settings {
            Some(row) => (row.lurker_tax_enabled.unwrap_or(0) != 0, row.twitch_user_id),
            None => return,
        };
        if !enabled {
            return;
        }

        // is_paid_plan: effektiver Plan muss chat.lurker_tax-Entitlement haben
        // (promos.py:355). Volle Snapshot-Resolution → abgelaufene Pläne taxen
        // nicht mehr. user_id (aus streamer_plans oder identities) priorisiert
        // den Override-Match.
        let user_id = if !plan_user_id.is_empty() {
            plan_user_id
        } else {
            sqlx::query_scalar!(
                "SELECT twitch_user_id AS \"twitch_user_id?\" FROM twitch_streamer_identities
                  WHERE LOWER(twitch_login) = $1 LIMIT 1",
                login.to_lowercase(),
            )
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
            .flatten()
            .unwrap_or_default()
        };
        if !self.lurker_tax_is_paid_plan(login, &user_id).await {
            return;
        }

        // has_moderator_read_chatters: Scope muss im Auth-Store vorliegen (promos.py:1410).
        // Prüft twitch_raid_auth.scopes für diesen Streamer.
        let auth_scopes = sqlx::query_scalar!(
            "SELECT scopes AS \"scopes?\" FROM twitch_raid_auth
              WHERE LOWER(COALESCE(twitch_login,'')) = $1
              LIMIT 1",
            login.to_lowercase(),
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let scopes_raw = auth_scopes.flatten().unwrap_or_default();
        if !self.has_chatters_scope(&scopes_raw).await {
            return;
        }

        // Kandidaten holen (promos.py:408).
        let candidates = self.get_lurker_tax_candidates(login).await;
        if candidates.is_empty() {
            return;
        }

        // Aktive Session-ID fürs Per-Session-Dedup.
        let session_id: i64 = sqlx::query_scalar!(
            "SELECT active_session_id FROM twitch_live_state WHERE streamer_login = $1",
            login,
        )
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
    async fn get_lurker_tax_candidates(&self, login: &str) -> Vec<String> {
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
                 WHERE LOWER(sc.streamer_login) = LOWER($1)
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
                   JOIN twitch_live_state ls ON LOWER(ls.streamer_login) = LOWER($1) AND ls.active_session_id = sc.session_id
                  WHERE LOWER(sc.streamer_login) = LOWER($1)
                    AND sc.last_seen_at >= NOW() - INTERVAL '{freshness} minutes'
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
            .bind(login)
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
                    login,
                    "Lurker-Tax-Kandidaten konnten nicht geladen werden"
                );
                Vec::new()
            }
        };

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
            let cd = last
                .is_none_or(|t| now.duration_since(t).as_secs() >= CHANNEL_TARGETED_COOLDOWN_SEC);
            let last_type = ts_state
                .channel_last_type
                .get(login)
                .map(|s| s.as_str())
                .unwrap_or("global");
            let want = last_type == "global"; // alternieren (targeted_promo.py)
            (cd, want)
        };

        if !cd_ok {
            return false;
        }

        // User-Targeted-Pfad (targeted_promo.py:198).
        if want_user && !active_chatters.is_empty() {
            if let Some((target_login, target_id)) =
                self.pick_user_target(active_chatters, login, now).await
            {
                // User-Context laden (targeted_promo.py: _sync_user_context_snippets).
                let snippets = self.load_user_context_snippets(&target_id, login).await;
                let presets = user_presets();
                let preset = tokio::time::timeout(
                    Duration::from_secs(MINIMAX_TIMEOUT_SEC),
                    self.preset_picker
                        .pick_preset(&presets, &snippets, &target_login),
                )
                .await
                .unwrap_or_else(|_| {
                    let mut rng = rand::thread_rng();
                    presets.choose(&mut rng).unwrap_or(&presets[0])
                });

                let text = preset
                    .text
                    .replace("{invite}", invite)
                    .replace("{login}", &target_login);
                // Python targeted_promo.py:264: `if not ok: return False` — bei nicht
                // zugestelltem Send keine State-Mutation/keinen Cooldown verbrennen.
                let outcome = self
                    .guarded_api_for("promo", login)
                    .send_message(channel_id, &text)
                    .await;
                // channel_settings-Drop → Kanal stummschalten (P1.1, source="promo").
                self.record_suppression_on_drop(login, channel_id, "promo", &outcome)
                    .await;
                if !matches!(outcome, Ok(crate::types::SendOutcome::Sent)) {
                    return false;
                }

                {
                    let mut ts_state = self.targeted_state.lock().await;
                    ts_state
                        .channel_last_targeted
                        .insert(login.to_string(), now);
                    ts_state
                        .channel_last_type
                        .insert(login.to_string(), "user".to_string());
                    ts_state
                        .user_last_pitched
                        .insert((login.to_string(), target_login), now);
                }
                // Promo-Slot belegen — Python: mark(channel_login, now, reason="targeted_promo").
                self.mark_promo_sent(login, now, "targeted_promo", Utc::now().timestamp() as f64)
                    .await;

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

        let text = preset
            .text
            .replace("{invite}", invite)
            .replace("{login}", "");
        // Global → Announcement, color="purple" (promo_presets.py).
        // Python targeted_promo.py:264: `if not ok: return False` — Send-Ergebnis prüfen.
        let sent = self
            .guarded_api_for("promo", login)
            .send_announcement(channel_id, &text, "purple")
            .await
            .unwrap_or(false);
        if !sent {
            return false;
        }

        {
            let mut ts_state = self.targeted_state.lock().await;
            ts_state
                .channel_last_targeted
                .insert(login.to_string(), now);
            ts_state
                .channel_last_type
                .insert(login.to_string(), "global".to_string());
        }
        // Promo-Slot belegen — Python: mark(channel_login, now, reason="targeted_promo").
        self.mark_promo_sent(login, now, "targeted_promo", Utc::now().timestamp() as f64)
            .await;

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
                ts_state
                    .user_last_pitched
                    .get(&key)
                    .is_none_or(|&t| now.duration_since(t).as_secs() >= USER_PITCH_COOLDOWN_SEC)
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
            let row = sqlx::query_scalar!(
                "SELECT chatter_id AS \"chatter_id!\" FROM twitch_session_chatters
	                  WHERE LOWER(chatter_login) = LOWER($1)
	                    AND LOWER(streamer_login) = LOWER($2)
	                    AND chatter_id IS NOT NULL
	                  ORDER BY last_seen_at DESC LIMIT 1",
                chatter.as_str(),
                channel_login,
            )
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            let Some(chatter_id) = row else { continue };

            // Stammgast-Check (targeted_promo.py: _sync_is_stammgast).
            // twitch_engagement_conversation: role=text, ts=timestamptz, twitch_user_id=text (prod schema)
            let count: i64 = sqlx::query_scalar!(
                "SELECT COUNT(*) AS \"count!\" FROM twitch_engagement_conversation
                  WHERE channel_login = $1 AND twitch_user_id = $2 AND role = 'user'
                    AND ts > NOW() - ($3::int8 * INTERVAL '1 day')",
                channel_login,
                &chatter_id,
                STAMMGAST_DAYS,
            )
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
        let rows = sqlx::query!(
            "SELECT content AS \"content!\" FROM twitch_engagement_conversation
              WHERE channel_login = $1 AND twitch_user_id = $2 AND role = 'user'
              ORDER BY ts DESC LIMIT 5",
            channel_login,
            user_id,
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter().map(|row| row.content).collect()
    }

    /// Aktive Chatter aus dem Aktivitäts-Bucket (promos.py:1466).
    async fn get_active_chatters(&self, login: &str) -> Vec<String> {
        let state_ref = self
            .channel_states
            .entry(login.to_string())
            .or_insert_with(|| Mutex::new(ChannelState::new()));
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
        if all_promo_messages().is_empty() {
            return false;
        }

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

    /// Lurker-Tax `is_paid_plan`-Gate (promos.py:355: der Plan muss das
    /// Entitlement `chat.lurker_tax` tragen). Nutzt die volle Snapshot-Resolution,
    /// damit abgelaufene Pläne (`manual_plan_expires_at` in der Vergangenheit) das
    /// kostenpflichtige Lurker-Tax-Feature NICHT mehr freischalten.
    /// `user_id` priorisiert den Override-Match (CASE-Order in der Resolution).
    async fn lurker_tax_is_paid_plan(&self, login: &str, user_id: &str) -> bool {
        match tb_analytics::plan::resolve_plan_snapshot(&self.pool, login, user_id).await {
            Ok(snapshot) => snapshot.entitlements.contains(&"chat.lurker_tax"),
            Err(error) => {
                tracing::warn!(
                    %error,
                    login,
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
                true // Im Lock → behalten.
            }
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

    /// Ein Pool-Text ohne `{invite}` wird still ohne Link gepostet, statt zu
    /// scheitern. Doppelte Texte verzerren zusätzlich die Zufallsauswahl und
    /// hebeln den Anti-Repeat-Filter aus (der nur auf Textgleichheit prüft).
    #[test]
    fn jeder_promo_text_traegt_den_invite_platzhalter_und_ist_einzigartig() {
        let alle = all_promo_messages();
        for text in &alle {
            assert!(text.contains("{invite}"), "Promo ohne Invite-Link: {text}");
        }
        let unique: std::collections::HashSet<_> = alle.iter().collect();
        assert_eq!(unique.len(), alle.len(), "doppelter Promo-Text im Pool");
    }

    /// Kategorie ist Routing: `community` muss im `chat_activity`-Pool landen,
    /// sonst laufen die Texte nur bei generischen Anlässen.
    #[test]
    fn community_texte_liegen_im_chat_activity_pool() {
        let activity = activity_promo_messages();
        for text in promo_messages_community() {
            assert!(activity.contains(&text), "community-Text fehlt: {text}");
        }
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

        pub(super) async fn announcement_colors(&self) -> Vec<String> {
            self.announcements
                .lock()
                .await
                .iter()
                .map(|(_, _, color)| color.clone())
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
    // Anti-Repeat-Text
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn promo_text_rotiert_anti_repeat() {
        // Testet, dass build_promo_text nicht zweimal denselben Text zurückgibt
        // wenn der letzte gesetzt ist und Pool groß genug ist.
        let engine = make_engine_no_db();
        // Erster Aufruf: kein last_text.
        let text1 = engine
            .build_promo_text("kanal", DEFAULT_PROMO_DISCORD_INVITE, "promo")
            .await;
        // last_text setzen.
        {
            let state_ref = engine
                .channel_states
                .entry("kanal".to_string())
                .or_insert_with(|| Mutex::new(ChannelState::new()));
            let mut state = state_ref.lock().await;
            state.last_promo_text = Some(text1.clone());
        }
        // Zweiter Aufruf: muss sich unterscheiden (bei Pool-Größe 22 fast sicher).
        // Wir wiederholen mehrmals um Flakiness zu minimieren.
        let mut different = false;
        for _ in 0..20 {
            let text2 = engine
                .build_promo_text("kanal", DEFAULT_PROMO_DISCORD_INVITE, "promo")
                .await;
            if text2 != text1 {
                different = true;
                break;
            }
        }
        assert!(
            different,
            "Anti-Repeat: nach 20 Versuchen sollte ein anderer Text kommen"
        );
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
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    struct FixedSuppression(bool);

    #[async_trait]
    impl OutboundSuppressionCheck for FixedSuppression {
        async fn is_muted(&self, _channel_login: &str) -> bool {
            self.0
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
                trials_granted INTEGER NOT NULL DEFAULT 0,
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
                twitch_login TEXT
            )"#,
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
                scopes TEXT
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
    async fn targeted_promo_respektiert_promo_channel_allowed_gate() {
        let pool = pool_or_skip!("promo_targeted_allowed_gate");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck));

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
            "ohne aktiven Partner-State darf auch Targeted-Global nicht senden"
        );
        assert_eq!(api.message_count().await, 0);
    }

    #[tokio::test]
    async fn targeted_promo_suppression_guard_skippt_global_send() {
        let pool = pool_or_skip!("promo_targeted_guard_suppressed");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(FixedSuppression(true)));

        let sent = engine
            .maybe_send_targeted_promo(
                "targetkanal",
                "u-target",
                "https://discord.gg/deadlock",
                &[],
                Instant::now(),
            )
            .await;

        assert!(!sent);
        assert_eq!(
            api.announcement_count().await,
            0,
            "Suppression-Guard verhindert den Targeted-Global-Send"
        );
    }

    #[tokio::test]
    async fn targeted_promo_suppression_guard_allowed_sendet_global() {
        let pool = pool_or_skip!("promo_targeted_guard_allowed");
        let api = Arc::new(super::tests::MockApi::default());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(FixedSuppression(false)));

        let sent = engine
            .maybe_send_targeted_promo(
                "targetkanal",
                "u-target",
                "https://discord.gg/deadlock",
                &[],
                Instant::now(),
            )
            .await;

        assert!(sent);
        assert_eq!(
            api.announcement_count().await,
            1,
            "Allowed-Guard delegiert den Targeted-Global-Send"
        );
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
        let pool = pool_or_skip!("promo_lurker_paid_plan");
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
            engine.lurker_tax_is_paid_plan("paidkanal", "upaid").await,
            "aktiver raid_boost → chat.lurker_tax → is_paid_plan"
        );
        assert!(
            !engine.lurker_tax_is_paid_plan("expkanal", "uexp").await,
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
        assert!(
            !candidates.is_empty(),
            "Lurker-Kandidat sollte gefunden werden: {candidates:?}"
        );
        assert!(candidates.contains(&"lurker1".to_string()));
    }

    #[tokio::test]
    async fn lurker_tax_kandidaten_joinen_ueber_chatter_identity_key() {
        let pool = pool_or_skip!("promo_lurker_identity_key");
        let engine = make_engine(pool.clone());

        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, avg_viewers)
             VALUES ('renamekanal', 10.0)
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
                "INSERT INTO twitch_stream_sessions (streamer_login, ended_at, avg_viewers)
                 VALUES ('renamekanal', NOW() - ($1 || ' hours')::INTERVAL, 5.0)
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

        let candidates = engine.get_lurker_tax_candidates("renamekanal").await;
        assert_eq!(candidates, vec!["newlogin".to_string()]);
    }

    #[tokio::test]
    async fn lurker_tax_sendet_orange_announcement_ohne_plain_fallback() {
        let pool = pool_or_skip!("promo_lurker_announcement_drop");
        let api = Arc::new(super::tests::MockApi::announcement_dropped());
        let engine = PromoEngine::new(pool.clone(), api.clone(), Arc::new(NoopSuppressionCheck))
            .set_bot_scope_provider(Arc::new(super::tests::FakeBotScopes(vec![
                "moderator:read:chatters".into(),
            ])));

        let live_session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, avg_viewers)
             VALUES ('taxkanal', 10.0)
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
                "INSERT INTO twitch_stream_sessions (streamer_login, ended_at, avg_viewers)
                 VALUES ('taxkanal', NOW() - ($1 || ' hours')::INTERVAL, 5.0)
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
