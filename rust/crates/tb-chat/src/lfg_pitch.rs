//! LFG-Mitspieler-Pitch: billiger Regex-Vorfilter vor dem KI-Judge.

use std::sync::OnceLock;

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tb_engagement::minimax_chat::EngagementMinimaxClient;
use tracing::debug;

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
