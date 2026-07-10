//! Deadlock-Zugangsfragen: billiger Regex-Vorfilter, Newcomer-Gate, Cooldowns,
//! dann MiniMax-Judge und Antwort/Rückfrage.
//!
//! Der KI-Call wird nicht gespawnt, sondern strikt hinter allen billigen Gates
//! gehalten: Command-Präfix, Rückfragefenster, Regex, Rollup-Neuheit und
//! In-Memory-Cooldowns müssen vorher passieren. Dadurch blockiert nur der sehr
//! seltene Kandidatenpfad.

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
use tracing::{debug, warn};

use crate::api::ChatApi;
use crate::commands::DiscordLinkPort;
use crate::types::{ChatMessageEvent, SendOutcome};

const INVITE_QUESTION_CHANNEL_COOLDOWN: Duration = Duration::from_secs(120);
const INVITE_QUESTION_USER_COOLDOWN: Duration = Duration::from_secs(3600);
const PENDING_CONFIRMATION_WINDOW: Duration = Duration::from_secs(120);

const GO_REPLY: &str = "Wenn du Zugang zu Deadlock brauchst: Auf unserem Discord bekommst du eine Einladung und Hilfe beim Einstieg.";
const CONFIRM_REPLY: &str =
    "Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg.";

const INVITE_JUDGE_SYSTEM_PROMPT: &str = r#"Du bist ein vorsichtiger deutschsprachiger Twitch-Chat-Moderator für einen Deadlock-Stream.

Beurteile, ob die Nachricht danach fragt, wie der Chatter Zugang zum Spiel Deadlock bekommt: Einladung, Invite, Beta-Key, Early Access oder wie man mitspielen kann.

Antworte EXAKT mit einem JSON-Objekt ohne Markdown und ohne weiteren Text:
{"verdict":"yes"|"no"|"unsure","confidence":0.0-1.0,"reasoning":"..."}

Regeln:
- "yes" nur, wenn die Nachricht wirklich nach Zugang zu Deadlock fragt.
- "no" bei normalem Gameplay, Meinung, Smalltalk oder Discord ohne Zugangsfrage.
- "unsure" wenn die Absicht unklar ist."#;

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

#[derive(Debug, Clone, PartialEq)]
pub struct InviteQuestionVerdict {
    pub verdict: InviteQuestionVerdictKind,
    pub confidence: f32,
    pub reasoning: String,
}

impl InviteQuestionVerdict {
    pub fn unsure() -> Self {
        Self {
            verdict: InviteQuestionVerdictKind::Unsure,
            confidence: 0.0,
            reasoning: String::new(),
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
                InviteQuestionVerdict::unsure()
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
        return InviteQuestionVerdict::unsure();
    };
    if !parsed.confidence.is_finite() {
        return InviteQuestionVerdict::unsure();
    }
    let verdict = match parsed.verdict.as_str() {
        "yes" => InviteQuestionVerdictKind::Yes,
        "no" => InviteQuestionVerdictKind::No,
        "unsure" => InviteQuestionVerdictKind::Unsure,
        _ => return InviteQuestionVerdict::unsure(),
    };
    InviteQuestionVerdict {
        verdict,
        confidence: parsed.confidence.clamp(0.0, 1.0),
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

pub struct InviteQuestionResponder {
    api: Arc<dyn ChatApi>,
    discord_link: Arc<dyn DiscordLinkPort>,
    store: Arc<dyn InviteQuestionStore>,
    judge: Arc<dyn InviteQuestionJudge>,
    channel_cooldowns: Mutex<HashMap<String, Instant>>,
    user_cooldowns: Mutex<HashMap<(String, String), Instant>>,
    pending_confirmations: Mutex<HashMap<(String, String), Instant>>,
}

impl InviteQuestionResponder {
    pub fn new(
        api: Arc<dyn ChatApi>,
        discord_link: Arc<dyn DiscordLinkPort>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
    ) -> Self {
        Self {
            api,
            discord_link,
            store,
            judge,
            channel_cooldowns: Mutex::new(HashMap::new()),
            user_cooldowns: Mutex::new(HashMap::new()),
            pending_confirmations: Mutex::new(HashMap::new()),
        }
    }

    pub async fn maybe_respond(&self, event: &ChatMessageEvent, channel_login: &str) {
        let raw = event.text();
        if raw.is_empty() || raw.starts_with('!') {
            return;
        }

        let channel_login = normalize_login(channel_login);
        let chatter_login = normalize_login(&event.chatter_user_login);
        if channel_login.is_empty() || chatter_login.is_empty() {
            return;
        }

        if self
            .maybe_handle_pending_confirmation(event, &channel_login, &chatter_login, raw)
            .await
        {
            return;
        }

        let signal = classify_invite_question(raw);
        if !signal.is_candidate {
            return;
        }

        let is_newcomer = self.is_newcomer(&channel_login, &chatter_login).await;
        // ponytail: Stammgäste nur bei explizitem Zugangswort, Schwelle statt Schalter
        if !is_newcomer && !signal.has_strong_access {
            return;
        }

        if !self.reserve_cooldown(&channel_login, &chatter_login) {
            return;
        }

        let verdict = self
            .judge
            .judge(InviteQuestionJudgeInput {
                message: raw.to_string(),
                is_newcomer,
                is_deadlock_live: true,
            })
            .await;

        match verdict.verdict {
            InviteQuestionVerdictKind::Yes if verdict.confidence >= 0.7 => {
                self.send_go(event, &channel_login, &chatter_login).await;
            }
            InviteQuestionVerdictKind::Yes | InviteQuestionVerdictKind::Unsure if is_newcomer => {
                if !self.send_confirmation_question(event, &chatter_login).await {
                    return;
                }
                self.remember_pending_confirmation(&channel_login, &chatter_login);
            }
            _ => {}
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

    fn reserve_cooldown(&self, channel_login: &str, chatter_login: &str) -> bool {
        let now = Instant::now();
        let Ok(mut channels) = self.channel_cooldowns.lock() else {
            return false;
        };
        if channels
            .get(channel_login)
            .is_some_and(|last| now.duration_since(*last) < INVITE_QUESTION_CHANNEL_COOLDOWN)
        {
            return false;
        }

        let key = (channel_login.to_string(), chatter_login.to_string());
        let Ok(mut users) = self.user_cooldowns.lock() else {
            return false;
        };
        if users
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) < INVITE_QUESTION_USER_COOLDOWN)
        {
            return false;
        }

        channels.insert(channel_login.to_string(), now);
        users.insert(key, now);
        true
    }

    async fn maybe_handle_pending_confirmation(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        chatter_login: &str,
        raw: &str,
    ) -> bool {
        let key = (channel_login.to_string(), chatter_login.to_string());
        let now = Instant::now();
        let is_open = {
            let Ok(mut pending) = self.pending_confirmations.lock() else {
                return false;
            };
            match pending.get(&key).copied() {
                Some(last) if now.duration_since(last) <= PENDING_CONFIRMATION_WINDOW => true,
                Some(_) => {
                    pending.remove(&key);
                    false
                }
                None => false,
            }
        };
        if !is_open || !is_affirmative(raw) {
            return false;
        }
        if let Ok(mut pending) = self.pending_confirmations.lock() {
            pending.remove(&key);
        }
        self.send_go(event, channel_login, chatter_login).await;
        true
    }

    fn remember_pending_confirmation(&self, channel_login: &str, chatter_login: &str) {
        if let Ok(mut pending) = self.pending_confirmations.lock() {
            pending.insert(
                (channel_login.to_string(), chatter_login.to_string()),
                Instant::now(),
            );
        }
    }

    async fn send_confirmation_question(
        &self,
        event: &ChatMessageEvent,
        chatter_login: &str,
    ) -> bool {
        let msg = format!("@{chatter_login} {CONFIRM_REPLY}");
        self.send(event, &msg).await
    }

    async fn send_go(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        chatter_login: &str,
    ) -> bool {
        let invite = match self.discord_link.discord_invite(channel_login).await {
            Ok(Some(url)) if !url.trim().is_empty() => url,
            Ok(_) => return false,
            Err(error) => {
                debug!(%error, channel_login, "Invite-Question-Discord-Link nicht lesbar");
                return false;
            }
        };
        let msg = format!("@{chatter_login} {GO_REPLY} {invite}");
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
    use crate::commands::DiscordLinkPort;
    use crate::types::{ChatMessageBody, ChatMessageEvent, SendOutcome};
    use async_trait::async_trait;
    use std::collections::VecDeque;
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

    struct FakeDiscordLink;

    #[async_trait]
    impl DiscordLinkPort for FakeDiscordLink {
        async fn discord_invite(&self, _channel_login: &str) -> Result<Option<String>, String> {
            Ok(Some("https://discord.gg/test".to_string()))
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
    }

    impl FakeJudge {
        fn new(verdicts: Vec<InviteQuestionVerdict>) -> Arc<Self> {
            Arc::new(Self {
                verdicts: Mutex::new(verdicts.into()),
                calls: Mutex::new(vec![]),
            })
        }

        fn calls(&self) -> Vec<InviteQuestionJudgeInput> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl InviteQuestionJudge for FakeJudge {
        async fn judge(&self, input: InviteQuestionJudgeInput) -> InviteQuestionVerdict {
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
        }
    }

    fn responder(
        api: Arc<MockApi>,
        store: Arc<dyn InviteQuestionStore>,
        judge: Arc<dyn InviteQuestionJudge>,
    ) -> InviteQuestionResponder {
        InviteQuestionResponder::new(api, Arc::new(FakeDiscordLink), store, judge)
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
    async fn provider_fehler_wird_unsure_und_niemals_go() {
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
        assert_eq!(
            api.messages(),
            vec!["@newbie Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg."]
        );

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
        assert_eq!(
            api.messages(),
            vec!["@viewer Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg."]
        );

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
        assert_eq!(
            api.messages(),
            vec!["@viewer Wenn du Zugang zu Deadlock brauchst: Auf unserem Discord bekommst du eine Einladung und Hilfe beim Einstieg. https://discord.gg/test"]
        );
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
            vec![
                "@viewer Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg.",
                "@viewer Wenn du Zugang zu Deadlock brauchst: Auf unserem Discord bekommst du eine Einladung und Hilfe beim Einstieg. https://discord.gg/test",
            ]
        );

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
        invite.pending_confirmations.lock().unwrap().insert(
            ("streamer".to_string(), "viewer".to_string()),
            Instant::now() - Duration::from_secs(121),
        );
        invite
            .maybe_respond(&event("viewer", "ja"), "streamer")
            .await;
        assert_eq!(
            api.messages(),
            vec!["@viewer Suchst du einen Invite für Deadlock? Sag einfach kurz ja, dann schick ich dir den Weg."]
        );

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
    async fn cooldowns_blocken_kanal_und_user_wiederholungen() {
        let api = MockApi::new();
        let judge = FakeJudge::new(vec![
            verdict(InviteQuestionVerdictKind::Yes, 0.7),
            verdict(InviteQuestionVerdictKind::Yes, 0.7),
            verdict(InviteQuestionVerdictKind::Yes, 0.7),
        ]);
        let invite = responder(
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

        invite.channel_cooldowns.lock().unwrap().insert(
            "streamer".to_string(),
            Instant::now() - Duration::from_secs(121),
        );
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
