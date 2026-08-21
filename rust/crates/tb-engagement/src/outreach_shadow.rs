use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use uuid::Uuid;

use crate::crew_review::{
    FIREWORKS_DEFAULT_BASE_URL, FIREWORKS_DEFAULT_MODEL,
};

const FIREWORKS_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_HOOKS: usize = 5;
const MAX_HOOK_TEXT_CHARS: usize = 500;

/// Systemprompt für das Fireworks-Modell.
///
/// Abgeleitet aus `docs/superpowers/specs/2026-07-27-selbstvermarktung-stilvertrag.md`,
/// der wiederum auf 4974 echten Chatnachrichten des Betreibers beruht. Der Text
/// enthält bewusst keinen Gedankenstrich: das Modell imitiert die Zeichensetzung
/// des Prompts, und im erfassten Twitch-Chat kommt der Gedankenstrich in 0,03
/// Prozent aller Nachrichten vor, beim Betreiber in keiner einzigen.
pub const OUTREACH_SYSTEM_PROMPT: &str = r#"Du beobachtest den Stream eines deutschen Deadlock-Streamers, der noch nicht Teil unseres Streamer-Netzwerks ist. Du suchst Anknüpfungspunkte: Momente, an denen sich ein Gespräch natürlich anfangen ließe.

Du sendest nichts. Du schlägst nur vor.

So läuft ein Gespräch bei uns ab, in dieser Reihenfolge:

1. Ankommen und Interesse zeigen. Etwas zum Spiel sagen, das gerade passiert. Fragen, wie es läuft. Nichts wollen.
2. Qualifizieren. Herausfinden, ob die Person regelmäßig Deadlock streamt. Zum Beispiel: "Streamst du öfters DL?"
3. Anbieten, aber nur an eine Bedingung geknüpft und über die Community in dritter Person. Zum Beispiel: "Aber wenn du generell mehr DL zockst, auf Discord gibts ne Deutsche Deadlock Community. Die bieten auch so ne Streamer Partnerschaft, hat einige sehr geile vorteile."

Ein Angebot machst du nur, wenn es gerade zu etwas passt, das die Person selbst angesprochen hat. Vier Momente passen:

going_offline: Sie will Schluss machen oder spricht von der letzten Runde. Das ist der beste Moment. Dann geht es um den Raid: wenn sie offline geht, schickt der Bot ihre Zuschauer zu einem anderen deutschen Deadlock-Streamer, statt dass alle einfach weg sind.
low_viewers: Sie spricht wenig Zuschauer oder Reichweite an. Dann geht es darum, dass umgekehrt auch Raids reinkommen, wenn andere offline gehen.
looking_for_players: Sie sucht Mitspieler oder ärgert sich über Solo-Queue. Dann geht es um die Community, wo Leute zum Zocken sind.
chat_trouble: Sie hat Ärger mit Spam oder Scam-Bots im Chat. Dann geht es um den Chat-Schutz.

Passt keiner dieser Momente, machst du kein Angebot. Dann bleibt es bei einer Bemerkung zum Spiel oder einer Frage.

Wurde dieser Kanal schon mal von uns geraidet, darfst du das erwähnen, aber nur als etwas, das schon passiert ist, nie als Gegenforderung.

So schreibst du:

Deutsch, kurz, locker. Kleinschreibung ist normal, Tippfehler sind normal. Lachen schreibst du aus: haha, hahah. Emojis benutzt du nicht, höchstens :) Anreden wie Meister, Bro, Moin passen. Keine Ausrufezeichen-Werbung, keine Superlative, keine Mitgliederzahlen. Du sagst nie, dass wir die größte oder aktivste Community sind. Du machst niemandem ein schlechtes Gewissen und fragst nicht, warum jemand noch nicht dabei ist. Du benutzt keine Gedankenstriche. Du schickst keinen Link, denn im echten Ablauf wird vorher gefragt, ob einer geschickt werden darf, und diese Zustimmung gibt es hier nicht.

Wenn es um den Bot geht, erklärst du nur die Mechanik und bleibst ehrlich: Wenn ein Partner offline geht, raidet der Bot einen anderen deutschen Deadlock-Streamer, damit man sich gegenseitig unterstützt. Wer streamt, bekommt Raids aus dem Netzwerk. Bei anderen Spielen ist der Bot nur Chat-Schutz gegen Spam und Scam. Was der Bot nicht kann, sagst du dazu. Fragt jemand, ob du ein Bot bist, sagst du ja.

Jeder Vorschlag braucht einen Beleg: ein wörtliches Zitat aus dem Transkript oder dem Chat dieser Sitzung. Erfinde niemals ein Zitat. Findest du keinen echten Anlass, gibst du keine Vorschläge zurück und schreibst kurz in silent_reason, warum.

Antworte ausschließlich mit diesem JSON:

{"hooks": [{"kind": "smalltalk|qualify|offer", "occasion": null oder einer der vier Anlässe, "evidence": "wörtliches Zitat", "evidence_source": "transcript|chat", "evidence_at": "ISO-Zeitstempel", "opener": "der Satz, den du sagen würdest", "why": "kurze Begründung", "confidence": 0.0}], "stage": "watch|smalltalk|qualify|offer", "silent_reason": null oder kurzer Text}"#;

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

/// Anwendungsfall in der gemeinsamen Anbieterauswahl.
pub const USE_CASE: &str = "outreach_shadow";

/// Schatten-Review der Ansprache. Wie das Crew-Review fail-closed auf Adresse
/// und Modell festgenagelt; der HTTP-Weg liegt in [`tb_llm::complete`].
pub struct OutreachReviewClient {
    endpoint: tb_llm::LlmEndpoint,
}

impl OutreachReviewClient {
    pub fn from_env() -> Result<Self, OutreachError> {
        if OUTREACH_SYSTEM_PROMPT.trim().is_empty() {
            return Err(OutreachError::Unavailable);
        }
        // Gleiches Verhalten wie `crew_review::FireworksReviewClient::from_env`:
        // Adresse und Modell kommen aus der zentralen Auswahl, und weichen
        // sie vom Standard ab (etwa per `TB_LLM_MODEL_OUTREACH_SHADOW`),
        // startet das Review nicht. Ein fehlendes oder fremdes Modell wird
        // nicht still durch den Standard ersetzt, sondern ist `Unavailable`.
        let endpoint = tb_llm::endpoint_for(USE_CASE);
        if endpoint.provider != "fireworks"
            || endpoint.base_url.trim_end_matches('/') != FIREWORKS_DEFAULT_BASE_URL
            || endpoint.model != FIREWORKS_DEFAULT_MODEL
            || endpoint.api_key.as_deref().is_none_or(|key| key.trim().is_empty())
        {
            return Err(OutreachError::Unavailable);
        }
        Ok(Self {
            endpoint: tb_llm::LlmEndpoint {
                base_url: FIREWORKS_DEFAULT_BASE_URL.to_string(),
                ..endpoint
            },
        })
    }

    pub async fn decide(
        &self,
        input: &OutreachModelInput,
        evidence: &SessionEvidence,
    ) -> Result<OutreachDecision, OutreachError> {
        let user_data = serde_json::to_string(input).map_err(|_| OutreachError::Decode)?;
        let response = tb_llm::complete(
            USE_CASE,
            tb_llm::Request::simple(OUTREACH_SYSTEM_PROMPT, user_data)
                .temperature(0.0)
                .json_object()
                .timeout(FIREWORKS_TIMEOUT)
                .no_ledger()
                .endpoint(self.endpoint.clone()),
        )
        .await
        .map_err(outreach_error)?;
        if response.text.trim().is_empty() {
            return Err(OutreachError::Decode);
        }
        parse_outreach_decision(&response.text, evidence)
    }
}

/// Uebersetzt den Fehler des gemeinsamen Eingangs in die Fehlerklassen, die der
/// Ansprache-Pfad meldet und zaehlt.
fn outreach_error(error: tb_llm::LlmError) -> OutreachError {
    match error {
        tb_llm::LlmError::Timeout(_) => OutreachError::Timeout,
        tb_llm::LlmError::Http { .. } => OutreachError::HttpStatus,
        tb_llm::LlmError::Unparsable(_) => OutreachError::Decode,
        tb_llm::LlmError::Unavailable(_) | tb_llm::LlmError::Transport(_) => {
            OutreachError::Unavailable
        }
    }
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
            // Der belegte Anlass ersetzt die Qualifizierung: wer sagt, dass er
            // gleich offline geht, streamt offensichtlich. Ein vorheriges
            // `qualify` ist deshalb keine Pflicht mehr, die Mindestlaufzeit
            // verhindert weiterhin das Reinplatzen.
            if has_offer
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
    use serde_json::Value;
    use uuid::Uuid;

    use super::*;

    fn evidence() -> SessionEvidence {
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
            parse_outreach_decision(&raw, &evidence()),
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

        let parsed = parse_outreach_decision(&raw, &evidence()).expect("gültiger Beleg");

        assert_eq!(
            parsed.hooks[0].evidence_at,
            Utc.with_ymd_and_hms(2026, 7, 27, 20, 13, 0).unwrap()
        );
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
            parse_outreach_decision(&raw.to_string(), &evidence()),
            Err(OutreachError::Validation)
        );

        let mut too_early = evidence();
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

        let mut already_offered = evidence();
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
            parse_outreach_decision(&raw.to_string(), &evidence()),
            Err(OutreachError::Validation)
        );

        let mut streamer_evidence = evidence();
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
                parse_outreach_decision(&raw, &evidence()),
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

        assert!(parse_outreach_decision(&raw, &evidence()).is_ok());
    }

    #[test]
    fn leere_hooks_brauchen_einen_silent_reason() {
        assert_eq!(
            parse_outreach_decision(&decision(vec![], None), &evidence()),
            Err(OutreachError::Validation)
        );
        assert!(parse_outreach_decision(
            &decision(vec![], Some("kein belegter anlass")),
            &evidence()
        )
        .is_ok());
    }

    #[test]
    fn modelltexte_und_hookanzahl_sind_begrenzt() {
        let too_long = "x".repeat(MAX_HOOK_TEXT_CHARS + 1);
        assert_eq!(
            parse_outreach_decision(&decision(vec![], Some(&too_long)), &evidence()),
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
            parse_outreach_decision(&decision(hooks, None), &evidence()),
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
