use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
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

const SCAM_JUDGE_SYSTEM_PROMPT: &str = r#"Du bist ein wachsamer Chat-Moderator für den Twitch-Kanal eines deutschsprachigen Deadlock-Streamers. Du beurteilst, ob ein ERSTSCHREIBER (jemand, der zum ersten Mal in diesem Kanal schreibt) eine aufgesetzte, betrügerische Konversation führt.

Du bekommst die Nachrichten EINES Chatters nacheinander als JSON-Objekte mit den Feldern "message" (der Text), "is_first_global" (true = dieser Chatter wurde im ganzen Netzwerk noch nie gesehen) und "unicode_obfuscation_detected" (true = die Schrift war verfremdet). Bewerte immer den GESAMTEN bisherigen Verlauf, nicht nur die letzte Nachricht.

Zwei Maschen, auf die du achtest:

1) Beziehungs- und Vertrauens-Masche: generischer Beziehungsaufbau ohne echten Spielbezug ("Heya", "How's it going?", "How's your day been?", "Welcome back <3"), übertrieben schleimiges Dauerlob ohne Anlass ("you have good taste", "you deserve it"), einseitiges, vorgefertigt wirkendes Reden, persönliche Ausfrage-Fragen (Wohnort, Job, Alter, Uhrzeit bei dir, PC oder PS5, "wie lange streamst du schon"), Mitleids-Haken ("hab kein Geld, aber ich probier's"), und am Ende der Pivot weg von Twitch: "can we talk on chat now?", "can we connect?", Discord, Freundschaftsanfrage.

2) Wachstums- und Clout-Pitch (oft EINE einzige lange Nachricht): unaufgefordertes Angebot, deinen Kanal "wachsen" zu lassen oder dich mit einem "großen Streamer" zu verbinden, geködert mit "real viewers, active chat, supporters who donate and sub", und der Aufforderung "add him on Discord … tell him X sent you". Häufig in verfremdeter Schrift, um Filter zu täuschen.

Gewichtung der Indizien:
- Sprache: Diese Scammer schreiben fast immer Englisch in einem deutschsprachigen Kanal. Ein englischsprachiger Erstschreiber, der sofort Beziehungs-Smalltalk oder einen Wachstums-Pitch fährt, ist deutlich verdächtiger. Deutschsprachige Erstschreiber sind selten diese Masche — im Zweifel "clean" oder "unsure".
- "unicode_obfuscation_detected": true (verfremdete Schrift) ist ein starkes Verdachtssignal.
- "is_first_global": true (netzwerkweit brandneu) erhöht den Verdacht leicht.

Echte neue Zuschauer unterscheiden sich klar: konkrete Spiel- oder Stream-Fragen ("lohnt sich Haze?", "welcher Rang bist du?", "was baust du auf McGinnis?"), echte Reaktionen auf das Geschehen, kein Beziehungs-Skript, kein Off-Platform-Pivot. Solche sind "clean".

Sei zurückhaltend: Stufe nur dann als "scam" mit hoher confidence ein, wenn das Muster klar erkennbar ist. Reicht der bisherige Verlauf für ein Urteil noch nicht, antworte "unsure" — es kommen weitere Nachrichten. Echte Zuschauer sind "clean".

Antworte AUSSCHLIESSLICH mit einem einzigen JSON-Objekt, ohne Markdown und ohne weiteren Text:
{"verdict":"scam"|"clean"|"unsure","confidence":<Zahl 0.0 bis 1.0>,"category":"<kurzes Label, z.B. befriending_pivot, growth_pitch, recon_smalltalk>","reasoning":"<2 bis 4 Sätze auf Deutsch, allgemeinverständlich für einen unerfahrenen Streamer: WARUM ist das verdächtig oder unverdächtig? Benenne die konkreten Auffälligkeiten aus dem Verlauf. Kein Fachjargon, keine Zahlen.>"}"#;
const TIMEOUT_SECONDS: u32 = 600;
const SUBSTANTIAL_MESSAGE_TARGET: usize = 3;

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
}

impl Verdict {
    pub fn unsure() -> Self {
        Self {
            verdict: VerdictKind::Unsure,
            confidence: 0.0,
            category: String::new(),
            reasoning: String::new(),
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
    completed: bool,
}

impl DialogState {
    pub fn new(is_first_global: bool) -> Self {
        Self::with_learnings(is_first_global, None)
    }

    /// Wie [`DialogState::new`], hängt aber die netzwerkweit destillierten
    /// Self-Learning-Erkenntnisse als Zusatzhinweis an den System-Prompt an
    /// (`None`/leer → unverändert).
    pub fn with_learnings(is_first_global: bool, learnings: Option<&str>) -> Self {
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
        })
        .to_string();
        self.messages.push(DialogMessage {
            role: "user".to_string(),
            content,
        });
    }

    pub fn has_enough_substance(&self) -> bool {
        self.single_message_pitch || self.substantial_messages >= SUBSTANTIAL_MESSAGE_TARGET
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
            "discord",
            "add him",
            "add me",
            "grow",
            "real viewers",
            "donate and sub",
            "connect with",
            "talk on chat",
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
}

pub fn parse_verdict(raw: &str) -> Verdict {
    let parsed = serde_json::from_str::<RawVerdict>(raw.trim()).or_else(|_| {
        extract_json_object(raw)
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing JSON object")))
            .and_then(serde_json::from_str::<RawVerdict>)
    });
    let Ok(parsed) = parsed else {
        return Verdict::unsure();
    };
    let kind = match parsed.verdict.as_str() {
        "scam" => VerdictKind::Scam,
        "clean" => VerdictKind::Clean,
        "unsure" => VerdictKind::Unsure,
        _ => return Verdict::unsure(),
    };
    if !parsed.confidence.is_finite() {
        return Verdict::unsure();
    }
    Verdict {
        verdict: kind,
        confidence: parsed.confidence.clamp(0.0, 1.0),
        category: parsed.category,
        reasoning: parsed.reasoning,
    }
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
                parse_verdict(&raw)
            }
            Err(error) => {
                debug!("Conversation-Scam-Judge nicht verfügbar: {error}");
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
    async fn load_settings(&self, channel_login: &str) -> Result<GuardSettings, String>;
    async fn first_time_context(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<FirstTimeContext>, String>;
    async fn persist_verdict(&self, record: &VerdictRecord) -> Result<(), String>;

    /// Netzwerkweit destillierte Self-Learning-Erkenntnisse (oder `None`, solange
    /// noch keine vorliegen). Default: keine — Mocks müssen nichts liefern.
    async fn load_learnings(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[async_trait]
pub trait ScamModeration: Send + Sync {
    async fn auto_ban_and_cleanup(&self, request: AutoBanRequest<'_>) -> bool;
}

#[async_trait]
impl ScamModeration for ModerationEngine {
    async fn auto_ban_and_cleanup(&self, request: AutoBanRequest<'_>) -> bool {
        ModerationEngine::auto_ban_and_cleanup(self, request).await
    }
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
    async fn load_settings(&self, channel_login: &str) -> Result<GuardSettings, String> {
        let row = sqlx::query_as::<_, (bool, String, f32, f32)>(
            "SELECT enabled, mode, threshold, suggestion_floor \
             FROM twitch_scam_guard_settings WHERE channel_login = $1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(match row {
            Some((enabled, mode, threshold, suggestion_floor)) => GuardSettings {
                enabled,
                mode: GuardMode::from_db(&mode),
                threshold,
                suggestion_floor,
            },
            None => GuardSettings::default(),
        })
    }

    async fn first_time_context(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<FirstTimeContext>, String> {
        let session_value = sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(sc.is_first_time_streamer, FALSE) \
             FROM twitch_session_chatters sc \
             JOIN twitch_stream_sessions ss ON ss.id = sc.session_id \
             WHERE LOWER(sc.streamer_login) = $1 \
               AND LOWER(sc.chatter_login) = $2 \
               AND ss.ended_at IS NULL \
             ORDER BY ss.started_at DESC LIMIT 1",
        )
        .bind(channel_login)
        .bind(chatter_login)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;

        let is_first_time_streamer = match session_value {
            Some(value) => value,
            None => sqlx::query_scalar::<_, bool>(
                "SELECT NOT EXISTS ( \
                       SELECT 1 FROM twitch_chatter_rollup \
                       WHERE LOWER(streamer_login) = $1 AND LOWER(chatter_login) = $2 \
                     )",
            )
            .bind(channel_login)
            .bind(chatter_login)
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

        let is_first_global = sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS ( \
               SELECT 1 FROM twitch_chatter_rollup WHERE LOWER(chatter_login) = $1 \
             )",
        )
        .bind(chatter_login)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(Some(FirstTimeContext {
            is_first_time_streamer,
            is_first_global,
        }))
    }

    async fn persist_verdict(&self, record: &VerdictRecord) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO twitch_scam_guard_verdicts \
             (channel_login, chatter_login, chatter_id, verdict, confidence, category, \
              reasoning, transcript_snapshot, action_taken) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&record.channel_login)
        .bind(&record.chatter_login)
        .bind(record.chatter_id.as_deref())
        .bind(record.verdict.as_str())
        .bind(record.confidence)
        .bind(&record.category)
        .bind(&record.reasoning)
        .bind(&record.transcript_snapshot)
        .bind(&record.action_taken)
        .execute(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    async fn load_learnings(&self) -> Result<Option<String>, String> {
        load_learnings(&self.pool).await
    }
}

pub struct ConversationScamGuard {
    bot_user_id: String,
    store: Arc<dyn ScamGuardStore>,
    judge: Arc<dyn ScamJudge>,
    api: Arc<dyn ChatApi>,
    moderation: Arc<dyn ScamModeration>,
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
            states: DashMap::new(),
        }
    }

    pub fn observe(self: &Arc<Self>, event: &ChatMessageEvent) {
        let guard = Arc::clone(self);
        let event = event.clone();
        tokio::spawn(async move {
            guard.process(&event).await;
        });
    }

    async fn process(&self, event: &ChatMessageEvent) {
        let channel_login = event.broadcaster_user_login.to_lowercase();
        let chatter_login = event.chatter_user_login.to_lowercase();
        if channel_login.is_empty() || chatter_login.is_empty() || event.text().is_empty() {
            return;
        }

        let settings = match self.store.load_settings(&channel_login).await {
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
                // Erkenntnisse nur einmal pro Chatter laden (beim ersten Treffer),
                // nicht bei jeder Folgenachricht.
                let learnings = self.store.load_learnings().await.ok().flatten();
                self.states
                    .entry(key)
                    .or_insert_with(|| {
                        Arc::new(Mutex::new(DialogState::with_learnings(
                            context.is_first_global,
                            learnings.as_deref(),
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
        let (action_taken, completed) = self.apply_decision(event, &settings, &verdict).await;
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
        if let Err(error) = self.store.persist_verdict(&record).await {
            warn!("Conversation-Scam-Verdict nicht persistiert: {error}");
        }
        dialog.completed = completed;
    }

    async fn apply_decision(
        &self,
        event: &ChatMessageEvent,
        settings: &GuardSettings,
        verdict: &Verdict,
    ) -> (String, bool) {
        match verdict.verdict {
            VerdictKind::Clean => ("none".to_string(), true),
            VerdictKind::Unsure => ("none".to_string(), false),
            VerdictKind::Scam if verdict.confidence < settings.suggestion_floor => {
                ("none".to_string(), false)
            }
            VerdictKind::Scam if verdict.confidence < settings.threshold => {
                ("suggested".to_string(), false)
            }
            VerdictKind::Scam => match settings.mode {
                GuardMode::AlertOnly => ("suggested".to_string(), true),
                GuardMode::Timeout => {
                    let outcome = self
                        .api
                        .timeout_user(
                            &event.broadcaster_user_id,
                            &event.chatter_user_id,
                            TIMEOUT_SECONDS,
                            &verdict.reasoning,
                        )
                        .await;
                    match outcome {
                        Ok(BanOutcome::Banned | BanOutcome::AlreadyBanned) => {
                            ("timed_out".to_string(), true)
                        }
                        Ok(_) | Err(_) => ("suggested".to_string(), true),
                    }
                }
                GuardMode::AutoBan => {
                    if event.chatter_user_id.is_empty() {
                        return ("ban_failed_no_mod".to_string(), true);
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
        if current.chars().count() + separator + word.chars().count() > max_len && !current.is_empty()
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
        let row = sqlx::query_as::<_, (i64, String, String, String, String)>(
            "SELECT id, chatter_login, category, reasoning, transcript_snapshot \
             FROM twitch_scam_guard_verdicts \
             WHERE channel_login = $1 AND verdict = 'scam' \
               AND ($2::text IS NULL OR chatter_login = $2) \
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(channel_login.to_lowercase())
        .bind(target.as_deref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        Ok(row.map(
            |(id, chatter_login, category, reasoning, transcript_snapshot)| StoredVerdict {
                id,
                chatter_login,
                category,
                reasoning,
                transcript_snapshot,
            },
        ))
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

        let text = match self.client.messages_completion_uncapped(messages, 0.3).await {
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
        sqlx::query(
            "UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' \
             WHERE id = ( \
               SELECT id FROM twitch_scam_guard_verdicts \
               WHERE channel_login = $1 AND chatter_id = $2 AND verdict = 'scam' \
               ORDER BY created_at DESC, id DESC LIMIT 1 )",
        )
        .bind(channel_login.to_lowercase())
        .bind(chatter_id)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .unwrap_or(false)
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
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT category, reasoning, transcript_snapshot \
         FROM twitch_scam_guard_verdicts \
         WHERE verdict = 'scam' AND action_taken = ANY($1) \
         ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(actions)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(
        |(category, reasoning, transcript_snapshot)| LearningSample {
            category,
            reasoning,
            transcript_snapshot,
        },
    )
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
    sqlx::query(
        "INSERT INTO twitch_scam_guard_learnings (id, guidance, source_count, updated_at) \
         VALUES (TRUE, $1, $2, NOW()) \
         ON CONFLICT (id) DO UPDATE SET guidance = EXCLUDED.guidance, \
           source_count = EXCLUDED.source_count, updated_at = EXCLUDED.updated_at",
    )
    .bind(guidance)
    .bind(source_count)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Lädt die aktuell gültigen Erkenntnisse (oder `None`, solange noch keine
/// destilliert wurden bzw. leer).
pub async fn load_learnings(pool: &PgPool) -> Result<Option<String>, String> {
    sqlx::query_scalar::<_, String>(
        "SELECT guidance FROM twitch_scam_guard_learnings WHERE id = TRUE",
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
    let row = sqlx::query_as::<_, (String, String, Option<String>, String)>(
        "SELECT channel_login, chatter_login, chatter_id, action_taken \
         FROM twitch_scam_guard_verdicts WHERE id = $1",
    )
    .bind(verdict_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(
        row.map(|(channel_login, chatter_login, chatter_id, action_taken)| RevokeTarget {
            channel_login,
            chatter_login,
            chatter_id,
            action_taken,
        }),
    )
}

/// Markiert genau dieses Urteil als `overturned` (False-Positive-Spur fürs
/// Self-Learning). Liefert `true`, wenn eine Zeile aktualisiert wurde.
pub async fn mark_overturned_by_id(pool: &PgPool, verdict_id: i64) -> Result<bool, String> {
    sqlx::query("UPDATE twitch_scam_guard_verdicts SET action_taken = 'overturned' WHERE id = $1")
        .bind(verdict_id)
        .execute(pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(|error| error.to_string())
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

    fn verdict(kind: VerdictKind, confidence: f32) -> Verdict {
        Verdict {
            verdict: kind,
            confidence,
            category: "test-category".to_string(),
            reasoning: "test reasoning".to_string(),
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
        let user = array[1]["content"].as_str().expect("user content ist String");
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
        );
        assert_eq!(direct.verdict, VerdictKind::Scam);
        assert!((direct.confidence - 0.93).abs() < f32::EPSILON);

        let wrapped = parse_verdict(
            "analysis follows\n```json\n{\"verdict\":\"clean\",\"confidence\":0.81,\"category\":\"viewer\",\"reasoning\":\"game-specific\"}\n```",
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
            assert_eq!(parse_verdict(raw).verdict, VerdictKind::Unsure);
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

    #[derive(Default)]
    struct MockStore {
        settings: StdMutex<GuardSettings>,
        context: StdMutex<Option<FirstTimeContext>>,
        records: StdMutex<Vec<VerdictRecord>>,
    }

    #[async_trait]
    impl ScamGuardStore for MockStore {
        async fn load_settings(&self, _channel_login: &str) -> Result<GuardSettings, String> {
            Ok(self.settings.lock().unwrap().clone())
        }

        async fn first_time_context(
            &self,
            _channel_login: &str,
            _chatter_login: &str,
        ) -> Result<Option<FirstTimeContext>, String> {
            Ok(*self.context.lock().unwrap())
        }

        async fn persist_verdict(&self, record: &VerdictRecord) -> Result<(), String> {
            self.records.lock().unwrap().push(record.clone());
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

    struct MockApi {
        ban_result: StdMutex<BanOutcome>,
        timeout_result: StdMutex<BanOutcome>,
        ban_reasons: StdMutex<Vec<String>>,
        timeout_reasons: StdMutex<Vec<String>>,
    }

    impl MockApi {
        fn new() -> Self {
            Self {
                ban_result: StdMutex::new(BanOutcome::Banned),
                timeout_result: StdMutex::new(BanOutcome::Banned),
                ban_reasons: StdMutex::new(Vec::new()),
                timeout_reasons: StdMutex::new(Vec::new()),
            }
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
            Ok(None)
        }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn bot_user_id(&self) -> String {
            "bot-id".to_string()
        }
    }

    struct MockModeration {
        succeeds: StdMutex<bool>,
        reasons: StdMutex<Vec<String>>,
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
        let store = Arc::new(MockStore {
            settings: StdMutex::new(settings),
            context: StdMutex::new(Some(FirstTimeContext {
                is_first_time_streamer: true,
                is_first_global: true,
            })),
            records: StdMutex::new(Vec::new()),
        });
        let judge = Arc::new(MockJudge::new(verdicts));
        let api = Arc::new(MockApi::new());
        let moderation = Arc::new(MockModeration {
            succeeds: StdMutex::new(true),
            reasons: StdMutex::new(Vec::new()),
        });
        let guard = ConversationScamGuard::with_store(
            "bot-id".to_string(),
            Arc::clone(&store) as Arc<dyn ScamGuardStore>,
            Arc::clone(&judge) as Arc<dyn ScamJudge>,
            Arc::clone(&api) as Arc<dyn ChatApi>,
            Arc::clone(&moderation) as Arc<dyn ScamModeration>,
        );
        (guard, store, judge, api, moderation)
    }

    async fn feed(guard: &ConversationScamGuard, login: &str, messages: &[&str]) {
        for text in messages {
            guard.process(&event(login, text)).await;
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
        let (guard, store, _, _, moderation) = build_guard(
            GuardSettings::default(),
            [
                verdict(VerdictKind::Scam, 0.89),
                verdict(VerdictKind::Scam, 0.90),
            ],
        );
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
        let (guard, store, _, api, _) =
            build_guard(timeout_settings, [verdict(VerdictKind::Scam, 0.95)]);
        feed(
            &guard,
            "timeout_user",
            &["This long growth pitch asks the streamer to add somebody on Discord immediately."],
        )
        .await;
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "timed_out");
        assert_eq!(
            api.timeout_reasons.lock().unwrap().as_slice(),
            ["test reasoning"]
        );
    }

    #[tokio::test]
    async fn ban_forbidden_wird_als_no_mod_fallback_persistiert() {
        let (guard, store, _, _, moderation) =
            build_guard(GuardSettings::default(), [verdict(VerdictKind::Scam, 0.97)]);
        *moderation.succeeds.lock().unwrap() = false;
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
        assert_eq!(store.records.lock().unwrap()[0].action_taken, "none");
        assert!(api.ban_reasons.lock().unwrap().is_empty());
        assert!(moderation.reasons.lock().unwrap().is_empty());
    }
}
