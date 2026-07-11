//! LFG-Mitspieler-Pitch: billiger Regex-Vorfilter vor dem KI-Judge.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tb_engagement::minimax_chat::EngagementMinimaxClient;
use tracing::{debug, info, warn};

use crate::api::ChatApi;
use crate::commands::{InviteReplyNotifier, PromoBlockCheck};
use crate::invite_question::InviteQuestionInviteUrlPort;
use crate::types::{ChatMessageEvent, SendOutcome};

const LFG_PITCH_CHANNEL_COOLDOWN: Duration = Duration::from_secs(120);
const LFG_PITCH_USER_COOLDOWN: Duration = Duration::from_secs(6 * 60 * 60);
const LFG_PITCH_JUDGE_COOLDOWN: Duration = Duration::from_secs(30);

pub const LFG_PITCH_REPLY: &str =
    "@{chatter} Schau gerne mal in unsere Community rein: {invite} da findest du jederzeit passende Mitspieler :)";

const LFG_JUDGE_SYSTEM_PROMPT: &str = r#"Du bist ein vorsichtiger deutschsprachiger Twitch-Chat-Moderator für einen Deadlock-Stream.

Beurteile, ob die Nachricht gerade Mitspieler für Deadlock sucht: LFG, Gruppe, Duo, Stack, Lobby oder Leute zum Zocken.

Antworte EXAKT mit einem JSON-Objekt ohne Markdown und ohne weiteren Text:
{"verdict":"yes"|"no"|"unsure","confidence":0.0-1.0,"reasoning":"..."}

Regeln:
- "yes" nur, wenn die Person wirklich gerade Mitspieler für Deadlock sucht.
- "no" bei Builds, Gameplay-Fragen, Smalltalk oder Zugang/Invite-Fragen ohne Mitspieler-Suche.
- "unsure" wenn die Absicht unklar ist."#;

fn direct_lfg_re() -> &'static Result<Regex, regex::Error> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(lfg|looking\s+for\s+group)\b"))
}

fn search_lfg_re() -> &'static Result<Regex, regex::Error> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(such\w*|brauche?\w*|wer\s+hat\s+bock|noch\s+jemand|jemand)\b")
    })
}

fn object_lfg_re() -> &'static Result<Regex, regex::Error> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(rank(?:ed)?|zock\w*|spiel\w*|duo|trio|stack|lobby|team|gruppe|group|mate\w*|mitspieler\w*)\b",
        )
    })
}

fn is_match(re: &Result<Regex, regex::Error>, raw: &str) -> bool {
    re.as_ref().is_ok_and(|re| re.is_match(raw))
}

pub fn classify_lfg(content: &str) -> bool {
    let raw = content.trim();
    if raw.is_empty() || raw.starts_with('!') {
        return false;
    }

    is_match(direct_lfg_re(), raw)
        || (is_match(search_lfg_re(), raw) && is_match(object_lfg_re(), raw))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfgVerdictKind {
    Yes,
    No,
    Unsure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfgVerdictSource {
    Model,
    ProviderError,
    ParseError,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LfgVerdict {
    pub verdict: LfgVerdictKind,
    pub confidence: f32,
    pub reasoning: String,
    pub source: LfgVerdictSource,
}

impl LfgVerdict {
    fn provider_error() -> Self {
        Self {
            verdict: LfgVerdictKind::Unsure,
            confidence: 0.0,
            reasoning: "provider_error".to_string(),
            source: LfgVerdictSource::ProviderError,
        }
    }

    fn parse_error() -> Self {
        Self {
            verdict: LfgVerdictKind::Unsure,
            confidence: 0.0,
            reasoning: "parse_error".to_string(),
            source: LfgVerdictSource::ParseError,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfgJudgeInput {
    pub message: String,
}

#[async_trait]
pub trait LfgJudge: Send + Sync {
    async fn judge(&self, input: LfgJudgeInput) -> LfgVerdict;
}

pub struct MiniMaxLfgJudge {
    client: EngagementMinimaxClient,
}

impl MiniMaxLfgJudge {
    pub fn new(client: EngagementMinimaxClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl LfgJudge for MiniMaxLfgJudge {
    async fn judge(&self, input: LfgJudgeInput) -> LfgVerdict {
        let user = format!(
            "Sucht diese Person gerade Mitspieler für Deadlock? yes/no/unsure\n\nNachricht: {}",
            input.message
        );
        let messages = Value::Array(vec![
            serde_json::json!({"role": "system", "content": LFG_JUDGE_SYSTEM_PROMPT}),
            serde_json::json!({"role": "user", "content": user}),
        ]);

        match self
            .client
            .messages_completion_uncapped(messages, 0.0)
            .await
        {
            Ok(raw) => parse_lfg_verdict(&raw),
            Err(error) => {
                debug!("LFG-Pitch-Judge nicht verfügbar: {error}");
                LfgVerdict::provider_error()
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawLfgVerdict {
    verdict: String,
    confidence: f32,
    reasoning: String,
}

fn parse_lfg_verdict(raw: &str) -> LfgVerdict {
    let parsed = serde_json::from_str::<RawLfgVerdict>(raw.trim()).or_else(|_| {
        extract_json_object(raw)
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing JSON object")))
            .and_then(serde_json::from_str::<RawLfgVerdict>)
    });
    let Ok(parsed) = parsed else {
        return LfgVerdict::parse_error();
    };
    if !parsed.confidence.is_finite() {
        return LfgVerdict::parse_error();
    }
    let verdict = match parsed.verdict.as_str() {
        "yes" => LfgVerdictKind::Yes,
        "no" => LfgVerdictKind::No,
        "unsure" => LfgVerdictKind::Unsure,
        _ => return LfgVerdict::parse_error(),
    };
    LfgVerdict {
        verdict,
        confidence: parsed.confidence.clamp(0.0, 1.0),
        reasoning: parsed.reasoning,
        source: LfgVerdictSource::Model,
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

trait LfgClock: Send + Sync {
    fn now(&self) -> Instant;
}

struct SystemLfgClock;

impl LfgClock for SystemLfgClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

pub fn lfg_pitch_enabled_from_env() -> bool {
    match std::env::var("LFG_PITCH_ENABLED") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfgPitchAction {
    Silent(SilentReason),
    SendGo,
}

impl LfgPitchAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Silent(_) => "silent",
            Self::SendGo => "send_go",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilentReason {
    EmptyMessage,
    CommandPrefix,
    MissingLogin,
    NoRegexMatch,
    PromoBlockedByPlan,
    NoInviteUrl,
    CooldownChannel,
    CooldownUserReplied,
    CooldownJudgeBrake,
    JudgeNo,
    JudgeUnsure,
    JudgeProviderError,
    JudgeParseError,
    KillSwitchOff,
}

impl SilentReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::EmptyMessage => "empty_message",
            Self::CommandPrefix => "command_prefix",
            Self::MissingLogin => "missing_login",
            Self::NoRegexMatch => "no_regex_match",
            Self::PromoBlockedByPlan => "promo_blocked_by_plan",
            Self::NoInviteUrl => "no_invite_url",
            Self::CooldownChannel => "cooldown_channel",
            Self::CooldownUserReplied => "cooldown_user_replied",
            Self::CooldownJudgeBrake => "cooldown_judge_brake",
            Self::JudgeNo => "judge_no",
            Self::JudgeUnsure => "judge_unsure",
            Self::JudgeProviderError => "judge_provider_error",
            Self::JudgeParseError => "judge_parse_error",
            Self::KillSwitchOff => "kill_switch_off",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfgLoggedVerdict {
    Yes,
    No,
    Unsure,
    ProviderError,
    ParseError,
}

impl LfgLoggedVerdict {
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

impl From<&LfgVerdict> for LfgLoggedVerdict {
    fn from(verdict: &LfgVerdict) -> Self {
        match verdict.source {
            LfgVerdictSource::ProviderError => Self::ProviderError,
            LfgVerdictSource::ParseError => Self::ParseError,
            LfgVerdictSource::Model => match verdict.verdict {
                LfgVerdictKind::Yes => Self::Yes,
                LfgVerdictKind::No => Self::No,
                LfgVerdictKind::Unsure => Self::Unsure,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LfgPitchDecision {
    pub action: LfgPitchAction,
    pub verdict: Option<LfgLoggedVerdict>,
    pub confidence: f32,
    channel_login: String,
    chatter_login: String,
    message: String,
    invite_url: Option<String>,
}

impl LfgPitchDecision {
    fn silent(
        reason: SilentReason,
        channel_login: String,
        chatter_login: String,
        message: String,
    ) -> Self {
        Self {
            action: LfgPitchAction::Silent(reason),
            verdict: None,
            confidence: 0.0,
            channel_login,
            chatter_login,
            message,
            invite_url: None,
        }
    }

    fn judged(
        action: LfgPitchAction,
        verdict: &LfgVerdict,
        channel_login: String,
        chatter_login: String,
        message: String,
    ) -> Self {
        Self {
            action,
            verdict: Some(LfgLoggedVerdict::from(verdict)),
            confidence: verdict.confidence,
            channel_login,
            chatter_login,
            message,
            invite_url: None,
        }
    }

    fn log_level(&self) -> tracing::Level {
        match self.action {
            LfgPitchAction::Silent(
                SilentReason::JudgeProviderError | SilentReason::JudgeParseError,
            ) => tracing::Level::WARN,
            LfgPitchAction::SendGo => tracing::Level::INFO,
            LfgPitchAction::Silent(_) if self.verdict.is_some() => tracing::Level::INFO,
            LfgPitchAction::Silent(_) => tracing::Level::DEBUG,
        }
    }
}

pub struct LfgPitchResponder {
    api: Arc<dyn ChatApi>,
    invite_url: Arc<dyn InviteQuestionInviteUrlPort>,
    judge: Arc<dyn LfgJudge>,
    clock: Arc<dyn LfgClock>,
    enabled: bool,
    promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
    invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    channel_cooldowns: Mutex<HashMap<String, Instant>>,
    user_cooldowns: Mutex<HashMap<(String, String), Instant>>,
    judge_cooldowns: Mutex<HashMap<(String, String), Instant>>,
}

impl LfgPitchResponder {
    pub fn new(
        api: Arc<dyn ChatApi>,
        invite_url: Arc<dyn InviteQuestionInviteUrlPort>,
        judge: Arc<dyn LfgJudge>,
        enabled: bool,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> Self {
        Self::new_with_clock(
            api,
            invite_url,
            judge,
            Arc::new(SystemLfgClock),
            enabled,
            promo_block_check,
            invite_reply_notifier,
        )
    }

    fn new_with_clock(
        api: Arc<dyn ChatApi>,
        invite_url: Arc<dyn InviteQuestionInviteUrlPort>,
        judge: Arc<dyn LfgJudge>,
        clock: Arc<dyn LfgClock>,
        enabled: bool,
        promo_block_check: Option<Arc<dyn PromoBlockCheck>>,
        invite_reply_notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> Self {
        Self {
            api,
            invite_url,
            judge,
            clock,
            enabled,
            promo_block_check,
            invite_reply_notifier,
            channel_cooldowns: Mutex::new(HashMap::new()),
            user_cooldowns: Mutex::new(HashMap::new()),
            judge_cooldowns: Mutex::new(HashMap::new()),
        }
    }

    pub async fn maybe_respond(&self, event: &ChatMessageEvent, channel_login: &str) {
        let decision = self.decide(event, channel_login).await;
        self.log_decision(&decision);

        if decision.action != LfgPitchAction::SendGo {
            return;
        }
        let Some(invite_url) = decision.invite_url.as_deref() else {
            return;
        };
        if self
            .send_go(event, &decision.chatter_login, invite_url)
            .await
        {
            self.mark_replied(&decision.channel_login, &decision.chatter_login);
            self.note_invite_reply(&decision.channel_login).await;
        }
    }

    pub async fn decide(&self, event: &ChatMessageEvent, channel_login: &str) -> LfgPitchDecision {
        let raw = event.text();
        let channel_login = normalize_login(channel_login);
        let chatter_login = normalize_login(&event.chatter_user_login);

        if !self.enabled {
            return LfgPitchDecision::silent(
                SilentReason::KillSwitchOff,
                channel_login,
                chatter_login,
                raw.to_string(),
            );
        }
        if raw.is_empty() {
            return LfgPitchDecision::silent(
                SilentReason::EmptyMessage,
                channel_login,
                chatter_login,
                raw.to_string(),
            );
        }
        if raw.starts_with('!') {
            return LfgPitchDecision::silent(
                SilentReason::CommandPrefix,
                channel_login,
                chatter_login,
                raw.to_string(),
            );
        }
        if channel_login.is_empty() || chatter_login.is_empty() {
            return LfgPitchDecision::silent(
                SilentReason::MissingLogin,
                channel_login,
                chatter_login,
                raw.to_string(),
            );
        }
        if !classify_lfg(raw) {
            return LfgPitchDecision::silent(
                SilentReason::NoRegexMatch,
                channel_login,
                chatter_login,
                raw.to_string(),
            );
        }
        if let Some(promo_block_check) = &self.promo_block_check {
            if promo_block_check.is_promo_blocked(&channel_login).await {
                return LfgPitchDecision::silent(
                    SilentReason::PromoBlockedByPlan,
                    channel_login,
                    chatter_login,
                    raw.to_string(),
                );
            }
        }
        let Some(invite_url) = self.resolve_invite_url(&channel_login).await else {
            return LfgPitchDecision::silent(
                SilentReason::NoInviteUrl,
                channel_login,
                chatter_login,
                raw.to_string(),
            );
        };
        if let Some(reason) = self.cooldown_block_reason(&channel_login, &chatter_login) {
            return LfgPitchDecision::silent(reason, channel_login, chatter_login, raw.to_string());
        }

        let verdict = self
            .judge
            .judge(LfgJudgeInput {
                message: raw.to_string(),
            })
            .await;

        let action = match verdict.source {
            LfgVerdictSource::ProviderError => {
                LfgPitchAction::Silent(SilentReason::JudgeProviderError)
            }
            LfgVerdictSource::ParseError => LfgPitchAction::Silent(SilentReason::JudgeParseError),
            LfgVerdictSource::Model => match verdict.verdict {
                LfgVerdictKind::Yes if verdict.confidence >= 0.7 => LfgPitchAction::SendGo,
                LfgVerdictKind::No => LfgPitchAction::Silent(SilentReason::JudgeNo),
                LfgVerdictKind::Yes | LfgVerdictKind::Unsure => {
                    LfgPitchAction::Silent(SilentReason::JudgeUnsure)
                }
            },
        };
        let mut decision = LfgPitchDecision::judged(
            action,
            &verdict,
            channel_login,
            chatter_login,
            raw.to_string(),
        );
        if action == LfgPitchAction::SendGo {
            decision.invite_url = Some(invite_url);
        }
        decision
    }

    fn log_decision(&self, decision: &LfgPitchDecision) {
        let message = truncate_log_message(&decision.message);
        let verdict = decision
            .verdict
            .map(LfgLoggedVerdict::as_str)
            .unwrap_or("not_judged");
        match decision.action {
            LfgPitchAction::Silent(reason) if decision.log_level() == tracing::Level::WARN => {
                warn!(
                    channel = %decision.channel_login,
                    chatter = %decision.chatter_login,
                    message = %message,
                    verdict,
                    confidence = decision.confidence,
                    action = decision.action.as_str(),
                    silent_reason = reason.as_str(),
                );
            }
            LfgPitchAction::Silent(reason) if decision.log_level() == tracing::Level::INFO => {
                info!(
                    channel = %decision.channel_login,
                    chatter = %decision.chatter_login,
                    message = %message,
                    verdict,
                    confidence = decision.confidence,
                    action = decision.action.as_str(),
                    silent_reason = reason.as_str(),
                );
            }
            LfgPitchAction::Silent(reason) => {
                debug!(
                    channel = %decision.channel_login,
                    chatter = %decision.chatter_login,
                    message = %message,
                    verdict,
                    confidence = decision.confidence,
                    action = decision.action.as_str(),
                    silent_reason = reason.as_str(),
                );
            }
            LfgPitchAction::SendGo => {
                info!(
                    channel = %decision.channel_login,
                    chatter = %decision.chatter_login,
                    message = %message,
                    verdict,
                    confidence = decision.confidence,
                    action = decision.action.as_str(),
                );
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
            .is_some_and(|last| now.duration_since(*last) < LFG_PITCH_CHANNEL_COOLDOWN)
        {
            return Some(SilentReason::CooldownChannel);
        }
        drop(channels);

        let key = (channel_login.to_string(), chatter_login.to_string());
        let Ok(users) = self.user_cooldowns.lock() else {
            return Some(SilentReason::CooldownUserReplied);
        };
        if users
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) < LFG_PITCH_USER_COOLDOWN)
        {
            return Some(SilentReason::CooldownUserReplied);
        }
        drop(users);

        let Ok(judges) = self.judge_cooldowns.lock() else {
            return Some(SilentReason::CooldownJudgeBrake);
        };
        if judges
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) < LFG_PITCH_JUDGE_COOLDOWN)
        {
            return Some(SilentReason::CooldownJudgeBrake);
        }

        None
    }

    fn mark_replied(&self, channel_login: &str, chatter_login: &str) {
        let now = self.clock.now();
        if let Ok(mut channels) = self.channel_cooldowns.lock() {
            channels.insert(channel_login.to_string(), now);
        }
        let key = (channel_login.to_string(), chatter_login.to_string());
        if let Ok(mut users) = self.user_cooldowns.lock() {
            users.insert(key.clone(), now);
        }
        if let Ok(mut judges) = self.judge_cooldowns.lock() {
            judges.insert(key, now);
        }
    }

    async fn resolve_invite_url(&self, channel_login: &str) -> Option<String> {
        match self.invite_url.invite_url(channel_login).await {
            Ok(Some(url)) if !url.trim().is_empty() => Some(url),
            Ok(_) => None,
            Err(error) => {
                debug!(%error, channel_login, "LFG-Pitch-Invite-URL nicht lesbar");
                None
            }
        }
    }

    async fn note_invite_reply(&self, channel_login: &str) {
        if let Some(notifier) = &self.invite_reply_notifier {
            notifier.note_invite_reply(channel_login).await;
        }
    }

    async fn send_go(&self, event: &ChatMessageEvent, chatter_login: &str, invite: &str) -> bool {
        let message = LFG_PITCH_REPLY
            .replace("{chatter}", chatter_login)
            .replace("{invite}", invite);
        self.send(event, &message).await
    }

    async fn send(&self, event: &ChatMessageEvent, message: &str) -> bool {
        match self
            .api
            .send_message(&event.broadcaster_user_id, message)
            .await
        {
            Ok(SendOutcome::Sent) => true,
            Ok(outcome) => {
                debug!(?outcome, "LFG-Pitch-Antwort von Twitch verworfen");
                false
            }
            Err(error) => {
                warn!(%error, "LFG-Pitch-Antwort konnte nicht gesendet werden");
                false
            }
        }
    }
}

fn normalize_login(login: &str) -> String {
    login.trim().trim_start_matches('#').to_lowercase()
}

fn truncate_log_message(raw: &str) -> String {
    match raw.char_indices().nth(120) {
        Some((index, _)) => format!("{}...", &raw[..index]),
        None => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{BanOutcome, ChatApi};
    use crate::commands::{InviteReplyNotifier, PromoBlockCheck};
    use crate::invite_question::InviteQuestionInviteUrlPort;
    use crate::types::{ChatMessageBody, ChatMessageEvent, SendOutcome};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tb_engagement::minimax_chat::EngagementMinimaxClient;

    #[test]
    fn parse_lfg_verdict_liefert_yes_mit_confidence() {
        let verdict = parse_lfg_verdict(r#"{"verdict":"yes","confidence":0.82,"reasoning":"lfg"}"#);

        assert_eq!(verdict.verdict, LfgVerdictKind::Yes);
        assert_eq!(verdict.source, LfgVerdictSource::Model);
        assert_eq!(verdict.confidence, 0.82);
        assert_eq!(verdict.reasoning, "lfg");
    }

    #[test]
    fn parse_lfg_verdict_markiert_muell_als_parse_error() {
        let verdict = parse_lfg_verdict("kaputt");

        assert_eq!(verdict.source, LfgVerdictSource::ParseError);
    }

    #[tokio::test]
    async fn minimax_lfg_judge_ohne_provider_liefert_provider_error() {
        let judge = MiniMaxLfgJudge::new(EngagementMinimaxClient::new(
            None,
            Some("http://127.0.0.1:1".to_string()),
            None,
            None,
        ));

        let verdict = judge
            .judge(LfgJudgeInput {
                message: "lfg".to_string(),
            })
            .await;

        assert_eq!(verdict.source, LfgVerdictSource::ProviderError);
    }

    struct MockApi {
        sent: Mutex<Vec<String>>,
        order: Arc<Mutex<Vec<String>>>,
    }

    impl MockApi {
        fn new(order: Arc<Mutex<Vec<String>>>) -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(vec![]),
                order,
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
            self.order.lock().unwrap().push("send".to_string());
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

    struct FakeDiscordLink {
        url: Option<String>,
    }

    #[async_trait]
    impl InviteQuestionInviteUrlPort for FakeDiscordLink {
        async fn invite_url(&self, _channel_login: &str) -> Result<Option<String>, String> {
            Ok(self.url.clone())
        }
    }

    struct FakePromoBlock {
        blocked: bool,
    }

    #[async_trait]
    impl PromoBlockCheck for FakePromoBlock {
        async fn is_promo_blocked(&self, _channel_login: &str) -> bool {
            self.blocked
        }
    }

    struct FakeNotifier {
        calls: Mutex<Vec<String>>,
        order: Arc<Mutex<Vec<String>>>,
    }

    impl FakeNotifier {
        fn new(order: Arc<Mutex<Vec<String>>>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(vec![]),
                order,
            })
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl InviteReplyNotifier for FakeNotifier {
        async fn note_invite_reply(&self, channel_login: &str) {
            self.order.lock().unwrap().push("note".to_string());
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
    }

    impl LfgClock for FakeClock {
        fn now(&self) -> Instant {
            *self.now.lock().unwrap()
        }
    }

    struct FakeJudge {
        verdicts: Mutex<VecDeque<LfgVerdict>>,
        calls: Mutex<Vec<LfgJudgeInput>>,
    }

    impl FakeJudge {
        fn new(verdicts: Vec<LfgVerdict>) -> Arc<Self> {
            Arc::new(Self {
                verdicts: Mutex::new(verdicts.into()),
                calls: Mutex::new(vec![]),
            })
        }

        fn calls(&self) -> Vec<LfgJudgeInput> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LfgJudge for FakeJudge {
        async fn judge(&self, input: LfgJudgeInput) -> LfgVerdict {
            self.calls.lock().unwrap().push(input);
            self.verdicts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(LfgVerdict::provider_error)
        }
    }

    fn model_verdict(kind: LfgVerdictKind, confidence: f32) -> LfgVerdict {
        LfgVerdict {
            verdict: kind,
            confidence,
            reasoning: "test".to_string(),
            source: LfgVerdictSource::Model,
        }
    }

    fn event(chatter: &str, text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "broadcaster-id".to_string(),
            broadcaster_user_login: "streamer".to_string(),
            broadcaster_user_name: String::new(),
            chatter_user_id: chatter.to_string(),
            chatter_user_login: chatter.to_string(),
            chatter_user_name: String::new(),
            message_id: "msg-1".to_string(),
            message: ChatMessageBody {
                text: text.to_string(),
                fragments: vec![],
            },
            badges: vec![],
            color: String::new(),
            source_broadcaster_user_id: None,
            source_broadcaster_user_login: None,
            source_message_id: None,
        }
    }

    // ponytail: local test fixture tuple; a named struct would only add noise here.
    #[allow(clippy::type_complexity)]
    fn responder(
        enabled: bool,
        url: Option<&str>,
        promo_blocked: bool,
        judge: Arc<dyn LfgJudge>,
    ) -> (
        LfgPitchResponder,
        Arc<MockApi>,
        Arc<FakeNotifier>,
        Arc<FakeClock>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let order = Arc::new(Mutex::new(vec![]));
        let api = MockApi::new(Arc::clone(&order));
        let notifier = FakeNotifier::new(Arc::clone(&order));
        let clock = FakeClock::new();
        let api_trait: Arc<dyn ChatApi> = api.clone();
        let notifier_trait: Arc<dyn InviteReplyNotifier> = notifier.clone();
        let clock_trait: Arc<dyn LfgClock> = clock.clone();
        (
            LfgPitchResponder::new_with_clock(
                api_trait,
                Arc::new(FakeDiscordLink {
                    url: url.map(str::to_string),
                }),
                judge,
                clock_trait,
                enabled,
                Some(Arc::new(FakePromoBlock {
                    blocked: promo_blocked,
                })),
                Some(notifier_trait),
            ),
            api,
            notifier,
            clock,
            order,
        )
    }

    async fn decide_action(
        responder: &LfgPitchResponder,
        chatter: &str,
        text: &str,
    ) -> LfgPitchAction {
        responder
            .decide(&event(chatter, text), "streamer")
            .await
            .action
    }

    #[tokio::test]
    async fn decide_liefert_kill_switch_off() {
        let (responder, _, _, _, _) = responder(
            false,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::KillSwitchOff)
        );
    }

    #[tokio::test]
    async fn decide_liefert_empty_message() {
        let (responder, _, _, _, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );

        assert_eq!(
            decide_action(&responder, "viewer", "   ").await,
            LfgPitchAction::Silent(SilentReason::EmptyMessage)
        );
    }

    #[tokio::test]
    async fn decide_liefert_command_prefix() {
        let (responder, _, _, _, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );

        assert_eq!(
            decide_action(&responder, "viewer", "!lfg").await,
            LfgPitchAction::Silent(SilentReason::CommandPrefix)
        );
    }

    #[tokio::test]
    async fn decide_liefert_missing_login() {
        let (responder, _, _, _, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );

        assert_eq!(
            responder.decide(&event("", "lfg"), "streamer").await.action,
            LfgPitchAction::Silent(SilentReason::MissingLogin)
        );
    }

    #[tokio::test]
    async fn decide_liefert_no_regex_match() {
        let (responder, _, _, _, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );

        assert_eq!(
            decide_action(&responder, "viewer", "suche einen guten build").await,
            LfgPitchAction::Silent(SilentReason::NoRegexMatch)
        );
    }

    #[tokio::test]
    async fn decide_liefert_promo_blocked_by_plan_ohne_send() {
        let judge = FakeJudge::new(vec![model_verdict(LfgVerdictKind::Yes, 0.9)]);
        let (responder, api, _, _, _) =
            responder(true, Some("https://discord.gg/test"), true, judge.clone());

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::PromoBlockedByPlan)
        );
        assert!(api.messages().is_empty());
        assert!(judge.calls().is_empty());
    }

    #[tokio::test]
    async fn decide_liefert_no_invite_url() {
        let judge = FakeJudge::new(vec![model_verdict(LfgVerdictKind::Yes, 0.9)]);
        let (responder, _, _, _, _) = responder(true, None, false, judge.clone());

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::NoInviteUrl)
        );
        assert!(judge.calls().is_empty());
    }

    #[tokio::test]
    async fn decide_liefert_cooldown_channel() {
        let (responder, _, _, clock, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );
        responder
            .channel_cooldowns
            .lock()
            .unwrap()
            .insert("streamer".to_string(), clock.now());

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::CooldownChannel)
        );
    }

    #[tokio::test]
    async fn decide_liefert_cooldown_user_replied() {
        let (responder, _, _, clock, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );
        responder
            .user_cooldowns
            .lock()
            .unwrap()
            .insert(("streamer".to_string(), "viewer".to_string()), clock.now());

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::CooldownUserReplied)
        );
    }

    #[tokio::test]
    async fn decide_liefert_cooldown_judge_brake() {
        let (responder, _, _, clock, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![]),
        );
        responder
            .judge_cooldowns
            .lock()
            .unwrap()
            .insert(("streamer".to_string(), "viewer".to_string()), clock.now());

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::CooldownJudgeBrake)
        );
    }

    #[tokio::test]
    async fn judge_no_bleibt_still_ohne_notifier_und_ohne_cooldown() {
        let judge = FakeJudge::new(vec![
            model_verdict(LfgVerdictKind::No, 0.95),
            model_verdict(LfgVerdictKind::Yes, 0.9),
        ]);
        let (responder, api, notifier, _, _) =
            responder(true, Some("https://discord.gg/test"), false, judge);

        responder
            .maybe_respond(&event("viewer", "lfg"), "streamer")
            .await;
        assert!(api.messages().is_empty());
        assert!(notifier.calls().is_empty());

        responder
            .maybe_respond(&event("viewer", "lfg"), "streamer")
            .await;
        assert_eq!(api.messages().len(), 1);
        assert_eq!(notifier.calls(), vec!["streamer"]);
    }

    #[tokio::test]
    async fn decide_liefert_judge_unsure() {
        let (responder, api, notifier, _, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![model_verdict(LfgVerdictKind::Unsure, 0.6)]),
        );

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::JudgeUnsure)
        );
        assert!(api.messages().is_empty());
        assert!(notifier.calls().is_empty());
    }

    #[tokio::test]
    async fn decide_liefert_judge_provider_error() {
        let (responder, api, notifier, _, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![LfgVerdict::provider_error()]),
        );

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::JudgeProviderError)
        );
        assert!(api.messages().is_empty());
        assert!(notifier.calls().is_empty());
    }

    #[tokio::test]
    async fn decide_liefert_judge_parse_error() {
        let (responder, api, notifier, _, _) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![LfgVerdict::parse_error()]),
        );

        assert_eq!(
            decide_action(&responder, "viewer", "lfg").await,
            LfgPitchAction::Silent(SilentReason::JudgeParseError)
        );
        assert!(api.messages().is_empty());
        assert!(notifier.calls().is_empty());
    }

    #[test]
    fn judge_entscheidungen_sind_info_und_pre_judge_silents_debug() {
        let judge_no = LfgPitchDecision::judged(
            LfgPitchAction::Silent(SilentReason::JudgeNo),
            &model_verdict(LfgVerdictKind::No, 0.9),
            "streamer".to_string(),
            "viewer".to_string(),
            "lfg".to_string(),
        );
        let judge_unsure = LfgPitchDecision::judged(
            LfgPitchAction::Silent(SilentReason::JudgeUnsure),
            &model_verdict(LfgVerdictKind::Unsure, 0.5),
            "streamer".to_string(),
            "viewer".to_string(),
            "lfg".to_string(),
        );
        let send_go = LfgPitchDecision::judged(
            LfgPitchAction::SendGo,
            &model_verdict(LfgVerdictKind::Yes, 0.9),
            "streamer".to_string(),
            "viewer".to_string(),
            "lfg".to_string(),
        );
        let provider_error = LfgPitchDecision::judged(
            LfgPitchAction::Silent(SilentReason::JudgeProviderError),
            &LfgVerdict::provider_error(),
            "streamer".to_string(),
            "viewer".to_string(),
            "lfg".to_string(),
        );
        let parse_error = LfgPitchDecision::judged(
            LfgPitchAction::Silent(SilentReason::JudgeParseError),
            &LfgVerdict::parse_error(),
            "streamer".to_string(),
            "viewer".to_string(),
            "lfg".to_string(),
        );
        let pre_judge = LfgPitchDecision::silent(
            SilentReason::NoInviteUrl,
            "streamer".to_string(),
            "viewer".to_string(),
            "lfg".to_string(),
        );

        assert_eq!(judge_no.log_level(), tracing::Level::INFO);
        assert_eq!(judge_unsure.log_level(), tracing::Level::INFO);
        assert_eq!(send_go.log_level(), tracing::Level::INFO);
        assert_eq!(provider_error.log_level(), tracing::Level::WARN);
        assert_eq!(parse_error.log_level(), tracing::Level::WARN);
        assert_eq!(pre_judge.log_level(), tracing::Level::DEBUG);
    }

    #[tokio::test]
    async fn go_sendet_pitch_und_notifier_einmal_nach_send() {
        let (responder, api, notifier, _, order) = responder(
            true,
            Some("https://discord.gg/test"),
            false,
            FakeJudge::new(vec![model_verdict(LfgVerdictKind::Yes, 0.7)]),
        );

        responder
            .maybe_respond(&event("viewer", "lfg"), "streamer")
            .await;

        let messages = api.messages();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("@viewer"));
        assert!(messages[0].contains("https://discord.gg/test"));
        assert_eq!(notifier.calls(), vec!["streamer"]);
        assert_eq!(order.lock().unwrap().clone(), vec!["send", "note"]);
    }
}
