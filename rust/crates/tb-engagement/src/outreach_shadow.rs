use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::crew_review::{
    build_fireworks_http_client, FIREWORKS_DEFAULT_BASE_URL, FIREWORKS_DEFAULT_MODEL,
};

const FIREWORKS_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_HOOKS: usize = 5;
const MAX_HOOK_TEXT_CHARS: usize = 500;

// TODO(orchestrator): Den vollständigen deutschen Fireworks-Systemprompt einsetzen.
// Leer ist absichtlich fail-closed: Solange der Text fehlt, findet kein Modellaufruf statt.
pub const OUTREACH_SYSTEM_PROMPT: &str = "";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Smalltalk,
    Qualify,
    Offer,
}

impl HookKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smalltalk => "smalltalk",
            Self::Qualify => "qualify",
            Self::Offer => "offer",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Transcript,
    Chat,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Occasion {
    GoingOffline,
    LowViewers,
    LookingForPlayers,
    ChatTrouble,
}

impl Occasion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GoingOffline => "going_offline",
            Self::LowViewers => "low_viewers",
            Self::LookingForPlayers => "looking_for_players",
            Self::ChatTrouble => "chat_trouble",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutreachStage {
    Watch,
    Smalltalk,
    Qualify,
    Offer,
}

impl OutreachStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Smalltalk => "smalltalk",
            Self::Qualify => "qualify",
            Self::Offer => "offer",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutreachHook {
    pub kind: HookKind,
    pub occasion: Option<Occasion>,
    pub evidence: String,
    pub evidence_source: EvidenceSource,
    pub evidence_at: DateTime<Utc>,
    pub opener: String,
    pub why: String,
    pub confidence: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutreachDecision {
    pub hooks: Vec<OutreachHook>,
    pub stage: OutreachStage,
    pub silent_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SessionEvidence {
    pub transcripts: Vec<TimestampedText>,
    pub chat_messages: Vec<TimestampedText>,
    pub has_answered_qualify: bool,
    pub has_previous_offer: bool,
    pub session_started_at: DateTime<Utc>,
    pub now: DateTime<Utc>,
    pub streamer_login: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct OutreachModelInput {
    pub streamer_transcripts: Vec<TimestampedText>,
    pub chat_messages: Vec<TimestampedText>,
    pub previous_hooks: Vec<OutreachHook>,
    pub channel_state: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimestampedText {
    pub text: String,
    pub occurred_at: DateTime<Utc>,
    pub author: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OutreachError {
    #[error("unavailable")]
    Unavailable,
    #[error("timeout")]
    Timeout,
    #[error("http_status")]
    HttpStatus,
    #[error("decode")]
    Decode,
    #[error("validation")]
    Validation,
}

impl OutreachError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::HttpStatus => "http_status",
            Self::Decode => "decode",
            Self::Validation => "validation",
        }
    }
}

pub struct OutreachReviewClient {
    client: reqwest::Client,
    api_key: String,
}

impl OutreachReviewClient {
    pub fn from_env() -> Result<Self, OutreachError> {
        if OUTREACH_SYSTEM_PROMPT.trim().is_empty() {
            return Err(OutreachError::Unavailable);
        }
        let api_key = ["FIREWORKS_API_KEY", "FIREWORK_API_KEY"]
            .iter()
            .find_map(|name| nonempty_env(name))
            .ok_or(OutreachError::Unavailable)?;
        if nonempty_env("FIREWORKS_BASE_URL")
            .is_some_and(|url| url.trim_end_matches('/') != FIREWORKS_DEFAULT_BASE_URL)
        {
            return Err(OutreachError::Unavailable);
        }
        let client = build_fireworks_http_client(FIREWORKS_TIMEOUT)
            .map_err(|_| OutreachError::Unavailable)?;
        Ok(Self { client, api_key })
    }

    pub async fn decide(
        &self,
        input: &OutreachModelInput,
        evidence: &SessionEvidence,
    ) -> Result<OutreachDecision, OutreachError> {
        let user_data = serde_json::to_string(input).map_err(|_| OutreachError::Decode)?;
        let response = self
            .client
            .post(format!("{FIREWORKS_DEFAULT_BASE_URL}/chat/completions"))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": FIREWORKS_DEFAULT_MODEL,
                "messages": [
                    {"role": "system", "content": OUTREACH_SYSTEM_PROMPT},
                    {"role": "user", "content": user_data}
                ],
                "temperature": 0.0,
                "response_format": {"type": "json_object"}
            }))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    OutreachError::Timeout
                } else {
                    OutreachError::Unavailable
                }
            })?;
        if !response.status().is_success() {
            return Err(OutreachError::HttpStatus);
        }
        let completion = response.json::<ChatCompletion>().await.map_err(|error| {
            if error.is_timeout() {
                OutreachError::Timeout
            } else if error.is_decode() {
                OutreachError::Decode
            } else {
                OutreachError::Unavailable
            }
        })?;
        let raw = completion
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .filter(|content| !content.trim().is_empty())
            .ok_or(OutreachError::Decode)?;
        parse_outreach_decision(&raw, evidence)
    }
}

#[derive(Deserialize)]
struct ChatCompletion {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Deserialize)]
struct ChatCompletionMessage {
    content: Option<String>,
}

pub fn parse_outreach_decision(
    raw: &str,
    evidence: &SessionEvidence,
) -> Result<OutreachDecision, OutreachError> {
    let json = exact_json_payload(raw)?;
    let decision =
        serde_json::from_str::<OutreachDecision>(json).map_err(|_| OutreachError::Decode)?;
    validate_outreach_decision(decision, evidence)
}

pub fn validate_outreach_decision(
    mut decision: OutreachDecision,
    evidence: &SessionEvidence,
) -> Result<OutreachDecision, OutreachError> {
    decision.silent_reason = decision
        .silent_reason
        .map(|reason| reason.trim().to_owned())
        .filter(|reason| !reason.is_empty());
    if decision.hooks.is_empty() {
        if decision
            .silent_reason
            .as_ref()
            .is_none_or(|reason| reason.chars().count() > MAX_HOOK_TEXT_CHARS)
        {
            return Err(OutreachError::Validation);
        }
        return Ok(decision);
    }
    if decision.silent_reason.is_some() || decision.hooks.len() > MAX_HOOKS {
        return Err(OutreachError::Validation);
    }
    let mut has_offer = evidence.has_previous_offer;
    for hook in &mut decision.hooks {
        hook.evidence = hook.evidence.trim().to_owned();
        hook.opener = hook.opener.trim().to_owned();
        hook.why = hook.why.trim().to_owned();
        if hook.evidence.is_empty()
            || hook.opener.is_empty()
            || hook.why.is_empty()
            || hook.evidence.chars().count() > MAX_HOOK_TEXT_CHARS
            || hook.opener.chars().count() > MAX_HOOK_TEXT_CHARS
            || hook.why.chars().count() > MAX_HOOK_TEXT_CHARS
            || !hook.confidence.is_finite()
            || !(0.0..=1.0).contains(&hook.confidence)
            || forbidden_opener(&hook.opener)
        {
            return Err(OutreachError::Validation);
        }
        let evidence_match = evidence_match(evidence, hook.evidence_source, &hook.evidence)
            .ok_or(OutreachError::Validation)?;
        hook.evidence_at = evidence_match.occurred_at;
        if hook.kind == HookKind::Offer {
            let occasion = hook.occasion.ok_or(OutreachError::Validation)?;
            if has_offer
                || !evidence.has_answered_qualify
                || evidence.now - evidence.session_started_at < chrono::Duration::minutes(10)
                || !occasion_matches(occasion, &hook.evidence)
                || (hook.evidence_source == EvidenceSource::Chat
                    && !evidence_match.author.as_deref().is_some_and(|author| {
                        author.eq_ignore_ascii_case(&evidence.streamer_login)
                    }))
            {
                return Err(OutreachError::Validation);
            }
            has_offer = true;
        }
    }
    Ok(decision)
}

fn occasion_matches(occasion: Occasion, evidence: &str) -> bool {
    let evidence = evidence.to_lowercase();
    let markers: &[&str] = match occasion {
        Occasion::GoingOffline => &[
            "schluss",
            "letzte runde",
            "feierabend",
            "offline",
            "gleich weg",
        ],
        Occasion::LowViewers => &[
            "zuschauer",
            "viewer",
            "reichweite",
            "tote zeit",
            "niemand schaut",
        ],
        Occasion::LookingForPlayers => &[
            "brauch noch",
            "suche noch",
            "mitspieler",
            "solo queue",
            "solo-queue",
        ],
        Occasion::ChatTrouble => &[
            "spam",
            "scam",
            "follow-bot",
            "followbot",
            "moderation",
            "moderator",
        ],
    };
    markers.iter().any(|marker| evidence.contains(marker))
}

fn exact_json_payload(raw: &str) -> Result<&str, OutreachError> {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed
        .strip_prefix("```json\n")
        .and_then(|value| value.strip_suffix("\n```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```\n")
                .and_then(|value| value.strip_suffix("\n```"))
        })
    {
        return Ok(inner.trim());
    }
    if trimmed.starts_with("```") || trimmed.ends_with("```") {
        return Err(OutreachError::Decode);
    }
    Ok(trimmed)
}

fn evidence_match<'a>(
    evidence: &'a SessionEvidence,
    source: EvidenceSource,
    quote: &str,
) -> Option<&'a TimestampedText> {
    let values = match source {
        EvidenceSource::Transcript => &evidence.transcripts,
        EvidenceSource::Chat => &evidence.chat_messages,
    };
    values.iter().rev().find(|value| value.text.contains(quote))
}

fn forbidden_opener(opener: &str) -> bool {
    let lower = opener.to_lowercase();
    contains_link(&lower)
        || contains_member_count(&lower)
        || contains_superlative(&lower)
        || contains_forbidden_emoji(opener)
}

fn contains_link(lower: &str) -> bool {
    [
        "http://",
        "https://",
        "www.",
        "discord.gg",
        ".de/",
        ".com/",
        "twitch.tv",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn contains_member_count(lower: &str) -> bool {
    let words = lower.split_whitespace().collect::<Vec<_>>();
    words.windows(2).any(|pair| {
        let first = pair[0].trim_matches(|ch: char| !ch.is_alphanumeric());
        let second = pair[1].trim_matches(|ch: char| !ch.is_alphanumeric());
        let is_label = |word| {
            matches!(
                word,
                "mitglieder" | "mitgliedern" | "leute" | "member" | "personen"
            )
        };
        let is_count = |word: &str| {
            word.chars().any(|ch| ch.is_ascii_digit())
                || matches!(
                    word,
                    "ein"
                        | "eine"
                        | "einen"
                        | "zwei"
                        | "drei"
                        | "vier"
                        | "fünf"
                        | "sechs"
                        | "sieben"
                        | "acht"
                        | "neun"
                        | "zehn"
                )
                || word.ends_with("hundert")
                || word.ends_with("tausend")
                || word.ends_with("million")
                || word.ends_with("millionen")
        };
        (is_count(first) && is_label(second)) || (is_label(first) && is_count(second))
    })
}

fn contains_superlative(lower: &str) -> bool {
    [
        "größte",
        "grösste",
        "aktivste",
        "beste",
        "stärkste",
        "bekannteste",
        "erfolgreichste",
        "nummer 1",
        "nr. 1",
        "#1",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn contains_forbidden_emoji(opener: &str) -> bool {
    let without_smiley = opener.replace(":)", "");
    let ascii_lower = without_smiley.to_ascii_lowercase();
    if [
        ":-)", ":d", ":-d", ":p", ":-p", ":(", ":-(", ";)", ";-)", "<3", "^^", "xd", ":o",
    ]
    .iter()
    .any(|needle| ascii_lower.contains(needle))
    {
        return true;
    }
    without_smiley.chars().any(|ch| {
        !ch.is_ascii()
            && !ch.is_alphanumeric()
            && !ch.is_whitespace()
            && !matches!(ch, '„' | '“' | '‚' | '‘' | '…' | '–' | '—')
    })
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutreachSession {
    pub id: Uuid,
    pub channel_login: String,
    pub streamer_user_id: String,
    pub started_at: DateTime<Utc>,
    pub stage: OutreachStage,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CycleResult {
    Decision(OutreachDecision),
    ParserError,
    Timeout,
    ProviderError(String),
    WhisperError(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutreachOutcome {
    Hook,
    Silent,
    ParserError,
    Timeout,
    ProviderError,
    WhisperError,
}

impl OutreachOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hook => "hook",
            Self::Silent => "silent",
            Self::ParserError => "parser_error",
            Self::Timeout => "timeout",
            Self::ProviderError => "provider_error",
            Self::WhisperError => "whisper_error",
        }
    }
}

#[derive(Clone, Debug)]
pub struct NewOutreachEvent {
    pub session_id: Uuid,
    pub cycle_id: Uuid,
    pub channel_login: String,
    pub occurred_at: DateTime<Utc>,
    pub outcome: OutreachOutcome,
    pub stage: OutreachStage,
    pub transcript: Option<String>,
    pub decision: Option<OutreachDecision>,
    pub static_recruitment_text: Option<String>,
    pub error_class: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl NewOutreachEvent {
    pub fn from_cycle_result(
        session: &OutreachSession,
        cycle_id: Uuid,
        occurred_at: DateTime<Utc>,
        transcript: Option<String>,
        result: CycleResult,
    ) -> Self {
        let (outcome, decision, error_class, provider, model) = match result {
            CycleResult::Decision(decision) => {
                let outcome = if decision.hooks.is_empty() {
                    OutreachOutcome::Silent
                } else {
                    OutreachOutcome::Hook
                };
                (
                    outcome,
                    Some(decision),
                    None,
                    Some("fireworks".to_owned()),
                    Some(FIREWORKS_DEFAULT_MODEL.to_owned()),
                )
            }
            CycleResult::ParserError => (
                OutreachOutcome::ParserError,
                None,
                Some("decode".to_owned()),
                Some("fireworks".to_owned()),
                Some(FIREWORKS_DEFAULT_MODEL.to_owned()),
            ),
            CycleResult::Timeout => (
                OutreachOutcome::Timeout,
                None,
                Some("timeout".to_owned()),
                Some("fireworks".to_owned()),
                Some(FIREWORKS_DEFAULT_MODEL.to_owned()),
            ),
            CycleResult::ProviderError(error) => (
                OutreachOutcome::ProviderError,
                None,
                Some(error),
                Some("fireworks".to_owned()),
                Some(FIREWORKS_DEFAULT_MODEL.to_owned()),
            ),
            CycleResult::WhisperError(error) => (
                OutreachOutcome::WhisperError,
                None,
                Some(error),
                Some("openai_transcribe".to_owned()),
                Some("whisper-1".to_owned()),
            ),
        };
        let stage = decision
            .as_ref()
            .map_or(session.stage, |decision| decision.stage);
        Self {
            session_id: session.id,
            cycle_id,
            channel_login: session.channel_login.clone(),
            occurred_at,
            outcome,
            stage,
            transcript,
            decision,
            static_recruitment_text: None,
            error_class,
            provider,
            model,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::{json, Value};
    use uuid::Uuid;

    use super::*;

    fn evidence(answered_qualify: bool) -> SessionEvidence {
        SessionEvidence {
            transcripts: vec![
                TimestampedText {
                    text: "ich stream eigentlich jeden tag deadlock".to_owned(),
                    occurred_at: Utc.with_ymd_and_hms(2026, 7, 27, 20, 13, 0).unwrap(),
                    author: None,
                },
                TimestampedText {
                    text: "brauch noch wen für die runde".to_owned(),
                    occurred_at: Utc.with_ymd_and_hms(2026, 7, 27, 20, 13, 10).unwrap(),
                    author: None,
                },
            ],
            chat_messages: vec![TimestampedText {
                text: "mitspieler wären schon nice".to_owned(),
                occurred_at: Utc.with_ymd_and_hms(2026, 7, 27, 20, 13, 30).unwrap(),
                author: Some("viewer".to_owned()),
            }],
            has_answered_qualify: answered_qualify,
            has_previous_offer: false,
            session_started_at: Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap(),
            now: Utc.with_ymd_and_hms(2026, 7, 27, 20, 14, 0).unwrap(),
            streamer_login: "kandidat".to_owned(),
        }
    }

    fn hook(kind: &str, evidence: &str, opener: &str) -> Value {
        json!({
            "kind": kind,
            "occasion": if kind == "offer" {
                Value::String("looking_for_players".to_owned())
            } else {
                Value::Null
            },
            "evidence": evidence,
            "evidence_source": "transcript",
            "evidence_at": "2026-07-27T20:14:03Z",
            "opener": opener,
            "why": "interner Testgrund",
            "confidence": 0.8
        })
    }

    fn decision(hooks: Vec<Value>, silent_reason: Option<&str>) -> String {
        json!({
            "hooks": hooks,
            "stage": "smalltalk",
            "silent_reason": silent_reason
        })
        .to_string()
    }

    #[test]
    fn erfundener_beleg_verwirft_den_zyklus() {
        let raw = decision(
            vec![hook(
                "smalltalk",
                "das wurde nie gesagt",
                "wie läuft deadlock heute",
            )],
            None,
        );

        assert_eq!(
            parse_outreach_decision(&raw, &evidence(false)),
            Err(OutreachError::Validation)
        );
    }

    #[test]
    fn belegzeit_kommt_aus_dem_gespeicherten_treffer() {
        let raw = decision(
            vec![hook(
                "smalltalk",
                "ich stream eigentlich jeden tag deadlock",
                "wie läuft deadlock heute",
            )],
            None,
        );

        let parsed = parse_outreach_decision(&raw, &evidence(false)).expect("gültiger Beleg");

        assert_eq!(
            parsed.hooks[0].evidence_at,
            Utc.with_ymd_and_hms(2026, 7, 27, 20, 13, 0).unwrap()
        );
    }

    #[test]
    fn offer_ohne_beantwortetes_qualify_wird_verworfen() {
        let raw = decision(
            vec![hook(
                "offer",
                "brauch noch wen für die runde",
                "wenn du öfter deadlock streamst gibts da ne community",
            )],
            None,
        );

        assert_eq!(
            parse_outreach_decision(&raw, &evidence(false)),
            Err(OutreachError::Validation)
        );
    }

    #[test]
    fn offer_nach_beantwortetem_qualify_ist_zulaessig() {
        let raw = decision(
            vec![hook(
                "offer",
                "brauch noch wen für die runde",
                "wenn du öfter deadlock streamst gibts da ne community",
            )],
            None,
        );

        let parsed = parse_outreach_decision(&raw, &evidence(true)).expect("gültige Entscheidung");

        assert_eq!(parsed.hooks.len(), 1);
        assert_eq!(parsed.hooks[0].kind, HookKind::Offer);
    }

    #[test]
    fn offer_braucht_anlass_laufzeit_und_darf_nur_einmal_vorkommen() {
        let mut raw: Value = serde_json::from_str(&decision(
            vec![hook(
                "offer",
                "brauch noch wen für die runde",
                "wenn du öfter deadlock streamst gibts da ne community",
            )],
            None,
        ))
        .unwrap();
        raw["hooks"][0]["occasion"] = Value::Null;
        assert_eq!(
            parse_outreach_decision(&raw.to_string(), &evidence(true)),
            Err(OutreachError::Validation)
        );

        let mut too_early = evidence(true);
        too_early.now = too_early.session_started_at + chrono::Duration::minutes(9);
        assert_eq!(
            parse_outreach_decision(
                &decision(
                    vec![hook(
                        "offer",
                        "brauch noch wen für die runde",
                        "wenn du öfter deadlock streamst gibts da ne community",
                    )],
                    None,
                ),
                &too_early
            ),
            Err(OutreachError::Validation)
        );

        let mut already_offered = evidence(true);
        already_offered.has_previous_offer = true;
        assert_eq!(
            parse_outreach_decision(
                &decision(
                    vec![hook(
                        "offer",
                        "brauch noch wen für die runde",
                        "wenn du öfter deadlock streamst gibts da ne community",
                    )],
                    None,
                ),
                &already_offered
            ),
            Err(OutreachError::Validation)
        );
    }

    #[test]
    fn offer_aus_chat_braucht_eine_streamer_nachricht() {
        let mut raw: Value = serde_json::from_str(&decision(
            vec![hook(
                "offer",
                "mitspieler wären schon nice",
                "wenn du öfter deadlock streamst gibts da ne community",
            )],
            None,
        ))
        .unwrap();
        raw["hooks"][0]["evidence_source"] = Value::String("chat".to_owned());

        assert_eq!(
            parse_outreach_decision(&raw.to_string(), &evidence(true)),
            Err(OutreachError::Validation)
        );

        let mut streamer_evidence = evidence(true);
        streamer_evidence.chat_messages[0].author = Some("KANDIDAT".to_owned());
        assert!(parse_outreach_decision(&raw.to_string(), &streamer_evidence).is_ok());
    }

    #[test]
    fn opener_mit_emoji_mitgliederzahl_superlativ_oder_link_wird_verworfen() {
        for opener in [
            "moin 💜 wie laufen die runden",
            "bei uns sind 2.400 leute",
            "mitglieder: 2400",
            "bei uns sind zweitausend mitglieder",
            "moin :P wie laufen die runden",
            "das ist die größte community",
            "schau mal auf https://example.com",
        ] {
            let raw = decision(
                vec![hook(
                    "smalltalk",
                    "ich stream eigentlich jeden tag deadlock",
                    opener,
                )],
                None,
            );

            assert_eq!(
                parse_outreach_decision(&raw, &evidence(false)),
                Err(OutreachError::Validation),
                "unerlaubter opener wurde akzeptiert: {opener}"
            );
        }
    }

    #[test]
    fn smiley_ist_das_einzige_erlaubte_emoji() {
        let raw = decision(
            vec![hook(
                "smalltalk",
                "ich stream eigentlich jeden tag deadlock",
                "wie laufen die runden :)",
            )],
            None,
        );

        assert!(parse_outreach_decision(&raw, &evidence(false)).is_ok());
    }

    #[test]
    fn leere_hooks_brauchen_einen_silent_reason() {
        assert_eq!(
            parse_outreach_decision(&decision(vec![], None), &evidence(false)),
            Err(OutreachError::Validation)
        );
        assert!(parse_outreach_decision(
            &decision(vec![], Some("kein belegter anlass")),
            &evidence(false)
        )
        .is_ok());
    }

    #[test]
    fn modelltexte_und_hookanzahl_sind_begrenzt() {
        let too_long = "x".repeat(MAX_HOOK_TEXT_CHARS + 1);
        assert_eq!(
            parse_outreach_decision(&decision(vec![], Some(&too_long)), &evidence(false)),
            Err(OutreachError::Validation)
        );
        let hooks = (0..=MAX_HOOKS)
            .map(|_| {
                hook(
                    "smalltalk",
                    "ich stream eigentlich jeden tag deadlock",
                    "wie läuft deadlock heute",
                )
            })
            .collect();
        assert_eq!(
            parse_outreach_decision(&decision(hooks, None), &evidence(false)),
            Err(OutreachError::Validation)
        );
    }

    #[test]
    fn jeder_zyklusausgang_ist_genau_ein_persistierbares_ereignis() {
        let session = OutreachSession {
            id: Uuid::nil(),
            channel_login: "kandidat".to_owned(),
            streamer_user_id: "42".to_owned(),
            started_at: Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap(),
            stage: OutreachStage::Watch,
        };
        let decision = OutreachDecision {
            hooks: vec![],
            stage: OutreachStage::Watch,
            silent_reason: Some("kein belegter anlass".to_owned()),
        };
        let hook_decision = OutreachDecision {
            hooks: vec![OutreachHook {
                kind: HookKind::Smalltalk,
                occasion: None,
                evidence: "transkript".to_owned(),
                evidence_source: EvidenceSource::Transcript,
                evidence_at: Utc.with_ymd_and_hms(2026, 7, 27, 20, 1, 0).unwrap(),
                opener: "wie laufen die runden".to_owned(),
                why: "interner Testgrund".to_owned(),
                confidence: 0.8,
            }],
            stage: OutreachStage::Smalltalk,
            silent_reason: None,
        };

        for result in [
            CycleResult::Decision(hook_decision),
            CycleResult::Decision(decision.clone()),
            CycleResult::ParserError,
            CycleResult::Timeout,
            CycleResult::ProviderError("http_status".to_owned()),
        ] {
            let event = NewOutreachEvent::from_cycle_result(
                &session,
                Uuid::new_v4(),
                Utc.with_ymd_and_hms(2026, 7, 27, 20, 1, 0).unwrap(),
                Some("transkript".to_owned()),
                result,
            );
            assert!(!event.outcome.as_str().is_empty());
        }
    }
}
