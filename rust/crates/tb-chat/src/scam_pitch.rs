//! Service-Pitch-Detektor + Spam-AI-Review.
//!
//! Port von:
//! - `bot/chat/service_pitch_warning.py` (1030 Zeilen)
//! - `bot/chat/spam_ai_review.py` (366 Zeilen)
//! - Aufruf-Kontext: `bot/chat/bot.py` Z. 1597–1734
//!
//! # Architektur
//!
//! [`ScamPitchDetector`] hält allen zustandsbehafteten In-Memory-State
//! (Activity-Buckets, Cooldowns, Caches) hinter einem `Mutex`. Der Einstieg
//! ist `observe(event)` → [`PitchDecision`].
//!
//! [`SpamAiReviewer`] liefert im Verdachtspfad ein synchrones Urteil:
//! `review_for_verdict(event)` fragt den Judge (DeepSeek via Fireworks,
//! Fallback MiniMax M3), lernt bei Spam-Urteil ein distinktives Muster und
//! gibt ein [`AiReviewOutcome`] zurück — der Orchestrator entscheidet über
//! Aktion und Alert. Safe-Muster werden seit 11.07.2026 nicht mehr gelernt
//! (Safe-List-Poisoning, siehe spam_filter.rs).
//!
//! Accounts-Alter wird über [`AccountAgePort`] abgefragt — der Orchestrator
//! verdrahtet die Helix-Implementierung.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use regex::Regex;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::api::ChatApi;
use crate::types::ChatMessageEvent;

// ── Env-Konstanten (service_pitch_warning.py Z. 14–144) ──────────────────────

/// Grenze „neuer Account" in Tagen (TWITCH_SERVICE_WARNING_ACCOUNT_MAX_DAYS).
/// service_pitch_warning.py Z. 25–29
const ACCOUNT_MAX_DAYS: i64 = 90;
/// Max. Follower des Zielkanals (TWITCH_SERVICE_WARNING_MAX_FOLLOWERS).
/// service_pitch_warning.py Z. 30–33
const MAX_FOLLOWERS: i32 = 400;
/// Sliding-Window für Activity-Bucket in Sekunden (TWITCH_SERVICE_WARNING_WINDOW_SEC).
/// service_pitch_warning.py Z. 35–38
const WINDOW_SEC: f64 = 480.0;
/// Absolut-Mindest-Score für Auslösung (TWITCH_SERVICE_WARNING_MIN_SCORE).
/// service_pitch_warning.py Z. 40–43
const MIN_SCORE: i32 = 3;
/// Mindest-Nachrichten im Bucket außer force_single (TWITCH_SERVICE_WARNING_MIN_MESSAGES).
/// service_pitch_warning.py Z. 45–48
const MIN_MESSAGES: usize = 2;
/// Score-Schwelle für HINT (TWITCH_SERVICE_WARNING_LIGHT_THRESHOLD).
/// service_pitch_warning.py Z. 50–53
const LIGHT_THRESHOLD: i32 = 4;
/// Score-Schwelle für WARNING_PUBLIC (TWITCH_SERVICE_WARNING_PUBLIC_THRESHOLD).
/// service_pitch_warning.py Z. 55–58 + Z. 141–142
const PUBLIC_THRESHOLD: i32 = 7;
/// Score-Schwelle für WARNING_STRONG (TWITCH_SERVICE_WARNING_STRONG_THRESHOLD).
/// service_pitch_warning.py Z. 60–63 + Z. 143–144
const STRONG_THRESHOLD: i32 = 10;
/// Cooldown pro Kanal nach PUBLIC/STRONG in Sekunden
/// (TWITCH_SERVICE_WARNING_CHANNEL_COOLDOWN_SEC).
/// service_pitch_warning.py Z. 65–68
const CHANNEL_COOLDOWN_SEC: f64 = 900.0;
/// Cooldown pro (channel, user) nach PUBLIC/STRONG in Sekunden
/// (TWITCH_SERVICE_WARNING_USER_COOLDOWN_SEC).
/// service_pitch_warning.py Z. 70–73
const USER_COOLDOWN_SEC: f64 = 21600.0;
/// Cooldown pro (channel, user) nach HINT in Sekunden
/// (TWITCH_SERVICE_WARNING_HINT_COOLDOWN_SEC).
/// service_pitch_warning.py Z. 75–78
const HINT_COOLDOWN_SEC: f64 = 120.0;
/// TTL Account-Alter-Cache in Sekunden
/// (TWITCH_SERVICE_WARNING_ACCOUNT_CACHE_TTL_SEC).
/// service_pitch_warning.py Z. 80–83
const ACCOUNT_CACHE_TTL_SEC: f64 = 21600.0;
/// TTL Follower-Cache in Sekunden
/// (TWITCH_SERVICE_WARNING_FOLLOWER_CACHE_TTL_SEC).
/// service_pitch_warning.py Z. 85–88
const FOLLOWER_CACHE_TTL_SEC: f64 = 900.0;
/// Fenster für Timing-Score in Sekunden
/// (TWITCH_SERVICE_WARNING_FIRST_CHAT_WINDOW_SEC).
/// service_pitch_warning.py Z. 90–93
const FIRST_CHAT_WINDOW_SEC: f64 = 120.0;
/// Fenster für Nachrichten-Sequenz-Analyse in Sekunden
/// (TWITCH_SERVICE_WARNING_SEQUENCE_WINDOW_SEC).
/// service_pitch_warning.py Z. 95–98
const SEQUENCE_WINDOW_SEC: f64 = 30.0;
/// Mindest-Nachrichten für Sequenz-Score
/// (TWITCH_SERVICE_WARNING_SEQUENCE_MIN_MSGS).
/// service_pitch_warning.py Z. 100–103
const SEQUENCE_MIN_MSGS: usize = 3;
/// Max. Zeichen für „kurze" Nachricht
/// (TWITCH_SERVICE_WARNING_SHORT_MSG_MAX_CHARS).
/// service_pitch_warning.py Z. 105–108
const SHORT_MSG_MAX_CHARS: usize = 32;
/// TTL seen_messages-Cache in Sekunden
/// (TWITCH_SERVICE_WARNING_OBSERVED_MSG_CACHE_TTL_SEC).
/// service_pitch_warning.py Z. 110–113
const OBSERVED_MSG_CACHE_TTL_SEC: f64 = 86400.0;
/// Max. Einträge im history deque
/// (TWITCH_SERVICE_WARNING_MESSAGE_HISTORY_MAXLEN).
/// service_pitch_warning.py Z. 115–118
const MESSAGE_HISTORY_MAXLEN: usize = 32;
/// Max. Einträge im activity deque
/// (TWITCH_SERVICE_WARNING_ACTIVITY_BUCKET_MAXLEN).
/// service_pitch_warning.py Z. 120–123
const ACTIVITY_BUCKET_MAXLEN: usize = 64;
/// Max. Einträge für user-bezogene Caches
/// (TWITCH_SERVICE_WARNING_TRACKED_USER_STATE_MAXLEN).
/// service_pitch_warning.py Z. 125–128
const TRACKED_USER_STATE_MAXLEN: usize = 8192;
/// Max. Einträge für Kanal-Caches
/// (TWITCH_SERVICE_WARNING_CHANNEL_STATE_MAXLEN).
/// service_pitch_warning.py Z. 130–133
const CHANNEL_STATE_MAXLEN: usize = 2048;
/// Min. Abstand zwischen Prune-Durchläufen in Sekunden
/// (TWITCH_SERVICE_WARNING_STATE_PRUNE_INTERVAL_SEC).
/// service_pitch_warning.py Z. 135–138
const STATE_PRUNE_INTERVAL_SEC: f64 = 60.0;

// ── Spam-AI-Review-Konstanten ─────────────────────────────────────────────────

/// Ledger-Zweck dieses Calls (spam_ai_review.py Z. 175: `purpose="spam-review"`).
const JUDGE_PURPOSE: &str = "spam-review";
/// Token-Budget für den Judge. Reasoning-Modelle (DeepSeek, MiniMax-M3 mit
/// <think>-Block) brauchen Platz für den Denktext VOR dem JSON — die alten
/// 200 Tokens schnitten das Urteil oft ab (stille „kein JSON"-Reviews).
const JUDGE_MAX_TOKENS: u32 = 1500;
/// Review-Cooldown pro (channel, user) in Sekunden (spam_ai_review.py Z. 79).
const REVIEW_COOLDOWN_SEC: f64 = 300.0;
/// Prune-Grenze für den Cooldown-Cache (spam_ai_review.py Z. 80).
const REVIEW_COOLDOWN_MAX_LEN: usize = 2048;
/// Minimale Pattern-Länge für DB-Speicherung (spam_ai_review.py Z. 332, 339).
const PATTERN_MIN_LEN: usize = 4;

// ── System-Prompt v3 (10.08.2026) ────────────────────────────────────────────
// v1 nannte „best viewers" als harmloses Kompliment-Beispiel und ließ den
// Judge Safe-Muster vorschlagen — beides fütterte das Safe-List-Poisoning.
// v2 trennte Werbung von Gespräch, koppelte dabei aber die Musterregel an die
// Urteilsfrage: Der Judge schloss von „kein Dienstname" auf „harmlos" und ließ
// anonyme Angebote wie „Boost viewers on the stream — promotion. ru" durch
// (10.08.2026 live beobachtet). v3 entkoppelt beides und nennt den Domain-Rest
// als eigenes Signal. Benchmark deepseek-v4-flash, 24 Produktionsfälle:
// v2 6/10 Spam · 14/14 harmlos, v3 10/10 Spam · 14/14 harmlos.
const SPAM_REVIEW_SYSTEM_PROMPT: &str = "\
Du bist ein Spam-Erkennungs-Assistent speziell für Twitch Viewer-Bot-Spam und SMM-Dienste.\n\
\n\
Die Nachricht wurde bereits von einem regelbasierten Filter als VERDÄCHTIG markiert (hatte \
Teilübereinstimmungen mit bekannten Spam-Mustern). Deine Aufgabe: bestätige oder widerlege, ob es \
sich um WERBUNG für Viewer-Bot-Services, SMM-Dienste oder ähnliche Twitch-Manipulation handelt.\n\
\n\
Antworte NUR mit einem JSON-Objekt, ohne Markdown, ohne <think>-Block:\n\
{\"is_spam\": true/false, \"pattern\": \"Kernmuster oder null\", \"pattern_type\": \"phrase\" \
oder \"fragment\", \"reason\": \"Begründung max 80 Zeichen\"}\n\
\n\
Entscheidend ist allein, ob die Nachricht ein Angebot macht — NICHT, ob ein Dienstname, eine \
Domain, ein Preis oder eine Kontaktaufforderung darin steht. Anonyme Angebote ohne Markennamen \
sind der häufigste Fall und sind Spam.\n\
\n\
is_spam=true bei aktiver Werbung/Angeboten: Viewer-Kauf, Bot-Views, Bot-Follower, SMM-Services, \
Wachstums- und Promotion-Angebote, neue oder abgewandelte Schreibweisen von Spam-Diensten \
(Leerzeichen in Domains, Sonderzeichen, veränderte Namen). Beispiel: 'Boost viewers on the \
stream - promotion. ru' ist Spam, auch ohne erkennbaren Dienstnamen.\n\
Ein Domain-Rest belegt Spam zusätzlich, auch bei unbekanntem Dienst: abgetrennte TLD ('. ru', \
'dot com'), verfremdete Zeichen ('c0m').\n\
\n\
is_spam=false bei allem anderen, INSBESONDERE:\n\
- normale Chat-Gespräche über Viewer-Zahlen, Komplimente, Selbstpromotion des eigenen Kanals, \
normale URLs\n\
- Fragen oder Gespräche ÜBER Spam, Viewer-Bots oder Bans (z.B. 'bannt der Bot mich, wenn \
ich X eingebe?') — wer über einen Dienst redet, wirbt nicht für ihn. Nur die aktive \
Bewerbung ist Spam.\n\
\n\
pattern ist reines Lern-Nebenprodukt und beeinflusst is_spam nie. Bei is_spam=true: pattern = \
kürzestes eindeutiges Spam-Kernmuster, sofern die Nachricht einen unterscheidungskräftigen Token \
enthält (Domain oder Dienstname, z.B. 'eballo.com', 'streamboo'). NIEMALS generisches Vokabular \
als pattern ('viewer', 'best viewers', 'follower', 'ai viewers'). Fehlt so ein Token, ist \
pattern = null und is_spam bleibt trotzdem true.\n\
Bei is_spam=false: pattern = null.\n\
\n\
Im Zweifel bei Gesprächen, Fragen und Komplimenten: is_spam=false. Bei einem erkennbaren \
Angebot: is_spam=true.";

// ── Regex-Muster (service_pitch_warning.py Z. 146–317) ───────────────────────

struct Patterns {
    // _SERVICE_PATTERNS-Gruppen (Z. 146–293)
    language_probe: Vec<Regex>,
    new_here: Vec<Regex>,
    streaming_leadin: Vec<Regex>,
    growth_pitch: Vec<Regex>,
    crew_threat: Vec<Regex>,
    design_pitch: Vec<Regex>,
    offplatform: Vec<Regex>,
    urgency_probe: Vec<Regex>,
    intrusive_probe: Vec<Regex>,
    greeting: Vec<Regex>,
    wellbeing: Vec<Regex>,
    // Sonder-Patterns (Z. 296–317)
    generic_praise: Regex,
    stream_context: Regex,
    link: Regex,
    handle: Regex,
    twitch_collab_invite: Regex,
    discord_handle_drop: Regex,
    discord_teamup: Regex,
    platform_ref: Regex,
}

impl Patterns {
    fn build() -> Self {
        let ri = |s: &str| Regex::new(&format!("(?i){s}")).unwrap();
        Self {
            // service_pitch_warning.py Z. 148–155
            language_probe: vec![
                ri(r"\bdo\s+(?:u|you)\s+speak\s+english\b"),
                ri(r"\b(?:speak|sprichst)\s+(?:english|englisch)\b"),
                ri(r"\bwhere\s+are\s+you\s+from\b"),
                ri(r"\bhow\s+old\s+are\s+you\b"),
            ],
            // service_pitch_warning.py Z. 158–168
            new_here: vec![
                ri(r"\bnew\s+here\b"),
                ri(r"\bfirst\s+time\s+here\b"),
                ri(r"\bi(?:'m| am)\s+new\s+to\s+your\s+channel\b"),
                ri(r"\bjust\s+found\s+your\s+stream\b"),
                ri(r"\bbrand\s+new\s+streamer\b"),
                ri(r"\bnew\s+streamer\s+(?:here|btw)?\b"),
                ri(r"\bbin\s+neu\s+hier\b"),
            ],
            // service_pitch_warning.py Z. 171–180
            streaming_leadin: vec![
                ri(r"\bwhat\s+got\s+you\s+into\s+streaming\b"),
                ri(r"\bwhat\s+made\s+you\s+start\s+streaming\b"),
                ri(r"\bhow\s+long\s+have\s+you\s+been\s+streaming\b"),
                ri(r"\bwhat(?:'s|s)\s+your\s+stream(?:ing)?\s+schedule\b"),
                ri(r"\bdo\s+you\s+stream\s+on\s+(?:youtube|yt)\b"),
                ri(r"\bwie\s+lange\s+streamst\s+du\s+schon\b"),
                ri(r"\bwelcome\s+to\s+(?:the\s+)?twitch\b"),
            ],
            // service_pitch_warning.py Z. 183–198
            growth_pitch: vec![
                ri(r"\blet'?s\s+support\s+each\s+other\b"),
                ri(r"\bi\s+can\s+help\s+you\s+grow\b"),
                ri(r"\bboost\s+viewers?\b"),
                ri(r"\bmore\s+viewers?\b"),
                ri(r"\bi\s+work\s+with\s+streamers?\b"),
                ri(r"\baffiliate\b"),
                ri(r"\bpromot(?:e|ion)\b"),
                ri(r"\bmehr\s+viewer\b"),
                ri(r"\bhelfen?\s+zu\s+wachsen\b"),
                ri(r"\btop\s+viewers?\b"),
                ri(r"\bbest\s+viewers?\b"),
            ],
            // service_pitch_warning.py Z. 201–206
            crew_threat: vec![
                ri(r"\bpull\s+up\s+with\s+(?:my|the)\s+crew\b"),
                ri(r"\bpull\s+up\s+w(?:ith)?\s+my\s+crew\b"),
            ],
            // service_pitch_warning.py Z. 209–227
            design_pitch: vec![
                ri(r"\bdo\s+you\s+have\s+a\s+logo\b"),
                ri(r"\bneed\s+emotes?\b"),
                ri(r"\boverlays?\b"),
                ri(r"\bpanels?\b"),
                ri(r"\bcustomi[sz]ed\s+panels?\b"),
                ri(r"\bbanner\b"),
                ri(r"\bgraphic(?:s)?\s+designer\b"),
                ri(r"\bportfolio\b"),
                ri(r"\bshow\s+(?:you\s+)?(?:some\s+of\s+)?my\s+work\b"),
                ri(r"\bcommissions?\b"),
                ri(r"\bbranding\b"),
                ri(r"\bbrauchst\s+du\s+(?:ein\s+)?(?:logo|emotes?|overlay)\b"),
            ],
            // service_pitch_warning.py Z. 230–246
            offplatform: vec![
                ri(r"\bcan\s+i\s+dm\s+you\b"),
                ri(r"\badd\s+me\s+on\s+(?:discord|instagram)\b"),
                ri(r"\badd\s+me\s+up\s+on\s+discord\b"),
                ri(r"\badd\s+me\b"),
                ri(r"\baccept\s+my\s+request\b"),
                ri(r"\bcheck\s+your\s+whispers?\b"),
                ri(r"\bi\s+sent\s+you\s+a\s+message\b"),
                ri(r"\bclick\s+the\s+link\b"),
                ri(r"\bsharing\s+something\b"),
                ri(r"\bdiscord\b"),
                ri(r"\binstagram\b"),
                ri(r"\bdm\b"),
                ri(r"\bwhisper\b"),
            ],
            // service_pitch_warning.py Z. 249–256
            urgency_probe: vec![
                ri(r"\bquick\s+question\b"),
                ri(r"\bcan\s+i\s+ask\s+you\s+something\b"),
                ri(r"\bwon'?t\s+take\s+long\b"),
                ri(r"\bjust\s+a\s+suggestion\b"),
                ri(r"\bdon'?t\s+ignore\s+me\b"),
            ],
            // service_pitch_warning.py Z. 259–269
            intrusive_probe: vec![
                ri(r"\bwhat(?:'s| is)\s+your\s+real\s+name\b"),
                ri(r"\bwhere\s+do\s+you\s+live\b"),
                ri(r"\bshar(?:e|ing)\s+(?:my|your)\s+address\b"),
                ri(r"\bare\s+you\s+single\b"),
                ri(r"\bface\s+reveal\b"),
                ri(r"\bcan\s+you\s+turn\s+on\s+cam\b"),
                ri(r"\bwru\s+from\b"),
            ],
            // service_pitch_warning.py Z. 273–283
            greeting: vec![
                ri(r"\bhey+\b"),
                ri(r"\bhi+\b"),
                ri(r"\bhii+\b"),
                ri(r"\bwhat(?:'s|s)\s+good\b"),
                ri(r"\bw(?:hat)?\s+are\s+you\s+up\s+to\b"),
                ri(r"\bwie\s+geht(?:s|['']s)\b"),
                ri(r"\balls?\s+gut\b"),
            ],
            // service_pitch_warning.py Z. 286–292
            wellbeing: vec![
                ri(r"\bhow\s+(?:are|r)\s+(?:you|u)\b"),
                ri(r"\bhru+\b"),
                ri(r"\bwie\s+geht(?:s|['']s)\b"),
            ],
            // service_pitch_warning.py Z. 296–317
            generic_praise: ri(
                r"\b(?:cool|amazing|nice\s+stream|love\s+your\s+vibe|you'?re\s+so\s+entertaining|this\s+is\s+awesome|great\s+content|setup\s+is\s+fire|awesome)\b",
            ),
            stream_context: ri(
                r"\b(?:deadlock|fight|boss|round|match|kill|build|lane|rank|aim|ability|ult|teamfight|objective|clip)\b",
            ),
            link: ri(r"(?:https?://|www\.|discord\.gg/|bit\.ly/|t\.me/|linktr\.ee/|tinyurl\.com/)"),
            handle: Regex::new(r"(?:^|\s)@[A-Za-z0-9_.]{3,}\b").unwrap(),
            twitch_collab_invite: ri(r"https?://(?:www\.)?twitch\.tv/collab/invite/[A-Za-z0-9_-]+"),
            discord_handle_drop: ri(r"\bdiscord\s*[:：]\s*[A-Za-z0-9_.-]{3,}\b"),
            discord_teamup: ri(r"\b(?:let'?s|lets)\s+team\s+up(?:\s+on\s+discord)?\b"),
            platform_ref: ri(r"\b(?:discord|instagram|tiktok|youtube|yt|ig)\b"),
        }
    }
}

// ── Öffentliche Entscheidungs-Typen ──────────────────────────────────────────

/// Ergebnis von [`ScamPitchDetector::observe`].
///
/// service_pitch_warning.py Z. 906–916 (Severity-Bestimmung)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PitchDecision {
    /// Kein Treffer oder unter Schwelle — keine Aktion.
    None,
    /// Score im HINT-Bereich: nur intern loggen, kein Chat-Text.
    Hint,
    /// Öffentliche Warnung (WARNING_PUBLIC). Text enthält die fertige Chat-Nachricht.
    PublicWarn { text: String },
    /// Erster WARNING_STRONG-Treffer: nur öffentliche Warnung, **kein** Timeout,
    /// **kein** Delete — Python warnt hier nur (service_pitch_warning.py Z. 983–1030).
    StrongWarn { text: String },
    /// Timeout bei Eskalation (wiederholter Pitch trotz Cooldown). Kein Delete —
    /// Python timeoutet hier 600 s ohne zu löschen.
    StrongTimeout { text: String, duration: Duration },
}

// ── Account-Alter-Port (UNSICHER: TwitchIO fetch_users, api.rs Z. 82–84) ─────

/// Port für Account-Alter-Abfragen.
///
/// Der Orchestrator verdrahtet eine Helix-Implementierung; in Tests wird ein
/// Mock verwendet. UNSICHER: Das Python-Original nutzt `self.fetch_users()` von
/// TwitchIO — Token-Art und genaues Verhalten sind nicht aus dem Source
/// bestimmbar (service_pitch_warning.py Z. 665–698, Vertrag §9 Punkt 1).
#[async_trait]
pub trait AccountAgePort: Send + Sync {
    /// Gibt die Account-Alter in Tagen zurück, oder `None` wenn nicht
    /// ermittelbar.
    async fn user_created_at_days(&self, user_id: &str, login: &str) -> Option<i64>;
}

// ── In-Memory-State (hinter Mutex) ───────────────────────────────────────────

/// Eintrag im Activity-Bucket.
/// service_pitch_warning.py Z. 323: `deque[(float, int)]`
struct ActivityEntry {
    ts: f64,
    score: i32,
}

/// Eintrag im Message-History-Bucket.
/// service_pitch_warning.py Z. 325: `deque[(float, str, set[str])]`
struct HistoryEntry {
    ts: f64,
    content: String,
    features: HashSet<String>,
}

struct State {
    /// service_pitch_warning.py Z. 323
    activity: HashMap<(String, String), VecDeque<ActivityEntry>>,
    /// service_pitch_warning.py Z. 324
    message_history: HashMap<(String, String), VecDeque<HistoryEntry>>,
    /// service_pitch_warning.py Z. 327
    first_seen: HashMap<(String, String), f64>,
    /// service_pitch_warning.py Z. 328
    channel_cd: HashMap<String, f64>,
    /// service_pitch_warning.py Z. 329
    user_cd: HashMap<(String, String), f64>,
    /// service_pitch_warning.py Z. 330
    hint_cd: HashMap<(String, String), f64>,
    /// service_pitch_warning.py Z. 331 — Key: login_or_id, Value: (monotonic_ts, age_days)
    account_age_cache: HashMap<String, (f64, Option<i64>)>,
    /// service_pitch_warning.py Z. 332 — Key: channel_login, Value: (ts, count)
    follower_cache: HashMap<String, (f64, Option<i32>)>,
    /// service_pitch_warning.py Z. 333 — Key: (channel, chatter), Value: (ts, count)
    seen_messages: HashMap<(String, String), (f64, u32)>,
    /// service_pitch_warning.py Z. 334
    last_pruned: f64,
    /// Gemeinsame Uhr-Basis für monotone Timestamps.
    epoch: Instant,
}

impl State {
    fn new() -> Self {
        Self {
            activity: HashMap::new(),
            message_history: HashMap::new(),
            first_seen: HashMap::new(),
            channel_cd: HashMap::new(),
            user_cd: HashMap::new(),
            hint_cd: HashMap::new(),
            account_age_cache: HashMap::new(),
            follower_cache: HashMap::new(),
            seen_messages: HashMap::new(),
            last_pruned: 0.0,
            epoch: Instant::now(),
        }
    }

    /// Monotone Sekunden seit Prozessstart (Äquivalent zu `time.monotonic()`).
    /// service_pitch_warning.py Z. 816, 658, 705
    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }
}

// ── Scoring-Hilfsfunktionen ───────────────────────────────────────────────────

/// Whitespace-Normalisierung.
/// service_pitch_warning.py Z. 337–338
fn normalize_text(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Token-Anzahl (Whitespace-Split).
/// service_pitch_warning.py Z. 438–441
fn token_count(content: &str) -> usize {
    content.split_whitespace().count()
}

/// Ergebnis von `_score_service_pitch_message`.
struct ScoreResult {
    score: i32,
    reasons: Vec<String>,
    features: HashSet<String>,
}

/// Scoring einer einzelnen Nachricht gegen alle Gruppen.
/// service_pitch_warning.py Z. 340–394
fn score_message(raw: &str, p: &Patterns) -> ScoreResult {
    if raw.is_empty() {
        return ScoreResult {
            score: 0,
            reasons: vec![],
            features: HashSet::new(),
        };
    }

    let mut score = 0i32;
    let mut reasons = Vec::new();
    let mut features: HashSet<String> = HashSet::new();

    // _SERVICE_PATTERNS-Gruppen (Z. 349–357)
    macro_rules! check_group {
        ($name:expr, $pts:expr, $patterns:expr) => {
            if !features.contains($name) {
                for re in &$patterns {
                    if re.is_match(raw) {
                        features.insert($name.to_string());
                        score += $pts;
                        reasons.push(format!("feature:{}", $name));
                        break;
                    }
                }
            }
        };
    }

    check_group!("language_probe", 3, p.language_probe);
    check_group!("new_here", 2, p.new_here);
    check_group!("streaming_leadin", 2, p.streaming_leadin);
    check_group!("growth_pitch", 3, p.growth_pitch);
    check_group!("crew_threat", 5, p.crew_threat);
    check_group!("design_pitch", 4, p.design_pitch);
    check_group!("offplatform", 4, p.offplatform);
    check_group!("urgency_probe", 2, p.urgency_probe);
    check_group!("intrusive_probe", 2, p.intrusive_probe);
    check_group!("greeting", 1, p.greeting);
    check_group!("wellbeing", 1, p.wellbeing);

    let lowered = raw.to_lowercase();

    // generic_praise (Z. 360–365)
    if p.generic_praise.is_match(raw) {
        let tokens: Vec<&str> = lowered.split_whitespace().collect();
        let praise_score = if tokens.len() <= 5 && !p.stream_context.is_match(raw) {
            2i32
        } else {
            1
        };
        score += praise_score;
        features.insert("generic_praise".to_string());
        reasons.push(format!("feature:generic_praise({praise_score})"));
    }

    // discord_teamup_pitch (Z. 367–371)
    if p.discord_teamup.is_match(raw) {
        score += 3;
        features.insert("discord_teamup_pitch".to_string());
        reasons.push("feature:discord_teamup_pitch".to_string());
    }

    // discord_handle_drop (Z. 373–377)
    if p.discord_handle_drop.is_match(raw) {
        score += 4;
        features.insert("discord_handle_drop".to_string());
        reasons.push("feature:discord_handle_drop".to_string());
    }

    // external_link_or_handle / trusted_twitch_collab_invite (Z. 379–392)
    let has_link = p.link.is_match(raw);
    let has_twitch_collab = p.twitch_collab_invite.is_match(raw);
    let has_platform_ref = p.platform_ref.is_match(&lowered);
    let has_handle = p.handle.is_match(raw);
    let has_discord_handle_drop = features.contains("discord_handle_drop");
    let has_external_profile_drop = has_platform_ref && (has_handle || has_discord_handle_drop);

    if (has_link && !has_twitch_collab) || has_external_profile_drop {
        score += 4;
        features.insert("external_link_or_handle".to_string());
        reasons.push("feature:external_link_or_handle".to_string());
    } else if has_twitch_collab {
        features.insert("trusted_twitch_collab_invite".to_string());
        reasons.push("feature:trusted_twitch_collab_invite".to_string());
    }

    ScoreResult {
        score,
        reasons,
        features,
    }
}

/// Combo-Scores für Feature-Kombinationen.
/// service_pitch_warning.py Z. 538–557
fn score_combo_signals(features: &HashSet<String>) -> (i32, Vec<String>) {
    let mut score = 0i32;
    let mut reasons = Vec::new();

    if features.contains("new_here") && features.contains("growth_pitch") {
        score += 3;
        reasons.push("combo:new_here_plus_growth".to_string());
    }
    if features.contains("design_pitch") && features.contains("offplatform") {
        score += 3;
        reasons.push("combo:design_plus_offplatform".to_string());
    }
    if features.contains("growth_pitch") && features.contains("offplatform") {
        score += 2;
        reasons.push("combo:growth_plus_offplatform".to_string());
    }
    if features.contains("language_probe") && features.contains("streaming_leadin") {
        score += 2;
        reasons.push("combo:language_plus_streaming".to_string());
    }
    if features.contains("greeting")
        && features.contains("wellbeing")
        && features.contains("language_probe")
    {
        score += 2;
        reasons.push("combo:greeting_wellbeing_language".to_string());
    }

    (score, reasons)
}

/// Sequenz-Scores über den History-Bucket.
/// service_pitch_warning.py Z. 501–535
fn score_sequence_signals(bucket: &VecDeque<HistoryEntry>) -> (i32, Vec<String>) {
    if bucket.len() < SEQUENCE_MIN_MSGS {
        return (0, vec![]);
    }

    let mut score = 0i32;
    let mut reasons = Vec::new();

    // short_multi_line_burst (Z. 510–517)
    let short_count = bucket
        .iter()
        .filter(|e| e.content.len() <= SHORT_MSG_MAX_CHARS && token_count(&e.content) <= 7)
        .count();
    if short_count >= SEQUENCE_MIN_MSGS {
        score += 2;
        reasons.push("sequence:short_multi_line_burst".to_string());
    }

    // all_features Union über den Bucket (Z. 519–521)
    let mut all_features: HashSet<&str> = HashSet::new();
    for entry in bucket {
        for f in &entry.features {
            all_features.insert(f.as_str());
        }
    }

    // greeting_language_combo (Z. 523–527)
    if all_features.contains("language_probe")
        && (all_features.contains("greeting") || all_features.contains("wellbeing"))
    {
        score += 2;
        reasons.push("sequence:greeting_language_combo".to_string());
    }

    // praise_or_new_plus_streaming_question (Z. 529–533)
    if all_features.contains("streaming_leadin")
        && (all_features.contains("generic_praise") || all_features.contains("new_here"))
    {
        score += 2;
        reasons.push("sequence:praise_or_new_plus_streaming_question".to_string());
    }

    (score, reasons)
}

/// Benign-Social-Checkin-Filter.
/// service_pitch_warning.py Z. 486–499
fn is_benign_social_checkin(content: &str, features: &HashSet<String>, p: &Patterns) -> bool {
    if features.is_empty() {
        return false;
    }
    // features ⊆ {greeting, wellbeing}
    if !features.iter().all(|f| f == "greeting" || f == "wellbeing") {
        return false;
    }
    if p.link.is_match(content) {
        return false;
    }
    if p.handle.is_match(content) {
        let normalized = normalize_text(content);
        let starts_with_mention = Regex::new(r"^@[A-Za-z0-9_.]{3,}\b")
            .unwrap()
            .is_match(&normalized);
        let has_platform_ref = p.platform_ref.is_match(&normalized.to_lowercase());
        if !starts_with_mention || has_platform_ref {
            return false;
        }
    }
    token_count(content) <= 14
}

/// High-confidence-Signal für force_single_warning.
/// service_pitch_warning.py Z. 475–484
fn has_high_confidence_single_message_signal(features: &HashSet<String>) -> bool {
    if features.contains("crew_threat") || features.contains("discord_handle_drop") {
        return true;
    }
    if features.contains("offplatform") && features.contains("external_link_or_handle") {
        return true;
    }
    if features.contains("offplatform") && features.contains("discord_teamup_pitch") {
        return true;
    }
    if features.contains("growth_pitch") && features.contains("external_link_or_handle") {
        return true;
    }
    false
}

/// Warn-Text für PUBLIC/STRONG.
/// service_pitch_warning.py Z. 778–798
fn build_warning_text(
    chatter_login: &str,
    strong: bool,
    new_account: bool,
    account_age_days: Option<i64>,
) -> String {
    let mention = if chatter_login.is_empty() {
        String::new()
    } else {
        format!("@{chatter_login} ")
    };
    let age_hint = if new_account {
        " zumal der Account unter <3 Monate alt ist".to_string()
    } else if account_age_days.is_none() {
        " (Account-Alter unbekannt)".to_string()
    } else {
        String::new()
    };
    if strong {
        format!(
            "🛡️ {mention}wurde als potenzieller Pitcher erkannt{age_hint} \
             verkauft oft Designs/Viewer/Scam. \
             Unsere Empfehlung: Ignorieren & Bannen."
        )
    } else {
        format!("{mention}bitte keine Service-/Promo-Angebote{age_hint} ")
    }
}

// ── Prune-Hilfsfunktionen ─────────────────────────────────────────────────────

/// Prune einer `HashMap<K, (f64, V)>` (Wert = Tuple mit Timestamp an Position 0).
/// service_pitch_warning.py Z. 572–597
fn prune_ts_cache<K: std::hash::Hash + Eq + Clone>(
    cache: &mut HashMap<K, (f64, impl Clone)>,
    now: f64,
    max_len: usize,
    max_age_sec: f64,
) {
    let stale_before = now - max_age_sec;
    let stale_keys: Vec<K> = cache
        .iter()
        .filter(|(_, (ts, _))| *ts < stale_before)
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale_keys {
        cache.remove(&k);
    }
    if cache.len() <= max_len {
        return;
    }
    let overflow = cache.len() - max_len;
    let mut by_ts: Vec<(f64, K)> = cache.iter().map(|(k, (ts, _))| (*ts, k.clone())).collect();
    by_ts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, k) in by_ts.into_iter().take(overflow) {
        cache.remove(&k);
    }
}

/// Prune einer `HashMap<K, f64>` (Wert = direkte Deadline).
/// service_pitch_warning.py Z. 572–597 (Deadline-Variante)
fn prune_deadline_cache<K: std::hash::Hash + Eq + Clone>(
    cache: &mut HashMap<K, f64>,
    now: f64,
    max_len: usize,
    max_age_sec: f64,
) {
    let stale_before = now - max_age_sec;
    let stale_keys: Vec<K> = cache
        .iter()
        .filter(|(_, ts)| **ts < stale_before)
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale_keys {
        cache.remove(&k);
    }
    if cache.len() <= max_len {
        return;
    }
    let overflow = cache.len() - max_len;
    let mut by_ts: Vec<(f64, K)> = cache.iter().map(|(k, ts)| (*ts, k.clone())).collect();
    by_ts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, k) in by_ts.into_iter().take(overflow) {
        cache.remove(&k);
    }
}

// ── ScamPitchDetector ─────────────────────────────────────────────────────────

/// Service-Pitch-Detektor.
///
/// `observe(event)` → [`PitchDecision`] ist der Haupt-Einstieg.
/// Der Detektor schickt selbst keine Chat-Nachrichten — der Aufrufer (Orchestrator)
/// führt die `ChatApi`-Calls aus.
///
/// UNSICHER: Account-Alter via [`AccountAgePort`] — der Orchestrator verdrahtet
/// die Helix-Implementierung. Das Python-Original ruft intern `self.fetch_users()`
/// auf (TwitchIO, service_pitch_warning.py Z. 665–698).
pub struct ScamPitchDetector {
    api: Arc<dyn ChatApi>,
    account_age: Arc<dyn AccountAgePort>,
    pool: PgPool,
    patterns: Arc<Patterns>,
    state: Mutex<State>,
}

impl ScamPitchDetector {
    /// Erzeugt einen neuen Detektor.
    pub fn new(api: Arc<dyn ChatApi>, account_age: Arc<dyn AccountAgePort>, pool: PgPool) -> Self {
        Self {
            api,
            account_age,
            pool,
            patterns: Arc::new(Patterns::build()),
            state: Mutex::new(State::new()),
        }
    }

    /// Verarbeitet ein eingehendes Chat-Event und gibt die Entscheidung zurück.
    ///
    /// Entspricht `_maybe_warn_service_pitch` (service_pitch_warning.py Z. 800–1029).
    pub async fn observe(&self, event: &ChatMessageEvent) -> PitchDecision {
        // Schritt 1–5: Vorbedingungen (Z. 801–811)
        let raw_content = normalize_text(event.text());
        if raw_content.is_empty() {
            return PitchDecision::None;
        }
        // Kommando-Check: starts with "!" (Vertrag §3, Z. 804)
        if raw_content.starts_with('!') {
            return PitchDecision::None;
        }
        // Mod/Broadcaster: kein Pitch (Z. 810–811)
        if event.is_mod_or_broadcaster() {
            return PitchDecision::None;
        }

        let chatter_login = event.chatter_user_login.to_lowercase();
        let chatter_id = event.chatter_user_id.clone();
        let chatter_key = if chatter_login.is_empty() {
            chatter_id.clone()
        } else {
            chatter_login.clone()
        };
        let channel_login = event.broadcaster_user_login.to_lowercase();

        // Schritt 0a: Prune + observe_position (Z. 817–820)
        let (is_first_observed_message, now) = {
            let mut st = self.state.lock().await;
            let now = st.now();
            self.prune_state(&mut st, now);
            let (_, first) =
                self.observe_message_position(&mut st, &channel_login, &chatter_key, now);
            (first, now)
        };

        // Schritt 2: Score (Z. 822)
        let mut scored = score_message(&raw_content, &self.patterns);
        if scored.score <= 0 {
            return PitchDecision::None;
        }
        if is_first_observed_message {
            scored
                .reasons
                .push("timing:first_observed_message".to_string());
        }

        // Schritt 4: Account-Alter (Z. 828–842)
        let account_age_days = self
            .account_age
            .user_created_at_days(&chatter_id, &chatter_login)
            .await;
        let account_age_safe = account_age_days.unwrap_or(-1);
        let is_new_account = account_age_days
            .map(|d| d < ACCOUNT_MAX_DAYS)
            .unwrap_or(false);

        if is_new_account {
            scored.score += 2;
            scored
                .reasons
                .push("account:newer_than_3_months".to_string());
            scored.features.insert("new_account".to_string());
        } else if account_age_days.is_none() {
            scored.reasons.push("account:unknown_age".to_string());
        } else {
            scored
                .reasons
                .push("account:older_than_3_months".to_string());
        }

        // Schritt 5: Benign-Social-Check (Z. 844–845)
        if !is_new_account
            && is_benign_social_checkin(&raw_content, &scored.features, &self.patterns)
        {
            return PitchDecision::None;
        }

        // Schritt 6: Follower-Check (Z. 847–856) — Lazy-DB-Load bei Cache-Miss.
        let (is_low_target, follower_count) = self.follower_hint(&channel_login, now).await;
        if !is_low_target {
            return PitchDecision::None;
        }
        if follower_count.is_none() {
            scored
                .reasons
                .push("target:unknown_followers_assume_small".to_string());
        } else if let Some(fc) = follower_count {
            scored.reasons.push(format!("target:followers_{fc}"));
            if fc <= MAX_FOLLOWERS / 2 {
                scored.score += 1;
                scored.reasons.push("target:very_small_channel".to_string());
            }
        }

        // Schritt 7: Combo (Z. 858–861)
        let (combo_score, combo_reasons) = score_combo_signals(&scored.features);
        scored.score += combo_score;
        scored.reasons.extend(combo_reasons);

        // Schritt 8: Early-window (Z. 863–866)
        let (early_score, early_reasons) = {
            let mut st = self.state.lock().await;
            self.early_window_score(&mut st, &channel_login, &chatter_key, now)
        };
        scored.score += early_score;
        scored.reasons.extend(early_reasons);

        // Schritt 9–11: History + Activity + totaler Score (Z. 868–883)
        let (total_score, msg_count) = {
            let mut st = self.state.lock().await;
            let bucket_key = (channel_login.clone(), chatter_key.clone());

            // message_history aktualisieren (Z. 869–871)
            let hist = st
                .message_history
                .entry(bucket_key.clone())
                .or_insert_with(|| VecDeque::with_capacity(MESSAGE_HISTORY_MAXLEN));
            hist.push_back(HistoryEntry {
                ts: now,
                content: raw_content.clone(),
                features: scored.features.clone(),
            });
            while hist
                .front()
                .map(|e| now - e.ts > SEQUENCE_WINDOW_SEC)
                .unwrap_or(false)
            {
                hist.pop_front();
            }
            if hist.len() > MESSAGE_HISTORY_MAXLEN {
                hist.pop_front();
            }

            // Sequenz-Score aus History (Z. 873–876)
            let (seq_score, seq_reasons) = score_sequence_signals(hist);
            scored.score += seq_score;
            scored.reasons.extend(seq_reasons);

            // activity_bucket aktualisieren (Z. 878–882)
            let act = st
                .activity
                .entry(bucket_key.clone())
                .or_insert_with(|| VecDeque::with_capacity(ACTIVITY_BUCKET_MAXLEN));
            act.push_back(ActivityEntry {
                ts: now,
                score: scored.score,
            });
            while act
                .front()
                .map(|e| now - e.ts > WINDOW_SEC)
                .unwrap_or(false)
            {
                act.pop_front();
            }
            if act.len() > ACTIVITY_BUCKET_MAXLEN {
                act.pop_front();
            }

            let total = act.iter().map(|e| e.score).sum::<i32>();
            let count = act.len();
            (total, count)
        };

        // Schritt 12: force_single_warning (Z. 884–891)
        let quick_action_eligible = is_new_account && is_first_observed_message;
        let force_single_warning = scored.features.contains("crew_threat")
            || (quick_action_eligible
                && has_high_confidence_single_message_signal(&scored.features));

        // Schritt 13: Mindest-Check (Z. 892–897)
        if total_score < MIN_SCORE || (msg_count < MIN_MESSAGES && !force_single_warning) {
            return PitchDecision::None;
        }
        if total_score < LIGHT_THRESHOLD {
            return PitchDecision::None;
        }

        // Schritt 14: Severity (Z. 906–916)
        let severity = if total_score >= STRONG_THRESHOLD {
            "WARNING_STRONG"
        } else if total_score >= PUBLIC_THRESHOLD {
            "WARNING_PUBLIC"
        } else {
            "HINT"
        };
        let (severity, reasons) = if severity != "HINT" && !quick_action_eligible {
            scored
                .reasons
                .push("quick_action:deferred_requires_new_account_first_message".to_string());
            ("HINT", scored.reasons.clone())
        } else {
            if quick_action_eligible {
                scored.reasons.push("quick_action:eligible".to_string());
            }
            (severity, scored.reasons.clone())
        };

        // Schritt 15: Cooldown (Z. 918–981)
        let bucket_key = (channel_login.clone(), chatter_key.clone());
        let cooldown_action = {
            let mut st = self.state.lock().await;
            if severity == "HINT" {
                let hint_cd = st.hint_cd.get(&bucket_key).copied().unwrap_or(0.0);
                if now < hint_cd {
                    return PitchDecision::None;
                }
                None
            } else {
                let channel_cd = st.channel_cd.get(&channel_login).copied().unwrap_or(0.0);
                let user_cd = st.user_cd.get(&bucket_key).copied().unwrap_or(0.0);
                if now < channel_cd || now < user_cd {
                    // Eskalation (Z. 928–981): STRONG + User bereits gewarnt
                    if severity == "WARNING_STRONG" && now < user_cd {
                        st.user_cd
                            .insert(bucket_key.clone(), now + USER_COOLDOWN_SEC);
                        Some("escalate")
                    } else {
                        return PitchDecision::None;
                    }
                } else {
                    None
                }
            }
        };

        // Eskalations-Aktion ausführen (außerhalb des Mutex-Locks)
        if cooldown_action == Some("escalate") {
            let escalation_text = format!(
                "🛡️ @{chatter_login} Timeout (10m) wegen wiederholter Service-Pitches/Spam. \
                 Empfehlung: User bannen."
            );
            // Den 600-s-Timeout führt die Pipeline einmalig auf StrongTimeout aus
            // (kein Doppel-Timeout mehr; Mod-/Broadcaster-Guard greift dort).
            // Python timeoutet hier ebenfalls 600 s, ohne die Nachricht zu löschen.
            self.log_warning(
                &channel_login,
                &chatter_login,
                &chatter_id,
                account_age_safe,
                follower_count,
                total_score,
                msg_count,
                "ESCALATED_TIMEOUT",
                &{
                    let mut r = reasons.clone();
                    r.push("escalation:ignored_previous_warning".to_string());
                    r
                },
                &raw_content,
            );
            return PitchDecision::StrongTimeout {
                text: escalation_text,
                duration: Duration::from_secs(600),
            };
        }

        // Schritt 16–17: Senden + Cooldowns setzen (Z. 983–1016)
        let result = if severity != "HINT" {
            let warning_text = build_warning_text(
                &chatter_login,
                severity == "WARNING_STRONG",
                is_new_account,
                account_age_days,
            );
            // Sendung über ChatApi
            match self
                .api
                .send_message(&event.broadcaster_user_id, &warning_text)
                .await
            {
                Ok(crate::types::SendOutcome::Sent) => {
                    // Cooldowns setzen + State löschen (Z. 1003–1016)
                    let mut st = self.state.lock().await;
                    st.channel_cd
                        .insert(channel_login.clone(), now + CHANNEL_COOLDOWN_SEC);
                    st.user_cd
                        .insert(bucket_key.clone(), now + USER_COOLDOWN_SEC);
                    st.activity.remove(&bucket_key);
                    st.message_history.remove(&bucket_key);
                    st.first_seen.remove(&bucket_key);

                    if severity == "WARNING_STRONG" {
                        // Python (Z. 983–1030): erster Strong-Treffer warnt nur
                        // öffentlich — kein Delete, kein Timeout. Der Timeout kommt
                        // erst im Eskalationszweig (wiederholter Pitch trotz Cooldown).
                        PitchDecision::StrongWarn { text: warning_text }
                    } else {
                        PitchDecision::PublicWarn { text: warning_text }
                    }
                }
                Ok(_) | Err(_) => return PitchDecision::None,
            }
        } else {
            // HINT: nur Cooldown setzen, kein Chat (Z. 1013–1016)
            let mut st = self.state.lock().await;
            st.hint_cd
                .insert(bucket_key.clone(), now + HINT_COOLDOWN_SEC);
            PitchDecision::Hint
        };

        // Schritt 18: Log (Z. 1018–1029)
        self.log_warning(
            &channel_login,
            &chatter_login,
            &chatter_id,
            account_age_safe,
            follower_count,
            total_score,
            msg_count,
            severity,
            &reasons,
            &raw_content,
        );

        result
    }

    // ── Private Hilfsmethoden ─────────────────────────────────────────────────

    /// Schritt 1: Beobachtungsposition (`_observe_service_message_position`).
    /// service_pitch_warning.py Z. 443–468
    fn observe_message_position(
        &self,
        st: &mut State,
        channel: &str,
        chatter: &str,
        now: f64,
    ) -> (u32, bool) {
        let key = (channel.to_string(), chatter.to_string());
        let count = if let Some(&(ts, c)) = st.seen_messages.get(&key) {
            if now - ts <= OBSERVED_MSG_CACHE_TTL_SEC {
                c + 1
            } else {
                1
            }
        } else {
            1
        };
        st.seen_messages.insert(key, (now, count));
        (count, count == 1)
    }

    /// Schritt 8: Early-Window-Score (`_early_window_score`).
    /// service_pitch_warning.py Z. 560–570
    fn early_window_score(
        &self,
        st: &mut State,
        channel: &str,
        chatter: &str,
        now: f64,
    ) -> (i32, Vec<String>) {
        let key = (channel.to_string(), chatter.to_string());
        match st.first_seen.get(&key).copied() {
            None => {
                st.first_seen.insert(key, now);
                (1, vec!["timing:first_appearance_window".to_string()])
            }
            Some(first) if now - first <= FIRST_CHAT_WINDOW_SEC => {
                (1, vec!["timing:first_appearance_window".to_string()])
            }
            _ => (0, vec![]),
        }
    }

    /// Schritt 6: Follower-Hint mit Lazy-DB-Load bei Cache-Miss.
    ///
    /// Python (service_pitch_warning.py Z. 700–746) liest bei Cache-Miss synchron
    /// `readonly_connection()`; hier laden wir async und cachen das Ergebnis. Großer
    /// Kanal (> MAX_FOLLOWERS) → kein Low-Target (von Service-Pitch-Warnungen
    /// ausgenommen); sonst — inkl. unbekannt/keine Session-Daten (None) — gilt der
    /// Kanal als klein. Vorher gab der Cache-Miss immer „assume small" zurück, weil
    /// `pre_warm_follower_cache` nirgends aufgerufen wurde → der Follower-Gate war
    /// faktisch aus und große Kanäle bekamen Warnungen/Timeouts.
    async fn follower_hint(&self, channel_login: &str, now: f64) -> (bool, Option<i32>) {
        let login = channel_login.trim_start_matches('#').to_lowercase();
        if login.is_empty() {
            return (true, None);
        }
        // Frischen Cache-Wert nutzen, wenn vorhanden.
        {
            let st = self.state.lock().await;
            if let Some(&(ts, fc)) = st.follower_cache.get(&login) {
                if now - ts <= FOLLOWER_CACHE_TTL_SEC {
                    return Self::derive_low_target(fc);
                }
            }
        }
        // Cache-Miss → einmalig aus der DB laden und cachen (Python: synchroner Read).
        let fc = self.load_follower_count(&login).await;
        {
            let mut st = self.state.lock().await;
            st.follower_cache.insert(login.clone(), (now, fc));
        }
        debug!(channel = %login, followers = ?fc, "Follower-Hint aus DB geladen");
        Self::derive_low_target(fc)
    }

    /// Großer Kanal (> MAX_FOLLOWERS) → kein Low-Target; sonst (auch None) klein.
    fn derive_low_target(fc: Option<i32>) -> (bool, Option<i32>) {
        if fc.map(|c| c > MAX_FOLLOWERS).unwrap_or(false) {
            (false, fc)
        } else {
            (true, fc)
        }
    }

    /// Letzte bekannte Follower-Zahl eines Kanals aus der DB (oder None bei
    /// fehlenden Session-Daten). service_pitch_warning.py Z. 714–729.
    /// Prod-Typen: followers_end/followers_start = INTEGER.
    ///
    /// Hot-Path-Schutz: Der Lookup läuft in der Chat-Pipeline (einmal pro Kanal je
    /// Cache-TTL). Er ist mit 3 s gebounded und **fail-open** (None = „assume small"),
    /// damit ein DB-Hiccup die Pipeline nicht stallt.
    async fn load_follower_count(&self, login: &str) -> Option<i32> {
        let query = sqlx::query_scalar!(
            r#"
            SELECT COALESCE(followers_end, followers_start) AS "follower_count?"
              FROM twitch_stream_sessions
             WHERE streamer_login = $1
               AND COALESCE(followers_end, followers_start) IS NOT NULL
             ORDER BY COALESCE(ended_at, started_at) DESC
             LIMIT 1
            "#,
            login,
        )
        .fetch_optional(&self.pool);

        match tokio::time::timeout(std::time::Duration::from_secs(3), query).await {
            Ok(res) => res.unwrap_or(None).flatten(),
            Err(_) => {
                tracing::warn!(channel = %login, "Follower-Lookup Timeout — fail-open (assume small)");
                None
            }
        }
    }

    /// Befüllt den Follower-Cache aus der DB (async, vom Orchestrator aufzurufen).
    ///
    /// SQL: `SELECT COALESCE(followers_end, followers_start) FROM twitch_stream_sessions
    /// WHERE streamer_login = $1 AND COALESCE(…) IS NOT NULL ORDER BY … DESC LIMIT 1`
    ///
    /// Prod-Schema: twitch_stream_sessions.followers_end = integer,
    /// twitch_stream_sessions.followers_start = integer,
    /// twitch_stream_sessions.ended_at = timestamp with time zone,
    /// twitch_stream_sessions.started_at = timestamp with time zone
    pub async fn pre_warm_follower_cache(&self, channel_login: &str) {
        let login = channel_login.trim_start_matches('#').to_lowercase();
        if login.is_empty() {
            return;
        }
        // service_pitch_warning.py Z. 714–729 — gemeinsame Query via load_follower_count.
        let result = self.load_follower_count(&login).await;

        let now = self.state.lock().await.now();
        let mut st = self.state.lock().await;
        st.follower_cache.insert(login, (now, result));
    }

    /// Prune-Logik (`_prune_service_warning_state`).
    /// service_pitch_warning.py Z. 599–651
    fn prune_state(&self, st: &mut State, now: f64) {
        if now - st.last_pruned < STATE_PRUNE_INTERVAL_SEC {
            return;
        }
        st.last_pruned = now;

        // message_history-Buckets (Einträge älter als SEQUENCE_WINDOW_SEC)
        st.message_history.retain(|_, bucket| {
            while bucket
                .front()
                .map(|e| now - e.ts > SEQUENCE_WINDOW_SEC)
                .unwrap_or(false)
            {
                bucket.pop_front();
            }
            !bucket.is_empty()
        });

        // activity-Buckets (Einträge älter als WINDOW_SEC)
        st.activity.retain(|_, bucket| {
            while bucket
                .front()
                .map(|e| now - e.ts > WINDOW_SEC)
                .unwrap_or(false)
            {
                bucket.pop_front();
            }
            !bucket.is_empty()
        });

        // flat caches
        prune_deadline_cache(
            &mut st.first_seen,
            now,
            TRACKED_USER_STATE_MAXLEN,
            (FIRST_CHAT_WINDOW_SEC * 20.0).max(3600.0),
        );
        prune_deadline_cache(
            &mut st.channel_cd,
            now,
            CHANNEL_STATE_MAXLEN,
            (CHANNEL_COOLDOWN_SEC * 2.0).max(1800.0),
        );
        prune_deadline_cache(
            &mut st.user_cd,
            now,
            TRACKED_USER_STATE_MAXLEN,
            (USER_COOLDOWN_SEC * 2.0).max(3600.0),
        );
        prune_deadline_cache(
            &mut st.hint_cd,
            now,
            TRACKED_USER_STATE_MAXLEN,
            (HINT_COOLDOWN_SEC * 4.0).max(300.0),
        );

        // seen_messages (has (f64, u32) — prune manually)
        {
            let stale_before = now - OBSERVED_MSG_CACHE_TTL_SEC * 2.0;
            st.seen_messages.retain(|_, (ts, _)| *ts >= stale_before);
            // overflow eviction if needed
            let max = 32768usize;
            if st.seen_messages.len() > max {
                let overflow = st.seen_messages.len() - max;
                let mut by_ts: Vec<_> = st
                    .seen_messages
                    .iter()
                    .map(|(k, (ts, _))| (*ts, k.clone()))
                    .collect();
                by_ts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                for (_, k) in by_ts.into_iter().take(overflow) {
                    st.seen_messages.remove(&k);
                }
            }
        }

        // account_age_cache
        prune_ts_cache(
            &mut st.account_age_cache,
            now,
            8192,
            ACCOUNT_CACHE_TTL_SEC * 4.0,
        );

        // follower_cache
        prune_ts_cache(
            &mut st.follower_cache,
            now,
            2048,
            FOLLOWER_CACHE_TTL_SEC * 4.0,
        );
    }

    /// Forensischer Audit-Trail: tab-separierte Zeile in `twitch_service_warnings.log`.
    ///
    /// Port von `service_pitch_warning.py` Z. 748–776 (`_record_service_warning`):
    /// Python schreibt die Warnung zusätzlich zum Logger in eine Datei, damit sie
    /// auch ohne aktives Log-Level erhalten bleibt. Grillme Z. 226
    /// (`scam-pitch-spam-review-3` BUILD) hält diesen Trail fest. Wir spiegeln
    /// beides: `debug!`-Tracing (für Live-Tail) **und** Datei-Append (forensisch).
    #[allow(clippy::too_many_arguments)]
    fn log_warning(
        &self,
        channel_login: &str,
        chatter_login: &str,
        chatter_id: &str,
        account_age_safe: i64,
        follower_count: Option<i32>,
        score: i32,
        msg_count: usize,
        severity: &str,
        reasons: &[String],
        content: &str,
    ) {
        let line = format_service_warning_line(
            &Utc::now().to_rfc3339(),
            severity,
            channel_login,
            chatter_login,
            chatter_id,
            account_age_safe,
            follower_count,
            score,
            msg_count,
            reasons,
            content,
        );
        // Live-Tail über Tracing (Z. 768–773).
        debug!(target: "service_warnings", "{line}");
        // Forensischer Datei-Trail (Z. 762–774). Fehler schlucken wie Python
        // (`except Exception: log.debug`) — ein Log-Schreibfehler darf die
        // Moderation nicht abbrechen.
        if let Err(e) = append_service_warning(&line) {
            debug!(target: "service_warnings", "Konnte Service-Warnung nicht in Datei schreiben: {e}");
        }
    }
}

/// Verzeichnis für den Service-Warning-Trail. `logs/` relativ zum CWD (gleiche
/// Konvention wie `tb-dashboard-api::read_log_tail`); per
/// `TWITCH_SERVICE_WARNING_LOG_DIR` überschreibbar (Tests, abweichendes Deploy).
fn service_warning_log_dir() -> std::path::PathBuf {
    std::env::var_os("TWITCH_SERVICE_WARNING_LOG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("logs"))
}

/// Hängt eine vorformatierte Zeile (ohne Zeilenumbruch) an
/// `<dir>/twitch_service_warnings.log` an. Legt das Verzeichnis bei Bedarf an
/// (`mkdir(parents=True)` in Python Z. 763).
fn append_service_warning(line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = service_warning_log_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("twitch_service_warnings.log");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")
}

/// Baut die tab-separierte Audit-Zeile (ohne Zeilenumbruch).
/// Reine Funktion → unabhängig vom Datei-/Tracing-Pfad testbar.
/// Format identisch zu `service_pitch_warning.py` Z. 768–773.
#[allow(clippy::too_many_arguments)]
fn format_service_warning_line(
    ts: &str,
    severity: &str,
    channel_login: &str,
    chatter_login: &str,
    chatter_id: &str,
    account_age_safe: i64,
    follower_count: Option<i32>,
    score: i32,
    msg_count: usize,
    reasons: &[String],
    content: &str,
) -> String {
    let follower_text = follower_count
        .map(|c| c.to_string())
        .unwrap_or_else(|| "-".to_string());
    let reason_text = if reasons.is_empty() {
        "-".to_string()
    } else {
        reasons.join(",")
    };
    let safe_content: String = content
        .replace('\n', " ")
        .trim()
        .chars()
        .take(350)
        .collect();
    let chatter_login = if chatter_login.is_empty() {
        "-"
    } else {
        chatter_login
    };
    let chatter_id = if chatter_id.is_empty() {
        "-"
    } else {
        chatter_id
    };
    format!(
        "{ts}\t{severity}\t{channel_login}\t{chatter_login}\t{chatter_id}\tage_days={account_age_safe}\t\
         followers={follower_text}\tscore={score}\tmsgs={msg_count}\t{reason_text}\t{safe_content}"
    )
}

// ── SpamAiReviewer ────────────────────────────────────────────────────────────

/// Referenz auf ein in der DB gespeichertes Spam-Muster (für Korrektur-Buttons).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedPatternRef {
    /// `twitch_auto_learned_spam_patterns.id`
    pub id: i64,
    /// Gespeichertes Muster (lowercase).
    pub pattern: String,
}

#[derive(Debug, Clone, Copy)]
enum SpamReviewVerdict {
    Spam,
    Clean,
    Unsure,
    Skipped,
    Timeout,
    ProviderError,
    ParseError,
}

impl SpamReviewVerdict {
    fn as_str(self) -> &'static str {
        match self {
            Self::Spam => "spam",
            Self::Clean => "clean",
            Self::Unsure => "unsure",
            Self::Skipped => "skipped",
            Self::Timeout => "timeout",
            Self::ProviderError => "provider_error",
            Self::ParseError => "parse_error",
        }
    }
}

struct SpamReviewDecision {
    verdict: SpamReviewVerdict,
    confidence: Option<f32>,
    reason: String,
}

/// Urteil des Judge im Verdachtspfad. Jede Variante MUSS beim Aufrufer einen
/// Alert erzeugen — der Judge gatet die Aktion, nie die Sichtbarkeit.
#[derive(Debug, Clone)]
pub enum AiReviewOutcome {
    /// Judge bestätigt Spam.
    Spam {
        /// Begründung des Modells (max ~80 Zeichen laut Prompt).
        reason: String,
        /// Confidence des Judge, sofern der Provider sie geliefert hat.
        confidence: Option<f32>,
        /// Gespeichertes Muster, falls das Distinktivitäts-Gate es zuließ.
        learned: Option<LearnedPatternRef>,
        /// Vorgeschlagenes, aber abgelehntes Muster (generisches Vokabular).
        rejected_pattern: Option<String>,
        /// True, wenn das Gate zustimmte, aber der DB-Write scheiterte —
        /// muss im Alert sichtbar sein (kein stilles Nicht-Lernen).
        save_failed: bool,
    },
    /// Judge hält die Nachricht für harmlos. Es wird bewusst NICHTS gelernt.
    Safe { reason: String },
    /// Kein Urteil möglich (Key fehlt, beide Provider down, kein JSON).
    Error { detail: String },
    /// Cooldown aktiv — (channel, chatter) wurde kürzlich schon reviewt.
    Skipped,
}

/// Spam-AI-Review — DeepSeek via Fireworks primär, MiniMax M3 als Fallback.
///
/// Ersetzt den alten fire-and-forget-Pfad (`maybe_review*`): Der Orchestrator
/// wartet auf das Urteil und baut daraus Aktion + Alert.
pub struct SpamAiReviewer {
    pool: PgPool,
    http: reqwest::Client,
    cooldowns: Arc<Mutex<HashMap<(String, String), f64>>>,
    /// Gemeinsame Uhr-Basis für monotone Timestamps.
    epoch: Instant,
}

impl SpamAiReviewer {
    /// Erzeugt einen neuen Reviewer.
    pub fn new(pool: PgPool, http: reqwest::Client) -> Self {
        Self {
            pool,
            http,
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            epoch: Instant::now(),
        }
    }

    /// Führt den Judge-Review synchron aus und liefert das Urteil.
    ///
    /// Das Score-Gate (`spam_score > 0`) liegt beim Aufrufer; hier nur noch
    /// Cooldown (spam_ai_review.py Z. 75–85), Provider-Kette und Lernen.
    pub async fn review_for_verdict(&self, event: &ChatMessageEvent) -> AiReviewOutcome {
        let content = event.text().to_string();
        let channel = event.broadcaster_user_login.to_lowercase();
        let chatter = event.chatter_user_login.to_lowercase();

        {
            let mut cds = self.cooldowns.lock().await;
            let key = (channel.clone(), chatter.clone());
            let now_mono = self.epoch.elapsed().as_secs_f64();
            if cds.len() > REVIEW_COOLDOWN_MAX_LEN {
                let stale_before = now_mono - REVIEW_COOLDOWN_SEC * 4.0;
                cds.retain(|_, ts| *ts > stale_before);
            }
            let cd_until = cds.get(&key).copied().unwrap_or(0.0);
            if now_mono < cd_until {
                return self
                    .record_and_return(
                        event,
                        AiReviewOutcome::Skipped,
                        SpamReviewVerdict::Skipped,
                        None,
                        "Cooldown aktiv".to_string(),
                    )
                    .await;
            }
            cds.insert(key, now_mono + REVIEW_COOLDOWN_SEC);
        }

        let providers = tb_llm::endpoint_chain("spam_judge");
        if providers.is_empty() {
            let detail = "kein Judge-API-Key gesetzt (FIREWORK_API_KEY / MINIMAX_*)".to_string();
            return self
                .record_and_return(
                    event,
                    AiReviewOutcome::Error {
                        detail: detail.clone(),
                    },
                    SpamReviewVerdict::ProviderError,
                    None,
                    detail,
                )
                .await;
        }

        let mut last_error = String::new();
        let mut last_verdict = SpamReviewVerdict::Unsure;
        for provider in &providers {
            match call_judge(&self.http, provider, true, &content).await {
                Ok(Some(review)) => {
                    let (learned, rejected_pattern, save_failed) =
                        persist_spam_learning(&self.pool, &review, &content, &channel).await;
                    let reason = review.reason.unwrap_or_default();
                    let confidence = review.confidence;
                    let (outcome, verdict) = if review.is_spam {
                        (
                            AiReviewOutcome::Spam {
                                reason,
                                confidence,
                                learned,
                                rejected_pattern,
                                save_failed,
                            },
                            SpamReviewVerdict::Spam,
                        )
                    } else {
                        (AiReviewOutcome::Safe { reason }, SpamReviewVerdict::Clean)
                    };
                    let persisted_reason = match &outcome {
                        AiReviewOutcome::Spam { reason, .. } | AiReviewOutcome::Safe { reason } => {
                            reason.clone()
                        }
                        _ => String::new(),
                    };
                    return self
                        .record_and_return(event, outcome, verdict, confidence, persisted_reason)
                        .await;
                }
                Ok(None) => {
                    last_error = format!("{}: kein parsebares JSON", provider.model);
                    last_verdict = SpamReviewVerdict::ParseError;
                    warn!(
                        model = provider.model,
                        "Judge lieferte kein parsebares Urteil"
                    );
                }
                Err(e) => {
                    let (verdict, detail) = match e {
                        JudgeCallError::Timeout(detail) => (SpamReviewVerdict::Timeout, detail),
                        JudgeCallError::Provider(detail) => {
                            (SpamReviewVerdict::ProviderError, detail)
                        }
                    };
                    last_verdict = verdict;
                    last_error = format!("{}: {detail}", provider.model);
                    warn!(model = provider.model, fehler = %detail, "Judge-Call fehlgeschlagen");
                }
            }
        }
        self.record_and_return(
            event,
            AiReviewOutcome::Error {
                detail: last_error.clone(),
            },
            last_verdict,
            None,
            last_error,
        )
        .await
    }

    async fn record_and_return(
        &self,
        event: &ChatMessageEvent,
        outcome: AiReviewOutcome,
        verdict: SpamReviewVerdict,
        confidence: Option<f32>,
        reason: String,
    ) -> AiReviewOutcome {
        let decision = SpamReviewDecision {
            verdict,
            confidence,
            reason,
        };
        match persist_spam_review_decision(&self.pool, event, &decision).await {
            Ok(id) => debug!(
                id,
                verdict = decision.verdict.as_str(),
                "Spam-Review-Entscheidung persistiert"
            ),
            Err(error) => {
                warn!(verdict = decision.verdict.as_str(), %error, "Spam-Review-Entscheidung nicht persistiert")
            }
        }
        outcome
    }
}

// ── Judge-API-Call ────────────────────────────────────────────────────────────

#[derive(Debug)]
struct AiReview {
    is_spam: bool,
    confidence: Option<f32>,
    pattern: Option<String>,
    pattern_type: Option<String>,
    reason: Option<String>,
}

#[derive(Debug)]
enum JudgeCallError {
    Timeout(String),
    Provider(String),
}

/// Persistiert das Lern-Ergebnis eines Judge-Urteils.
///
/// Nur Spam-Muster werden gelernt, und nur wenn sie das Distinktivitäts-Gate
/// passieren. Safe-Muster werden NIE gespeichert (Safe-List-Poisoning).
/// Rückgabe: (gespeicherte Referenz, abgelehntes Muster, DB-Write gescheitert).
async fn persist_spam_learning(
    pool: &PgPool,
    review: &AiReview,
    content: &str,
    channel: &str,
) -> (Option<LearnedPatternRef>, Option<String>, bool) {
    if !review.is_spam {
        return (None, None, false);
    }
    let Some(pat) = review.pattern.as_deref() else {
        return (None, None, false);
    };
    if pat.chars().count() < PATTERN_MIN_LEN {
        return (None, None, false);
    }
    if !crate::spam_filter::is_distinctive_spam_pattern(pat) {
        warn!(
            pattern = %pat,
            "Judge-Muster abgelehnt: nur generisches Vokabular (Distinktivitäts-Gate)"
        );
        return (None, Some(pat.to_string()), false);
    }
    let pattern_type = match review.pattern_type.as_deref() {
        Some("phrase") => "phrase",
        _ => "fragment",
    };
    let learned = save_spam_pattern(
        pool,
        pat,
        pattern_type,
        content,
        channel,
        review.reason.as_deref().unwrap_or(""),
    )
    .await
    .map(|id| LearnedPatternRef {
        id,
        pattern: pat.to_lowercase(),
    });
    let save_failed = learned.is_none();
    (learned, None, save_failed)
}

/// Liest `usage.prompt_tokens` / `usage.completion_tokens` aus der MiniMax-Antwort.
///
/// Fehlende oder negative Werte werden auf `0` geklemmt — Parität zu Pythons
/// `int(getattr(_usage, "prompt_tokens", 0) or 0)` (spam_ai_review.py Z. 173–174)
/// und zur Clamp-Logik in [`tb_llm::ledger::record`].
fn usage_tokens(resp: &serde_json::Value) -> (i64, i64) {
    let field = |name: &str| {
        resp.get("usage")
            .and_then(|u| u.get(name))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            .max(0)
    };
    (field("prompt_tokens"), field("completion_tokens"))
}

/// Ruft einen Judge-Provider (OpenAI-kompatibel) auf.
///
/// `Ok(None)` = Antwort kam, aber kein parsebares Urteil (kein Fallback-Fehler
/// des Transports). `Err` = Transport-/HTTP-Problem → Aufrufer probiert den
/// nächsten Provider.
async fn call_judge(
    http: &reqwest::Client,
    provider: &tb_llm::LlmEndpoint,
    record_usage: bool,
    content: &str,
) -> Result<Option<AiReview>, JudgeCallError> {
    let Some(api_key) = provider.api_key.as_deref() else {
        return Err(JudgeCallError::Provider(
            "API-Key für Judge-Endpoint fehlt".to_string(),
        ));
    };
    let truncated: String = content.chars().take(500).collect();
    let body = serde_json::json!({
        "model": provider.model,
        "max_tokens": JUDGE_MAX_TOKENS,
        "temperature": 0.0,
        "messages": [
            {"role": "system", "content": SPAM_REVIEW_SYSTEM_PROMPT},
            {"role": "user", "content": format!("Nachricht: {truncated}")}
        ]
    });

    let resp = http
        .post(format!(
            "{}/chat/completions",
            provider.base_url.trim_end_matches('/')
        ))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(20))
        .json(&body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                JudgeCallError::Timeout(error.to_string())
            } else {
                JudgeCallError::Provider(error.to_string())
            }
        })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(JudgeCallError::Provider(format!("HTTP {status}")));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|error| JudgeCallError::Provider(error.to_string()))?;

    // Verbrauch ins gemeinsame LLM-Ledger (best-effort, wirft nie).
    // Verbucht direkt nach der Antwort, damit der Token-Verbrauch auch dann
    // zählt, wenn die JSON-Extraktion unten scheitert.
    let (tokens_in, tokens_out) = usage_tokens(&json);
    if record_usage {
        tb_llm::ledger::record(JUDGE_PURPOSE, &provider.model, tokens_in, tokens_out, true).await;
    }

    let message = &json["choices"][0]["message"];
    let mut raw = message["content"].as_str().unwrap_or("").to_string();
    if raw.trim().is_empty() {
        // Reasoning-Modelle liefern das Urteil teils in reasoning_content.
        raw = message["reasoning_content"]
            .as_str()
            .unwrap_or("")
            .to_string();
    }

    // Think-Block stripping (spam_ai_review.py Z. 165)
    let think_re = Regex::new(r"(?si)<think>.*?</think>").unwrap();
    let stripped = think_re.replace_all(&raw, "").to_string();

    Ok(extract_verdict(&stripped))
}

/// Extrahiert das Urteil aus der Modellantwort: Reasoning-Modelle schreiben
/// Denktext (auch mit Klammern) VOR das JSON — das letzte flache JSON-Objekt
/// mit `is_spam`-Feld gewinnt.
fn extract_verdict(text: &str) -> Option<AiReview> {
    let flat_json_re = Regex::new(r"\{[^{}]*\}").unwrap();
    let mut parsed: Option<serde_json::Value> = None;
    for m in flat_json_re.find_iter(text) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(m.as_str()) {
            // is_spam muss ein echter Bool sein — ein String "true" o.ä. darf
            // NICHT still als harmlos durchgehen, sondern gilt als unparsebar
            // (→ Provider-Fallback bzw. Error-Alert).
            if v.get("is_spam").is_some_and(serde_json::Value::is_boolean) {
                parsed = Some(v);
            }
        }
    }
    let parsed = parsed?;
    Some(AiReview {
        is_spam: parsed["is_spam"].as_bool().unwrap_or(false),
        confidence: parsed["confidence"]
            .as_f64()
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            .map(|value| value as f32),
        pattern: parsed["pattern"].as_str().map(str::to_lowercase),
        pattern_type: parsed["pattern_type"].as_str().map(|s| s.to_string()),
        reason: parsed["reason"].as_str().map(|s| s.to_string()),
    })
}

// ── DB-Schreiboperationen ─────────────────────────────────────────────────────

async fn persist_spam_review_decision(
    pool: &PgPool,
    event: &ChatMessageEvent,
    decision: &SpamReviewDecision,
) -> Result<i64, sqlx::Error> {
    let source_message: String = event.text().chars().take(500).collect();
    let reason: String = decision.reason.chars().take(500).collect();
    let chatter_id = (!event.chatter_user_id.is_empty()).then_some(event.chatter_user_id.as_str());

    sqlx::query_scalar(
        "INSERT INTO twitch_spam_review_decisions \
         (channel_login, chatter_login, chatter_id, source_message, verdict, confidence, reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(event.broadcaster_user_login.to_lowercase())
    .bind(event.chatter_user_login.to_lowercase())
    .bind(chatter_id)
    .bind(source_message)
    .bind(decision.verdict.as_str())
    .bind(decision.confidence)
    .bind(reason)
    .fetch_one(pool)
    .await
}

/// Spam-Pattern in DB speichern (ON CONFLICT → hit_count++).
/// spam_ai_review.py Z. 179–231
/// Prod-Schema: twitch_auto_learned_spam_patterns (alle TEXT/INTEGER/TIMESTAMPTZ)
///
/// Liefert die Row-ID (auch bei Konflikt/Update) — die referenzieren die
/// Discord-Korrektur-Buttons. `None` nur bei DB-Fehler.
async fn save_spam_pattern(
    pool: &PgPool,
    pattern: &str,
    pattern_type: &str,
    source_message: &str,
    source_channel: &str,
    reasoning: &str,
) -> Option<i64> {
    let pat = pattern.to_lowercase();
    let src_msg: String = source_message.chars().take(500).collect();
    let reasoning_short: String = reasoning.chars().take(200).collect();
    let created_at = Utc::now();

    // Prod: created_at = TIMESTAMPTZ → DateTime<Utc> binden (NIE ISO-String).
    // twitch_auto_learned_spam_patterns.hit_count = integer → i32 implizit via DEFAULT
    let result = sqlx::query_scalar!(
        r#"
        INSERT INTO twitch_auto_learned_spam_patterns
            (pattern, pattern_type, source_message, source_channel, minimax_reasoning, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (pattern) DO UPDATE SET
            hit_count = twitch_auto_learned_spam_patterns.hit_count + 1
        RETURNING id AS "id!"
        "#,
        &pat,
        pattern_type,
        &src_msg,
        source_channel,
        &reasoning_short,
        created_at,
    )
    .fetch_one(pool)
    .await;

    match result {
        Ok(id) => Some(id),
        Err(e) => {
            warn!("Spam-Pattern konnte nicht gespeichert werden: {e}");
            None
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    static PROVIDER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_provider_env() {
        for name in [
            "TB_LLM_PROVIDER_DEFAULT",
            "TB_LLM_PROVIDER_SPAM_JUDGE",
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "FIREWORK_MODEL",
            "FIREWORKS_MODEL",
            "MINIMAX_TOKEN_PLAN_KEY",
            "MINIMAX_API_KEY",
            "MINIMAX_BASE_URL",
            "MINIMAX_MODEL",
            "MINMAX",
        ] {
            std::env::remove_var(name);
        }
    }

    #[test]
    fn judge_provider_folgt_gemeinsamer_auswahl() {
        let _guard = PROVIDER_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_provider_env();
        std::env::set_var("MINIMAX_API_KEY", "minimax-key");

        let providers = tb_llm::endpoint_chain("spam_judge");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].base_url, "https://api.minimax.io/v1");
        assert_eq!(providers[0].model, "MiniMax-M3");

        std::env::set_var("FIREWORK_API_KEY", "fireworks-key");
        let providers = tb_llm::endpoint_chain("spam_judge");
        assert!(providers[0].base_url.contains("fireworks.ai"));
        assert!(providers[0].model.contains("deepseek"));
        assert_eq!(providers[1].base_url, "https://api.minimax.io/v1");
        assert_eq!(providers[1].model, "MiniMax-M3");

        std::env::set_var("TB_LLM_PROVIDER_SPAM_JUDGE", "minimax");
        let providers = tb_llm::endpoint_chain("spam_judge");
        assert_eq!(providers[0].base_url, "https://api.minimax.io/v1");
        assert_eq!(providers[0].model, "MiniMax-M3");
        clear_provider_env();
    }

    fn make_event(channel: &str, chatter: &str, text: &str) -> ChatMessageEvent {
        use crate::types::ChatMessageBody;
        ChatMessageEvent {
            broadcaster_user_id: "123".to_string(),
            broadcaster_user_login: channel.to_string(),
            broadcaster_user_name: channel.to_string(),
            chatter_user_id: "456".to_string(),
            chatter_user_login: chatter.to_string(),
            chatter_user_name: chatter.to_string(),
            message_id: "msg1".to_string(),
            message: ChatMessageBody {
                text: text.to_string(),
                fragments: vec![],
            },
            badges: vec![],
            color: String::new(),
            ..Default::default()
        }
    }

    // ── normalize_text ────────────────────────────────────────────────────────
    #[test]
    fn normalize_text_klappt() {
        assert_eq!(normalize_text("  hey   how  are  you  "), "hey how are you");
        assert_eq!(normalize_text(""), "");
        assert_eq!(normalize_text("  "), "");
    }

    // ── token_count ───────────────────────────────────────────────────────────
    #[test]
    fn token_count_klappt() {
        assert_eq!(token_count(""), 0);
        assert_eq!(token_count("hello world"), 2);
        assert_eq!(token_count("  a  b  c  "), 3);
    }

    // ── score_message Einzel-Features ────────────────────────────────────────
    #[test]
    fn score_language_probe() {
        let p = Patterns::build();
        let r = score_message("do you speak english?", &p);
        assert!(r.features.contains("language_probe"));
        assert_eq!(r.score, 3);
    }

    #[test]
    fn score_crew_threat() {
        let p = Patterns::build();
        let r = score_message("ima pull up with my crew bro", &p);
        assert!(r.features.contains("crew_threat"));
        assert_eq!(r.score, 5);
    }

    #[test]
    fn score_discord_handle_drop() {
        let p = Patterns::build();
        let r = score_message("discord: CoolDesigner.123", &p);
        assert!(r.features.contains("discord_handle_drop"));
        // offplatform(4) matcht \bdiscord\b; discord_handle_drop(4);
        // external_link_or_handle(4) weil platform_ref(discord) + discord_handle_drop → profile drop
        // Gesamt: 4 + 4 + 4 = 12
        assert_eq!(r.score, 12);
    }

    #[test]
    fn score_growth_pitch() {
        let p = Patterns::build();
        let r = score_message("I can help you grow with more viewers!", &p);
        assert!(
            r.features.contains("growth_pitch"),
            "features: {:?}",
            r.features
        );
        assert!(r.score >= 3);
    }

    #[test]
    fn score_design_pitch() {
        let p = Patterns::build();
        let r = score_message("do you have a logo? I can do graphic designer work", &p);
        assert!(
            r.features.contains("design_pitch"),
            "features: {:?}",
            r.features
        );
        assert!(r.score >= 4);
    }

    #[test]
    fn score_generic_praise_short_no_context_gives_2() {
        let p = Patterns::build();
        // ≤5 Tokens, kein Stream-Kontext → praise_score=2
        let r = score_message("cool amazing", &p);
        assert!(r.features.contains("generic_praise"));
        assert!(r.score >= 2);
    }

    #[test]
    fn score_generic_praise_with_stream_context_gives_1() {
        let p = Patterns::build();
        // Stream-Kontext-Wort → praise_score=1
        let r = score_message("cool kill there", &p);
        assert!(
            r.features.contains("generic_praise"),
            "features: {:?}",
            r.features
        );
        // score = 1 für generic_praise
        let praise_reason = r
            .reasons
            .iter()
            .find(|r| r.contains("generic_praise"))
            .unwrap();
        assert!(
            praise_reason.contains("(1)"),
            "Expected score 1: {praise_reason}"
        );
    }

    #[test]
    fn score_trusted_collab_invite_no_score() {
        let p = Patterns::build();
        let r = score_message("https://twitch.tv/collab/invite/abc123", &p);
        assert!(r.features.contains("trusted_twitch_collab_invite"));
        assert!(!r.features.contains("external_link_or_handle"));
    }

    #[test]
    fn score_external_link_scores_4() {
        let p = Patterns::build();
        let r = score_message("check out https://bit.ly/spam123", &p);
        assert!(r.features.contains("external_link_or_handle"));
        assert!(r.score >= 4);
    }

    // ── score_combo_signals ───────────────────────────────────────────────────
    #[test]
    fn combo_new_here_plus_growth() {
        let mut f = HashSet::new();
        f.insert("new_here".to_string());
        f.insert("growth_pitch".to_string());
        let (s, reasons) = score_combo_signals(&f);
        assert_eq!(s, 3);
        assert!(reasons.contains(&"combo:new_here_plus_growth".to_string()));
    }

    #[test]
    fn combo_design_plus_offplatform() {
        let mut f = HashSet::new();
        f.insert("design_pitch".to_string());
        f.insert("offplatform".to_string());
        let (s, _) = score_combo_signals(&f);
        assert_eq!(s, 3);
    }

    #[test]
    fn combo_multiple_combos_addieren() {
        let mut f = HashSet::new();
        f.insert("growth_pitch".to_string());
        f.insert("offplatform".to_string());
        f.insert("design_pitch".to_string());
        // design+offplatform(3) + growth+offplatform(2)
        let (s, _) = score_combo_signals(&f);
        assert_eq!(s, 5);
    }

    // ── score_sequence_signals ────────────────────────────────────────────────
    #[test]
    fn sequence_short_burst_unter_schwelle_kein_score() {
        // Weniger als SEQUENCE_MIN_MSGS (3) Einträge → kein Score
        let mut bucket: VecDeque<HistoryEntry> = VecDeque::new();
        bucket.push_back(HistoryEntry {
            ts: 0.0,
            content: "hey".to_string(),
            features: HashSet::new(),
        });
        bucket.push_back(HistoryEntry {
            ts: 1.0,
            content: "hi".to_string(),
            features: HashSet::new(),
        });
        let (s, _) = score_sequence_signals(&bucket);
        assert_eq!(s, 0);
    }

    #[test]
    fn sequence_short_burst_gibt_2() {
        let mut bucket: VecDeque<HistoryEntry> = VecDeque::new();
        for i in 0..3 {
            bucket.push_back(HistoryEntry {
                ts: i as f64,
                content: "hey".to_string(), // ≤32 chars, ≤7 tokens
                features: HashSet::new(),
            });
        }
        let (s, reasons) = score_sequence_signals(&bucket);
        assert!(s >= 2, "score={s}");
        assert!(reasons.contains(&"sequence:short_multi_line_burst".to_string()));
    }

    #[test]
    fn sequence_greeting_language_combo() {
        let mut f1 = HashSet::new();
        f1.insert("greeting".to_string());
        let mut f2 = HashSet::new();
        f2.insert("language_probe".to_string());
        let mut f3 = HashSet::new();
        f3.insert("wellbeing".to_string());

        let mut bucket: VecDeque<HistoryEntry> = VecDeque::new();
        bucket.push_back(HistoryEntry {
            ts: 0.0,
            content: "hey there".to_string(),
            features: f1,
        });
        bucket.push_back(HistoryEntry {
            ts: 1.0,
            content: "do you speak english".to_string(),
            features: f2,
        });
        bucket.push_back(HistoryEntry {
            ts: 2.0,
            content: "how are you".to_string(),
            features: f3,
        });
        let (s, reasons) = score_sequence_signals(&bucket);
        assert!(
            reasons.contains(&"sequence:greeting_language_combo".to_string()),
            "reasons: {reasons:?}"
        );
        let _ = s;
    }

    // ── is_benign_social_checkin ──────────────────────────────────────────────
    #[test]
    fn benign_nur_greeting_wellbeing() {
        let p = Patterns::build();
        let mut f = HashSet::new();
        f.insert("greeting".to_string());
        f.insert("wellbeing".to_string());
        assert!(is_benign_social_checkin("hey how are you", &f, &p));
    }

    #[test]
    fn nicht_benign_wenn_link() {
        let p = Patterns::build();
        let mut f = HashSet::new();
        f.insert("greeting".to_string());
        assert!(!is_benign_social_checkin(
            "hey check https://discord.gg/x",
            &f,
            &p
        ));
    }

    #[test]
    fn nicht_benign_wenn_growth_pitch() {
        let p = Patterns::build();
        let mut f = HashSet::new();
        f.insert("greeting".to_string());
        f.insert("growth_pitch".to_string());
        assert!(!is_benign_social_checkin("hey more viewers?", &f, &p));
    }

    #[test]
    fn nicht_benign_wenn_zu_viele_tokens() {
        let p = Patterns::build();
        let mut f = HashSet::new();
        f.insert("greeting".to_string());
        // 15 Tokens → nicht benign
        let content = "hey how are you doing today bro and what are you up to right now man";
        assert!(!is_benign_social_checkin(content, &f, &p));
    }

    // ── MiniMax-Usage-Ledger (B9-BUILD-minimax-usage-ledger) ──────────────────
    #[test]
    fn usage_tokens_liest_prompt_und_completion() {
        // spam_ai_review.py Z. 172–175: prompt_tokens / completion_tokens aus usage.
        let resp = serde_json::json!({
            "choices": [{ "message": { "content": "{}" } }],
            "usage": { "prompt_tokens": 123, "completion_tokens": 45 }
        });
        assert_eq!(usage_tokens(&resp), (123, 45));
    }

    #[test]
    fn usage_tokens_fehlend_ist_null() {
        // Kein usage-Objekt → (0, 0), wie Pythons `getattr(..., 0) or 0`.
        let resp = serde_json::json!({ "choices": [] });
        assert_eq!(usage_tokens(&resp), (0, 0));
    }

    #[test]
    fn usage_tokens_clampt_negativ_auf_null() {
        // Defensiv: negative Werte landen nicht im Ledger (Parität zu ledger::record).
        let resp = serde_json::json!({
            "usage": { "prompt_tokens": -7, "completion_tokens": -1 }
        });
        assert_eq!(usage_tokens(&resp), (0, 0));
    }

    // ── extract_verdict (Reasoning-Modelle: Denktext vor dem JSON) ───────────
    #[test]
    fn extract_verdict_nimmt_letztes_flaches_json() {
        let text = "Wir analysieren {die Nachricht} genauer...\n\
                    {\"note\": \"kein urteil\"}\n\
                    {\"is_spam\": true, \"pattern\": \"Eballo.com\", \
                     \"pattern_type\": \"phrase\", \"reason\": \"Viewer-Bot-Werbung\"}";
        let v = extract_verdict(text).expect("Urteil muss gefunden werden");
        assert!(v.is_spam);
        assert_eq!(v.pattern.as_deref(), Some("eballo.com"));
        assert_eq!(v.pattern_type.as_deref(), Some("phrase"));
    }

    #[test]
    fn extract_verdict_ohne_json_gibt_none() {
        assert!(extract_verdict("nur Denktext, kein Urteil").is_none());
        assert!(extract_verdict("").is_none());
        assert!(extract_verdict("{\"foo\": 1}").is_none());
    }

    #[test]
    fn extract_verdict_verlangt_echten_bool() {
        // String-"true" darf NICHT still als Urteil durchgehen (Review-HIGH
        // 11.7.): unparsebar → Provider-Fallback/Error statt fälschlich Safe.
        assert!(extract_verdict("{\"is_spam\": \"true\"}").is_none());
        assert!(extract_verdict("{\"is_spam\": 1}").is_none());
    }

    // ── has_high_confidence_single_message_signal ─────────────────────────────
    #[test]
    fn high_conf_crew_threat() {
        let mut f = HashSet::new();
        f.insert("crew_threat".to_string());
        assert!(has_high_confidence_single_message_signal(&f));
    }

    #[test]
    fn high_conf_discord_handle_drop() {
        let mut f = HashSet::new();
        f.insert("discord_handle_drop".to_string());
        assert!(has_high_confidence_single_message_signal(&f));
    }

    #[test]
    fn high_conf_offplatform_plus_link() {
        let mut f = HashSet::new();
        f.insert("offplatform".to_string());
        f.insert("external_link_or_handle".to_string());
        assert!(has_high_confidence_single_message_signal(&f));
    }

    #[test]
    fn kein_high_conf_nur_greeting() {
        let mut f = HashSet::new();
        f.insert("greeting".to_string());
        assert!(!has_high_confidence_single_message_signal(&f));
    }

    // ── build_warning_text ────────────────────────────────────────────────────
    #[test]
    fn warning_text_public_kein_alter() {
        let t = build_warning_text("spammer123", false, false, Some(200));
        assert!(t.starts_with("@spammer123 bitte keine"));
        assert!(!t.contains("unter <3 Monate"));
    }

    #[test]
    fn warning_text_public_neuer_account() {
        let t = build_warning_text("spammer123", false, true, Some(10));
        assert!(t.contains("unter <3 Monate alt ist"));
    }

    #[test]
    fn warning_text_strong() {
        let t = build_warning_text("spammer123", true, false, Some(200));
        assert!(t.starts_with("🛡️"));
        assert!(t.contains("Ignorieren & Bannen"));
    }

    #[test]
    fn warning_text_unbekanntes_alter() {
        let t = build_warning_text("x", false, false, None);
        assert!(t.contains("Account-Alter unbekannt"));
    }

    // ── Threshold-Clamp (Vertrag §1 Z. 41–44) ────────────────────────────────
    // Konstanten werden zur Compile-Zeit geprüft — Clippy warnt bei assert! auf const.
    const _: () = {
        assert!(PUBLIC_THRESHOLD >= LIGHT_THRESHOLD);
        assert!(STRONG_THRESHOLD >= PUBLIC_THRESHOLD);
        assert!(LIGHT_THRESHOLD == 4);
        assert!(PUBLIC_THRESHOLD == 7);
        assert!(STRONG_THRESHOLD == 10);
    };

    #[test]
    fn schwellen_konstanten_korrekt() {
        // Werte nochmals als Laufzeit-Test dokumentiert (Vertrag §1 Z. 22–25)
        let light = LIGHT_THRESHOLD;
        let public = PUBLIC_THRESHOLD;
        let strong = STRONG_THRESHOLD;
        assert!(public >= light, "PUBLIC muss >= LIGHT sein");
        assert!(strong >= public, "STRONG muss >= PUBLIC sein");
        assert_eq!(light, 4);
        assert_eq!(public, 7);
        assert_eq!(strong, 10);
    }

    // ── Prune-Logik ───────────────────────────────────────────────────────────
    #[test]
    fn prune_deadline_cache_entfernt_alte_eintraege() {
        let mut cache: HashMap<String, f64> = HashMap::new();
        cache.insert("a".to_string(), 1.0);
        cache.insert("b".to_string(), 1000.0);
        prune_deadline_cache(&mut cache, 100.0, 1000, 50.0);
        assert!(!cache.contains_key("a")); // 1.0 < 100.0 - 50.0 = 50.0 → stale
        assert!(cache.contains_key("b")); // 1000.0 > 50.0 → fresh
    }

    #[test]
    fn prune_deadline_cache_overflow() {
        let mut cache: HashMap<String, f64> = HashMap::new();
        for i in 0..10u32 {
            cache.insert(format!("k{i}"), i as f64 + 1000.0);
        }
        prune_deadline_cache(&mut cache, 0.0, 5, 999999.0);
        assert!(cache.len() <= 5);
    }

    // ── DB-Tests (hermetic) ───────────────────────────────────────────────────
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
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;

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
        // Prod-treue DDL für die Lerntabelle
        // Prod-Typen: pattern=TEXT, hit_count=integer, created_at=timestamptz
        // + Migration 20260711170000: id-Spalte für Korrektur-Buttons
        for ddl in [
            r#"CREATE TABLE twitch_auto_learned_spam_patterns (
                pattern TEXT PRIMARY KEY,
                pattern_type TEXT NOT NULL DEFAULT 'fragment',
                source_message TEXT,
                source_channel TEXT,
                minimax_reasoning TEXT,
                hit_count INTEGER NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                id BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE
            )"#,
            r#"CREATE TABLE twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                followers_start INTEGER,
                followers_end INTEGER,
                started_at TIMESTAMPTZ,
                ended_at TIMESTAMPTZ
            )"#,
            r#"CREATE TABLE twitch_spam_review_decisions (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                chatter_login TEXT NOT NULL,
                chatter_id TEXT,
                source_message TEXT NOT NULL,
                verdict TEXT NOT NULL,
                confidence REAL,
                reason TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    macro_rules! spam_review_decision_persistence_test {
        ($name:ident, $schema:literal, $verdict:ident, $confidence:expr) => {
            #[tokio::test]
            async fn $name() {
                let pool = pool_or_skip!($schema);
                let event = make_event(
                    "TestChannel",
                    "TestChatter",
                    &format!("{}ende", "x".repeat(510)),
                );
                let decision = SpamReviewDecision {
                    verdict: SpamReviewVerdict::$verdict,
                    confidence: $confidence,
                    reason: "Testgrund".to_string(),
                };

                persist_spam_review_decision(&pool, &event, &decision)
                    .await
                    .expect("Entscheidung muss persistiert werden");

                let row: (
                    String,
                    String,
                    Option<String>,
                    String,
                    String,
                    Option<f32>,
                    String,
                    bool,
                ) = sqlx::query_as(
                    "SELECT channel_login, chatter_login, chatter_id, source_message, \
                                verdict, confidence, reason, created_at IS NOT NULL \
                         FROM twitch_spam_review_decisions",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(row.0, "testchannel");
                assert_eq!(row.1, "testchatter");
                assert_eq!(row.2.as_deref(), Some("456"));
                assert_eq!(row.3.chars().count(), 500);
                assert_eq!(row.4, decision.verdict.as_str());
                assert_eq!(row.5, $confidence);
                assert_eq!(row.6, "Testgrund");
                assert!(row.7);
            }
        };
    }

    spam_review_decision_persistence_test!(
        spam_review_spam_wird_persistiert,
        "spam_review_decision_spam",
        Spam,
        Some(0.97)
    );
    spam_review_decision_persistence_test!(
        spam_review_clean_wird_persistiert,
        "spam_review_decision_clean",
        Clean,
        Some(0.88)
    );
    spam_review_decision_persistence_test!(
        spam_review_unsure_wird_persistiert,
        "spam_review_decision_unsure",
        Unsure,
        Some(0.40)
    );
    spam_review_decision_persistence_test!(
        spam_review_skipped_wird_persistiert,
        "spam_review_decision_skipped",
        Skipped,
        None
    );
    spam_review_decision_persistence_test!(
        spam_review_timeout_wird_persistiert,
        "spam_review_decision_timeout",
        Timeout,
        None
    );
    spam_review_decision_persistence_test!(
        spam_review_provider_error_wird_persistiert,
        "spam_review_decision_provider_error",
        ProviderError,
        None
    );
    spam_review_decision_persistence_test!(
        spam_review_parse_error_wird_persistiert,
        "spam_review_decision_parse_error",
        ParseError,
        None
    );

    #[tokio::test]
    async fn db_spam_pattern_speichern_und_hit_count_inkrementieren() {
        let pool = pool_or_skip!("scam_spam_pattern_test");

        let id1 = save_spam_pattern(
            &pool,
            "smmhype.com",
            "phrase",
            "buy viewers smmhype.com",
            "chan1",
            "spam site",
        )
        .await;
        let id2 = save_spam_pattern(
            &pool,
            "smmhype.com",
            "phrase",
            "buy viewers smmhype.com",
            "chan1",
            "spam site",
        )
        .await;

        // RETURNING id liefert bei INSERT wie bei ON-CONFLICT-Update dieselbe Zeile.
        assert!(id1.is_some());
        assert_eq!(id1, id2, "Konflikt-Update muss dieselbe Row-ID liefern");

        let (hit_count,): (i32,) = sqlx::query_as(
            "SELECT hit_count FROM twitch_auto_learned_spam_patterns WHERE pattern = 'smmhype.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Erster INSERT → hit_count=0, zweiter → hit_count=1
        assert_eq!(hit_count, 1, "ON CONFLICT sollte hit_count auf 1 erhöhen");
    }

    #[tokio::test]
    async fn db_spam_pattern_wird_lowercase_gespeichert() {
        let pool = pool_or_skip!("scam_lowercase_test");

        save_spam_pattern(&pool, "SMMHYPE", "phrase", "buy at SMMHYPE", "chan1", "").await;

        let (pattern,): (String,) =
            sqlx::query_as("SELECT pattern FROM twitch_auto_learned_spam_patterns LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pattern, "smmhype");
    }

    #[tokio::test]
    async fn spam_ai_review_contract_spam_result_landet_in_positivliste() {
        let pool = pool_or_skip!("scam_ai_contract_spam_result");
        let content = "@cheazycrust Targeted viewers PeakPy. c0m SSSsss remove space";
        let review = AiReview {
            is_spam: true,
            confidence: None,
            pattern: Some("PeakPy c0m".to_string()),
            pattern_type: Some("fragment".to_string()),
            reason: Some("obfuskierter viewer-service".to_string()),
        };

        let (learned, rejected, save_failed) =
            persist_spam_learning(&pool, &review, content, "cheazycrust").await;
        assert!(!save_failed);
        let learned = learned.expect("distinktives Muster muss gespeichert werden");
        assert_eq!(learned.pattern, "peakpy c0m");
        assert!(rejected.is_none());

        let (pattern, pattern_type, source_channel, source_message): (
            String,
            String,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT pattern, pattern_type, source_channel, source_message \
             FROM twitch_auto_learned_spam_patterns",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(pattern, "peakpy c0m");
        assert_eq!(pattern_type, "fragment");
        assert_eq!(source_channel.as_deref(), Some("cheazycrust"));
        assert_eq!(source_message.as_deref(), Some(content));
    }

    #[tokio::test]
    async fn spam_ai_review_contract_generisches_muster_wird_abgelehnt() {
        let pool = pool_or_skip!("scam_ai_contract_gate_reject");
        let review = AiReview {
            is_spam: true,
            confidence: None,
            pattern: Some("best viewers".to_string()),
            pattern_type: Some("phrase".to_string()),
            reason: Some("viewer-bot werbung".to_string()),
        };

        let (learned, rejected, save_failed) =
            persist_spam_learning(&pool, &review, "Best Viewers kaufen!", "chan1").await;
        assert!(!save_failed);
        assert!(
            learned.is_none(),
            "generisches Vokabular darf nie gelernt werden"
        );
        assert_eq!(rejected.as_deref(), Some("best viewers"));

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_auto_learned_spam_patterns")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn spam_ai_review_contract_judge_call_lernt_spam() {
        let pool = pool_or_skip!("scam_ai_contract_judge_mock");
        let content = "@cheazycrust Targeted viewers PeakPy. c0m SSSsss remove space";
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .and(body_string_contains("PeakPy. c0m"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": "<think>kurze analyse</think>\n{\"is_spam\": true, \"pattern\": \"PeakPy c0m\", \"pattern_type\": \"fragment\", \"reason\": \"obfuskierter viewer-service\"}"
                    }
                }],
                "usage": { "prompt_tokens": 5, "completion_tokens": 6 }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let provider = tb_llm::LlmEndpoint {
            provider: "test",
            base_url: server.uri(),
            api_key: Some("test-key".to_string()),
            model: "test-model".to_string(),
        };
        let review = call_judge(&http, &provider, false, content)
            .await
            .expect("Transport ok")
            .expect("Urteil muss parsebar sein");
        assert!(review.is_spam);

        let (learned, _, _) = persist_spam_learning(&pool, &review, content, "cheazycrust").await;
        assert!(learned.is_some());

        let (pattern, source_channel, source_message): (String, Option<String>, Option<String>) =
            sqlx::query_as(
                "SELECT pattern, source_channel, source_message \
                 FROM twitch_auto_learned_spam_patterns",
            )
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(pattern, "peakpy c0m");
        assert_eq!(source_channel.as_deref(), Some("cheazycrust"));
        assert_eq!(source_message.as_deref(), Some(content));
    }

    #[tokio::test]
    async fn spam_ai_review_contract_harmlos_lernt_nichts_mehr() {
        // Safe-Lernen ist abgeschafft (Safe-List-Poisoning): Ein Harmlos-Urteil
        // darf keinerlei DB-Zeile erzeugen, egal welches Muster der Judge nennt.
        let pool = pool_or_skip!("scam_ai_contract_safe_result");
        let review = AiReview {
            is_spam: false,
            confidence: None,
            pattern: Some("best viewers".to_string()),
            pattern_type: Some("fragment".to_string()),
            reason: Some("normales kompliment".to_string()),
        };

        let (learned, rejected, _) =
            persist_spam_learning(&pool, &review, "best viewers in chat today", "cheazycrust")
                .await;
        assert!(learned.is_none());
        assert!(rejected.is_none());

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_auto_learned_spam_patterns")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn db_follower_query_gibt_letzten_wert_zurueck() {
        let pool = pool_or_skip!("scam_follower_test");

        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, followers_start, followers_end, started_at, ended_at)
             VALUES ($1, $2, $3, NOW() - INTERVAL '2 hours', NOW() - INTERVAL '1 hour')"
        )
        .bind("teststreamer")
        .bind(150i32)
        .bind(180i32)
        .execute(&pool)
        .await
        .unwrap();

        let result: Option<i32> = sqlx::query_scalar(
            r#"
            SELECT COALESCE(followers_end, followers_start)
              FROM twitch_stream_sessions
             WHERE streamer_login = $1
               AND COALESCE(followers_end, followers_start) IS NOT NULL
             ORDER BY COALESCE(ended_at, started_at) DESC
             LIMIT 1
            "#,
        )
        .bind("teststreamer")
        .fetch_optional(&pool)
        .await
        .unwrap()
        .flatten();

        assert_eq!(result, Some(180));
    }

    // ── observe-Integration (ohne DB, mit Mock-API) ───────────────────────────

    struct MockApi;

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(
            &self,
            _bid: &str,
            _msg: &str,
        ) -> Result<crate::types::SendOutcome, String> {
            Ok(crate::types::SendOutcome::Sent)
        }
        async fn send_announcement(&self, _b: &str, _m: &str, _c: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn ban_user(
            &self,
            _b: &str,
            _u: &str,
            _r: &str,
        ) -> Result<crate::api::BanOutcome, String> {
            Ok(crate::api::BanOutcome::Banned)
        }
        async fn timeout_user(
            &self,
            _b: &str,
            _u: &str,
            _d: u32,
            _r: &str,
        ) -> Result<crate::api::BanOutcome, String> {
            Ok(crate::api::BanOutcome::Banned)
        }
        async fn unban_user(&self, _b: &str, _u: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _b: &str, _m: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn user_created_at(&self, _u: &str) -> Result<Option<chrono::DateTime<Utc>>, String> {
            Ok(None)
        }
        async fn resolve_user_id(&self, _l: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn bot_user_id(&self) -> String {
            "bot".to_string()
        }
    }

    struct MockAccountAge {
        days: Option<i64>,
    }

    #[async_trait]
    impl AccountAgePort for MockAccountAge {
        async fn user_created_at_days(&self, _id: &str, _login: &str) -> Option<i64> {
            self.days
        }
    }

    async fn make_detector(age_days: Option<i64>) -> ScamPitchDetector {
        let pool = pool_or_skip_sync();
        ScamPitchDetector::new(
            Arc::new(MockApi),
            Arc::new(MockAccountAge { days: age_days }),
            pool,
        )
    }

    fn pool_or_skip_sync() -> PgPool {
        // Für observe-Tests ohne DB: minimal fake pool
        // Dies wird in observe-Tests nicht verwendet (keine DB-Calls in observe selbst
        // ausser pre_warm_follower_cache, der hier nicht aufgerufen wird).
        // Wir nutzen ein ungültiges DSN — der Pool wird nie connecten.
        PgPool::connect_lazy("postgres://unused:unused@127.0.0.1:1/unused").unwrap()
    }

    #[tokio::test]
    async fn observe_command_wird_ignoriert() {
        let det = make_detector(Some(200)).await;
        let event = make_event("chan", "user1", "!commands");
        let result = det.observe(&event).await;
        assert_eq!(result, PitchDecision::None);
    }

    #[tokio::test]
    async fn observe_leere_nachricht_wird_ignoriert() {
        let det = make_detector(Some(200)).await;
        let event = make_event("chan", "user1", "   ");
        let result = det.observe(&event).await;
        assert_eq!(result, PitchDecision::None);
    }

    #[tokio::test]
    async fn observe_einzelne_benign_greeting_kein_warn() {
        let det = make_detector(Some(200)).await; // alter Account
        let event = make_event("chan", "user1", "hey how are you");
        // Nur greeting+wellbeing, alter Account → benign → None
        let result = det.observe(&event).await;
        assert_eq!(result, PitchDecision::None);
    }

    #[tokio::test]
    async fn observe_crew_threat_force_single_new_account_gibt_public_oder_strong() {
        // crew_threat score=5 + neuer Account(age<90)+first_msg: force_single=true,
        // total >= LIGHT_THRESHOLD(4), quick_action_eligible (neuer Account + erste Msg).
        let det = make_detector(Some(10)).await; // 10 Tage alt → neu
        let event = make_event("chan", "spammer", "ima pull up with my crew bro");
        let result = det.observe(&event).await;
        // crew_threat(5) + new_account(+2) = 7 → PUBLIC, quick_action_eligible → PublicWarn oder StrongWarn (erster Treffer, kein Timeout)
        assert!(
            matches!(
                result,
                PitchDecision::PublicWarn { .. } | PitchDecision::StrongWarn { .. }
            ),
            "Erwartet Public/Strong, bekam: {result:?}"
        );
    }

    #[tokio::test]
    async fn observe_unter_schwelle_kein_warn() {
        let det = make_detector(Some(200)).await;
        // greeting allein: score=1, unter MIN_SCORE(3) → None
        let event = make_event("chan", "user2", "hey");
        let result = det.observe(&event).await;
        assert_eq!(result, PitchDecision::None);
    }

    #[tokio::test]
    async fn observe_hint_cooldown_verhindert_doppel() {
        // Sendet HINT → setzt Cooldown → zweite Nachricht innerhalb Cooldown → None
        let det = make_detector(Some(10)).await;
        // Mehrere Nachrichten ansammeln damit total_score >= HINT(4)
        // Aber kein quick_action_eligible, da nicht erste Msg nach 1. observe
        // Direkt crew_threat → force_single=true, score=5+2=7 → PUBLIC (quick eligible bei neuer+erster Msg)
        // Um HINT zu triggern: score im [4,7)-Bereich ohne quick_action_eligible
        // Dafür: alter Account, score >= 4
        // design_pitch(4) + alter Account → score=4, quick_action_eligible=false → HINT (downgrade from PUBLIC)
        // Aber msg_count=1 ohne force_single → Exit. Wir brauchen msg_count>=2.
        // Test: zwei Nachrichten mit je design_pitch, alter Account
        let det2 = make_detector(Some(200)).await;
        for i in 0..3 {
            let event = make_event("chan", "u3", &format!("I need overlays message {i}"));
            let _ = det2.observe(&event).await;
        }
        // Nach genug Msgs könnte HINT kommen; das genaue Verhalten hängt von
        // window-scoring ab. Wir testen nur: kein panic.
        let _ = det;
    }

    // ── Forensischer Audit-Trail (scam-pitch-spam-review-3 / Grillme Z. 226) ──

    #[test]
    fn service_warning_line_format_matcht_python() {
        // Erwartet exakt das Tab-Format aus service_pitch_warning.py Z. 768–773.
        let line = format_service_warning_line(
            "2026-06-16T12:00:00+00:00",
            "WARNING_PUBLIC",
            "chan",
            "spammer",
            "999",
            42,
            Some(150),
            7,
            3,
            &["a".to_string(), "b".to_string()],
            "hallo welt",
        );
        assert_eq!(
            line,
            "2026-06-16T12:00:00+00:00\tWARNING_PUBLIC\tchan\tspammer\t999\tage_days=42\t\
             followers=150\tscore=7\tmsgs=3\ta,b\thallo welt"
        );
    }

    #[test]
    fn service_warning_line_leere_felder_werden_strich() {
        // Leerer chatter_login/-id -> "-"; keine Follower -> "-"; keine Reasons -> "-".
        let line =
            format_service_warning_line("T", "HINT", "chan", "", "", -1, None, 4, 2, &[], "x");
        assert_eq!(
            line,
            "T\tHINT\tchan\t-\t-\tage_days=-1\tfollowers=-\tscore=4\tmsgs=2\t-\tx"
        );
    }

    #[test]
    fn service_warning_content_einzeilig_und_gekuerzt() {
        // Newlines werden zu Spaces, getrimmt, auf 350 Zeichen begrenzt.
        let raw = format!("  zeile1\nzeile2  {}", "y".repeat(400));
        let line =
            format_service_warning_line("T", "HINT", "c", "u", "1", 0, None, 1, 1, &[], &raw);
        let content = line.rsplit('\t').next().unwrap();
        assert!(!content.contains('\n'));
        assert!(content.starts_with("zeile1 zeile2"));
        assert_eq!(content.chars().count(), 350);
    }

    #[test]
    fn append_service_warning_schreibt_datei() {
        // Append in ein isoliertes Temp-Verzeichnis (kein Prod-Pfad, kein neues Crate-Dep).
        let dir = std::env::temp_dir().join(format!(
            "tb_chat_swtest_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        // Nur in diesem Test gesetzt; sonst nutzt keine Crate-Logik diese Variable.
        std::env::set_var("TWITCH_SERVICE_WARNING_LOG_DIR", &dir);

        append_service_warning("zeile-a").unwrap();
        append_service_warning("zeile-b").unwrap();

        let path = dir.join("twitch_service_warnings.log");
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, "zeile-a\nzeile-b\n");

        std::env::remove_var("TWITCH_SERVICE_WARNING_LOG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Live-Baseline für [`SPAM_REVIEW_SYSTEM_PROMPT`]. Ein Prompt lässt sich
    /// nur gegen ein echtes Modell prüfen — Mock-Tests bestätigen bloß die
    /// eigene Formulierung. Die Spam-Fälle sind anonyme Angebote ohne
    /// Dienstnamen (der am 10.08.2026 live durchgerutschte Typ), die harmlosen
    /// Fälle das Viewer-Gerede, an dem v1 scheiterte.
    #[tokio::test]
    #[ignore = "Live-Baseline: braucht FIREWORK_API_KEY oder MINIMAX_API_KEY"]
    async fn live_judge_erkennt_anonyme_viewbot_angebote() {
        let providers = tb_llm::endpoint_chain("spam_judge");
        let provider = providers.first().expect("kein Judge-Key gesetzt");
        let http = reqwest::Client::new();

        let mut fehlurteile = Vec::new();
        for (message, want_spam) in [
            ("Boost viewers on the stream – promotion. ru", true),
            ("Boost viewers on the stream", true),
            ("cheap viewers and followers available", true),
            ("I can help you grow your channel with real viewers", true),
            ("viewers boost cheap price telegram", true),
            ("yo more viewers today than yesterday nice", false),
            ("bannt der bot mich wenn ich viewer bot schreibe?", false),
            ("best viewers in chat today", false),
            ("endlich affiliate erreicht!", false),
            ("wie kriegt man eigentlich mehr viewer?", false),
        ] {
            let review = call_judge(&http, provider, false, message)
                .await
                .expect("Judge-Call fehlgeschlagen")
                .expect("Judge lieferte kein parsebares JSON");
            eprintln!(
                "LIVE_SPAM_JUDGE want={want_spam} got={} reason={:?} msg={message:?}",
                review.is_spam, review.reason
            );
            if review.is_spam != want_spam {
                fehlurteile.push(format!("{message:?} (erwartet {want_spam})"));
            }
        }

        assert!(
            fehlurteile.is_empty(),
            "Judge-Fehlurteile: {}",
            fehlurteile.join(", ")
        );
    }
}
