use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use tb_engagement::minimax_chat::EngagementMinimaxClient;
use tokio::sync::Mutex;
use tracing::{debug, warn};
use unicode_normalization::UnicodeNormalization;

use crate::api::{BanOutcome, ChatApi};
use crate::mention_scoring::WHITELISTED_BOTS;
use crate::moderation::{AutoBanRequest, ModerationEngine};
use crate::types::ChatMessageEvent;

const SCAM_JUDGE_SYSTEM_PROMPT: &str = r#"Du bist ein wachsamer, aber besonnener Chat-Moderator für den Twitch-Kanal eines deutschsprachigen Deadlock-Streamers. Du beurteilst, ob ein ERSTSCHREIBER eine aufgesetzte, betrügerische Konversation führt — oder ob es einfach ein ganz normaler Zuschauer ist. Im Zweifel ist es ein normaler Zuschauer.

Du bekommst die Nachrichten EINES Chatters nacheinander als JSON-Objekte mit den Feldern "message" (der Text), "is_first_global" (true = dieser Chatter wurde im ganzen Netzwerk noch nie gesehen) und "unicode_obfuscation_detected" (true = die Schrift war verfremdet). Bewerte immer den GESAMTEN bisherigen Verlauf.

DAS WICHTIGSTE MERKMAL — die Sprache:
Diese Betrüger arbeiten ein auswendig gelerntes Skript ab und sind fast nie deutsche Muttersprachler. Sie schreiben Englisch ODER sichtbar maschinell übersetztes, steifes, leicht falsches Deutsch — typische Übersetzer-Spuren sind unnatürliche Wortstellung, gestelzte Höflichkeit, fehlende Füllwörter, wörtlich übersetzte Wendungen ("antworte mir auf Discord", "reply me").
Flüssiges, lockeres, umgangssprachliches Deutsch kann ein fremdsprachiger Scammer mit Übersetzer NICHT erzeugen. Wenn der Chatter natürliche deutsche Alltags- und Jugendsprache schreibt — Slang und Abkürzungen ("brudi", "digga", "was geht", "läuft", "zocken", "hdf", "wsg"), Füllpartikeln ("ja", "mal", "halt", "eh", "doch", "grad"), regionale oder flapsige Schreibweise, kleine Schludrigkeiten — dann ist das sehr wahrscheinlich ein ECHTER deutscher Zuschauer. Das ist ein STARKES "clean"-Signal und wiegt schwerer als oberflächliche Ähnlichkeit mit einem Skript. Bei flüssigem Umgangs-Deutsch lautet dein Urteil "clean" oder höchstens "unsure" — NIEMALS "scam" mit hoher confidence.

DISCORD / WOANDERS WEITERREDEN IST FÜR SICH HARMLOS:
Dieser Kanal hat eine eigene, aktive Discord-Community, und Zuschauer werden ausdrücklich eingeladen, dorthin zu kommen. Ein Chatter, der "lass uns mal auf Discord schreiben", "bin im Discord", "adden wir uns" oder Ähnliches sagt, ist deshalb NICHT verdächtig — das ist hier das Normalste der Welt. Ein Discord- oder Off-Platform-Hinweis zählt NUR dann als Warnzeichen, wenn er zusammen mit den unten genannten Skript-Merkmalen UND fremdsprachig/übersetzt auftritt und jeder echte Bezug fehlt. Allein, und erst recht in lockerem Deutsch, ist er kein Pivot.

ECHTER KONTEXT SCHLÄGT SKRIPT-VERDACHT:
Echte Zuschauer haben echten Bezug: sie nennen konkrete, plausible gemeinsame Vorgeschichte, reagieren auf den Stream oder das Spiel, erwähnen reale Details. Dass jemand herzlich ist ("Kuss brudi"), einen Sub verspricht, von einem Zweit- oder Troll-Account schreibt oder sich auf ein früheres Treffen bezieht, ist in deutschen Gaming-Communities völlig normal und KEINE Masche. Eine Masche erkennt man nicht an Freundlichkeit oder einem Sub-Versprechen, sondern am leeren, gesichtslosen Skript ohne jeden echten Bezug.

DIE DREI ECHTEN MASCHEN (typischerweise englisch oder übersetzt):
1) Beziehungs- und Vertrauens-Masche: generischer Beziehungsaufbau OHNE echten Spielbezug ("Heya", "how's your day been?", "Welcome back <3"), übertriebenes Dauerlob ohne Anlass ("you have good taste", "you deserve it"), Ausfrage-Fragen (Wohnort, Job, Alter), am Ende der Pivot weg von Twitch.
2) Wachstums- und Clout-Pitch (oft eine einzige lange Nachricht): unaufgefordertes Angebot, deinen Kanal "wachsen" zu lassen oder dich mit einem "großen Streamer" zu verbinden, geködert mit "real viewers, active chat, supporters who donate and sub", und der Aufforderung "add him on Discord … tell him X sent you". Oft in verfremdeter Schrift.
3) Ausreden- und Sofort-Pivot-Masche: ein Erstschreiber behauptet ohne Anlass ein technisches Problem ("my headphones aren't working", "can't hear the stream") und drängt sofort woandershin ("reply me on Discord", "dm me", "add me"). Verräterisch sind die gebrochene Scammer-Grammatik und der fehlende echte Stream- oder Spielbezug.

KLARER BEFRIENDING-PIVOT IN EINER EINZIGEN NACHRICHT:
Wenn ein am selben Tag erstellter Account ("account_age_days": 0) in seiner ersten Nachricht auf Englisch generisches Stream-Lob, ein vages Beziehungsangebot wie zusammen spielen oder Tipps teilen UND einen direkten Discord-Pivot kombiniert, ohne irgendein konkretes Detail zum Spiel oder laufenden Stream zu nennen, ist das die vollständige Beziehungs- und Vertrauens-Masche. Urteile dann "scam" mit hoher confidence; warte nicht auf einen Link oder weitere Nachrichten und stufe den Fall nicht nur wegen der kurzen Historie als "unsure" ein. Diese Regel gilt nur für die Kombination ALLER genannten Merkmale. Discord allein, natürliches Deutsch, ein konkreter Spiel-/Stream-Bezug oder ein nicht brandneuer Account reichen dafür ausdrücklich nicht.

GEWICHTUNG:
- Sprache ist das stärkste Signal: Englisch oder übersetztes Deutsch + Skript ohne Bezug = verdächtig. Flüssiges Umgangs-Deutsch = clean.
- "unicode_obfuscation_detected": true (verfremdete Schrift, um Filter zu täuschen) ist ein echtes Warnsignal.
- Ein junger Account ("account_age_days" unter 90) ist ein deutliches Warnsignal, wenn dazu englischer Script-Smalltalk, Druckaufbau oder ein Pivot kommt. Allein reicht das junge Alter nicht für "scam".
- "is_first_global": true erhöht den Verdacht nur LEICHT und nur zusammen mit den Skript-Merkmalen — ein neuer oder Zweit-Account allein ist normal.

NETZWERK-SIGNAL — DERSELBE ACCOUNT IN MEHREREN KANÄLEN:
Du bekommst zusätzlich die Felder "account_age_days" (Alter des Twitch-Accounts in Tagen, null = unbekannt) und "other_channels_last_hour" (Anzahl ANDERER Kanäle im Netzwerk, in denen dieser Account in der letzten Stunde ebenfalls zum ersten Mal geschrieben hat).
Ein echter Zuschauer, der einen Streamer entdeckt, taucht in genau EINEM Kanal auf. Wer binnen einer Stunde bei mehreren verschiedenen Streamern gleichzeitig neu im Chat steht und überall dieselbe generische Eröffnung fährt ("hey, how are you?"), arbeitet eine Liste ab. Das ist keine Neugier, das ist ein Skript.

WARTE NICHT AUF DEN PIVOT:
Bei der Beziehungs- und Vertrauens-Masche passiert der eigentliche Betrug fast nie im öffentlichen Chat, sondern später per Whisper oder Discord-Direktnachricht. Im Chat siehst du nur den Aufbau: Begrüßung, Dauerlob, Ausfragen, Füllsätze. Wenn du auf einen sichtbaren Pivot wartest, kommt er nie, und der Betrüger hat sein Ziel längst privat angeschrieben.
Deshalb gilt: Wenn "other_channels_last_hour" mindestens 1 ist UND der Account frisch ist ("account_age_days" unter 90) UND die Sprache ins Skript-Muster passt (Englisch oder übersetztes Deutsch, kein echter Stream- oder Spielbezug), dann IST das die Masche. Urteile "scam" mit hoher confidence, auch wenn noch kein Discord-Link, kein Wachstums-Angebot und keine Ausrede gefallen ist. Der leere, gesichtslose Aufbau ist hier selbst der Beweis.

DIE GEGENSIGNALE BLEIBEN STÄRKER:
Flüssiges deutsches Umgangsdeutsch, echter Bezug zum Stream oder zum Spiel, ein alter Account ("account_age_days" deutlich über 90) oder eine plausible gemeinsame Vorgeschichte machen den Chatter "clean" — auch dann, wenn er in mehreren Kanälen unterwegs ist. Ein deutscher Zuschauer, der mehreren Deadlock-Streamern folgt und überall mal Hallo sagt, ist völlig normal und wird NICHT gebannt.

URTEILSDISZIPLIN:
Stufe nur dann als "scam" mit hoher confidence ein, wenn das fremdsprachige oder übersetzte Skript klar erkennbar ist UND echter Bezug fehlt. Reicht der Verlauf dafür nicht, antworte "unsure". Echte oder natürlich-deutschsprachige Zuschauer sind "clean". Lass deine confidence NICHT allein deshalb steigen, weil ein harmloses Gespräch weitergeht; bewerte jede Nachricht neu am realen Inhalt und behandle deine eigenen früheren Verdachtsmomente NICHT als Beweis.

MUSTER FUER DEN VORFILTER:
Urteilst du "scam", gib in "pattern" den kuerzesten woertlichen Ausschnitt der Nachricht an, an dem die Masche haengt: einen Dienstnamen, eine Domain oder ein Skript-Token wie "stream_promotion_bot". Regeln: hoechstens zwei Woerter, mindestens vier Zeichen, in normaler lateinischer Schrift (verfremdete Zeichen vorher zurueckuebersetzen), und der Ausschnitt muss genau so in der Nachricht stehen. Allgemeines Chat-Vokabular wie "viewers", "promotion", "free" oder "stream" ist kein Muster. Findest du keinen solchen Ausschnitt, lass das Feld leer. Bei "clean" und "unsure" ist das Feld immer leer.

Antworte AUSSCHLIESSLICH mit einem einzigen JSON-Objekt, ohne Markdown und ohne weiteren Text:
{"verdict":"scam"|"clean"|"unsure","confidence":<Zahl 0.0 bis 1.0>,"category":"<kurzes Label, z.B. befriending_pivot, growth_pitch, excuse_pivot, recon_smalltalk>","pattern":"<siehe MUSTER FUER DEN VORFILTER, sonst leerer String>","reasoning":"<2 bis 4 Sätze auf Deutsch, allgemeinverständlich für einen unerfahrenen Streamer: WARUM ist das verdächtig oder unverdächtig? Benenne die konkreten Auffälligkeiten aus dem Verlauf. Kein Fachjargon, keine Zahlen.>"}"#;
const TIMEOUT_SECONDS: u32 = 600;
/// Account gilt als "neu" unter dieser Tagesgrenze (konsistent mit scam_pitch::ACCOUNT_MAX_DAYS = 90).
const ACCOUNT_NEW_MAX_DAYS: i64 = 90;
/// Bei sehr jungen Accounts steigt die AutoBan-Schwelle linear von 80 Prozent
/// am Erstellungstag bis zur regulären Kanalschwelle nach 60 Tagen.
const YOUNG_ACCOUNT_CURVE_DAYS: i64 = 60;
const YOUNG_ACCOUNT_START_THRESHOLD: f32 = 0.80;
const SUBSTANTIAL_MESSAGE_TARGET: usize = 3;
const CROSS_CHANNEL_WINDOW_MINUTES: i64 = 60;
const CONVERSATION_SCAM_GLOBAL_BAN_ADDED_BY: &str = "conversation_scam_ai";
/// Ab dieser Konfidenz darf ein Scam-Urteil ein Muster in den Vorfilter
/// schreiben. Ein gelerntes Muster ist ein hartes Signal und wirkt in allen
/// Kanaelen, deshalb liegt die Schwelle ueber jeder Ban-Schwelle.
const LEARN_MIN_CONFIDENCE: f32 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictKind {
    Scam,
    Clean,
    Unsure,
}

impl VerdictKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Scam => "scam",
            Self::Clean => "clean",
            Self::Unsure => "unsure",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub verdict: VerdictKind,
    pub confidence: f32,
    pub category: String,
    pub reasoning: String,
    /// Woertlicher Ausschnitt, an dem die Masche haengt. Nur bei "scam"
    /// gefuellt und nur, wenn der Judge einen findet.
    pub pattern: Option<String>,
}

impl Verdict {
    pub fn unsure() -> Self {
        Self {
            verdict: VerdictKind::Unsure,
            confidence: 0.0,
            category: String::new(),
            reasoning: String::new(),
            pattern: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardMode {
    AutoBan,
    Timeout,
    AlertOnly,
}

impl GuardMode {
    fn from_db(value: &str) -> Self {
        match value {
            "timeout" => Self::Timeout,
            "alert_only" => Self::AlertOnly,
            _ => Self::AutoBan,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardSettings {
    pub enabled: bool,
    pub mode: GuardMode,
    pub threshold: f32,
    pub suggestion_floor: f32,
}

impl Default for GuardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: GuardMode::AutoBan,
            threshold: 0.90,
            suggestion_floor: 0.70,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstTimeContext {
    pub is_first_time_streamer: bool,
    pub is_first_global: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug)]
pub struct DialogState {
    messages: Vec<DialogMessage>,
    transcript: Vec<String>,
    substantial_messages: usize,
    single_message_pitch: bool,
    is_first_global: bool,
    obfuscation_detected: bool,
    account_age_days: Option<i64>,
    other_channels_last_hour: i64,
    channel_login: String,
    chatter_login: String,
    completed: bool,
}

impl DialogState {
    pub fn new(is_first_global: bool) -> Self {
        Self::with_context(is_first_global, None, 0, None, "", "")
    }

    /// Wie [`DialogState::new`], hängt aber die netzwerkweit destillierten
    /// Self-Learning-Erkenntnisse als Zusatzhinweis an den System-Prompt an
    /// (`None`/leer → unverändert).
    pub fn with_learnings(is_first_global: bool, learnings: Option<&str>) -> Self {
        Self::with_context(is_first_global, None, 0, learnings, "", "")
    }

    fn with_context(
        is_first_global: bool,
        account_age_days: Option<i64>,
        other_channels_last_hour: i64,
        learnings: Option<&str>,
        channel_login: &str,
        chatter_login: &str,
    ) -> Self {
        let mut system = SCAM_JUDGE_SYSTEM_PROMPT.to_string();
        if let Some(learnings) = learnings.map(str::trim).filter(|l| !l.is_empty()) {
            system.push_str(
                "\n\nZusätzliche Erkenntnisse aus zuletzt bestätigten Fällen und \
                 aufgehobenen Fehlalarmen (als Hilfestellung — sie ersetzen dein \
                 eigenes Urteil nicht):\n",
            );
            system.push_str(learnings);
        }
        Self {
            messages: vec![DialogMessage {
                role: "system".to_string(),
                content: system,
            }],
            transcript: Vec::new(),
            substantial_messages: 0,
            single_message_pitch: false,
            is_first_global,
            obfuscation_detected: false,
            account_age_days,
            other_channels_last_hour,
            channel_login: channel_login.to_string(),
            chatter_login: chatter_login.to_string(),
            completed: false,
        }
    }

    pub fn push_user_message(&mut self, text: &str) {
        let normalized = normalize_for_judge(text);
        if normalized.text.is_empty() {
            return;
        }
        if is_substantial(&normalized.text) {
            self.substantial_messages += 1;
        }
        if is_single_message_pitch(&normalized.text) {
            self.single_message_pitch = true;
        }
        self.obfuscation_detected |= normalized.was_obfuscated;
        self.transcript.push(normalized.text.clone());

        let content = serde_json::json!({
            "message": normalized.text,
            "is_first_global": self.is_first_global,
            "unicode_obfuscation_detected": self.obfuscation_detected,
            "account_age_days": self.account_age_days,
            "other_channels_last_hour": self.other_channels_last_hour,
        })
        .to_string();
        self.messages.push(DialogMessage {
            role: "user".to_string(),
            content,
        });
    }

    pub fn has_enough_substance(&self) -> bool {
        self.other_channels_last_hour >= 1
            || self.single_message_pitch
            || self.substantial_messages >= SUBSTANTIAL_MESSAGE_TARGET
    }

    pub fn messages(&self) -> &[DialogMessage] {
        &self.messages
    }

    fn append_assistant(&mut self, content: String) {
        self.messages.push(DialogMessage {
            role: "assistant".to_string(),
            content,
        });
    }

    fn transcript_snapshot(&self) -> String {
        serde_json::to_string(&self.transcript).unwrap_or_else(|_| "[]".to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedText {
    pub text: String,
    pub was_obfuscated: bool,
}

pub fn normalize_for_judge(input: &str) -> NormalizedText {
    let normalized: String = input
        .nfkc()
        .map(|ch| match ch {
            'ᴀ' => 'a',
            'ʙ' => 'b',
            'ᴄ' => 'c',
            'ᴅ' => 'd',
            'ᴇ' => 'e',
            'ꜰ' => 'f',
            'ɢ' => 'g',
            'ʜ' => 'h',
            'ɪ' => 'i',
            'ᴊ' => 'j',
            'ᴋ' => 'k',
            'ʟ' => 'l',
            'ᴍ' => 'm',
            'ɴ' => 'n',
            'ᴏ' => 'o',
            'ᴘ' => 'p',
            'ʀ' => 'r',
            'ꜱ' => 's',
            'ᴛ' => 't',
            'ᴜ' => 'u',
            'ᴠ' => 'v',
            'ᴡ' => 'w',
            'ʏ' => 'y',
            'ᴢ' => 'z',
            'а' => 'a',
            'е' => 'e',
            'о' => 'o',
            'р' => 'p',
            'с' => 'c',
            'х' => 'x',
            'у' => 'y',
            other => other,
        })
        .collect();
    NormalizedText {
        was_obfuscated: normalized != input,
        text: normalized,
    }
}

fn is_substantial(text: &str) -> bool {
    let words = text
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .count();
    words >= 4 && text.chars().count() >= 18
}

fn is_single_message_pitch(text: &str) -> bool {
    if !is_substantial(text) {
        return false;
    }
    let lower = text.to_lowercase();
    text.chars().count() >= 80
        || [
            "add him",
            "add me",
            "grow",
            "real viewers",
            "donate and sub",
            "connect with",
            "talk on chat",
            "reply me",
            "dm me",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
}

pub fn should_consider_event(
    event: &ChatMessageEvent,
    bot_user_id: &str,
    context: FirstTimeContext,
) -> bool {
    if !context.is_first_time_streamer
        || event.chatter_user_id == bot_user_id
        || WHITELISTED_BOTS.contains(&event.chatter_user_login.to_lowercase().as_str())
    {
        return false;
    }
    !event.badges.iter().any(|badge| {
        matches!(
            badge.set_id.as_str(),
            "moderator" | "vip" | "subscriber" | "broadcaster"
        )
    })
}

#[derive(Debug, Deserialize)]
struct RawVerdict {
    verdict: String,
    confidence: f32,
    category: String,
    reasoning: String,
    /// Aeltere Judge-Antworten kennen das Feld nicht — fehlend ist kein Fehler.
    #[serde(default)]
    pattern: Option<String>,
}

fn parse_verdict(raw: &str, channel: &str, chatter: &str) -> Verdict {
    match parse_verdict_result(raw) {
        Ok(verdict) => verdict,
        Err(reason) => {
            warn!(
                reason,
                channel, chatter, "Conversation-Scam-Judge-Antwort unbrauchbar"
            );
            Verdict::unsure()
        }
    }
}

fn parse_verdict_result(raw: &str) -> Result<Verdict, &'static str> {
    if raw.trim().is_empty() {
        return Err("empty_response");
    }
    let parsed = serde_json::from_str::<RawVerdict>(raw.trim()).or_else(|_| {
        extract_json_object(raw)
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing JSON object")))
            .and_then(serde_json::from_str::<RawVerdict>)
    });
    let parsed = parsed.map_err(|_| "parse_error")?;
    let kind = match parsed.verdict.as_str() {
        "scam" => VerdictKind::Scam,
        "clean" => VerdictKind::Clean,
        "unsure" => VerdictKind::Unsure,
        _ => return Err("invalid_verdict"),
    };
    if !parsed.confidence.is_finite() {
        return Err("invalid_confidence");
    }
    let pattern = parsed
        .pattern
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty() && kind == VerdictKind::Scam);
    Ok(Verdict {
        verdict: kind,
        confidence: parsed.confidence.clamp(0.0, 1.0),
        category: parsed.category,
        reasoning: parsed.reasoning,
        pattern,
    })
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    for start in raw.match_indices('{').map(|(index, _)| index) {
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (offset, byte) in bytes[start..].iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return raw.get(start..=start + offset);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[async_trait]
pub trait ScamJudge: Send + Sync {
    async fn judge(&self, dialog: &mut DialogState) -> Verdict;
}

pub struct MiniMaxScamJudge {
    client: EngagementMinimaxClient,
}

impl MiniMaxScamJudge {
    pub fn new(client: EngagementMinimaxClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ScamJudge for MiniMaxScamJudge {
    async fn judge(&self, dialog: &mut DialogState) -> Verdict {
        let messages = Value::Array(
            dialog
                .messages
                .iter()
                .map(|message| {
                    serde_json::json!({
                        "role": message.role,
                        "content": message.content,
                    })
                })
                .collect(),
        );
        match self
            .client
            .messages_completion_uncapped(messages, 0.0)
            .await
        {
            Ok(raw) => {
                dialog.append_assistant(raw.clone());
                parse_verdict(&raw, &dialog.channel_login, &dialog.chatter_login)
            }
            Err(error) => {
                warn!(
                    reason = "llm_error",
                    channel = %dialog.channel_login,
                    chatter = %dialog.chatter_login,
                    %error,
                    "Conversation-Scam-Judge nicht verfügbar"
                );
                Verdict::unsure()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerdictRecord {
    pub channel_login: String,
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub verdict: VerdictKind,
    pub confidence: f32,
    pub category: String,
    pub reasoning: String,
    pub transcript_snapshot: String,
    pub action_taken: String,
}

#[async_trait]
pub trait ScamGuardStore: Send + Sync {
    /// `channel_user_id` ist die stabile Kanal-ID aus dem EventSub-Payload.
    /// Ohne sie fiele der Scam-Schutz aus, sobald ein Kanal umbenannt wird und
    /// seine Settings-Zeile noch den alten Namen trägt — und ein stummer
    /// Schutz sieht von außen aus wie ein Kanal ohne Vorfälle.
    async fn load_settings(
        &self,
        channel_login: &str,
        channel_user_id: Option<&str>,
    ) -> Result<GuardSettings, String>;
    async fn first_time_context(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<FirstTimeContext>, String>;
    async fn cross_channel_first_messages(
        &self,
        _channel_login: &str,
        _chatter_login: &str,
        _window_minutes: i64,
    ) -> Result<i64, String> {
        Ok(0)
    }
    /// Persistiert das Urteil und liefert dessen neue ID (für den Discord-Feed).
    async fn persist_verdict(&self, record: &VerdictRecord) -> Result<i64, String>;

    /// Netzwerkweit destillierte Self-Learning-Erkenntnisse (oder `None`, solange
    /// noch keine vorliegen). Default: keine — Mocks müssen nichts liefern.
    async fn load_learnings(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Schreibt ein vom Judge belegtes Muster in die gelernten Spam-Muster.
    /// `Ok(Some(muster))` heisst gespeichert, `Ok(None)` heisst von den Gates
    /// abgelehnt. Default: no-op, damit Mocks nichts lernen muessen.
    async fn learn_spam_pattern(
        &self,
        _pattern: &str,
        _source_message: &str,
        _channel_login: &str,
        _reasoning: &str,
    ) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Klare frische AI-Scams zusätzlich global vormerken, damit fehlende
    /// Mod-Rechte in einem einzelnen Kanal den Schutz nicht blockieren.
    async fn add_global_ban(
        &self,
        _chatter_login: &str,
        _chatter_id: Option<&str>,
        _reason: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
pub trait ScamModeration: Send + Sync {
    async fn auto_ban_and_cleanup(&self, request: AutoBanRequest<'_>) -> bool;
    #[allow(clippy::too_many_arguments)]
    async fn timeout_and_cleanup(
        &self,
        channel_login: Option<&str>,
        broadcaster_id: &str,
        chatter_login: Option<&str>,
        chatter_id: &str,
        message_id: &str,
        content: Option<&str>,
        duration_secs: u32,
        reason_text: &str,
    ) -> bool;
}

#[async_trait]
impl ScamModeration for ModerationEngine {
    async fn auto_ban_and_cleanup(&self, request: AutoBanRequest<'_>) -> bool {
        ModerationEngine::auto_ban_and_cleanup(self, request).await
    }

    async fn timeout_and_cleanup(
        &self,
        channel_login: Option<&str>,
        broadcaster_id: &str,
        chatter_login: Option<&str>,
        chatter_id: &str,
        message_id: &str,
        content: Option<&str>,
        duration_secs: u32,
        reason_text: &str,
    ) -> bool {
        ModerationEngine::timeout_and_cleanup(
            self,
            channel_login,
            broadcaster_id,
            chatter_login,
            chatter_id,
            message_id,
            content,
            duration_secs,
            reason_text,
        )
        .await
    }
}

/// Benachrichtigung über eine getroffene Wächter-Entscheidung — Datengrundlage
/// für den Discord-Feed (Sichtbarkeit + Revoke-Button). Bewusst sprach- und
/// layoutfrei; die konkrete deutsche Darstellung (Embed + `scam_revoke`-
/// view_spec) liegt im Notifier in der tb-bot-Composition-Root.
#[derive(Debug, Clone)]
pub struct ScamNotification {
    pub verdict_id: i64,
    pub channel_login: String,
    pub chatter_login: String,
    pub category: String,
    pub reasoning: String,
    pub confidence: f32,
    pub verdict: String,
    pub action_taken: String,
}

/// Port für die Discord-Sichtbarkeit (Dependency-Inversion: tb-chat kennt kein
/// Discord). Implementierung in tb-bot über das DiscordBackend/den Broker.
#[async_trait]
pub trait ScamGuardNotifier: Send + Sync {
    async fn notify(&self, notification: ScamNotification);
}

struct PgScamGuardStore {
    pool: PgPool,
}

impl PgScamGuardStore {
    fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ScamGuardStore for PgScamGuardStore {
    async fn load_settings(
        &self,
        channel_login: &str,
        channel_user_id: Option<&str>,
    ) -> Result<GuardSettings, String> {
        // Wie in tb-engagement::gate: ID trifft und überlebt die Umbenennung,
        // ohne Aufrufer-ID oder bei einer Zeile ohne ID greift der Login.
        let row = sqlx::query!(
            "SELECT enabled AS \"enabled!\", \
                    mode AS \"mode!\", \
                    threshold AS \"threshold!\", \
                    suggestion_floor AS \"suggestion_floor!\" \
             FROM twitch_scam_guard_settings \
              WHERE channel_user_id = $2 \
                 OR (channel_login = $1 AND (channel_user_id IS NULL OR $2::text IS NULL)) \
              ORDER BY (channel_user_id = $2) DESC NULLS LAST \
              LIMIT 1",
            channel_login,
            channel_user_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(match row {
            Some(row) => GuardSettings {
                enabled: row.enabled,
                mode: GuardMode::from_db(&row.mode),
                threshold: row.threshold,
                suggestion_floor: row.suggestion_floor,
            },
            None => GuardSettings::default(),
        })
    }

    async fn first_time_context(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<FirstTimeContext>, String> {
        let session_value = sqlx::query_scalar!(
            "SELECT COALESCE(sc.is_first_time_streamer, FALSE) AS \"is_first_time_streamer!\" \
             FROM twitch_session_chatters sc \
             JOIN twitch_stream_sessions ss ON ss.id = sc.session_id \
             WHERE LOWER(sc.streamer_login) = $1 \
               AND LOWER(sc.chatter_login) = $2 \
               AND ss.ended_at IS NULL \
             ORDER BY ss.started_at DESC LIMIT 1",
            channel_login,
            chatter_login,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;

        let is_first_time_streamer = match session_value {
            Some(value) => value,
            None => sqlx::query_scalar!(
                "SELECT NOT EXISTS ( \
                       SELECT 1 FROM twitch_chatter_rollup \
                       WHERE LOWER(streamer_login) = $1 AND LOWER(chatter_login) = $2 \
                     ) AS \"is_first_time_streamer!\"",
                channel_login,
                chatter_login,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|error| error.to_string())?,
        };
        if !is_first_time_streamer {
            return Ok(Some(FirstTimeContext {
                is_first_time_streamer: false,
                is_first_global: false,
            }));
        }

        let is_first_global = sqlx::query_scalar!(
            "SELECT NOT EXISTS ( \
               SELECT 1 FROM twitch_chatter_rollup WHERE LOWER(chatter_login) = $1 \
             ) AS \"is_first_global!\"",
            chatter_login,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(Some(FirstTimeContext {
            is_first_time_streamer,
            is_first_global,
        }))
    }

    async fn cross_channel_first_messages(
        &self,
        channel_login: &str,
        chatter_login: &str,
        window_minutes: i64,
    ) -> Result<i64, String> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM ( \
               SELECT streamer_login \
               FROM twitch_chat_messages \
               WHERE LOWER(chatter_login) = $1 \
                 AND LOWER(streamer_login) <> $2 \
               GROUP BY streamer_login \
               HAVING MIN(message_ts) >= NOW() - ($3 * INTERVAL '1 minute') \
             ) t",
        )
        .bind(chatter_login.to_lowercase())
        .bind(channel_login.to_lowercase())
        .bind(window_minutes)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())
    }

    async fn persist_verdict(&self, record: &VerdictRecord) -> Result<i64, String> {
        let id: i64 = sqlx::query_scalar!(
            "INSERT INTO twitch_scam_guard_verdicts \
             (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
              reasoning, transcript_snapshot, action_taken) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id AS \"id!\"",
            &record.channel_login,
            &record.chatter_login,
            record.chatter_id.as_deref(),
            record.verdict.as_str(),
            record.confidence,
            &record.category,
            &record.reasoning,
            &record.transcript_snapshot,
            &record.action_taken,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(id)
    }

    async fn load_learnings(&self) -> Result<Option<String>, String> {
        load_learnings(&self.pool).await
    }

    async fn learn_spam_pattern(
        &self,
        pattern: &str,
        source_message: &str,
        channel_login: &str,
        reasoning: &str,
    ) -> Result<Option<String>, String> {
        use crate::scam_pitch::{LearnOutcome, learn_pattern_from_judge};
        match learn_pattern_from_judge(
            &self.pool,
            pattern,
            "fragment",
            source_message,
            channel_login,
            reasoning,
        )
        .await
        {
            LearnOutcome::Saved { pattern, .. } => Ok(Some(pattern)),
            LearnOutcome::Rejected | LearnOutcome::Skipped => Ok(None),
            LearnOutcome::SaveFailed => Err("DB-Schreibfehler".to_string()),
        }
    }

    async fn add_global_ban(
        &self,
        chatter_login: &str,
        chatter_id: Option<&str>,
        reason: &str,
    ) -> Result<(), String> {
        add_conversation_scam_global_ban(&self.pool, chatter_login, chatter_id, reason).await
    }
}

pub struct ConversationScamGuard {
    bot_user_id: String,
    store: Arc<dyn ScamGuardStore>,
    judge: Arc<dyn ScamJudge>,
    api: Arc<dyn ChatApi>,
    moderation: Arc<dyn ScamModeration>,
    notifier: Option<Arc<dyn ScamGuardNotifier>>,
    states: DashMap<(String, String), Arc<Mutex<DialogState>>>,
}

impl ConversationScamGuard {
    pub fn new(
        pool: PgPool,
        bot_user_id: String,
        judge: Arc<dyn ScamJudge>,
        api: Arc<dyn ChatApi>,
        moderation: Arc<ModerationEngine>,
    ) -> Self {
        Self::with_store(
            bot_user_id,
            Arc::new(PgScamGuardStore::new(pool)),
            judge,
            api,
            moderation as Arc<dyn ScamModeration>,
        )
    }

    pub fn with_store(
        bot_user_id: String,
        store: Arc<dyn ScamGuardStore>,
        judge: Arc<dyn ScamJudge>,
        api: Arc<dyn ChatApi>,
        moderation: Arc<dyn ScamModeration>,
    ) -> Self {
        Self {
            bot_user_id,
            store,
            judge,
            api,
            moderation,
            notifier: None,
            states: DashMap::new(),
        }
    }

    /// Optionaler Discord-Notifier (Sichtbarkeit + Revoke). Ohne ihn postet der
    /// Wächter nichts nach Discord — Tests und der headless-Betrieb laufen so.
    pub fn with_notifier(mut self, notifier: Arc<dyn ScamGuardNotifier>) -> Self {
        self.notifier = Some(notifier);
        self
    }

    pub fn observe(self: &Arc<Self>, event: &ChatMessageEvent) {
        let guard = Arc::clone(self);
        let event = event.clone();
        let channel = event.broadcaster_user_login.clone();
        let chatter = event.chatter_user_login.clone();
        let handle = tokio::spawn(async move {
            guard.process(&event).await;
        });
        tokio::spawn(async move {
            if let Err(error) = handle.await {
                tracing::error!(
                    channel = %channel,
                    chatter = %chatter,
                    %error,
                    "Conversation-Scam-Task fehlerhaft beendet"
                );
            }
        });
    }

    async fn process(&self, event: &ChatMessageEvent) {
        let channel_login = event.broadcaster_user_login.to_lowercase();
        let chatter_login = event.chatter_user_login.to_lowercase();
        if channel_login.is_empty() || chatter_login.is_empty() || event.text().is_empty() {
            return;
        }

        let settings = match self
            .store
            .load_settings(&channel_login, Some(&event.broadcaster_user_id))
            .await
        {
            Ok(settings) => settings,
            Err(error) => {
                warn!("Conversation-Scam-Settings nicht ladbar, Defaults aktiv: {error}");
                GuardSettings::default()
            }
        };
        if !settings.enabled {
            return;
        }

        let context = match self
            .store
            .first_time_context(&channel_login, &chatter_login)
            .await
        {
            Ok(Some(context)) => context,
            Ok(None) => return,
            Err(error) => {
                debug!("Conversation-Scam-Erstschreiber-Check fehlgeschlagen: {error}");
                return;
            }
        };
        if !should_consider_event(event, &self.bot_user_id, context) {
            return;
        }

        let key = (channel_login.clone(), chatter_login.clone());
        let state = match self.states.get(&key) {
            Some(existing) => existing.clone(),
            None => {
                let other_channels_last_hour = match self
                    .store
                    .cross_channel_first_messages(
                        &channel_login,
                        &chatter_login,
                        CROSS_CHANNEL_WINDOW_MINUTES,
                    )
                    .await
                {
                    Ok(count) => count,
                    Err(error) => {
                        warn!(
                            reason = "cross_channel_error",
                            channel = %channel_login,
                            chatter = %chatter_login,
                            %error,
                            "Conversation-Scam-Netzwerk-Signal nicht ladbar; nutze 0"
                        );
                        0
                    }
                };
                let account_age_days = if event.chatter_user_id.is_empty() {
                    None
                } else {
                    match self.api.user_created_at(&event.chatter_user_id).await {
                        Ok(Some(created)) => Some((Utc::now() - created).num_days()),
                        Ok(None) => {
                            warn!(
                                reason = "account_age_unknown",
                                channel = %channel_login,
                                chatter = %chatter_login,
                                "Conversation-Scam-Account-Alter unbekannt; AutoBan deaktiviert"
                            );
                            None
                        }
                        Err(error) => {
                            warn!(
                                reason = "account_age_error",
                                channel = %channel_login,
                                chatter = %chatter_login,
                                %error,
                                "Conversation-Scam-Account-Alter nicht ladbar; AutoBan deaktiviert"
                            );
                            None
                        }
                    }
                };
                // Erkenntnisse nur einmal pro Chatter laden (beim ersten Treffer),
                // nicht bei jeder Folgenachricht.
                let learnings = match self.store.load_learnings().await {
                    Ok(learnings) => learnings,
                    Err(error) => {
                        warn!("Conversation-Scam-Learnings nicht ladbar: {error}");
                        None
                    }
                };
                self.states
                    .entry(key)
                    .or_insert_with(|| {
                        Arc::new(Mutex::new(DialogState::with_context(
                            context.is_first_global,
                            account_age_days,
                            other_channels_last_hour,
                            learnings.as_deref(),
                            &channel_login,
                            &chatter_login,
                        )))
                    })
                    .clone()
            }
        };
        let mut dialog = state.lock().await;
        if dialog.completed {
            return;
        }
        dialog.push_user_message(event.text());
        if !dialog.has_enough_substance() {
            return;
        }

        let verdict = self.judge.judge(&mut dialog).await;
        let enforcement_threshold =
            effective_scam_enforcement_threshold(&settings, dialog.account_age_days);
        let regular_threshold = settings.threshold.max(settings.suggestion_floor);
        let threshold_source = if enforcement_threshold < regular_threshold {
            "young_account_curve"
        } else {
            "channel_threshold"
        };
        let (action_taken, completed) = self
            .apply_decision(event, &settings, &verdict, dialog.account_age_days)
            .await;
        let message: String = event.text().chars().take(120).collect();
        let reasoning: String = verdict.reasoning.chars().take(200).collect();
        tracing::info!(
            channel = %channel_login,
            chatter = %chatter_login,
            verdict = verdict.verdict.as_str(),
            confidence = verdict.confidence,
            category = %verdict.category,
            action_taken = %action_taken,
            cross_channel = dialog.other_channels_last_hour,
            account_age_days = ?dialog.account_age_days,
            enforcement_threshold,
            threshold_source,
            message = %message,
            reasoning = %reasoning,
            "Conversation-Scam-Judge-Entscheidung"
        );
        // Muster fuer den Vorfilter lernen: erkennt der Bot denselben Pitch
        // beim naechsten Mal schon am Regex, greift die Moderation sofort statt
        // erst nach dem Judge-Lauf.
        if verdict.verdict == VerdictKind::Scam
            && verdict.confidence >= LEARN_MIN_CONFIDENCE
            && action_taken != "watching"
        {
            if let Some(pattern) = verdict
                .pattern
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                match self
                    .store
                    .learn_spam_pattern(
                        pattern,
                        event.text(),
                        &channel_login,
                        &verdict.reasoning,
                    )
                    .await
                {
                    Ok(Some(saved)) => tracing::info!(
                        channel = %channel_login,
                        pattern = %saved,
                        "Spam-Muster aus Judge-Urteil gelernt"
                    ),
                    Ok(None) => {}
                    Err(error) => warn!(%error, pattern, "Spam-Muster nicht gelernt"),
                }
            }
        }

        let record = VerdictRecord {
            channel_login,
            chatter_login,
            chatter_id: (!event.chatter_user_id.is_empty()).then(|| event.chatter_user_id.clone()),
            verdict: verdict.verdict,
            confidence: verdict.confidence,
            category: verdict.category.clone(),
            reasoning: verdict.reasoning.clone(),
            transcript_snapshot: dialog.transcript_snapshot(),
            action_taken,
        };
        match self.store.persist_verdict(&record).await {
            Ok(verdict_id) => self.notify_verdict(verdict_id, &record),
            Err(error) => warn!("Conversation-Scam-Verdict nicht persistiert: {error}"),
        }
        dialog.completed = completed;
    }

    /// Postet jedes persistierte Urteil fire-and-forget in den
    /// Discord-Aufsichts-Feed. Ohne Notifier passiert nichts.
    fn notify_verdict(&self, verdict_id: i64, record: &VerdictRecord) {
        if record.action_taken == "watching" {
            return;
        }
        let Some(notifier) = &self.notifier else {
            return;
        };
        let notifier = Arc::clone(notifier);
        let notification = ScamNotification {
            verdict_id,
            channel_login: record.channel_login.clone(),
            chatter_login: record.chatter_login.clone(),
            category: record.category.clone(),
            reasoning: record.reasoning.clone(),
            confidence: record.confidence,
            verdict: record.verdict.as_str().to_string(),
            action_taken: record.action_taken.clone(),
        };
        let handle = tokio::spawn(async move {
            notifier.notify(notification).await;
        });
        tokio::spawn(async move {
            if let Err(error) = handle.await {
                tracing::error!(
                    verdict_id,
                    %error,
                    "Conversation-Scam-Notify-Task fehlerhaft beendet"
                );
            }
        });
    }

    async fn apply_decision(
        &self,
        event: &ChatMessageEvent,
        settings: &GuardSettings,
        verdict: &Verdict,
        account_age_days: Option<i64>,
    ) -> (String, bool) {
        let enforcement_threshold =
            effective_scam_enforcement_threshold(settings, account_age_days);
        match verdict.verdict {
            VerdictKind::Clean => ("none".to_string(), true),
            VerdictKind::Unsure => ("watching".to_string(), false),
            VerdictKind::Scam if verdict.confidence < settings.suggestion_floor => {
                ("none".to_string(), false)
            }
            VerdictKind::Scam if verdict.confidence < enforcement_threshold => {
                ("suggested".to_string(), false)
            }
            VerdictKind::Scam => match settings.mode {
                GuardMode::AlertOnly => ("suggested".to_string(), true),
                GuardMode::Timeout => {
                    let timed_out = self
                        .moderation
                        .timeout_and_cleanup(
                            Some(&event.broadcaster_user_login),
                            &event.broadcaster_user_id,
                            Some(&event.chatter_user_login),
                            &event.chatter_user_id,
                            &event.message_id,
                            Some(event.text()),
                            TIMEOUT_SECONDS,
                            &verdict.reasoning,
                        )
                        .await;
                    if timed_out {
                        ("timed_out".to_string(), true)
                    } else {
                        ("suggested".to_string(), true)
                    }
                }
                GuardMode::AutoBan => {
                    if event.chatter_user_id.is_empty() {
                        return ("ban_failed_no_mod".to_string(), true);
                    }
                    let is_known_new = matches!(
                        account_age_days,
                        Some(age_days) if (0..ACCOUNT_NEW_MAX_DAYS).contains(&age_days)
                    );
                    if !is_known_new {
                        let timed_out = self
                            .moderation
                            .timeout_and_cleanup(
                                Some(&event.broadcaster_user_login),
                                &event.broadcaster_user_id,
                                Some(&event.chatter_user_login),
                                &event.chatter_user_id,
                                &event.message_id,
                                Some(event.text()),
                                TIMEOUT_SECONDS,
                                &verdict.reasoning,
                            )
                            .await;
                        if timed_out {
                            return ("timed_out".to_string(), true);
                        }
                        return ("suggested".to_string(), true);
                    }
                    if !crate::safe_list::is_safe(
                        Some(&event.chatter_user_id),
                        &event.chatter_user_login,
                    ) {
                        if let Err(error) = self
                            .store
                            .add_global_ban(
                                &event.chatter_user_login,
                                Some(&event.chatter_user_id),
                                &verdict.reasoning,
                            )
                            .await
                        {
                            warn!(
                                channel = %event.broadcaster_user_login,
                                chatter = %event.chatter_user_login,
                                %error,
                                "Conversation-Scam-Globalban-Vormerkung fehlgeschlagen"
                            );
                        }
                    }
                    let banned = self
                        .moderation
                        .auto_ban_and_cleanup(AutoBanRequest {
                            channel_login: &event.broadcaster_user_login,
                            broadcaster_id: &event.broadcaster_user_id,
                            bot_id: &self.bot_user_id,
                            chatter_login: &event.chatter_user_login,
                            chatter_id: &event.chatter_user_id,
                            message_id: &event.message_id,
                            content: event.text(),
                            ban: true,
                            reason_text: &verdict.reasoning,
                            notice_text: None,
                            silent: true,
                        })
                        .await;
                    if banned {
                        ("banned".to_string(), true)
                    } else {
                        ("ban_failed_no_mod".to_string(), true)
                    }
                }
            },
        }
    }
}

fn effective_scam_enforcement_threshold(
    settings: &GuardSettings,
    account_age_days: Option<i64>,
) -> f32 {
    let regular_threshold = settings.threshold.max(settings.suggestion_floor);
    if settings.mode != GuardMode::AutoBan {
        return regular_threshold;
    }
    let Some(age_days) = account_age_days else {
        return regular_threshold;
    };
    if !(0..=YOUNG_ACCOUNT_CURVE_DAYS).contains(&age_days) {
        return regular_threshold;
    }

    let start_threshold = YOUNG_ACCOUNT_START_THRESHOLD
        .max(settings.suggestion_floor)
        .min(regular_threshold);
    let age_ratio = age_days as f32 / YOUNG_ACCOUNT_CURVE_DAYS as f32;
    start_threshold + (regular_threshold - start_threshold) * age_ratio
}

/// Maximale Zeichen pro Twitch-Chat-Nachricht für gesplittete `!explain`-Antworten
/// (konservativ unter dem 500er-Hardlimit).
pub const TWITCH_MAX_MESSAGE_CHARS: usize = 480;

const SCAM_EXPLAIN_SYSTEM_PROMPT: &str = r#"Du erklärst einem Twitch-Streamer — oft unerfahren — in einfachen Worten, warum der automatische Chat-Wächter einen Account als Betrugsversuch eingestuft hat. Du bekommst die gespeicherte Einschätzung (Kategorie, Begründung) und den Nachrichtenverlauf dieses Accounts.

Erkläre ausführlich, ruhig und lehrreich auf Deutsch: Was ist die Masche? An welchen konkreten Stellen im Verlauf erkennt man sie? Und woran kann der Streamer so etwas in Zukunft selbst erkennen? Schreibe als zusammenhängenden Fließtext, ohne Aufzählungszeichen, ohne Markdown, ohne Emojis. Nenne keine Zahlenwerte oder Scores. Du darfst ausführlich sein — der Text wird automatisch in mehrere Chat-Nachrichten aufgeteilt."#;

/// Zerlegt einen langen Text in Twitch-taugliche Häppchen (höchstens `max_len`
/// Zeichen), bevorzugt an Wortgrenzen. Kein Mengen-Limit.
pub fn chunk_for_twitch(text: &str, max_len: usize) -> Vec<String> {
    let max_len = max_len.max(1);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if word.chars().count() > max_len {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let mut piece = String::new();
            for ch in word.chars() {
                if piece.chars().count() >= max_len {
                    chunks.push(std::mem::take(&mut piece));
                }
                piece.push(ch);
            }
            current = piece;
            continue;
        }
        let separator = usize::from(!current.is_empty());
        if current.chars().count() + separator + word.chars().count() > max_len
            && !current.is_empty()
        {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Gespeichertes Scam-Urteil als Grundlage für `!explain` / `!unban`.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredVerdict {
    pub id: i64,
    pub chatter_login: String,
    pub category: String,
    pub reasoning: String,
    pub transcript_snapshot: String,
}

/// Backing für die Chat-Commands `!explain` und die `overturned`-Markierung bei
/// `!unban`. Hält eigene DB-/MiniMax-Zugriffe, unabhängig vom Live-Detektor.
pub struct ScamGuardCommands {
    pool: PgPool,
    client: EngagementMinimaxClient,
}

impl ScamGuardCommands {
    pub fn new(pool: PgPool, client: EngagementMinimaxClient) -> Self {
        Self { pool, client }
    }

    async fn latest_scam_verdict(
        &self,
        channel_login: &str,
        target: Option<&str>,
    ) -> Result<Option<StoredVerdict>, String> {
        let target = target.map(|t| t.trim().trim_start_matches('@').to_lowercase());
        let row = sqlx::query!(
            "SELECT id AS \"id!\", \
                    chatter_login AS \"chatter_login!\", \
                    category AS \"category!\", \
                    reasoning AS \"reasoning!\", \
                    transcript_snapshot AS \"transcript_snapshot!\" \
             FROM twitch_scam_guard_verdicts \
             WHERE channel_login = $1 AND verdict = 'scam' \
               AND ($2::text IS NULL OR chatter_login = $2) \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            channel_login.to_lowercase(),
            target.as_deref(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(row.map(|row| StoredVerdict {
            id: row.id,
            chatter_login: row.chatter_login,
            category: row.category,
            reasoning: row.reasoning,
            transcript_snapshot: row.transcript_snapshot,
        }))
    }

    /// Liefert eine ausführliche, in Twitch-Häppchen gesplittete Erklärung des
    /// jüngsten Scam-Urteils. Leerer Vektor = kein Fall gefunden. Fällt bei
    /// MiniMax-Ausfall auf die gespeicherte Begründung zurück.
    pub async fn explain(&self, channel_login: &str, target: Option<&str>) -> Vec<String> {
        let verdict = match self.latest_scam_verdict(channel_login, target).await {
            Ok(Some(verdict)) => verdict,
            Ok(None) => return Vec::new(),
            Err(error) => {
                warn!("Scam-Explain DB-Fehler: {error}");
                return Vec::new();
            }
        };

        let messages = serde_json::json!([
            {"role": "system", "content": SCAM_EXPLAIN_SYSTEM_PROMPT},
            {"role": "user", "content": serde_json::json!({
                "kategorie": verdict.category,
                "begruendung": verdict.reasoning,
                "verlauf": verdict.transcript_snapshot,
            }).to_string()},
        ]);

        let text = match self
            .client
            .messages_completion_uncapped(messages, 0.3)
            .await
        {
            Ok(text) if !text.trim().is_empty() => text,
            _ => verdict.reasoning.clone(),
        };
        chunk_for_twitch(text.trim(), TWITCH_MAX_MESSAGE_CHARS)
    }

    /// Markiert das jüngste Scam-Urteil dieses Accounts als aufgehoben
    /// (False-Positive-Spur für `!unban`). Match über `chatter_id`. Liefert
    /// `true`, wenn eine Zeile aktualisiert wurde.
    pub async fn overturn(&self, channel_login: &str, chatter_id: &str) -> bool {
        if chatter_id.is_empty() {
            return false;
        }
        let target = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, chatter_login, action_taken \
             FROM twitch_scam_guard_verdicts \
             WHERE channel_login = $1 AND chatter_id = $2 AND verdict = 'scam' \
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(channel_login.to_lowercase())
        .bind(chatter_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let Some((verdict_id, chatter_login, action_taken)) = target else {
            return false;
        };

        let marked = mark_overturned_by_id(&self.pool, verdict_id)
            .await
            .unwrap_or(false);
        if marked
            && matches!(
                action_taken.as_str(),
                "banned" | "timed_out" | "ban_failed_no_mod"
            )
        {
            if let Err(error) =
                remove_conversation_scam_global_ban(&self.pool, &chatter_login).await
            {
                warn!(
                    chatter = %chatter_login,
                    "Scam-Overturn: AI-Globalban konnte nicht entfernt werden: {error}"
                );
            }
        }
        marked
    }
}

#[async_trait]
impl crate::commands::ScamGuardCommandPort for ScamGuardCommands {
    async fn explain(&self, channel_login: &str, target: Option<&str>) -> Vec<String> {
        ScamGuardCommands::explain(self, channel_login, target).await
    }

    async fn overturn(&self, channel_login: &str, chatter_id: &str) -> bool {
        ScamGuardCommands::overturn(self, channel_login, chatter_id).await
    }
}

// ---- Self-Learning ----------------------------------------------------------
//
// Analog zum SpamAiReviewer, aber für den LLM-Judge: ein Hintergrundjob lässt
// MiniMax periodisch aus den jüngsten bestätigten Scams und den vom Streamer
// aufgehobenen Fehlalarmen kompakte Erkenntnisse destillieren. Diese fließen als
// Zusatzhinweis in den Judge-System-Prompt ein (`DialogState::with_learnings`),
// sodass der Wächter mit der Zeit treffsicherer wird, ohne Codeänderung.

/// Intervall des Self-Learning-Jobs (alle 6 Stunden).
const SCAM_LEARNINGS_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Maximale Fälle je Klasse (bestätigt / aufgehoben) pro Destillation.
const LEARNINGS_SAMPLE_LIMIT: i64 = 40;
/// Untergrenze: erst ab so vielen Gesamtfällen lohnt eine Destillation.
const LEARNINGS_MIN_SAMPLES: usize = 3;

const SCAM_LEARNING_SYSTEM_PROMPT: &str = r#"Du wertest die jüngsten Entscheidungen eines automatischen Twitch-Chat-Wächters aus, der aufgesetzte Betrugs-Konversationen von Erstschreibern erkennt. Dein Ziel: kompakte, konkrete Erkenntnisse destillieren, die einem Prüfer künftig helfen, schneller und treffsicherer zu urteilen.

Du bekommst zwei Listen. "bestaetigte_faelle" = Konversationen, die zu Recht als Betrug eingestuft wurden. "aufgehobene_faelle" = Fälle, die der Streamer als Fehlalarm zurückgenommen hat — also harmlose Zuschauer, die fälschlich getroffen wurden. Jeder Eintrag hat eine Kategorie, eine Begründung und den Nachrichtenverlauf.

Leite daraus ab: Welche konkreten Formulierungen, Gesprächsmuster oder Abläufe tauchen bei echten Maschen wiederholt auf und sind verlässliche Warnzeichen? Und welche Merkmale haben zu Fehlalarmen geführt, sodass man dort vorsichtiger sein und NICHT vorschnell bannen sollte?

Schreibe höchstens etwa 180 Wörter, als kurze Stichpunkte oder knappen Fließtext, nüchtern und konkret auf Deutsch. Keine Zahlen, keine Scores, keine Anrede, keine Einleitung — nur die Erkenntnisse selbst. Sind die Fälle zu dünn für belastbare Muster, fasse nur das Offensichtliche knapp zusammen."#;

/// Ein einzelner Fall aus dem Lern-Korpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearningSample {
    pub category: String,
    pub reasoning: String,
    pub transcript_snapshot: String,
}

/// Lern-Korpus: bestätigte Scams + aufgehobene Fehlalarme.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LearningCorpus {
    pub confirmed: Vec<LearningSample>,
    pub false_positives: Vec<LearningSample>,
}

impl LearningCorpus {
    pub fn total(&self) -> usize {
        self.confirmed.len() + self.false_positives.len()
    }
}

async fn fetch_samples(pool: &PgPool, actions: &[&str], limit: i64) -> Vec<LearningSample> {
    let actions_owned: Vec<String> = actions.iter().map(|s| s.to_string()).collect();

    sqlx::query!(
        "SELECT category AS \"category!\", \
                reasoning AS \"reasoning!\", \
                transcript_snapshot AS \"transcript_snapshot!\" \
         FROM twitch_scam_guard_verdicts \
         WHERE verdict = 'scam' AND action_taken = ANY($1::text[]) \
         ORDER BY created_at DESC, id DESC LIMIT $2",
        &actions_owned,
        limit,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|row| LearningSample {
        category: row.category,
        reasoning: row.reasoning,
        transcript_snapshot: row.transcript_snapshot,
    })
    .collect()
}

/// Lädt die jüngsten bestätigten Scams (gebannt/getimeoutet/vorgeschlagen) und
/// die aufgehobenen Fehlalarme als Lern-Korpus.
pub async fn fetch_learning_corpus(pool: &PgPool, limit: i64) -> LearningCorpus {
    LearningCorpus {
        confirmed: fetch_samples(pool, &["banned", "timed_out", "suggested"], limit).await,
        false_positives: fetch_samples(pool, &["overturned"], limit).await,
    }
}

/// Baut die MiniMax-Nachrichten für die Destillation. Reiner, testbarer Aufbau.
pub fn build_distill_messages(corpus: &LearningCorpus) -> Value {
    fn render(samples: &[LearningSample]) -> Vec<Value> {
        samples
            .iter()
            .map(|sample| {
                serde_json::json!({
                    "kategorie": sample.category,
                    "begruendung": sample.reasoning,
                    "verlauf": sample.transcript_snapshot,
                })
            })
            .collect()
    }
    let user = serde_json::json!({
        "bestaetigte_faelle": render(&corpus.confirmed),
        "aufgehobene_faelle": render(&corpus.false_positives),
    });
    serde_json::json!([
        {"role": "system", "content": SCAM_LEARNING_SYSTEM_PROMPT},
        {"role": "user", "content": user.to_string()},
    ])
}

/// Speichert die destillierten Erkenntnisse als Singleton-Zeile (UPSERT).
pub async fn persist_learnings(
    pool: &PgPool,
    guidance: &str,
    source_count: i32,
) -> Result<(), String> {
    sqlx::query!(
        "INSERT INTO twitch_scam_guard_learnings (id, guidance, source_count, updated_at) \
         VALUES (TRUE, $1, $2, NOW()) \
         ON CONFLICT (id) DO UPDATE SET guidance = EXCLUDED.guidance, \
           source_count = EXCLUDED.source_count, updated_at = EXCLUDED.updated_at",
        guidance,
        source_count,
    )
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Lädt die aktuell gültigen Erkenntnisse (oder `None`, solange noch keine
/// destilliert wurden bzw. leer).
pub async fn load_learnings(pool: &PgPool) -> Result<Option<String>, String> {
    sqlx::query_scalar!(
        "SELECT guidance AS \"guidance!\" FROM twitch_scam_guard_learnings WHERE id = TRUE",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())
    .map(|opt| opt.filter(|guidance| !guidance.trim().is_empty()))
}

/// Ein Durchlauf des Self-Learning-Jobs: Korpus laden, von MiniMax destillieren
/// lassen, Ergebnis ablegen. Best-effort — Fehler werden geloggt, nicht
/// propagiert; zu dünne Datenlage wird übersprungen (keine Überschreibung).
pub async fn run_scam_learnings_once(pool: &PgPool, client: &EngagementMinimaxClient) {
    let corpus = fetch_learning_corpus(pool, LEARNINGS_SAMPLE_LIMIT).await;
    if corpus.total() < LEARNINGS_MIN_SAMPLES {
        debug!(
            "Scam-Self-Learning: zu wenige Fälle ({}), übersprungen",
            corpus.total()
        );
        return;
    }
    let messages = build_distill_messages(&corpus);
    let guidance = match client.messages_completion_uncapped(messages, 0.2).await {
        Ok(text) if !text.trim().is_empty() => text.trim().to_string(),
        Ok(_) => {
            debug!("Scam-Self-Learning: leere Antwort, übersprungen");
            return;
        }
        Err(error) => {
            debug!("Scam-Self-Learning: MiniMax nicht verfügbar: {error}");
            return;
        }
    };
    if let Err(error) = persist_learnings(pool, &guidance, corpus.total() as i32).await {
        warn!("Scam-Self-Learning: Erkenntnisse nicht gespeichert: {error}");
    }
}

/// Endlos-Loop: einmal nach `initial_delay_secs`, danach alle 6 Stunden.
pub async fn schedule_scam_learnings(pool: PgPool, initial_delay_secs: u64) {
    tokio::time::sleep(Duration::from_secs(initial_delay_secs)).await;
    let client = EngagementMinimaxClient::new(None, None, None, None);
    loop {
        run_scam_learnings_once(&pool, &client).await;
        tokio::time::sleep(SCAM_LEARNINGS_INTERVAL).await;
    }
}

// ---- Revoke (Discord-Button / Dashboard-Override) ---------------------------
//
// Eine bereits getroffene Entscheidung gezielt zurücknehmen — angestoßen per
// Discord-Button oder Dashboard. Anders als der Chat-Command `!unban` (jüngster
// Fall je Chatter) adressiert der Revoke EXAKT eine `verdict_id`, weil die
// Discord-Nachricht genau diesen Fall referenziert. War tatsächlich gebannt
// worden, wird auf Twitch entbannt; in jedem Fall wird der Fall als
// `overturned` markiert (False-Positive-Signal fürs Self-Learning).

/// Adressdaten eines Urteils, das zurückgenommen werden soll.
#[derive(Debug, Clone)]
pub struct RevokeTarget {
    pub channel_login: String,
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub action_taken: String,
}

/// Ergebnis eines Revoke — serialisiert direkt als Port-/API-Antwort.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RevokeOutcome {
    pub status: &'static str,
    pub channel_login: String,
    pub chatter_login: String,
    /// Ob ursprünglich tatsächlich gebannt/getimeoutet wurde.
    pub was_banned: bool,
    /// Ob der Twitch-Unban erfolgreich war (nur bei `was_banned` relevant).
    pub unbanned: bool,
}

impl RevokeOutcome {
    fn not_found() -> Self {
        Self {
            status: "not_found",
            channel_login: String::new(),
            chatter_login: String::new(),
            was_banned: false,
            unbanned: false,
        }
    }
}

/// Lädt die Zieldaten eines Urteils anhand seiner ID.
pub async fn load_revoke_target(
    pool: &PgPool,
    verdict_id: i64,
) -> Result<Option<RevokeTarget>, String> {
    let row = sqlx::query!(
        "SELECT channel_login AS \"channel_login!\", \
                chatter_login AS \"chatter_login!\", \
                chatter_id, \
                action_taken AS \"action_taken!\" \
         FROM twitch_scam_guard_verdicts WHERE id = $1",
        verdict_id,
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(row.map(|row| RevokeTarget {
        channel_login: row.channel_login,
        chatter_login: row.chatter_login,
        chatter_id: row.chatter_id,
        action_taken: row.action_taken,
    }))
}

/// Markiert genau dieses Urteil als `overturned` (False-Positive-Spur fürs
/// Self-Learning). Liefert `true`, wenn eine Zeile aktualisiert wurde.
pub async fn mark_overturned_by_id(pool: &PgPool, verdict_id: i64) -> Result<bool, String> {
    sqlx::query!(
        "UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' WHERE id = $1",
        verdict_id,
    )
    .execute(pool)
    .await
    .map(|result| result.rows_affected() > 0)
    .map_err(|error| error.to_string())
}

pub async fn add_conversation_scam_global_ban(
    pool: &PgPool,
    chatter_login: &str,
    chatter_id: Option<&str>,
    reason: &str,
) -> Result<(), String> {
    let chatter_login = chatter_login.to_lowercase();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban \
         (chatter_login, chatter_id, reason, added_by) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (chatter_login) DO UPDATE SET \
             chatter_id = COALESCE(EXCLUDED.chatter_id, twitch_chatter_global_ban.chatter_id), \
             reason = EXCLUDED.reason, \
             added_at = NOW() \
         WHERE twitch_chatter_global_ban.added_by = $4",
    )
    .bind(&chatter_login)
    .bind(chatter_id)
    .bind(reason)
    .bind(CONVERSATION_SCAM_GLOBAL_BAN_ADDED_BY)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

async fn remove_conversation_scam_global_ban(
    pool: &PgPool,
    chatter_login: &str,
) -> Result<bool, String> {
    let chatter_login = chatter_login.to_lowercase();
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;
    let result = sqlx::query(
        "DELETE FROM twitch_chatter_global_ban \
         WHERE chatter_login = $1 AND added_by = $2",
    )
    .bind(&chatter_login)
    .bind(CONVERSATION_SCAM_GLOBAL_BAN_ADDED_BY)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;

    if result.rows_affected() > 0 {
        sqlx::query("DELETE FROM twitch_chatter_global_ban_applied WHERE chatter_login = $1")
            .bind(&chatter_login)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
    }

    tx.commit().await.map_err(|error| error.to_string())?;
    Ok(result.rows_affected() > 0)
}

/// Nimmt das Urteil `verdict_id` zurück: war es ein echter Ban/Timeout, wird auf
/// Twitch entbannt; in jedem Fall wird der Fall als `overturned` markiert.
/// Best-effort — ein fehlgeschlagener Unban verhindert die Markierung nicht
/// (das Urteil war trotzdem falsch). Unbekannte ID → `status = "not_found"`.
pub async fn revoke_verdict(pool: &PgPool, api: &dyn ChatApi, verdict_id: i64) -> RevokeOutcome {
    let target = match load_revoke_target(pool, verdict_id).await {
        Ok(Some(target)) => target,
        Ok(None) => return RevokeOutcome::not_found(),
        Err(error) => {
            warn!("Scam-Revoke DB-Fehler beim Laden ({verdict_id}): {error}");
            return RevokeOutcome::not_found();
        }
    };

    let was_banned = matches!(target.action_taken.as_str(), "banned" | "timed_out");
    let unbanned = if was_banned {
        try_unban(api, &target).await
    } else {
        false
    };
    if matches!(
        target.action_taken.as_str(),
        "banned" | "timed_out" | "ban_failed_no_mod"
    ) {
        if let Err(error) = remove_conversation_scam_global_ban(pool, &target.chatter_login).await {
            warn!(
                chatter = %target.chatter_login,
                "Scam-Revoke: AI-Globalban konnte nicht entfernt werden: {error}"
            );
        }
    }

    if let Err(error) = mark_overturned_by_id(pool, verdict_id).await {
        warn!("Scam-Revoke: Markierung overturned fehlgeschlagen ({verdict_id}): {error}");
    }

    RevokeOutcome {
        status: "revoked",
        channel_login: target.channel_login,
        chatter_login: target.chatter_login,
        was_banned,
        unbanned,
    }
}

/// Löst Broadcaster- und Chatter-ID auf und entbannt. Die im Urteil gespeicherte
/// Chatter-ID hat Vorrang; fehlt sie, wird sie über den Login aufgelöst.
async fn try_unban(api: &dyn ChatApi, target: &RevokeTarget) -> bool {
    let Some(broadcaster_id) = api
        .resolve_user_id(&target.channel_login)
        .await
        .ok()
        .flatten()
    else {
        return false;
    };
    let chatter_id = match &target.chatter_id {
        Some(id) if !id.is_empty() => Some(id.clone()),
        _ => api
            .resolve_user_id(&target.chatter_login)
            .await
            .ok()
            .flatten(),
    };
    let Some(chatter_id) = chatter_id else {
        return false;
    };
    api.unban_user(&broadcaster_id, &chatter_id)
        .await
        .unwrap_or(false)
}

/// Adress- und Begründungsdaten eines vorgeschlagenen Bans.
#[derive(Debug, Clone)]
pub struct EnforceTarget {
    pub channel_login: String,
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub action_taken: String,
    pub reasoning: String,
}

/// Ergebnis eines Enforce — serialisiert direkt als Port-/API-Antwort.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EnforceOutcome {
    pub status: &'static str,
    pub channel_login: String,
    pub chatter_login: String,
    pub banned: bool,
}

impl EnforceOutcome {
    fn not_found() -> Self {
        Self {
            status: "not_found",
            channel_login: String::new(),
            chatter_login: String::new(),
            banned: false,
        }
    }

    fn not_eligible() -> Self {
        Self {
            status: "not_eligible",
            channel_login: String::new(),
            chatter_login: String::new(),
            banned: false,
        }
    }
}

/// Lädt die Zieldaten eines vorgeschlagenen Bans anhand seiner Verdict-ID.
pub async fn load_enforce_target(
    pool: &PgPool,
    verdict_id: i64,
) -> Result<Option<EnforceTarget>, String> {
    let row = sqlx::query!(
        "SELECT channel_login AS \"channel_login!\", \
                chatter_login AS \"chatter_login!\", \
                chatter_id, \
                action_taken AS \"action_taken!\", \
                reasoning AS \"reasoning!\" \
         FROM twitch_scam_guard_verdicts WHERE id = $1",
        verdict_id,
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(row.map(|row| EnforceTarget {
        channel_login: row.channel_login,
        chatter_login: row.chatter_login,
        chatter_id: row.chatter_id,
        action_taken: row.action_taken,
        reasoning: row.reasoning,
    }))
}

/// Promotet genau einen noch wartenden Vorschlag zum gespeicherten Ban.
pub async fn mark_banned_by_id(pool: &PgPool, verdict_id: i64) -> Result<bool, String> {
    sqlx::query!(
        "UPDATE twitch_scam_guard_verdicts SET action_taken = 'banned' \
         WHERE id = $1 AND action_taken = 'suggested'",
        verdict_id,
    )
    .execute(pool)
    .await
    .map(|result| result.rows_affected() > 0)
    .map_err(|error| error.to_string())
}

/// Führt einen wartenden Scam-Guard-Ban-Vorschlag auf Twitch aus.
pub async fn enforce_verdict(pool: &PgPool, api: &dyn ChatApi, verdict_id: i64) -> EnforceOutcome {
    let target = match load_enforce_target(pool, verdict_id).await {
        Ok(Some(target)) => target,
        Ok(None) => return EnforceOutcome::not_found(),
        Err(error) => {
            warn!("Scam-Enforce DB-Fehler beim Laden ({verdict_id}): {error}");
            return EnforceOutcome::not_found();
        }
    };

    if target.action_taken != "suggested" {
        return EnforceOutcome::not_eligible();
    }

    let banned = try_ban(api, &target).await;
    if banned {
        if let Err(error) = mark_banned_by_id(pool, verdict_id).await {
            warn!("Scam-Enforce: Markierung banned fehlgeschlagen ({verdict_id}): {error}");
        }
    }

    EnforceOutcome {
        status: if banned {
            "enforced"
        } else {
            "ban_failed_no_mod"
        },
        channel_login: target.channel_login,
        chatter_login: target.chatter_login,
        banned,
    }
}

/// Löst Broadcaster- und Chatter-ID auf und führt den Ban aus. Die gespeicherte
/// Chatter-ID hat Vorrang; fehlt sie, wird sie über den Login aufgelöst.
async fn try_ban(api: &dyn ChatApi, target: &EnforceTarget) -> bool {
    let Some(broadcaster_id) = api
        .resolve_user_id(&target.channel_login)
        .await
        .ok()
        .flatten()
    else {
        return false;
    };
    let chatter_id = match &target.chatter_id {
        Some(id) if !id.is_empty() => Some(id.clone()),
        _ => api
            .resolve_user_id(&target.chatter_login)
            .await
            .ok()
            .flatten(),
    };
    let Some(chatter_id) = chatter_id else {
        return false;
    };

    // Safe-List: dieser Pfad bannt direkt, ohne auto_ban_and_cleanup. Der Check
    // steht NACH der ID-Auflösung: ein Eintrag ohne gespeicherte ID kann über
    // den Login auf ein Safe-Konto auflösen (etwa nach einer Umbenennung).
    if crate::safe_list::is_safe(Some(&chatter_id), &target.chatter_login) {
        tracing::warn!(
            chatter = %target.chatter_login,
            "conversation_scam: Ban gegen Safe-List-Konto unterdrückt"
        );
        return false;
    }

    let reason = crate::moderation::twitch_moderation_reason(&target.reasoning);
    matches!(
        api.ban_user(&broadcaster_id, &chatter_id, &reason).await,
        Ok(BanOutcome::Banned | BanOutcome::AlreadyBanned)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{BanOutcome, ChatApi};
    use crate::types::{ChatBadge, ChatMessageBody, ChatMessageEvent, SendOutcome};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use tb_engagement::minimax_chat::EngagementMinimaxClient;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const REPORTED_BEFRIENDING_PIVOT: &str = "Yo bruh, love ❤️ your stream Let's sometimes play together and share tips together. Let's connect on Discord";

    /// Real gemeldeter Verlauf (2026-07-28, Nicht-Partner-Kanal, Account 0 Tage
    /// alt): englischer Aufklärungs-Smalltalk ohne jeden Stream- oder Spielbezug.
    /// Dort lief nur der Crew-Guard-Radar, der nie bannt. Diese Fixture prüft,
    /// was in einem Partner-Kanal passiert wäre.
    const REPORTED_RECON_SMALLTALK: &[&str] = &[
        "hii",
        "aww nice",
        "how are you doing",
        "naah i randomly join your stream",
        "are you from germany",
        "cool such a nice country",
    ];

    fn event(login: &str, text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "channel-id".to_string(),
            broadcaster_user_login: "testchannel".to_string(),
            chatter_user_id: format!("{login}-id"),
            chatter_user_login: login.to_string(),
            message_id: format!("msg-{login}-{text}"),
            message: ChatMessageBody {
                text: text.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn parse_verdict_liest_muster_nur_bei_scam() {
        let scam = parse_verdict_result(
            r#"{"verdict":"scam","confidence":0.95,"category":"growth_pitch","pattern":"stream_promotion_bot","reasoning":"x"}"#,
        )
        .unwrap();
        assert_eq!(scam.pattern.as_deref(), Some("stream_promotion_bot"));

        // Ein Muster bei clean oder unsure ist ein Judge-Fehler und wird verworfen.
        let clean = parse_verdict_result(
            r#"{"verdict":"clean","confidence":0.1,"category":"","pattern":"deadlock","reasoning":"x"}"#,
        )
        .unwrap();
        assert_eq!(clean.pattern, None);

        // Antworten ohne das Feld bleiben gueltig.
        let ohne = parse_verdict_result(
            r#"{"verdict":"scam","confidence":0.9,"category":"x","reasoning":"y"}"#,
        )
        .unwrap();
        assert_eq!(ohne.pattern, None);
    }

    fn verdict(kind: VerdictKind, confidence: f32) -> Verdict {
        Verdict {
            verdict: kind,
            confidence,
            category: "test-category".to_string(),
            reasoning: "test reasoning".to_string(),
            pattern: None,
        }
    }

    #[test]
    fn trigger_nur_fuer_unprivilegierte_erstschreiber() {
        let first = FirstTimeContext {
            is_first_time_streamer: true,
            is_first_global: true,
        };
        assert!(should_consider_event(
            &event("new_viewer", "hello there"),
            "bot-id",
            first
        ));

        let returning = FirstTimeContext {
            is_first_time_streamer: false,
            is_first_global: false,
        };
        assert!(!should_consider_event(
            &event("returning", "hello again"),
            "bot-id",
            returning
        ));

        for badge in ["moderator", "vip", "subscriber", "broadcaster"] {
            let mut privileged = event("trusted", "hello");
            privileged.badges.push(ChatBadge {
                set_id: badge.to_string(),
                id: String::new(),
                info: String::new(),
            });
            assert!(!should_consider_event(&privileged, "bot-id", first));
        }

        assert!(!should_consider_event(
            &event("nightbot", "automated message"),
            "bot-id",
            first
        ));
        let mut bot = event("guard_bot", "self echo");
        bot.chatter_user_id = "bot-id".to_string();
        assert!(!should_consider_event(&bot, "bot-id", first));
    }

    #[test]
    fn substanziell_nach_drei_nachrichten_oder_einem_pitch() {
        let mut dialog = DialogState::new(true);
        dialog.push_user_message("How is your day going?");
        assert!(!dialog.has_enough_substance());
        dialog.push_user_message("What games do you usually play?");
        assert!(!dialog.has_enough_substance());
        dialog.push_user_message("How long have you been streaming?");
        assert!(dialog.has_enough_substance());

        let mut single = DialogState::new(true);
        single.push_user_message(
            "Yo bro, I know a big streamer who can help you grow with real viewers. Add him on Discord.",
        );
        assert!(single.has_enough_substance());

        let mut trivial = DialogState::new(true);
        trivial.push_user_message("hey");
        trivial.push_user_message("Kappa");
        trivial.push_user_message("lol");
        assert!(!trivial.has_enough_substance());
    }

    #[test]
    fn netzwerk_signal_macht_kurze_nachricht_sofort_substanziell_und_json_sichtbar() {
        let mut dialog =
            DialogState::with_context(true, Some(14), 1, None, "testchannel", "network_user");
        dialog.push_user_message("hey");

        assert!(dialog.has_enough_substance());
        let input: Value = serde_json::from_str(&dialog.messages()[1].content).unwrap();
        assert_eq!(input["account_age_days"], 14);
        assert_eq!(input["other_channels_last_hour"], 1);
    }

    #[test]
    fn unbekanntes_account_alter_steht_als_null_im_judge_json() {
        let mut dialog =
            DialogState::with_context(true, None, 0, None, "testchannel", "unknown_age_user");
        dialog.push_user_message("hello there");

        let input: Value = serde_json::from_str(&dialog.messages()[1].content).unwrap();
        assert!(input["account_age_days"].is_null());
        assert_eq!(input["other_channels_last_hour"], 0);
    }

    #[test]
    fn reply_me_opener_zaehlt_als_single_message_pitch() {
        assert!(is_single_message_pitch(
            "Sorry my headphones are not working can reply me on chat"
        ));
    }

    #[test]
    fn learnings_werden_in_system_prompt_eingebettet() {
        let ohne = DialogState::new(true);
        assert_eq!(ohne.messages()[0].content, SCAM_JUDGE_SYSTEM_PROMPT);

        // Leere/whitespace-Erkenntnisse ändern den Prompt nicht.
        let leer = DialogState::with_learnings(true, Some("   "));
        assert_eq!(leer.messages()[0].content, SCAM_JUDGE_SYSTEM_PROMPT);

        // Echte Erkenntnisse werden angehängt, der Basis-Prompt bleibt erhalten.
        let mit = DialogState::with_learnings(true, Some("MERKMAL_XYZ taucht oft auf"));
        let system = &mit.messages()[0].content;
        assert!(system.starts_with(SCAM_JUDGE_SYSTEM_PROMPT));
        assert!(system.contains("MERKMAL_XYZ taucht oft auf"));
        assert!(system.len() > SCAM_JUDGE_SYSTEM_PROMPT.len());
    }

    #[test]
    fn build_distill_messages_enthaelt_beide_klassen() {
        let corpus = LearningCorpus {
            confirmed: vec![LearningSample {
                category: "growth_pitch".to_string(),
                reasoning: "BESTAETIGT_GRUND".to_string(),
                transcript_snapshot: "[\"add him on discord\"]".to_string(),
            }],
            false_positives: vec![LearningSample {
                category: "recon_smalltalk".to_string(),
                reasoning: "FEHLALARM_GRUND".to_string(),
                transcript_snapshot: "[\"welcher rang bist du\"]".to_string(),
            }],
        };
        assert_eq!(corpus.total(), 2);

        let messages = build_distill_messages(&corpus);
        let array = messages.as_array().expect("messages ist ein Array");
        assert_eq!(array[0]["role"], "system");
        assert_eq!(array[0]["content"], SCAM_LEARNING_SYSTEM_PROMPT);
        let user = array[1]["content"]
            .as_str()
            .expect("user content ist String");
        assert!(user.contains("BESTAETIGT_GRUND"));
        assert!(user.contains("FEHLALARM_GRUND"));
        assert!(user.contains("bestaetigte_faelle"));
        assert!(user.contains("aufgehobene_faelle"));
    }

    #[test]
    fn small_caps_werden_lesbar_normalisiert_und_markiert() {
        let input = "ʏᴏ ʙʀᴏ, ɪ ᴊᴜꜱᴛ ᴄᴀᴍᴇ ᴀᴄʀᴏꜱꜱ ʏᴏᴜʀ ꜱᴛʀᴇᴀᴍ. ᴀᴅᴅ ʜɪᴍ ᴏɴ ᴅɪꜱᴄᴏʀᴅ.";
        let normalized = normalize_for_judge(input);
        assert!(normalized.was_obfuscated);
        assert_eq!(
            normalized.text,
            "yo bro, i just came across your stream. add him on discord."
        );
    }

    #[test]
    fn chunk_for_twitch_haelt_grenze_und_splittet_an_wortgrenzen() {
        let chunks = chunk_for_twitch("alpha beta gamma delta", 11);
        assert!(chunks.iter().all(|c| c.chars().count() <= 11));
        assert_eq!(chunks.join(" "), "alpha beta gamma delta");
        assert_eq!(chunks.len(), 2);

        // Kein Mengen-Cap: langer Text ergibt viele Häppchen, alle <= Grenze.
        let long = "wort ".repeat(300);
        let many = chunk_for_twitch(long.trim(), 480);
        assert!(many.len() > 1);
        assert!(many.iter().all(|c| c.chars().count() <= 480));

        // Überlanges Einzelwort wird hart auf Zeichengrenzen gesplittet.
        let hard = chunk_for_twitch(&"x".repeat(25), 10);
        assert_eq!(hard.len(), 3);
        assert!(hard.iter().all(|c| c.chars().count() <= 10));

        assert!(chunk_for_twitch("", 480).is_empty());
    }

    #[test]
    fn verdict_parser_akzeptiert_json_und_json_in_fliess_text() {
        let direct = parse_verdict(
            r#"{"verdict":"scam","confidence":0.93,"category":"social","reasoning":"clear pattern"}"#,
            "testchannel",
            "testchatter",
        );
        assert_eq!(direct.verdict, VerdictKind::Scam);
        assert!((direct.confidence - 0.93).abs() < f32::EPSILON);

        let wrapped = parse_verdict(
            "analysis follows\n```json\n{\"verdict\":\"clean\",\"confidence\":0.81,\"category\":\"viewer\",\"reasoning\":\"game-specific\"}\n```",
            "testchannel",
            "testchatter",
        );
        assert_eq!(wrapped.verdict, VerdictKind::Clean);
        assert!((wrapped.confidence - 0.81).abs() < f32::EPSILON);
    }

    #[test]
    fn verdict_parser_faellt_bei_muell_sicher_auf_unsure_zurueck() {
        for raw in [
            "not json",
            r#"{"verdict":"ban","confidence":1.0}"#,
            r#"{"verdict":"scam","confidence":"high"}"#,
        ] {
            assert_eq!(
                parse_verdict(raw, "testchannel", "testchatter").verdict,
                VerdictKind::Unsure
            );
        }
    }

    #[tokio::test]
    async fn minimax_judge_sendet_wachsenden_dialog_ohne_transkript_truncation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("first substantial message"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content":
                    "{\"verdict\":\"unsure\",\"confidence\":0.4,\"category\":\"early\",\"reasoning\":\"more context\"}"
                }}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = EngagementMinimaxClient::new(
            Some("test-key".to_string()),
            Some(server.uri()),
            Some("MiniMax-M3".to_string()),
            None,
        );
        let judge = MiniMaxScamJudge::new(client);
        let mut dialog = DialogState::new(true);
        dialog.push_user_message("first substantial message with enough context");
        let first = judge.judge(&mut dialog).await;
        assert_eq!(first.verdict, VerdictKind::Unsure);
        assert_eq!(
            dialog.messages().last().map(|m| m.role.as_str()),
            Some("assistant")
        );
        let requests = server
            .received_requests()
            .await
            .expect("Wiremock-Requests verfügbar");
        let request_body = String::from_utf8_lossy(&requests[0].body);
        assert!(!request_body.contains("\"max_tokens\""));

        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("more context"))
            .and(body_string_contains("second message continues the conversation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content":
                    "{\"verdict\":\"scam\",\"confidence\":0.95,\"category\":\"social\",\"reasoning\":\"clear pivot\"}"
                }}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        dialog.push_user_message("second message continues the conversation");
        let second = judge.judge(&mut dialog).await;
        assert_eq!(second.verdict, VerdictKind::Scam);
    }

    #[tokio::test]
    #[ignore = "benötigt produktive MiniMax-Zugangsdaten"]
    async fn live_minimax_erkennt_gemeldeten_befriending_pivot_als_sicheren_scam() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let judge: Arc<dyn ScamJudge> = Arc::new(MiniMaxScamJudge::new(
            EngagementMinimaxClient::new(None, None, None, None),
        ));
        let settings = GuardSettings::default();
        let enforcement_threshold = effective_scam_enforcement_threshold(&settings, Some(0));
        let (guard, store, api, moderation) = build_guard_with_judge(settings, judge);
        *api.created_at.lock().unwrap() = Ok(Some(Utc::now()));

        feed(&guard, "reported_account", &[REPORTED_BEFRIENDING_PIVOT]).await;

        let record = store.records.lock().unwrap()[0].clone();
        eprintln!(
            "Live-Scam-Guard-Urteil: verdict={:?}, confidence={}, threshold={}, category={}, action={}",
            record.verdict,
            record.confidence,
            enforcement_threshold,
            record.category,
            record.action_taken
        );
        assert_eq!(record.verdict, VerdictKind::Scam);
        assert!(
            record.confidence >= enforcement_threshold,
            "Befriending-Pivot muss die altersabhängige Auto-Ban-Schwelle erreichen: {record:?}"
        );
        assert_eq!(record.action_taken, "banned");
        assert_eq!(moderation.reasons.lock().unwrap().len(), 1);
        assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
    }

    struct MockStore {
        settings: StdMutex<GuardSettings>,
        context: StdMutex<Option<FirstTimeContext>>,
        cross_channel: StdMutex<Result<i64, String>>,
        records: StdMutex<Vec<VerdictRecord>>,
        global_bans: StdMutex<Vec<(String, Option<String>, String)>>,
    }

    #[async_trait]
    impl ScamGuardStore for MockStore {
        async fn load_settings(
            &self,
            _channel_login: &str,
            _channel_user_id: Option<&str>,
        ) -> Result<GuardSettings, String> {
            Ok(self.settings.lock().unwrap().clone())
        }

        async fn first_time_context(
            &self,
            _channel_login: &str,
            _chatter_login: &str,
        ) -> Result<Option<FirstTimeContext>, String> {
            Ok(*self.context.lock().unwrap())
        }

        async fn cross_channel_first_messages(
            &self,
            _channel_login: &str,
            _chatter_login: &str,
            _window_minutes: i64,
        ) -> Result<i64, String> {
            self.cross_channel.lock().unwrap().clone()
        }

        async fn persist_verdict(&self, record: &VerdictRecord) -> Result<i64, String> {
            let mut records = self.records.lock().unwrap();
            records.push(record.clone());
            Ok(records.len() as i64)
        }

        async fn add_global_ban(
            &self,
            chatter_login: &str,
            chatter_id: Option<&str>,
            reason: &str,
        ) -> Result<(), String> {
            self.global_bans.lock().unwrap().push((
                chatter_login.to_lowercase(),
                chatter_id.map(str::to_string),
                reason.to_string(),
            ));
            Ok(())
        }
    }

    struct MockJudge {
        calls: AtomicUsize,
        verdicts: StdMutex<VecDeque<Verdict>>,
    }

    impl MockJudge {
        fn new(verdicts: impl IntoIterator<Item = Verdict>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                verdicts: StdMutex::new(verdicts.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl ScamJudge for MockJudge {
        async fn judge(&self, _dialog: &mut DialogState) -> Verdict {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.verdicts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(Verdict::unsure)
        }
    }

    fn enforce_target(login: &str, id: Option<&str>) -> EnforceTarget {
        EnforceTarget {
            channel_login: "kanal".to_string(),
            chatter_login: login.to_string(),
            chatter_id: id.map(str::to_string),
            action_taken: String::new(),
            reasoning: "Scam-Verdacht".to_string(),
        }
    }

    /// `try_ban` bannt direkt, an `auto_ban_and_cleanup` vorbei.
    #[tokio::test]
    async fn try_ban_verschont_safe_konten() {
        for safe in crate::safe_list::SAFE_ACCOUNTS {
            let api = MockApi::resolving("broadcast-id");
            let target = enforce_target(safe.login, Some(safe.twitch_user_id));

            assert!(!try_ban(&api, &target).await, "Safe-Konto {}", safe.login);
            assert!(
                api.ban_reasons.lock().unwrap().is_empty(),
                "Safe-Konto {} wurde gebannt",
                safe.login
            );
        }
    }

    /// Merge-Kritiker 2026-07-10: Ein Eintrag OHNE gespeicherte chatter_id löst
    /// den Login per Helix auf. Zeigt die Auflösung auf ein Safe-Konto, darf
    /// trotzdem kein Ban fallen — der Guard muss NACH der Auflösung greifen.
    #[tokio::test]
    async fn try_ban_verschont_safe_konto_das_erst_per_login_aufloest() {
        for safe in crate::safe_list::SAFE_ACCOUNTS {
            // Der gespeicherte Login ist unverdächtig (z. B. nach Umbenennung),
            // erst die Auflösung liefert die Safe-ID.
            let api = MockApi::resolving(safe.twitch_user_id);
            let target = enforce_target("unbekannter_alias", None);

            assert!(
                !try_ban(&api, &target).await,
                "aufgeloeste Safe-ID {} wurde gebannt",
                safe.twitch_user_id
            );
            assert!(
                api.ban_reasons.lock().unwrap().is_empty(),
                "Safe-Konto {} wurde nach Login-Aufloesung gebannt",
                safe.login
            );
        }
    }

    /// Gegenprobe: ohne Safe-List-Treffer bannt derselbe Pfad wirklich.
    #[tokio::test]
    async fn try_ban_bannt_fremdes_konto() {
        let api = MockApi::resolving("broadcast-id");
        let target = enforce_target("irgendwer", Some("999999999"));

        assert!(try_ban(&api, &target).await);
        assert_eq!(api.ban_reasons.lock().unwrap().len(), 1);
    }

    /// Gegenprobe zur Login-Auflösung: fremde ID wird gebannt.
    #[tokio::test]
    async fn try_ban_bannt_fremdes_konto_nach_login_aufloesung() {
        let api = MockApi::resolving("999999999");
        let target = enforce_target("irgendwer", None);

        assert!(try_ban(&api, &target).await);
        assert_eq!(api.ban_reasons.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn try_ban_kuerzt_langen_twitch_ban_reason() {
        let api = MockApi::resolving("999999999");
        let mut target = enforce_target("irgendwer", Some("999999999"));
        target.reasoning = "x".repeat(650);

        assert!(try_ban(&api, &target).await);
        let reasons = api.ban_reasons.lock().unwrap();
        assert_eq!(reasons.len(), 1);
        assert_eq!(
            reasons[0].chars().count(),
            crate::moderation::TWITCH_MODERATION_REASON_MAX_CHARS
        );
    }

    struct MockApi {
        ban_result: StdMutex<BanOutcome>,
        timeout_result: StdMutex<BanOutcome>,
        ban_reasons: StdMutex<Vec<String>>,
        timeout_reasons: StdMutex<Vec<String>>,
        created_at: StdMutex<Result<Option<chrono::DateTime<Utc>>, String>>,
        created_at_calls: AtomicUsize,
        /// Ergebnis von `resolve_user_id`. Default `None` (bisheriges Verhalten).
        resolve_id: StdMutex<Option<String>>,
    }

    impl MockApi {
        fn new() -> Self {
            Self {
                ban_result: StdMutex::new(BanOutcome::Banned),
                timeout_result: StdMutex::new(BanOutcome::Banned),
                ban_reasons: StdMutex::new(Vec::new()),
                timeout_reasons: StdMutex::new(Vec::new()),
                created_at: StdMutex::new(Ok(Some(Utc::now() - chrono::Duration::days(10)))),
                created_at_calls: AtomicUsize::new(0),
                resolve_id: StdMutex::new(None),
            }
        }

        /// Lässt `resolve_user_id` auf `id` auflösen — nötig, damit `try_ban`
        /// überhaupt bis zum Ban-Aufruf kommt.
        fn resolving(id: &str) -> Self {
            let api = Self::new();
            *api.resolve_id.lock().unwrap() = Some(id.to_string());
            api
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn ban_user(&self, _: &str, _: &str, reason: &str) -> Result<BanOutcome, String> {
            self.ban_reasons.lock().unwrap().push(reason.to_string());
            Ok(self.ban_result.lock().unwrap().clone())
        }
        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            reason: &str,
        ) -> Result<BanOutcome, String> {
            self.timeout_reasons
                .lock()
                .unwrap()
                .push(reason.to_string());
            Ok(self.timeout_result.lock().unwrap().clone())
        }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn user_created_at(
            &self,
            _: &str,
        ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
            self.created_at_calls.fetch_add(1, Ordering::SeqCst);
            self.created_at.lock().unwrap().clone()
        }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            Ok(self.resolve_id.lock().unwrap().clone())
        }
        async fn bot_user_id(&self) -> String {
            "bot-id".to_string()
        }
    }

    type TimeoutEvidence = (Option<String>, Option<String>, Option<String>);

    struct MockModeration {
        succeeds: StdMutex<bool>,
        reasons: StdMutex<Vec<String>>,
        timeout_reasons: StdMutex<Vec<String>>,
        timeout_evidence: StdMutex<Vec<TimeoutEvidence>>,
    }

    #[async_trait]
    impl ScamModeration for MockModeration {
        async fn auto_ban_and_cleanup(&self, request: AutoBanRequest<'_>) -> bool {
            self.reasons
                .lock()
                .unwrap()
                .push(request.reason_text.to_string());
            *self.succeeds.lock().unwrap()
        }

        async fn timeout_and_cleanup(
            &self,
            channel_login: Option<&str>,
            _broadcaster_id: &str,
            chatter_login: Option<&str>,
            _chatter_id: &str,
            _message_id: &str,
            content: Option<&str>,
            _duration_secs: u32,
            reason_text: &str,
        ) -> bool {
            self.timeout_reasons
                .lock()
                .unwrap()
                .push(reason_text.to_string());
            self.timeout_evidence.lock().unwrap().push((
                channel_login.map(str::to_string),
                chatter_login.map(str::to_string),
                content.map(str::to_string),
            ));
            true
        }
    }

    #[derive(Default)]
    struct RecordingNotifier {
        seen: StdMutex<Vec<ScamNotification>>,
    }

    #[async_trait]
    impl ScamGuardNotifier for RecordingNotifier {
        async fn notify(&self, notification: ScamNotification) {
            self.seen.lock().unwrap().push(notification);
        }
    }

    fn build_guard_with_judge(
        settings: GuardSettings,
        judge: Arc<dyn ScamJudge>,
    ) -> (
        ConversationScamGuard,
        Arc<MockStore>,
        Arc<MockApi>,
        Arc<MockModeration>,
    ) {
        let store = Arc::new(MockStore {
            settings: StdMutex::new(settings),
            context: StdMutex::new(Some(FirstTimeContext {
                is_first_time_streamer: true,
                is_first_global: true,
            })),
            cross_channel: StdMutex::new(Ok(0)),
            records: StdMutex::new(Vec::new()),
            global_bans: StdMutex::new(Vec::new()),
        });
        let api = Arc::new(MockApi::new());
        let moderation = Arc::new(MockModeration {
            succeeds: StdMutex::new(true),
            reasons: StdMutex::new(Vec::new()),
            timeout_reasons: StdMutex::new(Vec::new()),
            timeout_evidence: StdMutex::new(Vec::new()),
        });
        let guard = ConversationScamGuard::with_store(
            "bot-id".to_string(),
            Arc::clone(&store) as Arc<dyn ScamGuardStore>,
            judge,
            Arc::clone(&api) as Arc<dyn ChatApi>,
            Arc::clone(&moderation) as Arc<dyn ScamModeration>,
        );
        (guard, store, api, moderation)
    }

    fn build_guard(
        settings: GuardSettings,
        verdicts: impl IntoIterator<Item = Verdict>,
    ) -> (
        ConversationScamGuard,
        Arc<MockStore>,
        Arc<MockJudge>,
        Arc<MockApi>,
        Arc<MockModeration>,
    ) {
        let judge = Arc::new(MockJudge::new(verdicts));
        let judge_port: Arc<dyn ScamJudge> = judge.clone();
        let (guard, store, api, moderation) = build_guard_with_judge(settings, judge_port);
        (guard, store, judge, api, moderation)
    }

    async fn feed(guard: &ConversationScamGuard, login: &str, messages: &[&str]) {
        for text in messages {
            guard.process(&event(login, text)).await;
        }
    }

    #[tokio::test]
    async fn gemeldeter_befriending_pivot_bannt_null_tage_account_ab_80_prozent() {
        let (guard, store, judge, api, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.80)]);
        *api.created_at.lock().unwrap() = Ok(Some(Utc::now()));

        feed(&guard, "reported_account", &[REPORTED_BEFRIENDING_PIVOT]).await;

        assert_eq!(judge.calls.load(Ordering::SeqCst), 1);
        let records = store.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].verdict, VerdictKind::Scam);
        assert_eq!(records[0].action_taken, "banned");
        drop(records);
        assert_eq!(moderation.reasons.lock().unwrap().len(), 1);
        assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
        assert_eq!(api.created_at_calls.load(Ordering::SeqCst), 1);
    }

    /// Der gemeldete Verlauf besteht aus sehr kurzen Nachrichten. Erst die
    /// sechste erreicht die Substanz-Schwelle — vorher wird der Judge gar nicht
    /// gefragt. Bricht der Chatter früher ab, sieht der Guard nichts.
    #[tokio::test]
    async fn gemeldeter_recon_smalltalk_erreicht_den_judge_erst_bei_nachricht_sechs() {
        let (guard, store, judge, api, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.85)]);
        *api.created_at.lock().unwrap() = Ok(Some(Utc::now()));

        feed(&guard, "recon_account", &REPORTED_RECON_SMALLTALK[..5]).await;
        assert_eq!(judge.calls.load(Ordering::SeqCst), 0);
        assert!(store.records.lock().unwrap().is_empty());

        feed(&guard, "recon_account", &REPORTED_RECON_SMALLTALK[5..]).await;

        assert_eq!(judge.calls.load(Ordering::SeqCst), 1);
        let records = store.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].action_taken, "banned");
        drop(records);
        assert_eq!(moderation.reasons.lock().unwrap().len(), 1);
    }

    /// Mit Netzwerk-Signal (derselbe Account binnen einer Stunde in einem
    /// weiteren Kanal) fällt die Substanz-Schwelle weg: der Judge urteilt schon
    /// nach der ersten Nachricht.
    #[tokio::test]
    async fn recon_smalltalk_mit_netzwerksignal_urteilt_ab_der_ersten_nachricht() {
        let (guard, store, judge, api, _moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.85)]);
        *api.created_at.lock().unwrap() = Ok(Some(Utc::now()));
        *store.cross_channel.lock().unwrap() = Ok(1);

        feed(&guard, "recon_account", &REPORTED_RECON_SMALLTALK[..1]).await;

        assert_eq!(judge.calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "banned");
    }

    /// Live-Baseline: schickt den gemeldeten Verlauf an das Modell, das auch
    /// live urteilt. Ohne echten Call ist „hätte der Bot gebannt?" nicht
    /// beantwortbar — der Mock-Judge misst nur die Verdrahtung.
    #[tokio::test]
    #[ignore = "Live-Baseline: braucht FIREWORK_API_KEY oder MINIMAX_API_KEY"]
    async fn live_judge_baseline_gemeldeter_recon_smalltalk() {
        let judge = MiniMaxScamJudge::new(EngagementMinimaxClient::new(None, None, None, None));
        let enforcement_threshold =
            effective_scam_enforcement_threshold(&GuardSettings::default(), Some(0));
        let mut would_ban = 0usize;

        for other_channels in [0i64, 1] {
            let mut dialog = DialogState::with_context(
                true,
                Some(0),
                other_channels,
                None,
                "partnerkanal",
                "recon_account",
            );
            for message in REPORTED_RECON_SMALLTALK {
                dialog.push_user_message(message);
            }
            assert!(dialog.has_enough_substance(), "Judge würde nie gefragt");

            let verdict = judge.judge(&mut dialog).await;
            let bans = verdict.verdict == VerdictKind::Scam
                && verdict.confidence >= enforcement_threshold;
            would_ban += usize::from(bans);
            eprintln!(
                "LIVE_BASELINE other_channels={other_channels} verdict={} confidence={:.2} threshold={enforcement_threshold:.2} category={} ban={bans} reasoning={}",
                verdict.verdict.as_str(),
                verdict.confidence,
                verdict.category,
                verdict.reasoning
            );
            assert!(
                !(verdict.verdict == VerdictKind::Unsure && verdict.confidence == 0.0),
                "fail-safe unsure — LLM nicht erreichbar, Baseline nicht messbar"
            );
        }

        assert!(
            would_ban > 0,
            "gemeldeter Verlauf löst in keiner Konstellation einen Ban aus"
        );
    }

    #[tokio::test]
    async fn alterskurve_steigt_von_80_auf_90_prozent() {
        for (age_days, confidence, expected_action) in [
            (0, 0.79, "suggested"),
            (0, 0.80, "banned"),
            (30, 0.84, "suggested"),
            (30, 0.85, "banned"),
            (60, 0.89, "suggested"),
            (60, 0.90, "banned"),
            (61, 0.89, "suggested"),
        ] {
            let (guard, store, _, api, moderation) = build_guard(
                GuardSettings::default(),
                [verdict(VerdictKind::Scam, confidence)],
            );
            *api.created_at.lock().unwrap() =
                Ok(Some(Utc::now() - chrono::Duration::days(age_days)));
            feed(
                &guard,
                &format!("age_{age_days}_confidence_{confidence}"),
                &[REPORTED_BEFRIENDING_PIVOT],
            )
            .await;

            assert_eq!(
                store.records.lock().unwrap()[0].action_taken,
                expected_action,
                "Alter {age_days}, Confidence {confidence}"
            );
            assert_eq!(
                moderation.reasons.lock().unwrap().len(),
                usize::from(expected_action == "banned")
            );
        }
    }

    #[test]
    fn alterskurve_interpoliert_und_respektiert_konfiguration() {
        let settings = GuardSettings::default();
        for (age_days, expected) in [(0, 0.80), (30, 0.85), (60, 0.90)] {
            let actual = effective_scam_enforcement_threshold(&settings, Some(age_days));
            assert!(
                (actual - expected).abs() < f32::EPSILON,
                "Alter {age_days}: {actual} statt {expected}"
            );
        }
        assert_eq!(
            effective_scam_enforcement_threshold(&settings, Some(61)),
            0.90
        );
        assert_eq!(
            effective_scam_enforcement_threshold(&settings, Some(-1)),
            0.90
        );

        let stricter = GuardSettings {
            threshold: 0.95,
            ..GuardSettings::default()
        };
        assert!(
            (effective_scam_enforcement_threshold(&stricter, Some(30)) - 0.875).abs()
                < f32::EPSILON
        );

        let lower_channel_threshold = GuardSettings {
            threshold: 0.75,
            ..GuardSettings::default()
        };
        assert_eq!(
            effective_scam_enforcement_threshold(&lower_channel_threshold, Some(0)),
            0.75
        );

        let high_floor = GuardSettings {
            suggestion_floor: 0.85,
            ..GuardSettings::default()
        };
        assert_eq!(
            effective_scam_enforcement_threshold(&high_floor, Some(0)),
            0.85
        );

        let floor_above_threshold = GuardSettings {
            threshold: 0.80,
            suggestion_floor: 0.85,
            ..GuardSettings::default()
        };
        assert_eq!(
            effective_scam_enforcement_threshold(&floor_above_threshold, Some(0)),
            0.85
        );
    }

    #[tokio::test]
    async fn alterskurve_greift_nur_im_autoban_mit_gueltigem_alter() {
        let (guard, store, _, api, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.89)]);
        *api.created_at.lock().unwrap() = Ok(Some(Utc::now() + chrono::Duration::days(2)));
        feed(&guard, "future_timestamp", &[REPORTED_BEFRIENDING_PIVOT]).await;
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "suggested");
        assert!(moderation.reasons.lock().unwrap().is_empty());

        for mode in [GuardMode::AlertOnly, GuardMode::Timeout] {
            let settings = GuardSettings {
                mode,
                ..GuardSettings::default()
            };
            let (guard, store, _, api, moderation) =
                build_guard(settings, [verdict(VerdictKind::Scam, 0.89)]);
            *api.created_at.lock().unwrap() = Ok(Some(Utc::now()));
            feed(&guard, "non_autoban", &[REPORTED_BEFRIENDING_PIVOT]).await;
            assert_eq!(store.records.lock().unwrap()[0].action_taken, "suggested");
            assert!(moderation.reasons.lock().unwrap().is_empty());
            assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn roh_korpus_sophia_minnie_sam_wird_mit_mock_judge_als_scam_persistiert() {
        let cases: [(&str, &[&str]); 3] = [
            (
                "sophiaa_star",
                &[
                    "How is the day going? I am waiting for you.",
                    "What other games are you into? You have good taste.",
                    "How long have you been streaming and what do you do besides streaming?",
                ],
            ),
            (
                "minniepearl19",
                &[
                    "Heya streamer",
                    "How's it going?",
                    "If possible can we talk on chat now?",
                ],
            ),
            (
                "sam_09995",
                &["ʏᴏ ʙʀᴏ, ɪ ᴊᴜꜱᴛ ᴄᴀᴍᴇ ᴀᴄʀᴏꜱꜱ ʏᴏᴜʀ ꜱᴛʀᴇᴀᴍ ᴀɴᴅ ᴅʀᴏᴘᴘᴇᴅ ᴀ ꜰᴏʟʟᴏᴡ. ᴀᴅᴅ ʜɪᴍ ᴏɴ ᴅɪꜱᴄᴏʀᴅ."],
            ),
        ];

        for (login, transcript) in cases {
            let settings = GuardSettings {
                mode: GuardMode::AlertOnly,
                ..GuardSettings::default()
            };
            let (guard, store, _, _, _) = build_guard(settings, [verdict(VerdictKind::Scam, 0.95)]);
            feed(&guard, login, transcript).await;
            let records = store.records.lock().unwrap();
            assert_eq!(records.last().map(|r| r.verdict), Some(VerdictKind::Scam));
            assert_eq!(
                records.last().map(|r| r.action_taken.as_str()),
                Some("suggested")
            );
        }
    }

    #[tokio::test]
    async fn notifier_postet_bestaetigte_aktion_mit_verdict_id() {
        let settings = GuardSettings {
            mode: GuardMode::AlertOnly,
            ..GuardSettings::default()
        };
        let (guard, _store, _, _, _) = build_guard(settings, [verdict(VerdictKind::Scam, 0.95)]);
        let notifier = Arc::new(RecordingNotifier::default());
        let guard = guard.with_notifier(Arc::clone(&notifier) as Arc<dyn ScamGuardNotifier>);

        feed(
            &guard,
            "sam_09995",
            &[
                "yo bro add him on discord and grow with real viewers",
                "real supporters who donate and sub, tell him sam sent you",
                "just add him quick, he connects you to a big streamer",
                "you deserve it, you have good taste",
            ],
        )
        .await;

        // notify() läuft fire-and-forget via tokio::spawn → kurz auf den Task warten.
        let seen = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                {
                    let guard = notifier.seen.lock().unwrap();
                    if !guard.is_empty() {
                        break guard.clone();
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Notifier wurde nicht aufgerufen");

        assert_eq!(
            seen.len(),
            1,
            "genau eine bestätigte Aktion → ein Discord-Post"
        );
        assert_eq!(seen[0].action_taken, "suggested");
        assert_eq!(seen[0].verdict, "scam");
        assert_eq!(seen[0].verdict_id, 1);
        assert_eq!(seen[0].chatter_login, "sam_09995");
    }

    #[tokio::test]
    async fn unsure_ab_suggestion_floor_bleibt_ohne_discord_post_im_monitor() {
        let (guard, store, _, api, moderation) = build_guard(
            GuardSettings::default(),
            [verdict(VerdictKind::Unsure, 0.75)],
        );
        let notifier = Arc::new(RecordingNotifier::default());
        let guard = guard.with_notifier(Arc::clone(&notifier) as Arc<dyn ScamGuardNotifier>);

        feed(
            &guard,
            "unsure_visible",
            &["This is a sufficiently long generic opening message without any real stream context."],
        )
        .await;

        assert_eq!(store.records.lock().unwrap()[0].action_taken, "watching");
        assert!(notifier.seen.lock().unwrap().is_empty());
        assert!(api.ban_reasons.lock().unwrap().is_empty());
        assert!(moderation.reasons.lock().unwrap().is_empty());
        assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unsure_unter_suggestion_floor_bleibt_ohne_discord_post_im_monitor() {
        let (guard, store, _, _, moderation) = build_guard(
            GuardSettings::default(),
            [verdict(VerdictKind::Unsure, 0.69)],
        );
        let notifier = Arc::new(RecordingNotifier::default());
        let guard = guard.with_notifier(Arc::clone(&notifier) as Arc<dyn ScamGuardNotifier>);

        feed(
            &guard,
            "unsure_quiet",
            &["This is a sufficiently long generic opening message without any real stream context."],
        )
        .await;

        assert_eq!(store.records.lock().unwrap()[0].action_taken, "watching");
        assert!(notifier.seen.lock().unwrap().is_empty());
        assert!(moderation.reasons.lock().unwrap().is_empty());
        assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn junger_englischer_recon_burst_bleibt_als_watching_im_monitor() {
        let (guard, store, _, api, moderation) = build_guard(
            GuardSettings::default(),
            [verdict(VerdictKind::Unsure, 0.40)],
        );
        *api.created_at.lock().unwrap() = Ok(Some(Utc::now() - chrono::Duration::days(50)));
        let notifier = Arc::new(RecordingNotifier::default());
        let guard = guard.with_notifier(Arc::clone(&notifier) as Arc<dyn ScamGuardNotifier>);

        feed(
            &guard,
            "bunnyrae_7",
            &[
                "yeeeee",
                "Deadlock xd",
                "I really like deadlock is this your fvrt game?",
                "Is your mic is muted or are u just not talking atm?",
                "umm",
                "did u read my messages?",
            ],
        )
        .await;

        assert_eq!(store.records.lock().unwrap()[0].action_taken, "watching");
        assert!(moderation.reasons.lock().unwrap().is_empty());
        assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
        assert!(notifier.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn clean_verdict_wird_nach_discord_gemeldet() {
        let (guard, store, _, _, _) = build_guard(
            GuardSettings::default(),
            [verdict(VerdictKind::Clean, 0.98)],
        );
        let notifier = Arc::new(RecordingNotifier::default());
        let guard = guard.with_notifier(Arc::clone(&notifier) as Arc<dyn ScamGuardNotifier>);

        feed(
            &guard,
            "viewer_de",
            &[
                "lohnt sich Haze grad? oder eher nerf gekriegt",
                "gg, was baust du auf McGinnis?",
                "welches Item kaufst du danach?",
                "noch eine konkrete Build-Frage zum Schluss",
            ],
        )
        .await;

        let seen = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let Some(notification) = notifier.seen.lock().unwrap().first().cloned() {
                    break notification;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Clean-Urteil wurde nicht gemeldet");
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "none");
        assert_eq!(seen.action_taken, "none");
        assert_eq!(seen.verdict, "clean");
    }

    #[tokio::test]
    async fn auto_ban_postet_mit_action_banned() {
        let settings = GuardSettings {
            mode: GuardMode::AutoBan,
            ..GuardSettings::default()
        };
        let (guard, store, _, _, moderation) =
            build_guard(settings, [verdict(VerdictKind::Scam, 0.97)]);
        let notifier = Arc::new(RecordingNotifier::default());
        let guard = guard.with_notifier(Arc::clone(&notifier) as Arc<dyn ScamGuardNotifier>);

        feed(
            &guard,
            "sam_09995",
            &[
                "yo bro add him on discord and grow with real viewers",
                "real supporters who donate and sub, tell him sam sent you",
                "just add him quick, he connects you to a big streamer",
                "trust me you deserve it, you have good taste",
            ],
        )
        .await;

        // Der Ban lief wirklich über die Moderation, und genau ein Post ging raus.
        assert_eq!(moderation.reasons.lock().unwrap().len(), 1);
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "banned");
        let seen = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                {
                    let g = notifier.seen.lock().unwrap();
                    if !g.is_empty() {
                        break g.clone();
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Notifier wurde nach Auto-Ban nicht aufgerufen");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].action_taken, "banned");
        assert_eq!(seen[0].chatter_login, "sam_09995");
    }

    #[tokio::test]
    async fn kontrastfall_wird_clean_und_dialog_ist_erledigt() {
        let (guard, store, judge, _, _) = build_guard(
            GuardSettings::default(),
            [verdict(VerdictKind::Clean, 0.98)],
        );
        feed(
            &guard,
            "viewer_de",
            &[
                "lohnt sich Haze grad? oder eher nerf gekriegt",
                "gg, was baust du auf McGinnis?",
                "welches Item kaufst du danach?",
                "noch eine konkrete Build-Frage",
            ],
        )
        .await;
        assert_eq!(judge.calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.records.lock().unwrap()[0].verdict, VerdictKind::Clean);
    }

    #[tokio::test]
    async fn decision_matrix_beachtet_schwellen_und_modi() {
        let (guard, store, _, api, moderation) = build_guard(
            GuardSettings::default(),
            [
                verdict(VerdictKind::Scam, 0.89),
                verdict(VerdictKind::Scam, 0.90),
            ],
        );
        *api.created_at.lock().unwrap() = Ok(Some(Utc::now() - chrono::Duration::days(61)));
        feed(
            &guard,
            "threshold_user",
            &[
                "first substantial suspicious message about channel growth",
                "second substantial suspicious message about active viewers",
                "third substantial suspicious message about Discord contact",
                "one more suspicious message to raise confidence",
            ],
        )
        .await;
        {
            let records = store.records.lock().unwrap();
            assert_eq!(records[0].action_taken, "suggested");
            assert_eq!(records[1].action_taken, "banned");
        }
        assert_eq!(
            moderation.reasons.lock().unwrap().as_slice(),
            ["test reasoning"]
        );

        let timeout_settings = GuardSettings {
            mode: GuardMode::Timeout,
            ..GuardSettings::default()
        };
        let (guard, store, _, _, moderation) =
            build_guard(timeout_settings, [verdict(VerdictKind::Scam, 0.95)]);
        feed(
            &guard,
            "timeout_user",
            &["This long growth pitch asks the streamer to add somebody on Discord immediately."],
        )
        .await;
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "timed_out");
        assert_eq!(
            moderation.timeout_reasons.lock().unwrap().as_slice(),
            ["test reasoning"]
        );
        assert_eq!(
            moderation.timeout_evidence.lock().unwrap().as_slice(),
            [(
                Some("testchannel".to_string()),
                Some("timeout_user".to_string()),
                Some(
                    "This long growth pitch asks the streamer to add somebody on Discord immediately."
                        .to_string()
                ),
            )]
        );
    }

    #[tokio::test]
    async fn ban_forbidden_wird_als_no_mod_fallback_persistiert() {
        let (guard, store, _, _, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.97)]);
        *moderation.succeeds.lock().unwrap() = false;
        let notifier = Arc::new(RecordingNotifier::default());
        let guard = guard.with_notifier(Arc::clone(&notifier) as Arc<dyn ScamGuardNotifier>);
        feed(
            &guard,
            "forbidden_user",
            &["This long growth pitch asks the streamer to add somebody on Discord immediately."],
        )
        .await;
        assert_eq!(
            store.records.lock().unwrap()[0].action_taken,
            "ban_failed_no_mod"
        );
        let seen = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let Some(notification) = notifier.seen.lock().unwrap().first().cloned() {
                    break notification;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Ban-Fehlschlag wurde nicht gemeldet");
        assert_eq!(seen.action_taken, "ban_failed_no_mod");
        assert_eq!(seen.verdict, "scam");
    }

    #[tokio::test]
    async fn klarer_ai_scam_wird_global_vorgemerkt_auch_wenn_channelban_scheitert() {
        let (guard, store, _, _, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.97)]);
        *moderation.succeeds.lock().unwrap() = false;

        feed(
            &guard,
            "ghostchambers83",
            &["Working right now, but my crew and I won't miss your next stream. Looking forward to hanging out! Add me on D1sc0rd: remah7"],
        )
        .await;

        assert_eq!(
            store.records.lock().unwrap()[0].action_taken,
            "ban_failed_no_mod"
        );
        assert_eq!(
            store.global_bans.lock().unwrap().as_slice(),
            [(
                "ghostchambers83".to_string(),
                Some("ghostchambers83-id".to_string()),
                "test reasoning".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn auto_ban_timeoutet_bei_unbekanntem_account_alter() {
        let (guard, store, _, api, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.97)]);
        *api.created_at.lock().unwrap() = Ok(None);

        feed(
            &guard,
            "unknown_age_user",
            &["This long growth pitch asks the streamer to add somebody on Discord immediately."],
        )
        .await;

        assert_eq!(store.records.lock().unwrap()[0].action_taken, "timed_out");
        assert!(moderation.reasons.lock().unwrap().is_empty());
        assert_eq!(moderation.timeout_reasons.lock().unwrap().len(), 1);
        assert_eq!(api.created_at_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn auto_ban_timeoutet_bei_account_alter_api_fehler() {
        let (guard, store, _, api, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.97)]);
        *api.created_at.lock().unwrap() = Err("helix unavailable".to_string());

        feed(
            &guard,
            "age_error_user",
            &["This long growth pitch asks the streamer to add somebody on Discord immediately."],
        )
        .await;

        assert_eq!(store.records.lock().unwrap()[0].action_taken, "timed_out");
        assert!(moderation.reasons.lock().unwrap().is_empty());
        assert_eq!(moderation.timeout_reasons.lock().unwrap().len(), 1);
        assert_eq!(api.created_at_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn auto_ban_bannt_bei_sicher_bekanntem_frischem_account() {
        let (guard, store, _, api, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.97)]);

        feed(
            &guard,
            "young_account_user",
            &["This long growth pitch asks the streamer to add somebody on Discord immediately."],
        )
        .await;

        assert_eq!(store.records.lock().unwrap()[0].action_taken, "banned");
        assert_eq!(moderation.reasons.lock().unwrap().len(), 1);
        assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
        assert_eq!(api.created_at_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cross_channel_db_fehler_faellt_auf_null_zurueck_und_bannt_nicht() {
        let (guard, store, judge, api, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.99)]);
        *store.cross_channel.lock().unwrap() = Err("database unavailable".to_string());

        feed(&guard, "db_error_user", &["hey"]).await;

        assert_eq!(judge.calls.load(Ordering::SeqCst), 0);
        assert!(store.records.lock().unwrap().is_empty());
        assert!(api.ban_reasons.lock().unwrap().is_empty());
        assert!(moderation.reasons.lock().unwrap().is_empty());
        assert!(moderation.timeout_reasons.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabled_und_unsure_bannen_nie() {
        let disabled = GuardSettings {
            enabled: false,
            ..GuardSettings::default()
        };
        let (guard, store, judge, _api, moderation) =
            build_guard(disabled, [verdict(VerdictKind::Scam, 1.0)]);
        feed(
            &guard,
            "disabled_user",
            &["This is a long suspicious Discord growth pitch with real viewers."],
        )
        .await;
        assert_eq!(judge.calls.load(Ordering::SeqCst), 0);
        assert!(store.records.lock().unwrap().is_empty());
        assert!(moderation.reasons.lock().unwrap().is_empty());

        let (guard, store, _, api, moderation) =
            build_guard(GuardSettings::default(), [Verdict::unsure()]);
        feed(
            &guard,
            "unsure_user",
            &["This is a long suspicious Discord growth pitch with real viewers."],
        )
        .await;
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "watching");
        assert!(api.ban_reasons.lock().unwrap().is_empty());
        assert!(moderation.reasons.lock().unwrap().is_empty());
    }
}
