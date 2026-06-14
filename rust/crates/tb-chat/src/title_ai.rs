//! `!title`-Generator-Kernlogik (B11, Slice 3a): Rate-Limiter + Response-
//! Verarbeitung (JSON-Extraktion, Titel-Sanitization, Parsing). Port der reinen
//! Teile aus `bot/title_generator/title_ai.py`.
//!
//! NICHT in dieser Slice: `build_title_prompt` (Prompt-Template, Slice 3b) und
//! der MiniMax-HTTP-Call `generate_title` (Slice 3c). Hier nur die seiteneffekt-
//! freie, vollständig unit-testbare Logik.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use regex::Regex;

/// OpenAI-kompatibler MiniMax-Endpoint (Python `MINIMAX_BASE_URL`).
pub const MINIMAX_BASE_URL: &str = "https://api.minimax.io/v1";
/// MiniMax-Modell (Python `MINIMAX_MODEL`).
pub const MINIMAX_MODEL: &str = "MiniMax-M3";

/// Deadlock-Rangnamen (Python `_CANONICAL_RANK_NAMES`).
const CANONICAL_RANK_NAMES: [&str; 11] = [
    "Obscurus",
    "Seeker",
    "Alchemist",
    "Arcanist",
    "Ritualist",
    "Emissary",
    "Archon",
    "Oracle",
    "Phantom",
    "Ascendant",
    "Eternus",
];

/// Generische Trailer-Floskeln, die aus Titeln entfernt werden
/// (Python `_GENERIC_FILLER_PHRASES`).
const GENERIC_FILLER_PHRASES: [&str; 4] = [
    "heute ist es soweit",
    "heute ist es endlich soweit",
    "endlich ist es soweit",
    "endlich soweit",
];

// ---------------------------------------------------------------------------
// Rate-Limiter (Python `TitleRateLimiter`)
// ---------------------------------------------------------------------------

/// Rate-Limit überschritten — `retry_after` Sekunden warten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitExceeded {
    pub retry_after: u64,
}

/// In-Memory-Rate-Limiter pro `streamer_id:source` (Python `TitleRateLimiter`):
/// `dashboard` bekommt das `dashboard_multiplier`-fache Budget.
pub struct TitleRateLimiter {
    max: usize,
    window: Duration,
    dashboard_max: usize,
    records: Mutex<HashMap<String, Vec<Instant>>>,
}

impl TitleRateLimiter {
    pub fn new(max_requests: usize, window_seconds: u64, dashboard_multiplier: usize) -> Self {
        Self {
            max: max_requests,
            window: Duration::from_secs(window_seconds),
            dashboard_max: max_requests * dashboard_multiplier,
            records: Mutex::new(HashMap::new()),
        }
    }

    /// Prüft + verbucht eine Anfrage. `Err` mit `retry_after`, wenn das Budget
    /// im Fenster erschöpft ist (Python `check_and_record`).
    pub fn check_and_record(&self, streamer_id: &str, source: &str) -> Result<(), RateLimitExceeded> {
        let now = Instant::now();
        let key = format!("{streamer_id}:{source}");
        let limit = if source == "dashboard" {
            self.dashboard_max
        } else {
            self.max
        };
        let mut records = self.records.lock().unwrap();
        let entry = records.entry(key).or_default();
        entry.retain(|t| now.duration_since(*t) < self.window);
        if entry.len() >= limit {
            let oldest = entry[0];
            let retry_after = self
                .window
                .as_secs()
                .saturating_sub(now.duration_since(oldest).as_secs())
                + 1;
            return Err(RateLimitExceeded { retry_after });
        }
        entry.push(now);
        Ok(())
    }
}

impl Default for TitleRateLimiter {
    /// Python-Defaults: 5 Anfragen / 600 s, Dashboard 2×.
    fn default() -> Self {
        Self::new(5, 600, 2)
    }
}

// ---------------------------------------------------------------------------
// Response-Verarbeitung
// ---------------------------------------------------------------------------

/// Geparste, noch nicht sanitisierte LLM-Antwort (Python `parse_title_response`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedTitle {
    pub primary: String,
    pub alternatives: Vec<String>,
    pub title_analysis: Vec<serde_json::Value>,
}

/// Sanitisiertes Endergebnis (Python `_sanitize_title_result`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TitleResult {
    pub primary: String,
    pub alternatives: Vec<String>,
    pub title_analysis: Vec<serde_json::Value>,
}

fn emoji_regex() -> Regex {
    // Python EMOJI_PATTERN. \x{10000}-\x{10ffff} deckt die astralen Emoji
    // bereits ab; die zusätzlichen BMP-Bereiche fürs Symbol-Set.
    Regex::new(r"[\x{10000}-\x{10ffff}\x{1F300}-\x{1F9FF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}]").unwrap()
}

/// Anteil der Titel mit mindestens einem Emoji (Python `_emoji_ratio`).
pub fn emoji_ratio(titles: &[&str]) -> f64 {
    if titles.is_empty() {
        return 0.0;
    }
    let re = emoji_regex();
    let with_emoji = titles.iter().filter(|t| re.is_match(t)).count();
    with_emoji as f64 / titles.len() as f64
}

/// Formatiert eine Metrik mit `digits` Nachkommastellen, `None` → "n/a"
/// (Python `_format_metric`).
pub fn format_metric(value: Option<f64>, digits: usize) -> String {
    match value {
        Some(v) => format!("{v:.digits$}"),
        None => "n/a".to_string(),
    }
}

fn strip_code_fence(raw: &str) -> String {
    // Python: re.sub(r"^```(?:json)?\s*|\s*```$", "", raw.strip(), MULTILINE)
    Regex::new(r"(?m)^```(?:json)?\s*|\s*```$")
        .unwrap()
        .replace_all(raw.trim(), "")
        .into_owned()
}

/// Extrahiert das JSON-Objekt aus einer (evtl. Markdown-umrahmten) LLM-Antwort
/// (Python `_extract_json_payload`).
pub fn extract_json_payload(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Some(cap) = Regex::new(r"(?is)```json\s*(\{.*?\})\s*```")
        .unwrap()
        .captures(text)
    {
        return cap[1].trim().to_string();
    }
    if let Some(cap) = Regex::new(r"(?s)(\{.*\})").unwrap().captures(text) {
        return cap[1].trim().to_string();
    }
    strip_code_fence(text)
}

/// Parst die LLM-Antwort zu `ParsedTitle` (Python `parse_title_response`).
/// Ungültiges JSON → leeres Ergebnis.
pub fn parse_title_response(raw: &str) -> ParsedTitle {
    let payload = extract_json_payload(raw);
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&payload) else {
        return ParsedTitle::default();
    };
    ParsedTitle {
        primary: data
            .get("primary_title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        alternatives: data
            .get("alternatives")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .take(2)
                    .collect()
            })
            .unwrap_or_default(),
        title_analysis: data
            .get("title_analysis")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

/// Bereinigt einen generierten Titel (Python `_sanitize_generated_title`):
/// Asc-Normalisierung, Entfernen nicht erlaubter Rangbegriffe + Füllphrasen,
/// Whitespace-/Trenner-Aufräumung.
pub fn sanitize_generated_title(title: &str, keywords: &str, rank_display: Option<&str>) -> String {
    let mut cleaned = title.trim().to_string();
    if cleaned.is_empty() {
        return String::new();
    }
    let lower_keywords = keywords.trim().to_lowercase();

    // "asc N" in den Keywords → "Ascension Rank N" im Titel auf "Asc N" zurück.
    if let Some(cap) = Regex::new(r"(?i)\basc\s*(\d)\b")
        .unwrap()
        .captures(&lower_keywords)
    {
        let digit = cap[1].to_string();
        let pat = format!(r"(?i)\bascension\s+rank\s*{}\b", regex::escape(&digit));
        cleaned = Regex::new(&pat)
            .unwrap()
            .replace_all(&cleaned, format!("Asc {digit}").as_str())
            .into_owned();
    }

    // Nicht erlaubte Rangbegriffe entfernen.
    let strip_rank = |c: &str, name: &str| {
        let pat = format!(r"(?i)\b{}(?:\s+\d)?\b", regex::escape(name));
        Regex::new(&pat).unwrap().replace_all(c, "").into_owned()
    };
    if let Some(rd) = rank_display {
        let allowed = rd
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        for name in CANONICAL_RANK_NAMES {
            if name.to_lowercase() != allowed {
                cleaned = strip_rank(&cleaned, name);
            }
        }
    } else {
        for name in CANONICAL_RANK_NAMES {
            if !lower_keywords.contains(&name.to_lowercase()) {
                cleaned = strip_rank(&cleaned, name);
            }
        }
    }

    // Füllphrasen nach einem Trenner entfernen.
    let filler = GENERIC_FILLER_PHRASES
        .iter()
        .map(|p| regex::escape(p))
        .collect::<Vec<_>>()
        .join("|");
    cleaned = Regex::new(&format!(r"(?i)\s*[\-|:|]\s*(?:{filler})\b"))
        .unwrap()
        .replace_all(&cleaned, "")
        .into_owned();

    // Whitespace + doppelte Trenner aufräumen.
    cleaned = Regex::new(r"\s{2,}").unwrap().replace_all(&cleaned, " ").into_owned();
    cleaned = Regex::new(r"\s+([|:,-])").unwrap().replace_all(&cleaned, "$1").into_owned();
    cleaned = Regex::new(r"([|:,-]){2,}").unwrap().replace_all(&cleaned, "$1").into_owned();
    cleaned.trim_matches(|c| " -|:,".contains(c)).to_string()
}

/// Sanitisiert primary + bis zu 2 deduplizierte Alternativen
/// (Python `_sanitize_title_result`).
pub fn sanitize_title_result(
    parsed: ParsedTitle,
    keywords: &str,
    rank_display: Option<&str>,
) -> TitleResult {
    let primary = sanitize_generated_title(&parsed.primary, keywords, rank_display);
    let mut alternatives: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    if !primary.is_empty() {
        seen.insert(primary.to_lowercase());
    }
    for title in &parsed.alternatives {
        let cleaned = sanitize_generated_title(title, keywords, rank_display);
        if cleaned.is_empty() {
            continue;
        }
        let key = cleaned.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        alternatives.push(cleaned);
        if alternatives.len() >= 2 {
            break;
        }
    }
    let primary_final = if !primary.is_empty() {
        primary
    } else {
        alternatives.first().cloned().unwrap_or_default()
    };
    TitleResult {
        primary: primary_final,
        alternatives,
        title_analysis: parsed.title_analysis,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_blockt_nach_max_und_dashboard_2x() {
        let rl = TitleRateLimiter::new(2, 600, 2);
        assert!(rl.check_and_record("s1", "chat").is_ok());
        assert!(rl.check_and_record("s1", "chat").is_ok());
        let err = rl.check_and_record("s1", "chat").unwrap_err();
        assert!(err.retry_after >= 1);
        // Anderer source/streamer hat eigenes Budget.
        assert!(rl.check_and_record("s2", "chat").is_ok());
        // dashboard = 2× = 4 erlaubt.
        for _ in 0..4 {
            assert!(rl.check_and_record("s1", "dashboard").is_ok());
        }
        assert!(rl.check_and_record("s1", "dashboard").is_err());
    }

    #[test]
    fn emoji_ratio_korrekt() {
        assert_eq!(emoji_ratio(&[]), 0.0);
        let ratio = emoji_ratio(&["Ranked Grind 🔥", "Chill Stream", "GG 🎮"]);
        assert!((ratio - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn format_metric_digits_und_na() {
        assert_eq!(format_metric(Some(1.2345), 2), "1.23");
        assert_eq!(format_metric(Some(0.5), 3), "0.500");
        assert_eq!(format_metric(None, 2), "n/a");
    }

    #[test]
    fn extract_json_payload_varianten() {
        // Fenced ```json … ```
        let fenced = "bla\n```json\n{\"primary_title\": \"X\"}\n```\nrest";
        assert_eq!(extract_json_payload(fenced), "{\"primary_title\": \"X\"}");
        // Reines Objekt im Text
        let plain = "Hier: {\"a\": 1} danke";
        assert_eq!(extract_json_payload(plain), "{\"a\": 1}");
        // Leer
        assert_eq!(extract_json_payload("   "), "");
    }

    #[test]
    fn parse_title_response_nimmt_max_2_alternativen() {
        let raw = "{\"primary_title\":\"Bester Titel\",\"alternatives\":[\"A1\",\"A2\",\"A3\"],\"title_analysis\":[]}";
        let parsed = parse_title_response(raw);
        assert_eq!(parsed.primary, "Bester Titel");
        assert_eq!(parsed.alternatives, vec!["A1".to_string(), "A2".to_string()]);
    }

    #[test]
    fn parse_title_response_ungueltig_gibt_leer() {
        assert_eq!(parse_title_response("kein json"), ParsedTitle::default());
    }

    #[test]
    fn sanitize_entfernt_nicht_erlaubte_raenge() {
        // rank_display = "Archon 3" → nur "archon" erlaubt; "Phantom" wird entfernt.
        let out = sanitize_generated_title("Archon Grind als Phantom 2", "ranked", Some("Archon 3"));
        assert!(out.contains("Archon"));
        assert!(!out.to_lowercase().contains("phantom"));
    }

    #[test]
    fn sanitize_ohne_rank_display_entfernt_alle_nicht_in_keywords() {
        // Kein rank_display, "eternus" nicht in Keywords → entfernt.
        let out = sanitize_generated_title("Eternus Grind", "ranked solo", None);
        assert!(!out.to_lowercase().contains("eternus"));
        // "obscurus" in Keywords → bleibt.
        let keep = sanitize_generated_title("Obscurus Climb", "obscurus grind", None);
        assert!(keep.to_lowercase().contains("obscurus"));
    }

    #[test]
    fn sanitize_asc_normalisierung() {
        let out = sanitize_generated_title("Ascension Rank 2 Grind", "asc 2 ranked", None);
        assert!(out.contains("Asc 2"));
        assert!(!out.to_lowercase().contains("ascension rank"));
    }

    #[test]
    fn sanitize_entfernt_fuellphrasen_und_trenner() {
        let out = sanitize_generated_title("Ranked Grind - heute ist es soweit", "ranked", None);
        assert_eq!(out, "Ranked Grind");
    }

    #[test]
    fn sanitize_title_result_dedupliziert() {
        let parsed = ParsedTitle {
            primary: "Ranked Grind".into(),
            alternatives: vec!["ranked grind".into(), "Anderer Titel".into()],
            title_analysis: vec![],
        };
        let result = sanitize_title_result(parsed, "ranked", None);
        assert_eq!(result.primary, "Ranked Grind");
        // "ranked grind" ist Dup von primary → raus; nur "Anderer Titel" bleibt.
        assert_eq!(result.alternatives, vec!["Anderer Titel".to_string()]);
    }

    #[test]
    fn sanitize_title_result_primary_fallback_auf_erste_alternative() {
        let parsed = ParsedTitle {
            primary: "".into(),
            alternatives: vec!["Fallback Titel".into()],
            title_analysis: vec![],
        };
        let result = sanitize_title_result(parsed, "ranked", None);
        assert_eq!(result.primary, "Fallback Titel");
    }
}
