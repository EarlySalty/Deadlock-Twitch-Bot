//! LLM-Layer für die Clip-Enrichment (Port von `bot/social_media/llm/` —
//! base/prompts/_parsing). Diese Slice (E2a) deckt die reinen, testbaren Teile
//! ab: Typen, Prompt-Bau und das Parsen des LLM-JSON-Outputs. Provider
//! (Ollama/Claude/MiniMax) + Dispatcher folgen.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::enrichment::PlatformEnrichment;

/// Plattformen (Python `SOCIAL_PLATFORMS`).
pub const SOCIAL_PLATFORMS: [&str; 3] = ["youtube", "tiktok", "instagram"];

/// LLM-Fehler.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("LLM provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("LLM provider error: {0}")]
    ProviderError(String),
}

/// Streamer-Kontext für den Prompt.
#[derive(Debug, Clone, Default)]
pub struct StreamerProfile {
    pub streamer_login: String,
    pub display_name: Option<String>,
    pub language: Option<String>,
    pub persona_hint: Option<String>,
}

/// Eingabe für die LLM-Anreicherung.
#[derive(Debug, Clone, Default)]
pub struct LlmRequest {
    pub transcript: String,
    pub detected_terms: Vec<String>,
    pub streamer: Option<StreamerProfile>,
    pub clip_title: Option<String>,
    pub game_name: Option<String>,
    pub duration_seconds: Option<f64>,
}

/// Plattform-spezifische LLM-Antwort.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub youtube: PlatformEnrichment,
    pub tiktok: PlatformEnrichment,
    pub instagram: PlatformEnrichment,
    pub provider: String,
    pub model: String,
    pub cost_usd_estimate: Option<f64>,
}

/// Freitext-Antwort (für generate_text-Provider).
#[derive(Debug, Clone)]
pub struct LlmTextResponse {
    pub content: String,
    pub provider: String,
    pub model: String,
}

// ---------- Prompts ----------

pub const SYSTEM_PROMPT: &str = "You are a social-media copywriter for short Deadlock gameplay clips.\n\
Deadlock is a hero-shooter MOBA by Valve. Your job: turn a clip transcript\n\
plus detected Deadlock vocabulary into ready-to-publish posts for\n\
YouTube Shorts, TikTok and Instagram Reels.\n\n\
Hard rules:\n\
- Output STRICT JSON only. No prose, no markdown, no code fences.\n\
- Each platform must have: title (string), description (string), hashtags (array of strings).\n\
- Never invent facts not present in the transcript or the detected terms.\n\
- Use 'Deadlock' as the game tag. Always include #Deadlock as one hashtag per platform.\n\
- Hashtags: 5-10 each, lowercase preferred where it makes sense, no duplicates,\n\
  no spaces inside a hashtag, never start with a number.\n\
- Title char limits: youtube <= 100, instagram <= 125, tiktok <= 150.\n\
- Description: 1-3 short sentences. Crisp, on-brand.\n\
- Language: write in the streamer's primary language if given, otherwise English.\n\
- Be concrete: name the hero/item/ability that appears in detected_terms when relevant.\n";

const JSON_SCHEMA_HINT: &str = "Required JSON schema:\n{\n  \"youtube\":   {\"title\": \"...\", \"description\": \"...\", \"hashtags\": [\"...\"]},\n  \"tiktok\":    {\"title\": \"...\", \"description\": \"...\", \"hashtags\": [\"...\"]},\n  \"instagram\": {\"title\": \"...\", \"description\": \"...\", \"hashtags\": [\"...\"]}\n}\n";

/// Baut den User-Prompt aus dem Request (Python `render_user_prompt`).
pub fn render_user_prompt(request: &LlmRequest) -> String {
    let streamer_block = match &request.streamer {
        Some(s) => {
            let mut bits = vec![format!("login={}", s.streamer_login)];
            if let Some(d) = s.display_name.as_deref().filter(|x| !x.is_empty()) {
                bits.push(format!("display_name={d}"));
            }
            if let Some(l) = s.language.as_deref().filter(|x| !x.is_empty()) {
                bits.push(format!("language={l}"));
            }
            if let Some(p) = s.persona_hint.as_deref().filter(|x| !x.is_empty()) {
                bits.push(format!("persona={p}"));
            }
            format!("Streamer: {}", bits.join(", "))
        }
        None => "Streamer: unknown".to_string(),
    };
    let detected = if request.detected_terms.is_empty() {
        "(none)".to_string()
    } else {
        request.detected_terms.join(", ")
    };
    let title_hint = request.clip_title.as_deref().filter(|s| !s.is_empty()).unwrap_or("(none)");
    let game = request.game_name.as_deref().filter(|s| !s.is_empty()).unwrap_or("Deadlock");
    let duration = match request.duration_seconds {
        Some(d) => format!("{:.0}s", d),
        None => "unknown".to_string(),
    };
    let transcript = {
        let t = request.transcript.trim();
        if t.is_empty() { "(empty transcript - rely on detected terms)" } else { t }
    };
    format!(
        "{streamer_block}\nGame: {game}\nClip duration: {duration}\n\
         Original Twitch clip title: {title_hint}\n\
         Detected Deadlock vocabulary: {detected}\n\n\
         Transcript (corrected):\n\"\"\"\n{transcript}\n\"\"\"\n\n{JSON_SCHEMA_HINT}"
    )
}

// ---------- Parsing ----------

const fn title_limit(platform: &str) -> usize {
    match platform.as_bytes() {
        b"youtube" => 100,
        b"instagram" => 125,
        _ => 150, // tiktok
    }
}

fn hashtag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_]{0,49}$").unwrap())
}

fn non_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[^A-Za-z0-9_]").unwrap())
}

fn coerce_str(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string().trim().to_string(),
    }
}

/// `#Token` oder `None` (Python `_normalize_hashtag`).
fn normalize_hashtag(raw: &str) -> Option<String> {
    let token = raw.trim().trim_start_matches('#').trim().replace(' ', "");
    let token = non_word_re().replace_all(&token, "").to_string();
    if token.is_empty() || !hashtag_re().is_match(&token) {
        return None;
    }
    Some(format!("#{token}"))
}

/// Normalisiert + dedupliziert Hashtags, stellt `#Deadlock` voran.
fn coerce_hashtags(raw: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let push = |s: &str, out: &mut Vec<String>, seen: &mut std::collections::HashSet<String>| {
        if let Some(tag) = normalize_hashtag(s) {
            if seen.insert(tag.to_lowercase()) {
                out.push(tag);
            }
        }
    };
    match raw {
        Some(Value::Array(a)) => {
            for entry in a {
                if let Some(s) = entry.as_str() {
                    push(s, &mut out, &mut seen);
                } else if !entry.is_null() {
                    push(&entry.to_string(), &mut out, &mut seen);
                }
            }
        }
        Some(Value::String(s)) => {
            for entry in s.split(|c: char| c == ',' || c.is_whitespace()) {
                push(entry, &mut out, &mut seen);
            }
        }
        _ => {}
    }
    if !seen.contains("#deadlock") {
        out.insert(0, "#Deadlock".to_string());
    }
    out
}

/// Kürzt auf `limit` Zeichen + „…" (char-basiert).
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

fn extract_platform(payload: &Value, platform: &str) -> Result<PlatformEnrichment, LlmError> {
    let block = payload.get(platform).filter(|v| v.is_object());
    let Some(block) = block else {
        return Err(LlmError::ProviderError(format!("missing platform block: {platform}")));
    };
    let title = coerce_str(block.get("title"));
    if title.is_empty() {
        return Err(LlmError::ProviderError(format!("empty title for platform: {platform}")));
    }
    Ok(PlatformEnrichment {
        title: Some(truncate(&title, title_limit(platform))),
        description: Some(coerce_str(block.get("description"))),
        hashtags: coerce_hashtags(block.get("hashtags")),
    })
}

fn strip_code_fence(text: &str) -> String {
    let stripped = text.trim();
    if let Some(body) = stripped.strip_prefix("```") {
        let body = body.strip_prefix("json").or_else(|| body.strip_prefix("JSON")).unwrap_or(body);
        let body = body.strip_suffix("```").unwrap_or(body);
        return body.trim().to_string();
    }
    stripped.to_string()
}

/// Extrahiert das erste balancierte JSON-Objekt (string-/escape-aware).
fn find_json_object(text: &str) -> Result<String, LlmError> {
    let cleaned = strip_code_fence(text);
    let chars: Vec<char> = cleaned.chars().collect();
    let Some(start) = chars.iter().position(|&c| c == '{') else {
        return Err(LlmError::ProviderError("LLM output contained no JSON object".to_string()));
    };
    let (mut depth, mut in_string, mut escape) = (0i32, false, false);
    for (idx, &ch) in chars.iter().enumerate().skip(start) {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' => escape = true,
            '"' => in_string = !in_string,
            _ if in_string => {}
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(chars[start..=idx].iter().collect());
                }
            }
            _ => {}
        }
    }
    Err(LlmError::ProviderError("LLM output had no balanced JSON object".to_string()))
}

/// Parst die Roh-LLM-Ausgabe in eine [`LlmResponse`] (Python `parse_llm_payload`).
pub fn parse_llm_payload(raw_text: &str, provider: &str, model: &str, cost: Option<f64>) -> Result<LlmResponse, LlmError> {
    let json_text = find_json_object(raw_text)?;
    let payload: Value = serde_json::from_str(&json_text)
        .map_err(|e| LlmError::ProviderError(format!("invalid JSON from LLM: {e}")))?;
    if !payload.is_object() {
        return Err(LlmError::ProviderError("LLM JSON must be an object".to_string()));
    }
    Ok(LlmResponse {
        youtube: extract_platform(&payload, "youtube")?,
        tiktok: extract_platform(&payload, "tiktok")?,
        instagram: extract_platform(&payload, "instagram")?,
        provider: provider.to_string(),
        model: model.to_string(),
        cost_usd_estimate: cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashtag_normalisierung() {
        assert_eq!(normalize_hashtag("#Deadlock").as_deref(), Some("#Deadlock"));
        assert_eq!(normalize_hashtag("  gaming  ").as_deref(), Some("#gaming"));
        assert_eq!(normalize_hashtag("soul orb").as_deref(), Some("#soulorb")); // Leerzeichen raus
        assert_eq!(normalize_hashtag("123tag"), None); // startet mit Ziffer
        assert_eq!(normalize_hashtag("#"), None);
        assert_eq!(normalize_hashtag("!!!"), None);
    }

    #[test]
    fn hashtags_dedup_und_deadlock_default() {
        let tags = coerce_hashtags(Some(&serde_json::json!(["gaming", "Gaming", "haze"])));
        // #Deadlock vorangestellt, "Gaming" dedup gegen "gaming".
        assert_eq!(tags, vec!["#Deadlock", "#gaming", "#haze"]);
        // String-Variante (komma/space-getrennt).
        let tags2 = coerce_hashtags(Some(&serde_json::json!("#deadlock, haze frost")));
        assert!(tags2.contains(&"#haze".to_string()) && tags2.contains(&"#frost".to_string()));
        // #deadlock schon da → nicht doppelt.
        assert_eq!(tags2.iter().filter(|t| t.to_lowercase() == "#deadlock").count(), 1);
    }

    #[test]
    fn truncate_char_basiert() {
        assert_eq!(truncate("kurz", 100), "kurz");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    #[test]
    fn find_json_object_und_fence() {
        let raw = "```json\n{\"a\": {\"b\": \"}\"}}\n```";
        // Balanced trotz } im String.
        assert_eq!(find_json_object(raw).unwrap(), "{\"a\": {\"b\": \"}\"}}");
        assert!(find_json_object("kein json").is_err());
    }

    #[test]
    fn parse_voller_payload() {
        let raw = r#"Here you go: {
            "youtube": {"title": "Haze 1v3 Clutch", "description": "Wild.", "hashtags": ["haze", "clutch"]},
            "tiktok": {"title": "T", "description": "", "hashtags": []},
            "instagram": {"title": "I", "description": "x", "hashtags": ["deadlock"]}
        } done"#;
        let r = parse_llm_payload(raw, "ollama", "llama3", None).unwrap();
        assert_eq!(r.youtube.title.as_deref(), Some("Haze 1v3 Clutch"));
        // #Deadlock immer dabei.
        assert!(r.youtube.hashtags.contains(&"#Deadlock".to_string()));
        assert!(r.youtube.hashtags.contains(&"#haze".to_string()));
        assert_eq!(r.provider, "ollama");
        // Fehlender Titel → Fehler.
        let bad = r#"{"youtube": {"title": "", "hashtags": []}, "tiktok": {"title":"t"}, "instagram":{"title":"i"}}"#;
        assert!(parse_llm_payload(bad, "x", "y", None).is_err());
    }

    #[test]
    fn user_prompt_enthaelt_kontext() {
        let req = LlmRequest {
            transcript: "haze ist stark".to_string(),
            detected_terms: vec!["Haze".to_string()],
            streamer: Some(StreamerProfile { streamer_login: "nani".to_string(), language: Some("de".to_string()), ..Default::default() }),
            clip_title: Some("Insane play".to_string()),
            game_name: None,
            duration_seconds: Some(28.7),
        };
        let p = render_user_prompt(&req);
        assert!(p.contains("login=nani"));
        assert!(p.contains("language=de"));
        assert!(p.contains("Game: Deadlock")); // Default
        assert!(p.contains("Clip duration: 29s"));
        assert!(p.contains("Detected Deadlock vocabulary: Haze"));
        assert!(p.contains("haze ist stark"));
    }
}
