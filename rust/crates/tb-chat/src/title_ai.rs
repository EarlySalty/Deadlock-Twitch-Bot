//! `!title`-Generator-Kernlogik: Rate-Limiter, Promptbau, MiniMax-HTTP-Call,
//! Usage-Ledger und Response-Verarbeitung aus `bot/title_generator/title_ai.py`.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use regex::Regex;

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
    pub fn check_and_record(
        &self,
        streamer_id: &str,
        source: &str,
    ) -> Result<(), RateLimitExceeded> {
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
    Regex::new(r"[\x{10000}-\x{10ffff}\x{1F300}-\x{1F9FF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}]")
        .unwrap()
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
        let allowed = rd.split_whitespace().next().unwrap_or("").to_lowercase();
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
    cleaned = Regex::new(r"\s{2,}")
        .unwrap()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = Regex::new(r"\s+([|:,-])")
        .unwrap()
        .replace_all(&cleaned, "$1")
        .into_owned();
    cleaned = Regex::new(r"([|:,-]){2,}")
        .unwrap()
        .replace_all(&cleaned, "$1")
        .into_owned();
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

// ---------------------------------------------------------------------------
// Prompt-Bau (Python `build_title_prompt`)
// ---------------------------------------------------------------------------

/// History-Eintrag fürs Prompt (mit den im Command berechneten Metriken
/// `relative_perf` / `engagement_rate`).
#[derive(Debug, Clone)]
pub struct PromptHistoryItem {
    pub title: String,
    pub relative_perf: Option<f64>,
    pub engagement_rate: Option<f64>,
}

/// Community-Benchmark fürs Prompt.
#[derive(Debug, Clone)]
pub struct PromptKnowledgeItem {
    pub title: String,
    pub normalized_score: Option<f64>,
}

/// Live-Daten fürs Prompt (Hero/Party).
#[derive(Debug, Clone)]
pub struct PromptLiveState {
    pub hero: Option<String>,
    pub party_hint: Option<String>,
}

fn lines_or_default(lines: Vec<String>) -> String {
    if lines.is_empty() {
        "  (keine Daten)".to_string()
    } else {
        lines.join("\n")
    }
}

fn history_line(item: &PromptHistoryItem) -> String {
    format!(
        "  - \"{}\" (relative Perf: {}, Engagement: {})",
        item.title,
        format_metric(item.relative_perf, 2),
        format_metric(item.engagement_rate, 3),
    )
}

/// Baut das MiniMax-Prompt (Python `build_title_prompt`). Sortiert die History
/// nach (relative_perf, engagement_rate) absteigend für die Top-Referenzen.
///
/// Hinweis: Für `Hero`/`Party` greift bei `None` der Python-Default
/// (`unbekannt`/`solo`) — der `.get(key, default)` in Python war für genau
/// diesen Fall gedacht (Live-Feld vorhanden aber leer).
pub fn build_title_prompt(
    keywords: &str,
    title_history: &[PromptHistoryItem],
    knowledge_titles: &[PromptKnowledgeItem],
    rank_display: Option<&str>,
    emoji_ratio: f64,
    live_state: Option<&PromptLiveState>,
) -> String {
    let emoji_rule = if emoji_ratio >= 0.3 {
        "Verwende maximal einen Emoji und nur dann, wenn der Streamer bereits Emojis in seinen Titeln nutzt."
    } else {
        "Verwende KEINE Emojis im Titel."
    };

    let mut sorted: Vec<&PromptHistoryItem> = title_history.iter().collect();
    sorted.sort_by(|a, b| {
        let ka = (
            a.relative_perf.unwrap_or(0.0),
            a.engagement_rate.unwrap_or(0.0),
        );
        let kb = (
            b.relative_perf.unwrap_or(0.0),
            b.engagement_rate.unwrap_or(0.0),
        );
        kb.partial_cmp(&ka).unwrap_or(std::cmp::Ordering::Equal)
    });

    let top_reference_lines =
        lines_or_default(sorted.iter().take(8).map(|t| history_line(t)).collect());
    let history_lines = lines_or_default(title_history.iter().take(20).map(history_line).collect());
    let benchmark_lines = lines_or_default(
        knowledge_titles
            .iter()
            .take(20)
            .map(|t| {
                format!(
                    "  - \"{}\" (Score: {})",
                    t.title,
                    format_metric(t.normalized_score, 2)
                )
            })
            .collect(),
    );

    let rank_line = rank_display
        .map(|rd| format!("\nStreamer-Rang: {rd}"))
        .unwrap_or_default();
    let live_line = live_state
        .map(|ls| {
            format!(
                "\nAktuelle Live-Daten: Hero={}, Party={}",
                ls.hero.as_deref().unwrap_or("unbekannt"),
                ls.party_hint.as_deref().unwrap_or("solo"),
            )
        })
        .unwrap_or_default();
    let canonical_ranks = CANONICAL_RANK_NAMES.join(", ");

    format!(
        r#"Du bist ein Twitch-Stream-Titel-Experte für das Spiel Deadlock.

AUFGABE:
1. Analysiere die letzten Stream-Titel des Streamers (mit Performance-Metriken).
2. Generiere EINEN optimalen Stream-Titel basierend auf den angegebenen Keywords.
3. Gib zusätzlich 2 Alternativen an.
4. Bewerte kurz die 3 schlechtesten eigenen Titel (max. 1 Satz je Titel).

KATEGORIE/SPIEL: Deadlock
KEYWORDS (Intent des Streamers heute): {keywords}{rank_line}{live_line}

BESTE EIGENE REFERENZEN (priorisieren, zuerst daran orientieren):
{top_reference_lines}

EIGENE TITEL-HISTORY (relative_perf = avg_viewers / eigener_durchschnitt):
{history_lines}

COMMUNITY BENCHMARKS (beste Deadlock-Titel nach normalisiertem Score):
{benchmark_lines}

REGELN:
- Der Titel soll vollständig und einladend sein - kein reiner Keyword-Dump.
- Nutze einen konkreten Hook aus Keywords, Rang, Hero oder Party-Kontext statt austauschbarer Gaming-Floskeln.
- Schreibe 45 bis 100 Zeichen; die harte Twitch-Obergrenze sind 140 Zeichen.
- Erzeuge keine generischen Titel wie "Ranked Grind", "Gaming heute" oder "Wir sind live" ohne konkreten Anlass.
- Passe dich stilistisch zuerst den BESTEN EIGENEN REFERENZEN an, erst danach den Community-Benchmarks.
- Erfinde möglichst wenig neu. Bevorzuge bekannte Formulierungsbausteine, Satzrhythmus und Tonalität aus den Referenzen.
- Wenn Keywords ungewohnt sind, formuliere konservativ statt kreativ.
- {emoji_rule}
- Halte den Titel unter 140 Zeichen.
- Verwende Rangbegriffe nur, wenn sie explizit in den Keywords oder in "Streamer-Rang" stehen.
- Erfinde niemals Ränge, Skill-Stufen oder Match-Kontext.
- Deadlock-Ränge heißen nur: {canonical_ranks}.
- Schreibe Keywords nicht in andere Begriffe um. Beispiel: "Asc 2" bleibt "Asc 2" und wird NICHT zu "Ascension Rank 2" oder ähnlichem erweitert.
- Vermeide generische Füllphrasen wie "heute ist es soweit", "endlich soweit" oder ähnliche Trailer-Sätze.
- Die Performance-Scores basieren auf Viewer-Zahlen als Proxy (keine echten CTR-Daten).

ANTWORT-FORMAT (JSON, kein Markdown drumherum):
{{
  "primary_title": "<optimaler Titel>",
  "alternatives": ["<Alternative 1>", "<Alternative 2>"],
  "title_analysis": [
    {{"title": "<schlechtester eigener Titel>", "score": <1-10>, "feedback": "<1 Satz>"}},
    ...
  ]
}}"#
    )
}

// ---------------------------------------------------------------------------
// MiniMax-HTTP-Call (Python `generate_title` + `_get_minimax_client`)
// ---------------------------------------------------------------------------

/// Fehler beim Generieren eines Titels.
#[derive(Debug)]
pub enum GenerateTitleError {
    /// Rate-Limit überschritten (Python `RateLimitExceeded`).
    RateLimit(RateLimitExceeded),
    /// Kein MiniMax-Key in der Umgebung (Python `LLMSecretNotFoundError`).
    NoApiKey,
    /// HTTP-/Decode-Fehler beim MiniMax-Call.
    Http(String),
}

/// Anwendungsfall in der gemeinsamen Anbieterauswahl.
const USE_CASE: &str = "title_ai";

/// Python `_DDC_PENTEST_DISABLE_RATE_LIMITS`: Rate-Limits aus, wenn die Env-Var
/// auf einen „wahren" Wert gesetzt ist.
fn pentest_disable_rate_limits() -> bool {
    std::env::var("DDC_PENTEST_DISABLE_RATE_LIMITS")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "" | "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(false)
}

/// Ein Titel- oder Insight-Aufruf ueber den gemeinsamen Eingang.
///
/// Der Titel-Pfad haengt am Twitch-Dashboard und laeuft in Stosszeiten in
/// 429er; deshalb zwei Wiederholungen mit `Retry-After`, wie bisher. Verbucht
/// wird unter dem jeweiligen Zweck, damit Titel und Insight im Ledger
/// unterscheidbar bleiben.
async fn titel_completion(
    endpoint: &tb_llm::LlmEndpoint,
    purpose: &str,
    prompt: &str,
    temperature: f64,
    max_tokens: i64,
) -> Result<String, String> {
    let response = tb_llm::complete(
        USE_CASE,
        tb_llm::Request::prompt(prompt)
            .temperature(temperature)
            .max_tokens(max_tokens)
            .retry_on_429(2)
            .ledger_purpose(purpose)
            .endpoint(endpoint.clone()),
    )
    .await
    .map_err(|error| match error {
        // Die Fehlerform der bisherigen Meldungen bleibt: die Aufrufer und die
        // Tests lesen "HTTP <status>".
        tb_llm::LlmError::Http { status, .. } => format!("HTTP {status}"),
        other => other.to_string(),
    })?;
    Ok(response.text)
}

/// Endpunkt dieses Anwendungsfalls aus expliziten Testwerten.
fn endpunkt(base_url: &str, api_key: &str, model: &str) -> tb_llm::LlmEndpoint {
    let mut endpoint = tb_llm::endpoint_for(USE_CASE);
    endpoint.base_url = base_url.to_string();
    endpoint.model = model.to_string();
    endpoint.api_key = Some(api_key.to_string());
    endpoint
}

/// Kern von `generate_title` mit injizierbarem Endpoint (für Tests).
/// emoji_ratio → Prompt → OpenAI-kompatibler POST → parse → sanitize.
#[allow(clippy::too_many_arguments)]
pub async fn generate_title_with(
    base_url: &str,
    api_key: &str,
    model: &str,
    keywords: &str,
    title_history: &[PromptHistoryItem],
    knowledge_titles: &[PromptKnowledgeItem],
    rank_display: Option<&str>,
    live_state: Option<&PromptLiveState>,
) -> Result<TitleResult, GenerateTitleError> {
    let titles: Vec<&str> = title_history.iter().map(|h| h.title.as_str()).collect();
    let ratio = emoji_ratio(&titles);
    let prompt = build_title_prompt(
        keywords,
        title_history,
        knowledge_titles,
        rank_display,
        ratio,
        live_state,
    );
    let content = titel_completion(
        &endpunkt(base_url, api_key, model),
        "title",
        &prompt,
        0.35,
        2000,
    )
    .await
    .map_err(GenerateTitleError::Http)?;
    let result = sanitize_title_result(parse_title_response(&content), keywords, rank_display);
    if result.primary.is_empty() {
        return Err(GenerateTitleError::Http(
            "MiniMax returned no usable title".to_string(),
        ));
    }
    Ok(result)
}

/// Generiert einen Stream-Titel via MiniMax (Python `generate_title`).
/// Reihenfolge wie Python: Rate-Limit zuerst, dann Key-Resolve, dann Call.
#[allow(clippy::too_many_arguments)]
pub async fn generate_title(
    rate_limiter: &TitleRateLimiter,
    streamer_id: &str,
    keywords: &str,
    title_history: &[PromptHistoryItem],
    knowledge_titles: &[PromptKnowledgeItem],
    rank_display: Option<&str>,
    live_state: Option<&PromptLiveState>,
    source: &str,
) -> Result<TitleResult, GenerateTitleError> {
    if !pentest_disable_rate_limits() {
        rate_limiter
            .check_and_record(streamer_id, source)
            .map_err(GenerateTitleError::RateLimit)?;
    }
    let endpoint = tb_llm::endpoint_for(USE_CASE);
    let api_key = endpoint
        .api_key
        .as_deref()
        .ok_or(GenerateTitleError::NoApiKey)?;
    generate_title_with(
        &endpoint.base_url,
        api_key,
        &endpoint.model,
        keywords,
        title_history,
        knowledge_titles,
        rank_display,
        live_state,
    )
    .await
}

// ───────────────────────────────────────────────────────────────────────────
// Wöchentliche Insight-Analyse (Python `generate_insight`).
// ───────────────────────────────────────────────────────────────────────────

/// Eine Titel-Zeile für die Insight-Analyse (Python `title_history`-Dict).
pub struct InsightHistoryItem {
    pub title: String,
    pub relative_perf: f64,
    pub engagement_rate: f64,
}

/// Ergebnis der wöchentlichen Insight-Analyse (Python `generate_insight`-Return).
#[derive(Debug, Clone)]
pub struct InsightResult {
    pub strengths: String,
    pub weaknesses: String,
    pub patterns: String,
    pub recommendations: String,
    pub raw: serde_json::Value,
}

const INSIGHT_PROMPT_TEMPLATE: &str = r#"Analysiere die Stream-Titel-Performance dieses Deadlock-Streamers für {period_label}.

TITEL-HISTORY (relative_perf = avg_viewers / eigener_durchschnitt):
{history_lines}

Identifiziere:
1. Was läuft gut (Stärken)
2. Was läuft schlecht (Schwächen)
3. Erkannte Muster (z.B. "Titles mit Rang performen besser")
4. Genau 3 konkrete Handlungsempfehlungen

ANTWORT-FORMAT (JSON):
{
  "strengths": "<Freitext>",
  "weaknesses": "<Freitext>",
  "patterns": "<Freitext>",
  "recommendations": ["<Empfehlung 1>", "<Empfehlung 2>", "<Empfehlung 3>"]
}"#;

fn build_insight_prompt(history: &[InsightHistoryItem], period_label: &str) -> String {
    let history_lines = history
        .iter()
        .take(40)
        .map(|t| {
            format!(
                "  - \"{}\" (relative Perf: {}, Engagement: {})",
                t.title,
                format_metric(Some(t.relative_perf), 2),
                format_metric(Some(t.engagement_rate), 3)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    INSIGHT_PROMPT_TEMPLATE
        .replace("{period_label}", period_label)
        .replace("{history_lines}", &history_lines)
}

fn value_to_plain(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Parst die KI-Antwort zur Insight (Python `generate_insight`-Parsing):
/// strip-code-fence → JSON-Objekt → strengths/weaknesses/patterns +
/// recommendations (Liste → „• "-Bullets der ersten 3, sonst `str`). Kein
/// Objekt / Parse-Fehler → `None`.
fn parse_insight_response(raw: &str) -> Option<InsightResult> {
    let stripped = strip_code_fence(raw);
    let data: serde_json::Value = serde_json::from_str(&stripped).ok()?;
    if !data.is_object() {
        return None;
    }
    let recommendations = match data.get("recommendations") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .take(3)
            .map(|r| format!("• {}", value_to_plain(r)))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => value_to_plain(other),
        None => String::new(),
    };
    let field = |k: &str| {
        data.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    Some(InsightResult {
        strengths: field("strengths"),
        weaknesses: field("weaknesses"),
        patterns: field("patterns"),
        recommendations,
        raw: data,
    })
}

/// Insight-Analyse mit injizierbarem `base_url`/`api_key` (für Tests). Leere
/// History / HTTP-Fehler / Parse-Fehler → `None`.
pub async fn generate_insight_with(
    base_url: &str,
    api_key: &str,
    model: &str,
    history: &[InsightHistoryItem],
    period_label: &str,
) -> Option<InsightResult> {
    if history.is_empty() {
        return None;
    }
    let prompt = build_insight_prompt(history, period_label);
    let content = titel_completion(
        &endpunkt(base_url, api_key, model),
        "title-insight",
        &prompt,
        0.5,
        1500,
    )
    .await
    .ok()?;
    parse_insight_response(&content)
}

/// Wöchentliche Insight-Analyse via MiniMax (Python `generate_insight`).
/// Leere History / fehlender Key / Fehler → `None`.
pub async fn generate_insight(
    history: &[InsightHistoryItem],
    period_label: &str,
) -> Option<InsightResult> {
    if history.is_empty() {
        return None;
    }
    let endpoint = tb_llm::endpoint_for(USE_CASE);
    let key = endpoint.api_key.as_deref()?;
    generate_insight_with(
        &endpoint.base_url,
        key,
        &endpoint.model,
        history,
        period_label,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    static PROVIDER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_provider_env() {
        for name in [
            "TB_LLM_PROVIDER_DEFAULT",
            "TB_LLM_PROVIDER_TITLE_AI",
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "FIREWORK_MODEL",
            "FIREWORKS_MODEL",
            "MINIMAX_TOKEN_PLAN_KEY",
            "MINIMAX_API_KEY",
            "MINIMAX_BASE_URL",
            "MINIMAX_MODEL",
            "MINMAX",
        ] {
            std::env::remove_var(name);
        }
    }

    // Die Env-Werte müssen bis nach dem HTTP-Call exklusiv bleiben; sonst
    // können parallele Tests den ausgewählten Endpoint während des Calls ändern.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn title_ai_folgt_gemeinsamer_provider_auswahl() {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let _guard = PROVIDER_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_provider_env();
        std::env::set_var("FIREWORK_API_KEY", "fireworks-key");

        let endpoint = tb_llm::endpoint_for("title_ai");
        assert!(endpoint.base_url.contains("fireworks.ai"));
        assert!(endpoint.model.contains("deepseek"));

        let server = MockServer::start().await;
        std::env::set_var("FIREWORK_BASE_URL", server.uri());
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("deepseek"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content":
                    "{\"primary_title\":\"Provider-Test\",\"alternatives\":[],\"title_analysis\":[]}"}}]
            })))
            .mount(&server)
            .await;

        let result = generate_title(
            &TitleRateLimiter::new(1, 60, 1),
            "streamer",
            "ranked",
            &[],
            &[],
            None,
            None,
            "chat",
        )
        .await;
        match result {
            Ok(title) => assert_eq!(title.primary, "Provider-Test"),
            Err(GenerateTitleError::NoApiKey) => {
                panic!("Fireworks-Key wurde nicht für title_ai verwendet")
            }
            Err(GenerateTitleError::RateLimit(error)) => {
                panic!("unerwartetes Rate-Limit: {}", error.retry_after)
            }
            Err(GenerateTitleError::Http(error)) => panic!("unerwarteter HTTP-Fehler: {error}"),
        }

        clear_provider_env();
        std::env::set_var("MINIMAX_API_KEY", "minimax-key");
        let endpoint = tb_llm::endpoint_for("title_ai");
        assert_eq!(endpoint.base_url, "https://api.minimax.io/v1");
        assert_eq!(endpoint.model, "MiniMax-M3");

        std::env::set_var("FIREWORK_API_KEY", "fireworks-key");
        std::env::set_var("TB_LLM_PROVIDER_TITLE_AI", "minimax");
        let endpoint = tb_llm::endpoint_for("title_ai");
        assert_eq!(endpoint.base_url, "https://api.minimax.io/v1");
        assert_eq!(endpoint.model, "MiniMax-M3");
        clear_provider_env();
    }

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

    // Das Auslesen der Token-Zahlen liegt jetzt im gemeinsamen Eingang und wird
    // dort getestet (tb-llm, hub::tests).

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
        assert_eq!(
            parsed.alternatives,
            vec!["A1".to_string(), "A2".to_string()]
        );
    }

    #[test]
    fn parse_title_response_ungueltig_gibt_leer() {
        assert_eq!(parse_title_response("kein json"), ParsedTitle::default());
    }

    #[test]
    fn sanitize_entfernt_nicht_erlaubte_raenge() {
        // rank_display = "Archon 3" → nur "archon" erlaubt; "Phantom" wird entfernt.
        let out =
            sanitize_generated_title("Archon Grind als Phantom 2", "ranked", Some("Archon 3"));
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

    #[test]
    fn prompt_emoji_regel_und_keine_daten_fallback() {
        let p = build_title_prompt("ranked grind", &[], &[], None, 0.0, None);
        assert!(p.contains("Verwende KEINE Emojis im Titel."));
        assert!(p.contains("KEYWORDS (Intent des Streamers heute): ranked grind\n"));
        assert!(p.contains("  (keine Daten)"));
        assert!(p.contains("Deadlock-Ränge heißen nur: Obscurus, Seeker"));

        let p2 = build_title_prompt("x", &[], &[], None, 0.5, None);
        assert!(p2.contains("Verwende maximal einen Emoji"));
    }

    #[test]
    fn prompt_rank_live_und_top_sortierung() {
        let hist = vec![
            PromptHistoryItem {
                title: "Schwach".into(),
                relative_perf: Some(0.5),
                engagement_rate: Some(0.1),
            },
            PromptHistoryItem {
                title: "Stark".into(),
                relative_perf: Some(2.0),
                engagement_rate: Some(0.3),
            },
        ];
        let live = PromptLiveState {
            hero: Some("Haze".into()),
            party_hint: None,
        };
        let p = build_title_prompt("ranked", &hist, &[], Some("Archon 3"), 0.0, Some(&live));
        assert!(p.contains("Streamer-Rang: Archon 3"));
        // party_hint None → Python-Default "solo".
        assert!(p.contains("Aktuelle Live-Daten: Hero=Haze, Party=solo"));
        // Top-Referenzen sind nach Perf sortiert: "Stark" (2.0) vor "Schwach" (0.5).
        assert!(p.find("Stark").unwrap() < p.find("Schwach").unwrap());
    }

    #[test]
    fn prompt_gibt_spiel_hook_und_praezises_format_vor() {
        let p = build_title_prompt("ranked solo", &[], &[], None, 0.0, None);
        assert!(p.contains("KATEGORIE/SPIEL: Deadlock"));
        assert!(p.contains("45 bis 100 Zeichen"));
        assert!(p.contains("konkreten Hook"));
        assert!(p.contains("keine generischen"));
    }

    #[tokio::test]
    async fn generate_title_with_end_to_end() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = serde_json::json!({
            "choices": [{"message": {"content":
                "{\"primary_title\":\"Ranked Grind\",\"alternatives\":[\"Alt Eins\"],\"title_analysis\":[]}"}}]
        });
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let result = generate_title_with(
            &server.uri(),
            "fakekey",
            "MiniMax-M3",
            "ranked",
            &[],
            &[],
            None,
            None,
        )
            .await
            .unwrap();
        assert_eq!(result.primary, "Ranked Grind");
        assert_eq!(result.alternatives, vec!["Alt Eins".to_string()]);
    }

    #[tokio::test]
    async fn generate_title_with_http_fehler() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let err = generate_title_with(&server.uri(), "k", "MiniMax-M3", "x", &[], &[], None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, GenerateTitleError::Http(_)));
    }

    #[tokio::test]
    async fn generate_title_with_wiederholt_429_einmal() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let calls = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with({
                let calls = Arc::clone(&calls);
                move |_: &wiremock::Request| {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        ResponseTemplate::new(429).insert_header("Retry-After", "0")
                    } else {
                        ResponseTemplate::new(200).set_body_json(serde_json::json!({
                            "choices": [{"message": {"content":
                                "{\"primary_title\":\"Ranked mit Plan\",\"alternatives\":[],\"title_analysis\":[]}"}}]
                        }))
                    }
                }
            })
            .mount(&server)
            .await;

        let result = generate_title_with(
            &server.uri(),
            "k",
            "MiniMax-M3",
            "ranked",
            &[],
            &[],
            None,
            None,
        )
            .await
            .unwrap();
        assert_eq!(result.primary, "Ranked mit Plan");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn generate_title_with_leerer_modellantwort_ist_fehler() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "kein JSON"}}]
            })))
            .mount(&server)
            .await;

        let err = generate_title_with(
            &server.uri(),
            "k",
            "MiniMax-M3",
            "ranked",
            &[],
            &[],
            None,
            None,
        )
            .await
            .unwrap_err();
        assert!(matches!(err, GenerateTitleError::Http(_)));
    }

    #[tokio::test]
    async fn generate_insight_with_parst_recommendations() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = serde_json::json!({
            "choices": [{"message": {"content":
                "```json\n{\"strengths\":\"stark\",\"weaknesses\":\"schwach\",\"patterns\":\"muster\",\"recommendations\":[\"a\",\"b\",\"c\",\"d\"]}\n```"}}]
        });
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let history = vec![InsightHistoryItem {
            title: "T".into(),
            relative_perf: 1.2,
            engagement_rate: 0.05,
        }];
        let r = generate_insight_with(
            &server.uri(),
            "k",
            "MiniMax-M3",
            &history,
            "01.06. – 28.06.2026",
        )
            .await
            .unwrap();
        assert_eq!(r.strengths, "stark");
        assert_eq!(r.patterns, "muster");
        // 4 Recs → erste 3, je „• "-Prefix.
        assert_eq!(r.recommendations, "• a\n• b\n• c");
        assert_eq!(r.raw["weaknesses"], "schwach"); // raw bleibt vollständig
    }

    #[tokio::test]
    async fn generate_insight_with_leer_und_parsefehler() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // Leere History → None ohne HTTP-Call.
        assert!(
            generate_insight_with("http://unused", "k", "MiniMax-M3", &[], "p")
                .await
                .is_none()
        );

        // Kein JSON → None.
        let server = MockServer::start().await;
        let body = serde_json::json!({"choices": [{"message": {"content": "kein json hier"}}]});
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let history = vec![InsightHistoryItem {
            title: "T".into(),
            relative_perf: 1.0,
            engagement_rate: 0.1,
        }];
        assert!(
            generate_insight_with(&server.uri(), "k", "MiniMax-M3", &history, "p")
                .await
                .is_none()
        );
    }
}
