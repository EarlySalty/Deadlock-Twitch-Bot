//! `!title`-Generator-Kernlogik: Rate-Limiter, Promptbau, KI-HTTP-Call,
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

/// Explizites Human-Feedback aus frueheren Generierungen.
#[derive(Debug, Clone)]
pub struct PromptFeedbackItem {
    pub proposed_title: String,
    pub feedback: String,
    pub selected_title: Option<String>,
    pub edited_title: Option<String>,
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

/// Baut das KI-Prompt (Python `build_title_prompt`). Sortiert die History
/// nach (relative_perf, engagement_rate) absteigend für die Top-Referenzen.
///
/// Hinweis: Für `Hero`/`Party` greift bei `None` der Python-Default
/// (`unbekannt`) — fehlender Kontext darf keine Gruppengröße erfinden.
/// diesen Fall gedacht (Live-Feld vorhanden aber leer).
pub fn derive_style_summary(title_history: &[PromptHistoryItem]) -> String {
    if title_history.is_empty() {
        return "Noch keine eigene Titel-Historie: Stil wird aus der gespeicherten Präferenz und hochwertigen Community-Mustern abgeleitet.".to_string();
    }

    let sample: Vec<&PromptHistoryItem> = title_history.iter().take(30).collect();
    let avg_len = sample
        .iter()
        .map(|item| item.title.chars().count() as f64)
        .sum::<f64>()
        / sample.len() as f64;
    let titles: Vec<&str> = sample.iter().map(|item| item.title.as_str()).collect();
    let emoji = emoji_ratio(&titles);
    let pipe = sample
        .iter()
        .filter(|item| item.title.contains('|'))
        .count();
    let colon = sample
        .iter()
        .filter(|item| item.title.contains(':'))
        .count();
    let dash = sample
        .iter()
        .filter(|item| item.title.contains(" - ") || item.title.contains(" – "))
        .count();
    let questions = sample
        .iter()
        .filter(|item| item.title.contains('?'))
        .count();
    let exclamations = sample
        .iter()
        .filter(|item| item.title.contains('!'))
        .count();

    let separator = [("|", pipe), (":", colon), ("–", dash)]
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count * 3 >= sample.len())
        .map(|(separator, _)| format!("häufiger Trenner {separator}"))
        .unwrap_or_else(|| "kein dominanter Trenner".to_string());
    let emoji_text = if emoji >= 0.3 {
        "Emojis gehören erkennbar zum Stil"
    } else {
        "Emojis sind untypisch"
    };
    let punctuation = if questions * 4 >= sample.len() {
        "Fragen kommen öfter vor"
    } else if exclamations * 4 >= sample.len() {
        "Ausrufezeichen kommen öfter vor"
    } else {
        "eher ruhige Satzzeichen"
    };

    format!(
        "Ø ca. {:.0} Zeichen; {}; {}; {}.",
        avg_len, emoji_text, separator, punctuation
    )
}

/// Expliziter Stil und aus bisherigen Titeln erkannte Emoji-Nutzung.
pub struct PromptStyle<'a> {
    pub preference: &'a str,
    pub emoji_ratio: f64,
}

/// Personalisierter Prompt des Dashboard-Moduls. Die explizite Nutzerpräferenz
/// steht über der automatisch erkannten Stil-DNA; Community-Titel dienen nur
/// als Inspirationsquelle und dürfen nicht wörtlich kopiert werden.
pub fn build_personalized_title_prompt_with_feedback(
    keywords: &str,
    style: PromptStyle<'_>,
    feedback: &[PromptFeedbackItem],
    title_history: &[PromptHistoryItem],
    knowledge_titles: &[PromptKnowledgeItem],
    rank_display: Option<&str>,
    live_state: Option<&PromptLiveState>,
) -> String {
    let PromptStyle {
        preference: style_preference,
        emoji_ratio,
    } = style;
    let emoji_rule = if emoji_ratio >= 0.3 {
        "Maximal einen Emoji verwenden – und nur wenn er natürlich zum erkannten Eigenstil passt."
    } else {
        "Keine Emojis verwenden, außer die gespeicherte Nutzerpräferenz verlangt sie ausdrücklich."
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
        lines_or_default(sorted.iter().take(10).map(|t| history_line(t)).collect());
    let recent_history_lines =
        lines_or_default(title_history.iter().take(18).map(history_line).collect());
    let benchmark_lines = lines_or_default(
        knowledge_titles
            .iter()
            .take(18)
            .map(|t| {
                format!(
                    "  - \"{}\" (Qualitäts-Score: {})",
                    t.title,
                    format_metric(t.normalized_score, 2)
                )
            })
            .collect(),
    );
    let feedback_lines = lines_or_default(
        feedback
            .iter()
            .take(16)
            .map(|item| match item.feedback.as_str() {
                "edited" => format!(
                    "  - SEHR STARKES SIGNAL: Vorschlag \"{}\" wurde zu \"{}\" umgeschrieben.",
                    item.proposed_title,
                    item.edited_title.as_deref().unwrap_or("")
                ),
                "selected" => format!(
                    "  - POSITIV: Gewählt wurde \"{}\" statt \"{}\".",
                    item.selected_title
                        .as_deref()
                        .unwrap_or(&item.proposed_title),
                    item.proposed_title
                ),
                "liked" => format!("  - POSITIV: \"{}\"", item.proposed_title),
                "disliked" => format!("  - NEGATIV / SO NICHT: \"{}\"", item.proposed_title),
                _ => format!("  - {}: \"{}\"", item.feedback, item.proposed_title),
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
                ls.party_hint.as_deref().unwrap_or("unbekannt"),
            )
        })
        .unwrap_or_default();
    let canonical_ranks = CANONICAL_RANK_NAMES.join(", ");
    let style_summary = derive_style_summary(title_history);
    let style_preference = style_preference.trim();
    let preference_line = if style_preference.is_empty() {
        "(keine zusätzliche Präferenz gespeichert)"
    } else {
        style_preference
    };
    let keyword_line = if keywords.trim().is_empty() {
        "AUTO-MODUS: Es wurden keine Keywords angegeben. Finde selbst einen konkreten, zeitlosen Deadlock-Hook, der zum Eigenstil passt. Nutze Rang/Hero/Party nur wenn diese Daten wirklich vorhanden sind. Der Titel muss auch ohne erfundenen Anlass funktionieren."
    } else {
        keywords.trim()
    };

    format!(
        r#"Du schreibst Twitch-Titel für einen Deadlock-Streamer. Dein Job ist nicht, nach KI zu klingen, sondern einen Titel zu liefern, den dieser konkrete Streamer selbst hätte schreiben können – nur besser.

HEUTIGER INPUT:
{keyword_line}{rank_line}{live_line}

GESPEICHERTE STANDARD-PRÄFERENZ DES STREAMERS (höchste Stil-Priorität):
{preference_line}

AUTOMATISCH ERKANNTE STIL-DNA:
{style_summary}

HUMAN-FEEDBACK AUS FRÜHEREN VORSCHLÄGEN (wichtiger als reine Performance-Daten):
{feedback_lines}

BESTE EIGENE REFERENZEN (Ton, Rhythmus und wiederkehrende Bausteine zuerst hier lernen):
{top_reference_lines}

LETZTE EIGENE TITEL (damit du Wiederholungen vermeidest):
{recent_history_lines}

COMMUNITY-INSPIRATION (starke Titel anderer Deadlock-Streamer; Hooks/Strukturen lernen, NICHT wörtlich kopieren):
{benchmark_lines}

QUALITÄTSREGELN:
- Liefere 1 starken Haupttitel und 2 deutlich unterschiedliche Alternativen.
- Der Haupttitel muss einen echten Hook haben. Kein Keyword-Dump, keine Meta-Sprache, kein Marketing-Sprech.
- Verbotene Slop-Muster ohne konkreten Anlass: "Ranked Grind", "Road to ...", "Gaming heute", "Wir sind live", "Heute wird rasiert", "mal schauen was geht", "chilliger Stream", "let's go" und austauschbare Varianten davon.
- Kreativ sein heißt: einen überraschenden, aber glaubwürdigen Blickwinkel oder Satzrhythmus finden – NICHT Fakten, Ziele, Win-Streaks, Ränge oder Events erfinden.
- Eigene erfolgreiche Formulierungsbausteine dürfen bewusst wiederverwendet und neu kombiniert werden.
- Community-Titel niemals 1:1 übernehmen; höchstens Idee, Hook-Typ oder Struktur adaptieren.
- Wenn Keywords vorhanden sind, müssen ihre Kernaussagen erkennbar im Titel landen. 2–3 Stichwörter sind Kontext, keine Pflicht-Reihenfolge.
- Wenn keine Keywords vorhanden sind, baue einen hochwertigen Evergreen-Titel, der für einen normalen Deadlock-Stream wahr bleibt.
- Zielbereich 45–105 Zeichen; harte Twitch-Grenze 140 Zeichen.
- {emoji_rule}
- Verwende Rangbegriffe nur, wenn sie in HEUTIGER INPUT / Streamer-Rang stehen.
- Erfinde niemals Rang, Hero, Party, Challenge, Streak, Turnier, Patch oder Mitspieler.
- Gültige Deadlock-Ränge: {canonical_ranks}.
- "Asc 2" bleibt exakt "Asc 2"; keine künstliche Expansion zu "Ascension Rank 2".
- Keine generischen Trailer-Floskeln wie "heute ist es soweit" oder "endlich soweit".
- Performance-Werte sind nur ein Viewer-Proxy, keine CTR. Nutze sie als Tendenz, nicht als absolute Wahrheit.

ANTWORT NUR ALS JSON:
{{
  "primary_title": "<bester Titel>",
  "alternatives": ["<klar anderer Ansatz 1>", "<klar anderer Ansatz 2>"],
  "title_analysis": []
}}"#
    )
}

/// Personalisierter Prompt ohne explizite Feedback-Liste (Kompatibilität).
pub fn build_personalized_title_prompt(
    keywords: &str,
    style_preference: &str,
    title_history: &[PromptHistoryItem],
    knowledge_titles: &[PromptKnowledgeItem],
    rank_display: Option<&str>,
    emoji_ratio: f64,
    live_state: Option<&PromptLiveState>,
) -> String {
    build_personalized_title_prompt_with_feedback(
        keywords,
        PromptStyle {
            preference: style_preference,
            emoji_ratio,
        },
        &[],
        title_history,
        knowledge_titles,
        rank_display,
        live_state,
    )
}

/// Rückwärtskompatibler Prompt ohne explizite Nutzerpräferenz – z. B. für den
/// bestehenden !title-Chat-Command und dessen Tests.
pub fn build_title_prompt(
    keywords: &str,
    title_history: &[PromptHistoryItem],
    knowledge_titles: &[PromptKnowledgeItem],
    rank_display: Option<&str>,
    emoji_ratio: f64,
    live_state: Option<&PromptLiveState>,
) -> String {
    build_personalized_title_prompt(
        keywords,
        "",
        title_history,
        knowledge_titles,
        rank_display,
        emoji_ratio,
        live_state,
    )
}

// ---------------------------------------------------------------------------
// KI-HTTP-Call (Python `generate_title` + `_get_client`)
// ---------------------------------------------------------------------------

/// Fehler beim Generieren eines Titels.
#[derive(Debug)]
pub enum GenerateTitleError {
    /// Rate-Limit überschritten (Python `RateLimitExceeded`).
    RateLimit(RateLimitExceeded),
    /// Kein KI-Key in der Umgebung (Python `LLMSecretNotFoundError`).
    NoApiKey,
    /// HTTP-/Decode-Fehler beim KI-Call.
    Http(String),
}

/// Anwendungsfall in der gemeinsamen Anbieterauswahl.
const USE_CASE: &str = "title_ai";
const ZAI_TITLE_BASE_URL: &str = "https://api.z.ai/api/paas/v4";
const ZAI_TITLE_MODEL: &str = "glm-5.3-flash";

/// Der Titelgenerator darf GLM-5.3-Flash gezielt nutzen, ohne die zentrale
/// Modellwahl der anderen Twitch-Bot-Anwendungsfaelle zu veraendern. Fehlt der
/// Z.ai-Key, bleibt Fireworks der sichere Rueckfall.
fn title_endpoint() -> tb_llm::LlmEndpoint {
    if let Ok(key) = std::env::var("ZAI_API_KEY") {
        if !key.trim().is_empty() {
            return tb_llm::LlmEndpoint {
                provider: "zai",
                base_url: std::env::var("ZAI_BASE_URL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| ZAI_TITLE_BASE_URL.to_string()),
                model: ZAI_TITLE_MODEL.to_string(),
                api_key: Some(key),
            };
        }
    }
    tb_llm::endpoint_for(USE_CASE)
}

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
            .denken_aus()
            .strip_think()
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
fn endpunkt(base_url: &str, api_key: &str, _model: &str) -> tb_llm::LlmEndpoint {
    let mut endpoint = tb_llm::endpoint_for(USE_CASE);
    endpoint.base_url = base_url.to_string();
    endpoint.api_key = Some(api_key.to_string());
    endpoint
}

/// Kern des personalisierten Dashboard-Generators mit injizierbarem Endpoint.
/// Ein eigener Wrapper bleibt darunter fuer den bestehenden !title-Command.
#[allow(clippy::too_many_arguments)]
pub async fn generate_title_personalized_with(
    base_url: &str,
    api_key: &str,
    model: &str,
    keywords: &str,
    style_preference: &str,
    feedback: &[PromptFeedbackItem],
    title_history: &[PromptHistoryItem],
    knowledge_titles: &[PromptKnowledgeItem],
    rank_display: Option<&str>,
    live_state: Option<&PromptLiveState>,
) -> Result<TitleResult, GenerateTitleError> {
    let titles: Vec<&str> = title_history.iter().map(|h| h.title.as_str()).collect();
    let ratio = emoji_ratio(&titles);
    let prompt = build_personalized_title_prompt_with_feedback(
        keywords,
        PromptStyle {
            preference: style_preference,
            emoji_ratio: ratio,
        },
        feedback,
        title_history,
        knowledge_titles,
        rank_display,
        live_state,
    );
    // Kreativer als der alte konservative Generator, aber mit wenig Output:
    // Die Titel selbst sind kurz und eine Analyse pro Klick waere nur Tokenlast.
    let content = titel_completion(
        &endpunkt(base_url, api_key, model),
        "title",
        &prompt,
        0.68,
        900,
    )
    .await
    .map_err(GenerateTitleError::Http)?;
    let result = sanitize_title_result(parse_title_response(&content), keywords, rank_display);
    if result.primary.is_empty() {
        return Err(GenerateTitleError::Http(
            "KI returned no usable title".to_string(),
        ));
    }
    Ok(result)
}

/// Rueckwaertskompatibler Test-/Chat-Einstieg ohne explizite Stilpraeferenz.
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
    generate_title_personalized_with(
        base_url,
        api_key,
        model,
        keywords,
        "",
        &[],
        title_history,
        knowledge_titles,
        rank_display,
        live_state,
    )
    .await
}

/// Personalisierter Dashboard-Einstieg: Rate-Limit + zentraler LLM-Endpunkt.
#[allow(clippy::too_many_arguments)]
pub async fn generate_title_personalized(
    rate_limiter: &TitleRateLimiter,
    streamer_id: &str,
    keywords: &str,
    style_preference: &str,
    feedback: &[PromptFeedbackItem],
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
    let endpoint = title_endpoint();
    let api_key = endpoint
        .api_key
        .as_deref()
        .ok_or(GenerateTitleError::NoApiKey)?;
    generate_title_personalized_with(
        &endpoint.base_url,
        api_key,
        &endpoint.model,
        keywords,
        style_preference,
        feedback,
        title_history,
        knowledge_titles,
        rank_display,
        live_state,
    )
    .await
}

/// Bestehender !title-Command ohne gespeicherte Dashboard-Praeferenz.
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
    generate_title_personalized(
        rate_limiter,
        streamer_id,
        keywords,
        "",
        &[],
        title_history,
        knowledge_titles,
        rank_display,
        live_state,
        source,
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

/// Wöchentliche Insight-Analyse via KI (Python `generate_insight`).
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
            "ZAI_API_KEY",
            "ZAI_BASE_URL",
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
        let endpoint = tb_llm::endpoint_for("title_ai");
        assert_eq!(endpoint.provider, "fireworks");
        assert!(endpoint.api_key.is_none());

        std::env::set_var("FIREWORK_API_KEY", "fireworks-key");
        std::env::set_var("TB_LLM_PROVIDER_TITLE_AI", "llm");
        let endpoint = tb_llm::endpoint_for("title_ai");
        assert_eq!(endpoint.provider, "fireworks");
        assert_eq!(endpoint.model, tb_llm::selection::FIREWORKS_DEFAULT_MODEL);
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
        assert!(p.contains("Keine Emojis verwenden"));
        assert!(p.contains("HEUTIGER INPUT:\nranked grind"));
        assert!(p.contains("  (keine Daten)"));
        assert!(p.contains("Gültige Deadlock-Ränge: Obscurus, Seeker"));

        let p2 = build_title_prompt("x", &[], &[], None, 0.5, None);
        assert!(p2.contains("Maximal einen Emoji"));
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
        // Der HTTP-Kontext enthält keinen Partystatus: keine Solo-Behauptung.
        assert!(p.contains("Aktuelle Live-Daten: Hero=Haze, Party=unbekannt"));
        // Top-Referenzen sind nach Perf sortiert: "Stark" (2.0) vor "Schwach" (0.5).
        assert!(p.find("Stark").unwrap() < p.find("Schwach").unwrap());
    }

    #[test]
    fn prompt_gibt_spiel_hook_und_praezises_format_vor() {
        let p = build_title_prompt("ranked solo", &[], &[], None, 0.0, None);
        assert!(p.contains("Deadlock-Streamer"));
        assert!(p.contains("Zielbereich 45–105 Zeichen"));
        assert!(p.contains("echten Hook"));
        assert!(p.contains("Verbotene Slop-Muster"));
    }

    #[test]
    fn prompt_priorisiert_human_feedback_und_edits() {
        let feedback = vec![PromptFeedbackItem {
            proposed_title: "Ranked Grind".into(),
            feedback: "edited".into(),
            selected_title: None,
            edited_title: Some("Drei Lanes, zwei Pläne, eine sehr schlechte Idee".into()),
        }];
        let p = build_personalized_title_prompt_with_feedback(
            "duo",
            PromptStyle {
                preference: "trocken, keine Emojis",
                emoji_ratio: 0.0,
            },
            &feedback,
            &[],
            &[],
            None,
            None,
        );
        assert!(p.contains("HUMAN-FEEDBACK"));
        assert!(p.contains("SEHR STARKES SIGNAL"));
        assert!(p.contains("Drei Lanes, zwei Pläne"));
        assert!(p.contains("trocken, keine Emojis"));
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
            "deepseek-v4-flash",
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
        let err = generate_title_with(
            &server.uri(),
            "k",
            "deepseek-v4-flash",
            "x",
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
            "deepseek-v4-flash",
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
            "deepseek-v4-flash",
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
            "deepseek-v4-flash",
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
    async fn titel_completion_schaltet_das_denken_ab() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content":
                    "{\"strengths\":\"a\",\"weaknesses\":\"b\",\"patterns\":\"c\",\"recommendations\":[\"x\"]}"}}]
            })))
            .mount(&server)
            .await;

        let history = vec![InsightHistoryItem {
            title: "T".into(),
            relative_perf: 1.2,
            engagement_rate: 0.05,
        }];
        generate_insight_with(
            &server.uri(),
            "k",
            "deepseek-v4-flash",
            &history,
            "Zeitraum",
        )
        .await
        .expect("Insight");

        let requests = server.received_requests().await.expect("Requests");
        assert_eq!(requests.len(), 1);
        let body = String::from_utf8(requests[0].body.clone()).expect("utf8");
        assert!(
            body.contains("\"reasoning_effort\":\"none\""),
            "Body: {body}"
        );
    }

    #[tokio::test]
    async fn generate_insight_with_leer_und_parsefehler() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // Leere History → None ohne HTTP-Call.
        assert!(
            generate_insight_with("http://unused", "k", "deepseek-v4-flash", &[], "p")
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
            generate_insight_with(&server.uri(), "k", "deepseek-v4-flash", &history, "p")
                .await
                .is_none()
        );
    }
}
