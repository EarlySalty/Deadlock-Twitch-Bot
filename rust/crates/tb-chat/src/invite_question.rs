//! Deadlock-Zugangsfragen: billiger Regex-Vorfilter, Newcomer-Gate, Cooldowns,
//! dann MiniMax-Judge und Antwort/Rückfrage.
//!
//! Der KI-Call wird nicht gespawnt, sondern strikt hinter allen billigen Gates
//! gehalten: Command-Präfix, Rückfragefenster, Regex, Rollup-Neuheit und
//! In-Memory-Cooldowns müssen vorher passieren. Dadurch blockiert nur der sehr
//! seltene Kandidatenpfad.
//!
//! Der Invite-URL-Lookup sitzt direkt nach Promo-Gate und vor Judge: Regex,
//! Newcomer-Check und Cooldowns bleiben davor billig, danach verhindert der
//! DB-Lesezugriff Rückfragen ohne einlösbare URL und spart bei fehlender
//! Konfiguration den Modellcall. Ein bestätigtes "ja" aus dem Rückfragefenster
//! löst die URL erneut auf; fehlt sie dann, wird das Fenster verbraucht.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use tb_engagement::minimax_chat::EngagementMinimaxClient;
use tracing::{debug, info, warn};

use crate::api::ChatApi;
use crate::commands::{InviteReplyNotifier, PromoBlockCheck};
use crate::types::{ChatMessageEvent, SendOutcome};

const INVITE_QUESTION_CHANNEL_COOLDOWN: Duration = Duration::from_secs(120);
const INVITE_QUESTION_USER_COOLDOWN: Duration = Duration::from_secs(3600);
const INVITE_QUESTION_JUDGED_COOLDOWN: Duration = Duration::from_secs(30);
const PENDING_CONFIRMATION_WINDOW: Duration = Duration::from_secs(120);

const GO_REPLY: &str = "@{chatter} Für einen Deadlock-Invite: Komm auf unseren Discord und frag im Channel frag-die-community nach einem Invite, am besten gleich mit deinem Steam Freundescode. Dann geht das schnell und unkompliziert. {invite}";
const CONFIRM_REPLY: &str =
    "@{chatter} Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg.";

const INVITE_JUDGE_SYSTEM_PROMPT: &str = r#"Du bist ein vorsichtiger deutschsprachiger Twitch-Chat-Moderator für einen Deadlock-Stream.

Beurteile, ob die Nachricht danach fragt, wie der Chatter Zugang zum Spiel Deadlock bekommt: Einladung, Invite, Beta-Key, Early Access oder wie man mitspielen kann.

Antworte EXAKT mit einem JSON-Objekt ohne Markdown und ohne weiteren Text:
{"verdict":"yes"|"no"|"unsure","confidence":0.0-1.0,"reasoning":"..."}

Regeln:
- "yes" nur, wenn die Nachricht wirklich nach Zugang zu Deadlock fragt.
- "no" bei normalem Gameplay, Meinung, Smalltalk oder Discord ohne Zugangsfrage.
- "unsure" wenn die Absicht unklar ist."#;

trait InviteQuestionClock: Send + Sync {
    fn now(&self) -> Instant;
}

struct SystemInviteQuestionClock;

impl InviteQuestionClock for SystemInviteQuestionClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

fn invite_question_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(wie|wo|wann|wieso|warum|woher)\b\
              |\b(kann|darf)\s+man\b\
              |\b(kann|kannst|konnte|koennte|könnte|darf|darfst)\s+(man|ich|du)\b\
              |\b(bekomm|krieg|erhalt)\w*\s+(man|ich)\b",
        )
        .expect("valid invite question regex")
    })
}

fn invite_access_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(spielen|play|zock\w*|zugang|einlad\w*|invit\w*|beta|key|access|ea|early\s*access|reinkomm\w*|rankomm\w*)\b",
        )
        .expect("valid invite access regex")
    })
}

fn invite_strong_access_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(zugang|einlad\w*|invit\w*|beta|key|access|ea|early\s*access|reinkomm\w*|rankomm\w*)\b",
        )
        .expect("valid invite strong access regex")
    })
}

fn invite_join_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(anschlie(?:ss|ß)\w*|mit\s*(?:spiel\w*|zock\w*)|mitspiel\w*|mitzock\w*)\b",
        )
        .expect("valid invite join regex")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InviteQuestionSignal {
    is_candidate: bool,
    has_strong_access: bool,
}

fn classify_invite_question(content: &str) -> InviteQuestionSignal {
    let raw = content.trim();
    if raw.is_empty() || raw.starts_with('!') {
        return InviteQuestionSignal {
            is_candidate: false,
            has_strong_access: false,
        };
    }

    let has_join = invite_join_re().is_match(raw);
    let has_access = invite_access_re().is_match(raw) || has_join;
    let has_strong_access = invite_strong_access_re().is_match(raw);
    let has_question = raw.contains('?') || invite_question_re().is_match(raw);

    InviteQuestionSignal {
        is_candidate: has_access && has_question,
        has_strong_access,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteQuestionRollup {
    pub total_messages: i64,
    pub total_sessions: i64,
    pub is_first_time_streamer: bool,
}

#[async_trait]
pub trait InviteQuestionStore: Send + Sync {
    async fn rollup(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<InviteQuestionRollup>, String>;
}

pub struct PgInviteQuestionStore {
    pool: PgPool,
}

impl PgInviteQuestionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InviteQuestionStore for PgInviteQuestionStore {
    async fn rollup(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<InviteQuestionRollup>, String> {
        let with_flag = sqlx::query(
            "SELECT COALESCE(total_messages, 0)::BIGINT AS total_messages, \
                    COALESCE(total_sessions, 0)::BIGINT AS total_sessions, \
                    COALESCE(is_first_time_streamer, FALSE) AS is_first_time_streamer \
             FROM twitch_chatter_rollup \
             WHERE LOWER(streamer_login) = $1 AND LOWER(chatter_login) = $2 \
             LIMIT 1",
        )
        .bind(channel_login)
        .bind(chatter_login)
        .fetch_optional(&self.pool)
        .await;

        match with_flag {
            Ok(row) => row.map(|row| rollup_from_row(row, true)).transpose(),
            Err(error) if is_undefined_column(&error) => {
                let row = sqlx::query(
                    "SELECT COALESCE(total_messages, 0)::BIGINT AS total_messages, \
                            COALESCE(total_sessions, 0)::BIGINT AS total_sessions \
                     FROM twitch_chatter_rollup \
                     WHERE LOWER(streamer_login) = $1 AND LOWER(chatter_login) = $2 \
                     LIMIT 1",
                )
                .bind(channel_login)
                .bind(chatter_login)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| error.to_string())?;
                row.map(|row| rollup_from_row(row, false)).transpose()
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

fn is_undefined_column(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("42703"),
        _ => false,
    }
}

fn rollup_from_row(row: PgRow, has_flag: bool) -> Result<InviteQuestionRollup, String> {
    Ok(InviteQuestionRollup {
        total_messages: row
            .try_get::<i64, _>("total_messages")
            .map_err(|error| error.to_string())?,
        total_sessions: row
            .try_get::<i64, _>("total_sessions")
            .map_err(|error| error.to_string())?,
        is_first_time_streamer: if has_flag {
            row.try_get::<bool, _>("is_first_time_streamer")
                .map_err(|error| error.to_string())?
        } else {
            false
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteQuestionVerdictKind {
    Yes,
    No,
    Unsure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteQuestionVerdictSource {
    Model,
    ProviderError,
    ParseError,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InviteQuestionVerdict {
    pub verdict: InviteQuestionVerdictKind,
    pub confidence: f32,
    pub reasoning: String,
    pub source: InviteQuestionVerdictSource,
}

impl InviteQuestionVerdict {
    pub fn unsure() -> Self {
        Self {
            verdict: InviteQuestionVerdictKind::Unsure,
            confidence: 0.0,
            reasoning: String::new(),
            source: InviteQuestionVerdictSource::Model,
        }
    }

    pub fn provider_error() -> Self {
        Self {
            verdict: InviteQuestionVerdictKind::Unsure,
            confidence: 0.0,
            reasoning: String::new(),
            source: InviteQuestionVerdictSource::ProviderError,
        }
    }

    fn parse_error() -> Self {
        Self {
            verdict: InviteQuestionVerdictKind::Unsure,
            confidence: 0.0,
            reasoning: String::new(),
            source: InviteQuestionVerdictSource::ParseError,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteQuestionLoggedVerdict {
    Yes,
    No,
    Unsure,
    ProviderError,
    ParseError,
}

impl InviteQuestionLoggedVerdict {
    fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unsure => "unsure",
            Self::ProviderError => "provider_error",
            Self::ParseError => "parse_error",
        }
    }
}

impl From<&InviteQuestionVerdict> for InviteQuestionLoggedVerdict {
    fn from(verdict: &InviteQuestionVerdict) -> Self {
        match verdict.source {
            InviteQuestionVerdictSource::ProviderError => Self::ProviderError,
            InviteQuestionVerdictSource::ParseError => Self::ParseError,
            InviteQuestionVerdictSource::Model => match verdict.verdict {
                InviteQuestionVerdictKind::Yes => Self::Yes,
                InviteQuestionVerdictKind::No => Self::No,
                InviteQuestionVerdictKind::Unsure => Self::Unsure,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteQuestionJudgeInput {
    pub message: String,
    pub is_newcomer: bool,
    pub is_deadlock_live: bool,
}

#[async_trait]
pub trait InviteQuestionJudge: Send + Sync {
    async fn judge(&self, input: InviteQuestionJudgeInput) -> InviteQuestionVerdict;
}

#[async_trait]
pub trait InviteQuestionInviteUrlPort: Send + Sync {
    async fn invite_url(&self, channel_login: &str) -> Result<Option<String>, String>;
}

pub struct MiniMaxInviteQuestionJudge {
    client: EngagementMinimaxClient,
}

impl MiniMaxInviteQuestionJudge {
    pub fn new(client: EngagementMinimaxClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl InviteQuestionJudge for MiniMaxInviteQuestionJudge {
    async fn judge(&self, input: InviteQuestionJudgeInput) -> InviteQuestionVerdict {
        let user = format!(
            "Fragt dieser Twitch-Chatter danach, wie er Zugang zum Spiel Deadlock bekommt \
             (Einladung / Beta-Key / wie man mitspielen kann)?\n\n\
             Kontext:\n\
             - Nachricht: {}\n\
             - Chatter ist neu im Kanal: {}\n\
             - Kanal streamt gerade Deadlock: {}",
            input.message, input.is_newcomer, input.is_deadlock_live,
        );
        let messages = Value::Array(vec![
            serde_json::json!({"role": "system", "content": INVITE_JUDGE_SYSTEM_PROMPT}),
            serde_json::json!({"role": "user", "content": user}),
        ]);

        match self
            .client
            .messages_completion_uncapped(messages, 0.0)
            .await
        {
            Ok(raw) => parse_invite_verdict(&raw),
            Err(error) => {
                debug!("Invite-Question-Judge nicht verfügbar: {error}");
                InviteQuestionVerdict::provider_error()
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawInviteVerdict {
    verdict: String,
    confidence: f32,
    reasoning: String,
}

fn parse_invite_verdict(raw: &str) -> InviteQuestionVerdict {
    let parsed = serde_json::from_str::<RawInviteVerdict>(raw.trim()).or_else(|_| {
        extract_json_object(raw)
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing JSON object")))
            .and_then(serde_json::from_str::<RawInviteVerdict>)
    });
    let Ok(parsed) = parsed else {
        return InviteQuestionVerdict::parse_error();
    };
    if !parsed.confidence.is_finite() {
        return InviteQuestionVerdict::parse_error();
    }
    let verdict = match parsed.verdict.as_str() {
        "yes" => InviteQuestionVerdictKind::Yes,
        "no" => InviteQuestionVerdictKind::No,
        "unsure" => InviteQuestionVerdictKind::Unsure,
        _ => return InviteQuestionVerdict::parse_error(),
    };
    InviteQuestionVerdict {
        verdict,
        confidence: parsed.confidence.clamp(0.0, 1.0),
        reasoning: parsed.reasoning,
        source: InviteQuestionVerdictSource::Model,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteQuestionAction {
    Silent(SilentReason),
    SendGo,
    AskConfirmation,
}

impl InviteQuestionAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Silent(_) => "silent",
            Self::SendGo => "send_go",
            Self::AskConfirmation => "ask_confirmation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilentReason {
    EmptyMessage,
    CommandPrefix,
    MissingLogin,
    NoRegexMatch,
    RegularWithoutStrongAccess,
    CooldownChannel,
    CooldownUserReplied,
    CooldownJudgeBrake,
    PromoBlockedByPlan,
    NoInviteUrl,
    JudgeNo,
    /// Judge-Provider hat nicht geantwortet; kein Modellurteil.
    JudgeProviderError,
    /// Judge-Output war nicht parsebar; kein verwertbares Modellurteil.
    JudgeParseError,
    /// Echtes Modell-`unsure` bei einem Stammgast.
    JudgeUnsureRegular,
    JudgeYesLowConfidenceRegular,
}

impl SilentReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::EmptyMessage => "empty_message",
            Self::CommandPrefix => "command_prefix",
            Self::MissingLogin => "missing_login",
            Self::NoRegexMatch => "no_regex_match",
            Self::RegularWithoutStrongAccess => "regular_without_strong_access",
            Self::CooldownChannel => "cooldown_channel",
            Self::CooldownUserReplied => "cooldown_user_replied",
            Self::CooldownJudgeBrake => "cooldown_judge_brake",
            Self::PromoBlockedByPlan => "promo_blocked_by_plan",
            Self::NoInviteUrl => "no_invite_url",
            Self::JudgeNo => "judge_no",
            Self::JudgeProviderError => "judge_provider_error",
            Self::JudgeParseError => "judge_parse_error",
            Self::JudgeUnsureRegular => "judge_unsure_regular",
            Self::JudgeYesLowConfidenceRegular => "judge_yes_low_confidence_regular",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InviteQuestionDecision {
    pub action: InviteQuestionAction,
    pub verdict: Option<InviteQuestionLoggedVerdict>,
    pub confidence: f32,
    pub is_newcomer: bool,
    pub has_strong_access: bool,
    channel_login: String,
    chatter_login: String,
    message: String,
    pending_confirmation: bool,
    invite_url: Option<String>,
}

impl InviteQuestionDecision {
    fn silent(
        reason: SilentReason,
        channel_login: String,
        chatter_login: String,
        message: String,
        is_newcomer: bool,
        has_strong_access: bool,
    ) -> Self {
        Self {
            action: InviteQuestionAction::Silent(reason),
            verdict: None,
            confidence: 0.0,
            is_newcomer,
            has_strong_access,
            channel_login,
            chatter_login,
            message,
            pending_confirmation: false,
            invite_url: None,
        }
    }

    fn pending_silent(
        reason: SilentReason,
        channel_login: String,
        chatter_login: String,
        message: String,
    ) -> Self {
        Self {
            pending_confirmation: true,
            ..Self::silent(reason, channel_login, chatter_login, message, false, false)
        }
    }

    fn pending_send_go(
        channel_login: String,
        chatter_login: String,
        message: String,
        invite_url: String,
    ) -> Self {
        Self {
            action: InviteQuestionAction::SendGo,
            verdict: None,
            confidence: 0.0,
            is_newcomer: false,
            has_strong_access: false,
            channel_login,
            chatter_login,
            message,
            pending_confirmation: true,
            invite_url: Some(invite_url),
        }
    }

    fn judged(
        action: InviteQuestionAction,
        verdict: &InviteQuestionVerdict,
        is_newcomer: bool,
        has_strong_access: bool,
        channel_login: String,
        chatter_login: String,
        message: String,
    ) -> Self {
        Self {
            action,
            verdict: Some(InviteQuestionLoggedVerdict::from(verdict)),
            confidence: verdict.confidence,
            is_newcomer,
            has_strong_access,
            channel_login,
            chatter_login,
            message,
            pending_confirmation: false,
            invite_url: None,
        }
    }
}

fn truncate_log_message(raw: &str) -> String {
    match raw.char_indices().nth(120) {
        Some((index, _)) => format!("{}…", &raw[..index]),
        None => raw.to_string(),
    }
}

pub struct InviteQuestionResponder {
    api: Arc<dyn ChatApi>,
    invite_url: Arc<dyn InviteQuestionInviteUrlPort>,
    store: Arc<dyn InviteQuestionStore>,
    judge: Arc<dyn InviteQuestionJudge>,
    promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
    invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    clock: Arc<dyn InviteQuestionClock>,
    channel_cooldowns: Mutex<HashMap<String, Instant>>,
    user_cooldowns: Mutex<HashMap<(String, String), (Instant, CooldownKind)>>,
    pending_confirmations: Mutex<HashMap<(String, String), Instant>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CooldownKind {
    Judged,
    Replied,
}

impl InviteQuestionResponder {
    pub fn new(
        api: Arc<dyn ChatApi>,
        invite_url: Arc<dyn InviteQuestionInviteUrlPort>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> Self {
        Self::new_with_clock(
            api,
            invite_url,
            store,
            judge,
            Arc::new(SystemInviteQuestionClock),
            promo_block_check,
            invite_reply_notifier,
        )
    }

    fn new_with_clock(
        api: Arc<dyn ChatApi>,
        invite_url: Arc<dyn InviteQuestionInviteUrlPort>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
        clock: Arc<dyn InviteQuestionClock>,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> Self {
        Self {
            api,
            invite_url,
            store,
            judge,
            promo_block_check,
            invite_reply_notifier,
            clock,
            channel_cooldowns: Mutex::new(HashMap::new()),
            user_cooldowns: Mutex::new(HashMap::new()),
            pending_confirmations: Mutex::new(HashMap::new()),
        }
    }

    pub async fn maybe_respond(&self, event: &ChatMessageEvent, channel_login: &str) -> bool {
        // Judged-Cooldown bleibt im Orchestratorpfad und läuft direkt vor dem Modellcall.
        let decision = self
            .decide_with_before_judge(event, channel_login, |channel_login, chatter_login| {
                self.mark_judged(channel_login, chatter_login);
            })
            .await;
        self.log_decision(&decision);
        if decision.pending_confirmation {
            self.forget_pending_confirmation(&decision.channel_login, &decision.chatter_login);
        }

        match decision.action {
            InviteQuestionAction::SendGo => {
                let Some(invite_url) = decision.invite_url.as_deref() else {
                    return false;
                };
                if !self
                    .send_go(event, &decision.chatter_login, invite_url)
                    .await
                {
                    return false;
                }
                self.mark_replied(&decision.channel_login, &decision.chatter_login);
                self.note_invite_reply(&decision.channel_login).await;
                true
            }
            InviteQuestionAction::AskConfirmation => {
                if !self
                    .send_confirmation_question(event, &decision.chatter_login)
                    .await
                {
                    return false;
                }
                self.mark_replied(&decision.channel_login, &decision.chatter_login);
                self.note_invite_reply(&decision.channel_login).await;
                self.remember_pending_confirmation(
                    &decision.channel_login,
                    &decision.chatter_login,
                );
                true
            }
            InviteQuestionAction::Silent(_) => false,
        }
    }

    pub async fn decide(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
    ) -> InviteQuestionDecision {
        self.decide_with_before_judge(event, channel_login, |_, _| {})
            .await
    }

    async fn decide_with_before_judge<F>(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        before_judge: F,
    ) -> InviteQuestionDecision
    where
        F: FnOnce(&str, &str),
    {
        let raw = event.text();
        let channel_login = normalize_login(channel_login);
        let chatter_login = normalize_login(&event.chatter_user_login);

        if raw.is_empty() {
            return InviteQuestionDecision::silent(
                SilentReason::EmptyMessage,
                channel_login,
                chatter_login,
                raw.to_string(),
                false,
                false,
            );
        }
        if raw.starts_with('!') {
            return InviteQuestionDecision::silent(
                SilentReason::CommandPrefix,
                channel_login,
                chatter_login,
                raw.to_string(),
                false,
                false,
            );
        }
        if channel_login.is_empty() || chatter_login.is_empty() {
            return InviteQuestionDecision::silent(
                SilentReason::MissingLogin,
                channel_login,
                chatter_login,
                raw.to_string(),
                false,
                false,
            );
        }

        if self.pending_confirmation_open(&channel_login, &chatter_login) && is_affirmative(raw) {
            let Some(invite_url) = self.resolve_invite_url(&channel_login).await else {
                return InviteQuestionDecision::pending_silent(
                    SilentReason::NoInviteUrl,
                    channel_login,
                    chatter_login,
                    raw.to_string(),
                );
            };
            return InviteQuestionDecision::pending_send_go(
                channel_login,
                chatter_login,
                raw.to_string(),
                invite_url,
            );
        }

        let signal = classify_invite_question(raw);
        if !signal.is_candidate {
            return InviteQuestionDecision::silent(
                SilentReason::NoRegexMatch,
                channel_login,
                chatter_login,
                raw.to_string(),
                false,
                signal.has_strong_access,
            );
        }

        let is_newcomer = self.is_newcomer(&channel_login, &chatter_login).await;
        // ponytail: Stammgäste nur bei explizitem Zugangswort, Schwelle statt Schalter
        if !is_newcomer && !signal.has_strong_access {
            return InviteQuestionDecision::silent(
                SilentReason::RegularWithoutStrongAccess,
                channel_login,
                chatter_login,
                raw.to_string(),
                is_newcomer,
                signal.has_strong_access,
            );
        }

        if let Some(reason) = self.cooldown_block_reason(&channel_login, &chatter_login) {
            return InviteQuestionDecision::silent(
                reason,
                channel_login,
                chatter_login,
                raw.to_string(),
                is_newcomer,
                signal.has_strong_access,
            );
        }
        if let Some(promo_block_check) = &self.promo_block_check {
            if promo_block_check.is_promo_blocked(&channel_login).await {
                return InviteQuestionDecision::silent(
                    SilentReason::PromoBlockedByPlan,
                    channel_login,
                    chatter_login,
                    raw.to_string(),
                    is_newcomer,
                    signal.has_strong_access,
                );
            }
        }
        let Some(invite_url) = self.resolve_invite_url(&channel_login).await else {
            return InviteQuestionDecision::silent(
                SilentReason::NoInviteUrl,
                channel_login,
                chatter_login,
                raw.to_string(),
                is_newcomer,
                signal.has_strong_access,
            );
        };
        before_judge(&channel_login, &chatter_login);

        let verdict = self
            .judge
            .judge(InviteQuestionJudgeInput {
                message: raw.to_string(),
                is_newcomer,
                is_deadlock_live: true,
            })
            .await;

        let action = match verdict.source {
            InviteQuestionVerdictSource::ProviderError => {
                InviteQuestionAction::Silent(SilentReason::JudgeProviderError)
            }
            InviteQuestionVerdictSource::ParseError => {
                InviteQuestionAction::Silent(SilentReason::JudgeParseError)
            }
            InviteQuestionVerdictSource::Model => match verdict.verdict {
                InviteQuestionVerdictKind::Yes if verdict.confidence >= 0.7 => {
                    InviteQuestionAction::SendGo
                }
                InviteQuestionVerdictKind::Yes | InviteQuestionVerdictKind::Unsure
                    if is_newcomer =>
                {
                    InviteQuestionAction::AskConfirmation
                }
                InviteQuestionVerdictKind::No => {
                    InviteQuestionAction::Silent(SilentReason::JudgeNo)
                }
                InviteQuestionVerdictKind::Unsure => {
                    InviteQuestionAction::Silent(SilentReason::JudgeUnsureRegular)
                }
                InviteQuestionVerdictKind::Yes => {
                    InviteQuestionAction::Silent(SilentReason::JudgeYesLowConfidenceRegular)
                }
            },
        };
        let mut decision = InviteQuestionDecision::judged(
            action,
            &verdict,
            is_newcomer,
            signal.has_strong_access,
            channel_login,
            chatter_login,
            raw.to_string(),
        );
        if action == InviteQuestionAction::SendGo {
            decision.invite_url = Some(invite_url);
        }
        decision
    }

    fn log_decision(&self, decision: &InviteQuestionDecision) {
        let message = truncate_log_message(&decision.message);
        match (decision.verdict, decision.action) {
            (Some(verdict), InviteQuestionAction::Silent(reason)) => {
                if matches!(
                    verdict,
                    InviteQuestionLoggedVerdict::ProviderError
                        | InviteQuestionLoggedVerdict::ParseError
                ) {
                    warn!(
                        channel = %decision.channel_login,
                        chatter = %decision.chatter_login,
                        message = %message,
                        verdict = verdict.as_str(),
                        confidence = decision.confidence,
                        is_newcomer = decision.is_newcomer,
                        has_strong_access = decision.has_strong_access,
                        action = decision.action.as_str(),
                        silent_reason = reason.as_str(),
                    );
                } else {
                    info!(
                        channel = %decision.channel_login,
                        chatter = %decision.chatter_login,
                        message = %message,
                        verdict = verdict.as_str(),
                        confidence = decision.confidence,
                        is_newcomer = decision.is_newcomer,
                        has_strong_access = decision.has_strong_access,
                        action = decision.action.as_str(),
                        silent_reason = reason.as_str(),
                    );
                }
            }
            (Some(verdict), _) => {
                info!(
                    channel = %decision.channel_login,
                    chatter = %decision.chatter_login,
                    message = %message,
                    verdict = verdict.as_str(),
                    confidence = decision.confidence,
                    is_newcomer = decision.is_newcomer,
                    has_strong_access = decision.has_strong_access,
                    action = decision.action.as_str(),
                );
            }
            (None, InviteQuestionAction::Silent(reason)) => {
                if reason == SilentReason::NoInviteUrl {
                    warn!(
                        channel = %decision.channel_login,
                        chatter = %decision.chatter_login,
                        message = %message,
                        action = decision.action.as_str(),
                        silent_reason = reason.as_str(),
                    );
                } else {
                    debug!(
                        channel = %decision.channel_login,
                        chatter = %decision.chatter_login,
                        message = %message,
                        action = decision.action.as_str(),
                        silent_reason = reason.as_str(),
                    );
                }
            }
            (None, _) => {}
        }
    }

    async fn is_newcomer(&self, channel_login: &str, chatter_login: &str) -> bool {
        match self.store.rollup(channel_login, chatter_login).await {
            Ok(None) => true,
            Ok(Some(row)) => row.is_first_time_streamer || row.total_messages <= 10,
            Err(error) => {
                debug!(%error, channel_login, chatter_login, "Invite-Question-Rollup nicht lesbar");
                true
            }
        }
    }

    fn cooldown_block_reason(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Option<SilentReason> {
        let now = self.clock.now();
        let Ok(channels) = self.channel_cooldowns.lock() else {
            return Some(SilentReason::CooldownChannel);
        };
        if channels
            .get(channel_login)
            .is_some_and(|last| now.duration_since(*last) < INVITE_QUESTION_CHANNEL_COOLDOWN)
        {
            return Some(SilentReason::CooldownChannel);
        }
        drop(channels);

        let key = (channel_login.to_string(), chatter_login.to_string());
        let Ok(users) = self.user_cooldowns.lock() else {
            return Some(SilentReason::CooldownJudgeBrake);
        };
        if let Some((last, kind)) = users.get(&key) {
            if self.user_cooldown_active(now, *last, *kind) {
                return Some(match kind {
                    CooldownKind::Judged => SilentReason::CooldownJudgeBrake,
                    CooldownKind::Replied => SilentReason::CooldownUserReplied,
                });
            }
        }

        None
    }

    fn user_cooldown_active(&self, now: Instant, last: Instant, kind: CooldownKind) -> bool {
        let ttl = match kind {
            CooldownKind::Judged => INVITE_QUESTION_JUDGED_COOLDOWN,
            CooldownKind::Replied => INVITE_QUESTION_USER_COOLDOWN,
        };
        now.duration_since(last) < ttl
    }

    fn mark_judged(&self, channel_login: &str, chatter_login: &str) {
        let now = self.clock.now();
        let key = (channel_login.to_string(), chatter_login.to_string());
        let Ok(mut users) = self.user_cooldowns.lock() else {
            return;
        };
        if users.get(&key).is_some_and(|(last, kind)| {
            *kind == CooldownKind::Replied && self.user_cooldown_active(now, *last, *kind)
        }) {
            return;
        }
        users.insert(key, (now, CooldownKind::Judged));
    }

    fn mark_replied(&self, channel_login: &str, chatter_login: &str) {
        let now = self.clock.now();
        if let Ok(mut channels) = self.channel_cooldowns.lock() {
            channels.insert(channel_login.to_string(), now);
        }
        if let Ok(mut users) = self.user_cooldowns.lock() {
            users.insert(
                (channel_login.to_string(), chatter_login.to_string()),
                (now, CooldownKind::Replied),
            );
        }
    }

    fn pending_confirmation_open(&self, channel_login: &str, chatter_login: &str) -> bool {
        let key = (channel_login.to_string(), chatter_login.to_string());
        let now = self.clock.now();
        let Ok(pending) = self.pending_confirmations.lock() else {
            return false;
        };
        pending
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) <= PENDING_CONFIRMATION_WINDOW)
    }

    fn forget_pending_confirmation(&self, channel_login: &str, chatter_login: &str) {
        let key = (channel_login.to_string(), chatter_login.to_string());
        if let Ok(mut pending) = self.pending_confirmations.lock() {
            pending.remove(&key);
        }
    }

    fn remember_pending_confirmation(&self, channel_login: &str, chatter_login: &str) {
        if let Ok(mut pending) = self.pending_confirmations.lock() {
            pending.insert(
                (channel_login.to_string(), chatter_login.to_string()),
                self.clock.now(),
            );
        }
    }

    async fn resolve_invite_url(&self, channel_login: &str) -> Option<String> {
        match self.invite_url.invite_url(channel_login).await {
            Ok(Some(url)) if !url.trim().is_empty() => Some(url),
            Ok(_) => None,
            Err(error) => {
                debug!(%error, channel_login, "Invite-Question-Invite-URL nicht lesbar");
                None
            }
        }
    }

    async fn note_invite_reply(&self, channel_login: &str) {
        if let Some(notifier) = &self.invite_reply_notifier {
            notifier.note_invite_reply(channel_login).await;
        }
    }

    async fn send_confirmation_question(
        &self,
        event: &ChatMessageEvent,
        chatter_login: &str,
    ) -> bool {
        let msg = CONFIRM_REPLY.replace("{chatter}", chatter_login);
        self.send(event, &msg).await
    }

    async fn send_go(&self, event: &ChatMessageEvent, chatter_login: &str, invite: &str) -> bool {
        let msg = GO_REPLY
            .replace("{chatter}", chatter_login)
            .replace("{invite}", invite);
        self.send(event, &msg).await
    }

    async fn send(&self, event: &ChatMessageEvent, message: &str) -> bool {
        match self
            .api
            .send_message(&event.broadcaster_user_id, message)
            .await
        {
            Ok(SendOutcome::Sent) => true,
            Ok(outcome) => {
                debug!(?outcome, "Invite-Question-Antwort von Twitch verworfen");
                false
            }
            Err(error) => {
                warn!(%error, "Invite-Question-Antwort konnte nicht gesendet werden");
                false
            }
        }
    }
}

fn normalize_login(login: &str) -> String {
    login.trim().trim_start_matches('#').to_lowercase()
}

fn is_affirmative(raw: &str) -> bool {
    let normalized: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .collect();
    let normalized = normalized.trim();
    [
        "ja", "jo", "jop", "jap", "jup", "joa", "klar", "gerne", "bitte", "yes", "ye", "yep",
        "yeah", "sure",
    ]
    .iter()
    .any(|token| normalized.starts_with(token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{BanOutcome, ChatApi};
    use crate::commands::{InviteReplyNotifier, PromoBlockCheck};
    use crate::types::{ChatMessageBody, ChatMessageEvent, SendOutcome};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tb_engagement::minimax_chat::EngagementMinimaxClient;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct MockApi {
        sent: Mutex<Vec<String>>,
    }

    impl MockApi {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(vec![]),
            })
        }

        fn messages(&self) -> Vec<String> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(
            &self,
            _broadcaster_id: &str,
            message: &str,
        ) -> Result<SendOutcome, String> {
            self.sent.lock().unwrap().push(message.to_string());
            Ok(SendOutcome::Sent)
        }

        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }

        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }

        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
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

    const TEST_INVITE_URL: &str = "https://discord.gg/test";

    struct FakeDiscordLink {
        urls: Mutex<VecDeque<Option<String>>>,
        default: Option<String>,
    }

    impl FakeDiscordLink {
        fn with_url(url: Option<&str>) -> Self {
            Self {
                urls: Mutex::new(VecDeque::new()),
                default: url.map(str::to_string),
            }
        }

        fn with_sources(db_url: Option<&str>, fallback_url: Option<&str>) -> Self {
            let url = db_url
                .filter(|url| !url.trim().is_empty())
                .or_else(|| fallback_url.filter(|url| !url.trim().is_empty()));
            Self::with_url(url)
        }

        fn with_sequence(urls: Vec<Option<&str>>) -> Self {
            Self {
                urls: Mutex::new(
                    urls.into_iter()
                        .map(|url| url.map(str::to_string))
                        .collect(),
                ),
                default: None,
            }
        }
    }

    #[async_trait]
    impl InviteQuestionInviteUrlPort for FakeDiscordLink {
        async fn invite_url(&self, _channel_login: &str) -> Result<Option<String>, String> {
            Ok(self
                .urls
                .lock()
                .unwrap()
                .pop_front()
                .or_else(|| Some(self.default.clone()))
                .flatten())
        }
    }

    struct FakePromoBlock {
        blocked: bool,
    }

    impl FakePromoBlock {
        fn new(blocked: bool) -> Arc<Self> {
            Arc::new(Self { blocked })
        }
    }

    #[async_trait]
    impl PromoBlockCheck for FakePromoBlock {
        async fn is_promo_blocked(&self, _channel_login: &str) -> bool {
            self.blocked
        }
    }

    struct FakeNotifier {
        calls: Mutex<Vec<String>>,
    }

    impl FakeNotifier {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(vec![]),
            })
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl InviteReplyNotifier for FakeNotifier {
        async fn note_invite_reply(&self, channel_login: &str) {
            self.calls.lock().unwrap().push(channel_login.to_string());
        }
    }

    struct FakeClock {
        now: Mutex<Instant>,
    }

    impl FakeClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                now: Mutex::new(Instant::now()),
            })
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.now.lock().unwrap();
            *now += duration;
        }
    }

    impl InviteQuestionClock for FakeClock {
        fn now(&self) -> Instant {
            *self.now.lock().unwrap()
        }
    }

    struct FakeStore {
        rollup: InviteQuestionRollup,
    }

    #[async_trait]
    impl InviteQuestionStore for FakeStore {
        async fn rollup(
            &self,
            _channel_login: &str,
            _chatter_login: &str,
        ) -> Result<Option<InviteQuestionRollup>, String> {
            Ok(Some(self.rollup.clone()))
        }
    }

    struct FakeJudge {
        verdicts: Mutex<VecDeque<InviteQuestionVerdict>>,
        calls: Mutex<Vec<InviteQuestionJudgeInput>>,
        call_count: AtomicUsize,
    }

    impl FakeJudge {
        fn new(verdicts: Vec<InviteQuestionVerdict>) -> Arc<Self> {
            Arc::new(Self {
                verdicts: Mutex::new(verdicts.into()),
                calls: Mutex::new(vec![]),
                call_count: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> Vec<InviteQuestionJudgeInput> {
            self.calls.lock().unwrap().clone()
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl InviteQuestionJudge for FakeJudge {
        async fn judge(&self, input: InviteQuestionJudgeInput) -> InviteQuestionVerdict {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.calls.lock().unwrap().push(input);
            self.verdicts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(InviteQuestionVerdict::unsure)
        }
    }

    fn event(chatter: &str, text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "channel-id".to_string(),
            broadcaster_user_login: "streamer".to_string(),
            chatter_user_id: format!("{chatter}-id"),
            chatter_user_login: chatter.to_string(),
            chatter_user_name: chatter.to_string(),
            message_id: format!("msg-{chatter}-{text}"),
            message: ChatMessageBody {
                text: text.to_string(),
                fragments: vec![],
            },
            ..Default::default()
        }
    }

    fn rollup(total_messages: i64, is_first_time_streamer: bool) -> InviteQuestionRollup {
        InviteQuestionRollup {
            total_messages,
            total_sessions: 1,
            is_first_time_streamer,
        }
    }

    fn verdict(kind: InviteQuestionVerdictKind, confidence: f32) -> InviteQuestionVerdict {
        InviteQuestionVerdict {
            verdict: kind,
            confidence,
            reasoning: "test".to_string(),
            source: InviteQuestionVerdictSource::Model,
        }
    }

    fn responder(
        api: Arc<MockApi>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
    ) -> InviteQuestionResponder {
        responder_with_ports(api, store, judge, None, None)
    }

    fn responder_with_ports(
        api: Arc<MockApi>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> InviteQuestionResponder {
        responder_with_clock_and_ports(api, store, judge, promo_block_check, invite_reply_notifier)
            .0
    }

    fn responder_with_clock(
        api: Arc<MockApi>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
    ) -> (InviteQuestionResponder, Arc<FakeClock>) {
        responder_with_clock_and_ports(api, store, judge, None, None)
    }

    fn responder_with_clock_and_ports(
        api: Arc<MockApi>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> (InviteQuestionResponder, Arc<FakeClock>) {
        responder_with_clock_ports_and_discord_link(
            api,
            store,
            judge,
            Arc::new(FakeDiscordLink::with_url(Some(TEST_INVITE_URL))),
            promo_block_check,
            invite_reply_notifier,
        )
    }

    fn responder_with_discord_link(
        api: Arc<MockApi>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
        discord_link: Arc<dyn InviteQuestionInviteUrlPort>,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> InviteQuestionResponder {
        responder_with_clock_ports_and_discord_link(
            api,
            store,
            judge,
            discord_link,
            promo_block_check,
            invite_reply_notifier,
        )
        .0
    }

    fn responder_with_clock_ports_and_discord_link(
        api: Arc<MockApi>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
        discord_link: Arc<dyn InviteQuestionInviteUrlPort>,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> (InviteQuestionResponder, Arc<FakeClock>) {
        let clock = FakeClock::new();
        let invite = InviteQuestionResponder::new_with_clock(
            api,
            discord_link,
            store,
            judge,
            clock.clone(),
            promo_block_check,
            invite_reply_notifier,
        );
        (invite, clock)
    }

    fn expected_go(chatter: &str) -> String {
        expected_go_with_invite(chatter, TEST_INVITE_URL)
    }

    fn expected_go_with_invite(chatter: &str, invite: &str) -> String {
        format!(
            "@{chatter} Für einen Deadlock-Invite: Komm auf unseren Discord und frag im Channel frag-die-community nach einem Invite, am besten gleich mit deinem Steam Freundescode. Dann geht das schnell und unkompliziert. {invite}"
        )
    }

    fn expected_confirm(chatter: &str) -> String {
        format!(
            "@{chatter} Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg."
        )
    }

    async fn decide_action(
        invite: &InviteQuestionResponder,
        chatter: &str,
        text: &str,
    ) -> InviteQuestionDecision {
        invite.decide(&event(chatter, text), "streamer").await
    }

    #[test]
    fn regex_klassifikation_erkennt_nur_zugangsfragen() {
        assert!(classify_invite_question("Wie kann man das Spiel denn spielen?").is_candidate);
        let strong = classify_invite_question(
            "Bin auf dem Discord, wie kommt man an eine Einladung / wird eingeladen?",
        );
        assert!(strong.is_candidate);
        assert!(strong.has_strong_access);
        assert!(
            !classify_invite_question(
                "ich zock den jetzt seit 50 Games durchgehend aber fühl das bisher noch nicht so",
            )
            .is_candidate
        );
        assert!(!classify_invite_question("Lategame... das Rasiere ich richtig").is_candidate);
        assert!(!classify_invite_question("bleib bei Mo").is_candidate);
        assert!(!classify_invite_question("!dldc").is_candidate);
    }

    #[tokio::test]
    async fn decide_liefert_silent_reason_fuer_jeden_ausstieg() {
        let cases = [
            (
                SilentReason::EmptyMessage,
                "",
                rollup(1, false),
                verdict(InviteQuestionVerdictKind::Yes, 0.9),
            ),
            (
                SilentReason::CommandPrefix,
                "!dldc",
                rollup(1, false),
                verdict(InviteQuestionVerdictKind::Yes, 0.9),
            ),
            (
                SilentReason::MissingLogin,
                "Wie bekomme ich einen invite?",
                rollup(1, false),
                verdict(InviteQuestionVerdictKind::Yes, 0.9),
            ),
            (
                SilentReason::NoRegexMatch,
                "ja",
                rollup(1, false),
                verdict(InviteQuestionVerdictKind::Yes, 0.9),
            ),
            (
                SilentReason::RegularWithoutStrongAccess,
                "Wie kann man das Spiel denn spielen?",
                rollup(50, false),
                verdict(InviteQuestionVerdictKind::Yes, 0.9),
            ),
            (
                SilentReason::JudgeNo,
                "Wie bekomme ich einen invite?",
                rollup(1, false),
                verdict(InviteQuestionVerdictKind::No, 1.0),
            ),
            (
                SilentReason::JudgeProviderError,
                "Wie bekomme ich einen invite?",
                rollup(1, false),
                InviteQuestionVerdict::provider_error(),
            ),
            (
                SilentReason::JudgeParseError,
                "Wie bekomme ich einen invite?",
                rollup(1, false),
                parse_invite_verdict("kaputt"),
            ),
            (
                SilentReason::JudgeUnsureRegular,
                "Wie bekomme ich einen invite?",
                rollup(50, false),
                verdict(InviteQuestionVerdictKind::Unsure, 0.0),
            ),
            (
                SilentReason::JudgeYesLowConfidenceRegular,
                "Wie bekomme ich einen invite?",
                rollup(50, false),
                verdict(InviteQuestionVerdictKind::Yes, 0.69),
            ),
        ];

        for (reason, text, rollup, verdict) in cases {
            let invite = responder(
                MockApi::new(),
                Arc::new(FakeStore { rollup }),
                FakeJudge::new(vec![verdict]),
            );
            let channel = if reason == SilentReason::MissingLogin {
                ""
            } else {
                "streamer"
            };
            assert_eq!(
                invite.decide(&event("viewer", text), channel).await.action,
                InviteQuestionAction::Silent(reason)
            );
        }

        let invite = responder_with_discord_link(
            MockApi::new(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]),
            Arc::new(FakeDiscordLink::with_url(None)),
            None,
            None,
        );
        assert_eq!(
            decide_action(&invite, "viewer", "Wie bekomme ich einen invite?")
                .await
                .action,
            InviteQuestionAction::Silent(SilentReason::NoInviteUrl)
        );

        let api = MockApi::new();
        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]);
        let (invite, _) = responder_with_clock(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge,
        );
        let decision = decide_action(&invite, "viewer", "Wie bekomme ich einen invite?").await;
        assert_eq!(decision.action, InviteQuestionAction::SendGo);
        invite.mark_replied("streamer", "viewer");
        assert_eq!(
            decide_action(&invite, "other", "Wie bekomme ich einen invite?")
                .await
                .action,
            InviteQuestionAction::Silent(SilentReason::CooldownChannel)
        );

        let api = MockApi::new();
        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]);
        let (invite, _) = responder_with_clock(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge,
        );
        invite.user_cooldowns.lock().unwrap().insert(
            ("streamer".to_string(), "viewer".to_string()),
            (invite.clock.now(), CooldownKind::Replied),
        );
        assert_eq!(
            decide_action(&invite, "viewer", "Wie bekomme ich einen invite?")
                .await
                .action,
            InviteQuestionAction::Silent(SilentReason::CooldownUserReplied)
        );

        let api = MockApi::new();
        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]);
        let (invite, _) = responder_with_clock(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge,
        );
        invite.mark_judged("streamer", "viewer");
        assert_eq!(
            decide_action(&invite, "viewer", "Wie bekomme ich einen invite?")
                .await
                .action,
            InviteQuestionAction::Silent(SilentReason::CooldownJudgeBrake)
        );
    }

    #[tokio::test]
    async fn decide_sendet_nichts() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]);
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        let decision = decide_action(&invite, "viewer", "Wie bekomme ich einen invite?").await;

        assert_eq!(decision.action, InviteQuestionAction::SendGo);
        assert!(api.messages().is_empty());
        assert_eq!(judge.call_count(), 1);
        assert!(invite.user_cooldowns.lock().unwrap().is_empty());
        assert!(invite.channel_cooldowns.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn promo_blockiert_verhindert_go_und_judge() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]);
        let promo_block: Arc<dyn PromoBlockCheck> = FakePromoBlock::new(true);
        let invite = responder_with_ports(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
            Some(promo_block),
            None,
        );

        let decision = decide_action(&invite, "viewer", "Wie bekomme ich einen invite?").await;

        assert_eq!(
            decision.action,
            InviteQuestionAction::Silent(SilentReason::PromoBlockedByPlan)
        );
        assert_eq!(judge.call_count(), 0);

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
        assert_eq!(judge.call_count(), 0);
    }

    #[tokio::test]
    async fn promo_erlaubt_laesst_go_durch() {
        let api = MockApi::new();
        let promo_block: Arc<dyn PromoBlockCheck> = FakePromoBlock::new(false);
        let invite = responder_with_ports(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]),
            Some(promo_block),
            None,
        );

        let sent = invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;

        assert!(sent);
        assert_eq!(api.messages(), vec![expected_go("viewer")]);
    }

    #[tokio::test]
    async fn gesendete_go_meldet_promo_kadenz() {
        let api = MockApi::new();
        let notifier = FakeNotifier::new();
        let notifier_port: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let invite = responder_with_ports(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]),
            None,
            Some(notifier_port),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;

        assert_eq!(notifier.calls(), vec!["streamer"]);
    }

    #[tokio::test]
    async fn gesendete_rueckfrage_meldet_promo_kadenz() {
        let api = MockApi::new();
        let notifier = FakeNotifier::new();
        let notifier_port: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let invite = responder_with_ports(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Unsure, 0.0)]),
            None,
            Some(notifier_port),
        );

        let sent = invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;

        assert!(sent);
        assert_eq!(notifier.calls(), vec!["streamer"]);
    }

    #[tokio::test]
    async fn bestaetigtes_ja_meldet_promo_kadenz() {
        let api = MockApi::new();
        let notifier = FakeNotifier::new();
        let notifier_port: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let invite = responder_with_ports(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Unsure, 0.0)]),
            None,
            Some(notifier_port),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        invite
            .maybe_respond(&event("viewer", "ja"), "streamer")
            .await;

        assert_eq!(notifier.calls(), vec!["streamer", "streamer"]);
    }

    #[tokio::test]
    async fn silent_meldet_keine_promo_kadenz() {
        let api = MockApi::new();
        let notifier = FakeNotifier::new();
        let notifier_port: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let invite = responder_with_ports(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::No, 1.0)]),
            None,
            Some(notifier_port),
        );

        let sent = invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;

        assert!(!sent);
        assert!(notifier.calls().is_empty());
    }

    #[tokio::test]
    async fn keine_invite_url_keine_rueckfrage() {
        let api = MockApi::new();
        let notifier = FakeNotifier::new();
        let notifier_port: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::Unsure, 0.0),
            verdict(InviteQuestionVerdictKind::Unsure, 0.0),
        ]);
        let invite = responder_with_discord_link(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge,
            Arc::new(FakeDiscordLink::with_url(None)),
            None,
            Some(notifier_port),
        );

        let decision = decide_action(&invite, "viewer", "Wie kann man Deadlock spielen?").await;
        assert_eq!(
            decision.action,
            InviteQuestionAction::Silent(SilentReason::NoInviteUrl)
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
        assert!(notifier.calls().is_empty());
        assert_eq!(SilentReason::NoInviteUrl.as_str(), "no_invite_url");
    }

    #[tokio::test]
    async fn keine_invite_url_kein_go() {
        let api = MockApi::new();
        let notifier = FakeNotifier::new();
        let notifier_port: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
        ]);
        let invite = responder_with_discord_link(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge,
            Arc::new(FakeDiscordLink::with_url(None)),
            None,
            Some(notifier_port),
        );

        let decision = decide_action(&invite, "viewer", "Wie bekomme ich einen invite?").await;
        assert_eq!(
            decision.action,
            InviteQuestionAction::Silent(SilentReason::NoInviteUrl)
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
        assert!(notifier.calls().is_empty());
    }

    #[tokio::test]
    async fn keine_invite_url_spart_judge_call() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]);
        let invite = responder_with_discord_link(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
            Arc::new(FakeDiscordLink::with_url(None)),
            None,
            None,
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;

        assert_eq!(judge.call_count(), 0);
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn env_fallback_wird_genutzt() {
        let api = MockApi::new();
        let fallback = "https://discord.gg/fallback";
        let invite = responder_with_discord_link(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]),
            Arc::new(FakeDiscordLink::with_sources(None, Some(fallback))),
            None,
            None,
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;

        assert_eq!(
            api.messages(),
            vec![expected_go_with_invite("viewer", fallback)]
        );
    }

    #[tokio::test]
    async fn db_invite_schlaegt_env_fallback() {
        let api = MockApi::new();
        let db_url = "https://discord.gg/db";
        let env_url = "https://discord.gg/env";
        let invite = responder_with_discord_link(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.9)]),
            Arc::new(FakeDiscordLink::with_sources(Some(db_url), Some(env_url))),
            None,
            None,
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;

        assert_eq!(
            api.messages(),
            vec![expected_go_with_invite("viewer", db_url)]
        );
    }

    #[tokio::test]
    async fn url_verschwindet_zwischen_rueckfrage_und_ja() {
        let api = MockApi::new();
        let notifier = FakeNotifier::new();
        let notifier_port: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let invite = responder_with_discord_link(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Unsure, 0.0)]),
            Arc::new(FakeDiscordLink::with_sequence(vec![
                Some(TEST_INVITE_URL),
                None,
            ])),
            None,
            Some(notifier_port),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        invite
            .maybe_respond(&event("viewer", "ja"), "streamer")
            .await;

        assert_eq!(api.messages(), vec![expected_confirm("viewer")]);
        assert!(invite.pending_confirmations.lock().unwrap().is_empty());
        assert_eq!(notifier.calls(), vec!["streamer"]);
    }

    #[test]
    fn go_text_unter_twitch_limit() {
        assert_eq!(
            GO_REPLY,
            "@{chatter} Für einen Deadlock-Invite: Komm auf unseren Discord und frag im Channel frag-die-community nach einem Invite, am besten gleich mit deinem Steam Freundescode. Dann geht das schnell und unkompliziert. {invite}"
        );
        assert_eq!(
            CONFIRM_REPLY,
            "@{chatter} Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg."
        );

        let chatter = "abcdefghijklmnopqrstuvwxy";
        let invite = "https://discord.gg/abcdefghijklmnopqrstu";
        assert_eq!(chatter.len(), 25);
        assert_eq!(invite.len(), 40);

        let rendered = GO_REPLY
            .replace("{chatter}", chatter)
            .replace("{invite}", invite);

        assert!(rendered.len() < 500);
    }

    #[tokio::test]
    async fn provider_fehler_und_parse_fehler_sind_unterscheidbar() {
        let provider_error = responder(
            MockApi::new(),
            Arc::new(FakeStore {
                rollup: rollup(50, false),
            }),
            FakeJudge::new(vec![InviteQuestionVerdict::provider_error()]),
        );
        let parse_error = responder(
            MockApi::new(),
            Arc::new(FakeStore {
                rollup: rollup(50, false),
            }),
            FakeJudge::new(vec![parse_invite_verdict("kaputt")]),
        );

        let provider_decision =
            decide_action(&provider_error, "viewer", "Wie bekomme ich einen invite?").await;
        let parse_decision =
            decide_action(&parse_error, "viewer", "Wie bekomme ich einen invite?").await;

        assert_eq!(
            provider_decision.action,
            InviteQuestionAction::Silent(SilentReason::JudgeProviderError)
        );
        assert_eq!(
            parse_decision.action,
            InviteQuestionAction::Silent(SilentReason::JudgeParseError)
        );
        assert_eq!(
            provider_decision.verdict,
            Some(InviteQuestionLoggedVerdict::ProviderError)
        );
        assert_eq!(
            parse_decision.verdict,
            Some(InviteQuestionLoggedVerdict::ParseError)
        );
    }

    #[test]
    fn gekuerzte_message_im_log() {
        let exactly_120 = "a".repeat(120);
        assert_eq!(truncate_log_message(&exactly_120), exactly_120);

        let over_120 = format!("{}b", "a".repeat(120));
        assert_eq!(
            truncate_log_message(&over_120),
            format!("{}…", "a".repeat(120))
        );

        let utf8 = format!("{}ä😀b", "a".repeat(118));
        assert_eq!(
            truncate_log_message(&utf8),
            format!("{}ä😀…", "a".repeat(118))
        );
    }

    #[tokio::test]
    async fn neuheits_gate_stammgaeste_nur_bei_starkem_zugangswort() {
        let api = MockApi::new();
        let old_store = Arc::new(FakeStore {
            rollup: rollup(50, false),
        });
        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::No, 1.0)]);
        let invite = responder(api.clone(), old_store, judge.clone());
        invite
            .maybe_respond(
                &event("viewer", "Wie kann man das Spiel denn spielen?"),
                "streamer",
            )
            .await;
        assert!(judge.calls().is_empty());
        assert!(api.messages().is_empty());

        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::No, 1.0)]);
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(50, false),
            }),
            judge.clone(),
        );
        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert_eq!(judge.calls().len(), 1);

        let judge = FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::No, 1.0)]);
        let invite = responder(
            api,
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );
        invite
            .maybe_respond(
                &event("viewer", "Wie kann man das Spiel denn spielen?"),
                "streamer",
            )
            .await;
        assert_eq!(judge.calls().len(), 1);
    }

    #[tokio::test]
    async fn provider_fehler_schweigt_immer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let judge = Arc::new(MiniMaxInviteQuestionJudge::new(
            EngagementMinimaxClient::new(
                Some("test-key".to_string()),
                Some(server.uri()),
                Some("MiniMax-M3".to_string()),
                Some(Duration::from_secs(2)),
            ),
        ));
        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );
        invite
            .maybe_respond(
                &event("newbie", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());

        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(50, false),
            }),
            judge,
        );
        invite
            .maybe_respond(
                &event("regular", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn provider_fehler_schweigt_auch_bei_newcomer() {
        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![
                InviteQuestionVerdict::provider_error(),
                InviteQuestionVerdict::provider_error(),
            ]),
        );

        let decision = decide_action(&invite, "newbie", "Wie bekomme ich einen invite?").await;
        assert_eq!(
            decision.action,
            InviteQuestionAction::Silent(SilentReason::JudgeProviderError)
        );

        invite
            .maybe_respond(
                &event("newbie", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn parse_fehler_schweigt_auch_bei_newcomer() {
        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![
                parse_invite_verdict("kaputt"),
                parse_invite_verdict("kaputt"),
            ]),
        );

        let decision = decide_action(&invite, "newbie", "Wie bekomme ich einen invite?").await;
        assert_eq!(
            decision.action,
            InviteQuestionAction::Silent(SilentReason::JudgeParseError)
        );

        invite
            .maybe_respond(
                &event("newbie", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn modell_unsure_fragt_newcomer_weiterhin_nach() {
        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![
                parse_invite_verdict(r#"{"verdict":"unsure","confidence":0.4,"reasoning":"test"}"#),
                parse_invite_verdict(r#"{"verdict":"unsure","confidence":0.4,"reasoning":"test"}"#),
            ]),
        );

        let decision = decide_action(&invite, "newbie", "Wie bekomme ich einen invite?").await;
        assert_eq!(decision.action, InviteQuestionAction::AskConfirmation);

        invite
            .maybe_respond(
                &event("newbie", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert_eq!(api.messages().len(), 1);
    }

    #[tokio::test]
    async fn provider_fehler_verbraucht_keinen_antwort_cooldown() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            InviteQuestionVerdict::provider_error(),
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
        ]);
        let (invite, clock) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        invite
            .maybe_respond(
                &event("newbie", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
        assert_eq!(
            invite
                .user_cooldowns
                .lock()
                .unwrap()
                .get(&("streamer".to_string(), "newbie".to_string()))
                .map(|(_, kind)| *kind),
            Some(CooldownKind::Judged)
        );

        clock.advance(Duration::from_secs(31));
        invite
            .maybe_respond(
                &event("newbie", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert_eq!(judge.call_count(), 2);
        assert_eq!(api.messages(), vec![expected_go("newbie")]);
    }

    #[tokio::test]
    async fn yes_confidence_schwelle_entscheidet_go_oder_rueckfrage() {
        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.69)]),
        );
        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        assert_eq!(api.messages(), vec![expected_confirm("viewer")]);

        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Yes, 0.7)]),
        );
        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        assert_eq!(api.messages(), vec![expected_go("viewer")]);
    }

    #[tokio::test]
    async fn rueckfrage_fenster_verbraucht_ja_nur_innerhalb_von_120_sekunden() {
        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Unsure, 0.0)]),
        );
        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        invite
            .maybe_respond(&event("viewer", "ja"), "streamer")
            .await;
        invite
            .maybe_respond(&event("viewer", "ja"), "streamer")
            .await;
        assert_eq!(
            api.messages(),
            vec![expected_confirm("viewer"), expected_go("viewer")]
        );

        let api = MockApi::new();
        let (invite, clock) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![verdict(InviteQuestionVerdictKind::Unsure, 0.0)]),
        );
        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        clock.advance(Duration::from_secs(121));
        invite
            .maybe_respond(&event("viewer", "ja"), "streamer")
            .await;
        assert_eq!(api.messages(), vec![expected_confirm("viewer")]);

        let api = MockApi::new();
        let invite = responder(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            FakeJudge::new(vec![]),
        );
        invite
            .maybe_respond(&event("viewer", "ja"), "streamer")
            .await;
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn judge_no_verbraucht_keinen_antwort_cooldown() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::No, 1.0),
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
        ]);
        let (invite, clock) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert!(api.messages().is_empty());
        assert_eq!(judge.call_count(), 1);

        clock.advance(Duration::from_secs(31));
        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert_eq!(judge.call_count(), 2);
        assert_eq!(api.messages(), vec![expected_go("viewer")]);
    }

    #[tokio::test]
    async fn judge_bremse_blockt_zweiten_call_binnen_30s() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::No, 1.0),
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
        ]);
        let (invite, _) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;

        assert_eq!(judge.call_count(), 1);
        assert!(api.messages().is_empty());
    }

    #[tokio::test]
    async fn gesendete_antwort_sperrt_user_eine_stunde() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
        ]);
        let (invite, clock) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        clock.advance(Duration::from_secs(59 * 60));
        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert_eq!(judge.call_count(), 1);
        assert_eq!(api.messages().len(), 1);

        clock.advance(Duration::from_secs(2 * 60));
        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert_eq!(judge.call_count(), 2);
        assert_eq!(api.messages().len(), 2);
    }

    #[tokio::test]
    async fn gesendete_antwort_sperrt_kanal_120s() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
        ]);
        let (invite, clock) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        clock.advance(Duration::from_secs(60));
        invite
            .maybe_respond(&event("other", "Wie bekomme ich einen invite?"), "streamer")
            .await;
        assert_eq!(judge.call_count(), 1);
        assert_eq!(api.messages().len(), 1);

        clock.advance(Duration::from_secs(61));
        invite
            .maybe_respond(&event("other", "Wie bekomme ich einen invite?"), "streamer")
            .await;
        assert_eq!(judge.call_count(), 2);
        assert_eq!(api.messages().len(), 2);
    }

    #[tokio::test]
    async fn rueckfrage_zaehlt_als_gesendete_antwort() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::Unsure, 0.0),
            verdict(InviteQuestionVerdictKind::Yes, 0.9),
        ]);
        let (invite, clock) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        invite
            .maybe_respond(&event("other", "Wie bekomme ich einen invite?"), "streamer")
            .await;
        assert_eq!(judge.call_count(), 1);
        assert_eq!(api.messages().len(), 1);

        clock.advance(Duration::from_secs(121));
        invite
            .maybe_respond(
                &event("viewer", "Wie bekomme ich einen invite?"),
                "streamer",
            )
            .await;
        assert_eq!(judge.call_count(), 1);
        assert_eq!(api.messages().len(), 1);
    }

    #[tokio::test]
    async fn cooldowns_blocken_kanal_und_user_wiederholungen() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::Yes, 0.7),
            verdict(InviteQuestionVerdictKind::Yes, 0.7),
            verdict(InviteQuestionVerdictKind::Yes, 0.7),
        ]);
        let (invite, clock) = responder_with_clock(
            api.clone(),
            Arc::new(FakeStore {
                rollup: rollup(1, false),
            }),
            judge.clone(),
        );

        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        invite
            .maybe_respond(
                &event("other", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        assert_eq!(api.messages().len(), 1);
        assert_eq!(judge.calls().len(), 1);

        clock.advance(Duration::from_secs(121));
        invite
            .maybe_respond(
                &event("viewer", "Wie kann man Deadlock spielen?"),
                "streamer",
            )
            .await;
        assert_eq!(api.messages().len(), 1);
        assert_eq!(judge.calls().len(), 1);
    }
}
