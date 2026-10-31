//! Legacy-kompatible Engagement-Fassade für den zentralen Fireworks-Connector
//! (ursprünglich Port von `bot/engagement/llm_chat.py`).
//!
//! Slice 3a (hier): die I/O-freien Teile — Nachrichten-/Antwort-Typen,
//! Text-Sanitizing, die Antwort-Nachbearbeitung ([`process_response_text`]:
//! `<think>`-Strip → Silent-Marker → Sanitize) und der Baseline-System-Prompt
//! ([`build_baseline_system_prompt`] + [`SOUL`]). Der reqwest-Client folgt in 3b.

use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;

/// Marker, mit dem das Modell bewusstes Schweigen signalisiert.
pub const SILENT_MARKER: &str = "<silent>";
const MAX_CHAT_TEXT_CHARS: usize = 120;

/// Eine Chat-Nachricht für den Modell-Call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub name: Option<String>,
}

/// Antwort des Modells inkl. Telemetrie. `text == None` = bewusstes Schweigen.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub text: Option<String>,
    /// Modelltext nach Think-/Silent-Behandlung, aber vor dem Ausgabefilter.
    pub raw_text: Option<String>,
    pub model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub latency_ms: i64,
}

/// API-Key fehlt oder Endpunkt nicht erreichbar (Python `LLMProviderUnavailable`).
#[derive(Debug, Clone)]
pub struct LlmProviderUnavailable(pub String);

impl std::fmt::Display for LlmProviderUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LLM-Provider nicht verfügbar: {}", self.0)
    }
}

impl std::error::Error for LlmProviderUnavailable {}

/// Entfernt KI-`<think>…</think>`-Reasoning-Blöcke (case-insensitive,
/// über Zeilen hinweg, non-greedy). Auch von [`crate::soul_store`] genutzt.
pub(crate) fn strip_think(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?is)<think>.*?</think>").expect("valide Regex"));
    re.replace_all(text, "").into_owned()
}

/// Nachbearbeitung der Modell-Rohantwort: `<think>`-Strip → Silent-Erkennung →
/// Sanitize. `None` = Schweigen (leer oder `<silent>`-Marker oder leer nach
/// Sanitize). Reiner Port der Logik aus `generate` (Zeilen 123–137).
pub fn process_response_text(raw_text: &str, max_answer_len: usize) -> Option<String> {
    process_text(raw_text, max_answer_len, sanitize_chat_text)
}

/// Nachbearbeitung für längere Antworten außerhalb des Twitch-Chats.
pub fn process_answer_text(raw_text: &str, max_answer_len: usize) -> Option<String> {
    process_text(raw_text, max_answer_len, sanitize_answer_text)
}

fn process_text(
    raw_text: &str,
    max_answer_len: usize,
    sanitize: fn(&str, usize) -> Option<String>,
) -> Option<String> {
    prepare_response_text(raw_text).and_then(|text| sanitize(&text, max_answer_len))
}

fn prepare_response_text(raw_text: &str) -> Option<String> {
    let without_think = strip_think(raw_text.trim());
    let without_think = without_think.trim();
    if without_think.is_empty() || without_think.to_lowercase().contains(SILENT_MARKER) {
        None
    } else {
        Some(without_think.to_string())
    }
}

/// Säubert Bot-Text vor dem Senden an Twitch. `None` verwirft leere oder mehr
/// als 120 Zeichen lange Ergebnisse; zu lange Nachrichten werden nie gekürzt.
pub fn sanitize_chat_text(text: &str, max_len: usize) -> Option<String> {
    let max_len = if max_len == 0 {
        MAX_CHAT_TEXT_CHARS
    } else {
        max_len.min(MAX_CHAT_TEXT_CHARS)
    };
    sanitize_text(text, max_len)
}

fn sanitize_answer_text(text: &str, max_len: usize) -> Option<String> {
    sanitize_text(text, max_len)
}

fn sanitize_text(text: &str, max_len: usize) -> Option<String> {
    let without_emoji_and_bang = strip_emoji_and_bang(text);
    let mut transformed = String::with_capacity(text.len());
    for c in without_emoji_and_bang.chars() {
        match c {
            '—' => {
                while transformed.chars().last().is_some_and(char::is_whitespace) {
                    transformed.pop();
                }
                transformed.push_str(", ");
            }
            '–' => transformed.push(' '),
            '„' | '“' | '”' | '»' | '«' => transformed.push('"'),
            '‚' | '‘' | '’' => transformed.push('\''),
            _ => transformed.push(c),
        }
    }

    let mut cleaned = transformed.split_whitespace().collect::<Vec<_>>().join(" ");
    while cleaned.starts_with('/') || cleaned.starts_with('.') {
        cleaned = cleaned[1..].trim_start().to_string();
    }
    cleaned = cleaned.replace("@everyone", "everyone");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned.chars().count() > max_len {
        None
    } else {
        Some(cleaned.to_string())
    }
}

/// Grund, warum ein erzeugter Testmodus-Text nicht sendefähig wäre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestModeRejectReason {
    Dash,
    Quote,
    List,
    RepeatedPunctuation,
    TooLong,
    OfferOrLink,
    Empty,
}

impl TestModeRejectReason {
    /// Stabiler Wert für die Persistenz.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dash => "dash",
            Self::Quote => "quote",
            Self::List => "list",
            Self::RepeatedPunctuation => "repeated_punctuation",
            Self::TooLong => "too_long",
            Self::OfferOrLink => "offer_or_link",
            Self::Empty => "empty",
        }
    }
}

/// Prüft Testmodus-Output. Anders als der bestehende Sendefilter werden
/// auffällige Texte nicht umgeschrieben, sondern mit einem stabilen Grund
/// abgelehnt.
pub fn sanitize_test_mode_text(text: &str) -> Result<String, TestModeRejectReason> {
    let cleaned = strip_emoji_and_bang(text);

    if cleaned.contains(['—', '–']) {
        return Err(TestModeRejectReason::Dash);
    }
    if cleaned
        .chars()
        .any(|c| matches!(c, '"' | '„' | '“' | '”' | '»' | '«' | '‚' | '‘'))
    {
        return Err(TestModeRejectReason::Quote);
    }
    // Ein Apostroph zwischen zwei Buchstaben ist deutsche Umgangssprache
    // ("hab's"), kein Anfuehrungszeichen. Nur freistehende Apostrophe zitieren
    // wirklich. Beides gleich zu behandeln haette normale Chat-Schreibe
    // verworfen und die Messung Richtung "Bot faellt auf" verschoben.
    static LOOSE_APOSTROPHE_RE: OnceLock<Regex> = OnceLock::new();
    let loose_apostrophe_re = LOOSE_APOSTROPHE_RE.get_or_init(|| {
        Regex::new(r"(?:^|[^\p{L}])['’]|['’](?:[^\p{L}]|$)")
            .expect("statische Apostroph-Regex ist gültig")
    });
    if loose_apostrophe_re.is_match(&cleaned) {
        return Err(TestModeRejectReason::Quote);
    }

    static LIST_RE: OnceLock<Regex> = OnceLock::new();
    let list_re = LIST_RE.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:[-*•]|\d+\.)").expect("statische Listen-Regex ist gültig")
    });
    if list_re.is_match(&cleaned) {
        return Err(TestModeRejectReason::List);
    }

    let lower = cleaned.to_lowercase();
    let has_offer_word = [
        "discord",
        "community",
        "partner",
        "netzwerk",
        "dashboard",
        "website",
    ]
    .iter()
    .any(|word| lower.contains(word));
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let url_re = URL_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:https?://|www\.|(?:[a-z0-9-]+\.)+(?:gg|com|de)\b)")
            .expect("statische URL-Regex ist gültig")
    });
    if has_offer_word || url_re.is_match(&cleaned) {
        return Err(TestModeRejectReason::OfferOrLink);
    }

    static PUNCTUATION_RE: OnceLock<Regex> = OnceLock::new();
    let punctuation_re = PUNCTUATION_RE
        .get_or_init(|| Regex::new(r"\p{P}{2,}").expect("statische Satzzeichen-Regex ist gültig"));
    // `...` und `:)` sind die beiden Ausnahmen: die Auslassungspunkte, weil sie
    // echter Chat-Rhythmus sind, und `:)`, weil der Stilvertrag es als einziges
    // Emoticon ausdruecklich erlaubt.
    if punctuation_re
        .find_iter(&cleaned)
        .any(|run| !matches!(run.as_str(), "..." | ":)"))
    {
        return Err(TestModeRejectReason::RepeatedPunctuation);
    }
    if cleaned.chars().count() > MAX_CHAT_TEXT_CHARS {
        return Err(TestModeRejectReason::TooLong);
    }

    let cleaned = normalize_chat_text(&cleaned);
    if cleaned.is_empty() {
        Err(TestModeRejectReason::Empty)
    } else {
        Ok(cleaned)
    }
}

fn strip_emoji_and_bang(text: &str) -> String {
    let without_emoji: Vec<char> = text.chars().filter(|c| !is_emoji_component(*c)).collect();
    let mut cleaned = String::with_capacity(text.len());
    let mut chars = without_emoji.into_iter().peekable();
    while let Some(c) = chars.next() {
        // `!` fällt weg, außer als Command-Präfix direkt vor Alphanumerik
        // (`!clip`); so verschwindet auch Hype-Spam wie `wow!!` komplett.
        if c != '!' || chars.peek().is_some_and(|next| next.is_alphanumeric()) {
            cleaned.push(c);
        }
    }
    cleaned
}

fn normalize_chat_text(text: &str) -> String {
    let mut cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    while cleaned.starts_with('/') || cleaned.starts_with('.') {
        cleaned = cleaned[1..].trim_start().to_string();
    }
    cleaned.replace("@everyone", "everyone").trim().to_string()
}

fn is_emoji_component(c: char) -> bool {
    // 0x1F000 statt 0x1F300: deckt auch Regional-Indicator-Flaggen (U+1F1E6..)
    // und Enclosed-Blöcke ab; 0x2B00..=0x2BFF fängt ⭐/⬛-Symbole.
    matches!(
        c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF | 0x200D
    )
}

/// Welche Persönlichkeit der Bot fährt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PersonaMode {
    /// Daily-Spieler mit Meinung zur Meta.
    #[default]
    Veteran,
    /// Neu im Spiel, fragt statt zu erklären.
    Rookie,
}

impl PersonaMode {
    /// Parst `ENGAGEMENT_PERSONA_MODE`; alles Unbekannte bleibt beim Veteranen.
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "rookie" | "neuling" => PersonaMode::Rookie,
            _ => PersonaMode::Veteran,
        }
    }

    /// Aktiver Modus aus der Env.
    pub fn from_env() -> Self {
        std::env::var("ENGAGEMENT_PERSONA_MODE")
            .map(|v| Self::parse(&v))
            .unwrap_or_default()
    }
}

/// Fehler beim Modell-Aufruf. Die Pipeline mappt alles ausser `Unavailable`
/// auf `PROVIDER_ERROR`; die Varianten bleiben getrennt, damit Logs und
/// Aufrufer eine gerissene Zeitgrenze von einem 4xx unterscheiden koennen.
#[derive(Debug)]
pub enum GenerateError {
    /// Kein API-Key gesetzt.
    Unavailable(LlmProviderUnavailable),
    /// Zeitgrenze gerissen.
    Timeout(String),
    /// Verbindung, TLS, Abbruch.
    Transport(String),
    /// Antwort kam, war aber kein Erfolg (non-2xx).
    Http(String),
    /// Antwort kam an, war aber nicht verwertbar (leer, ungueltiger Body).
    Unparsable(String),
}

impl std::fmt::Display for GenerateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerateError::Unavailable(e) => write!(f, "{e}"),
            GenerateError::Timeout(e) => write!(f, "Modell-Aufruf: Zeitgrenze gerissen: {e}"),
            GenerateError::Transport(e) => write!(f, "Modell-Aufruf fehlgeschlagen: {e}"),
            GenerateError::Http(e) => write!(f, "Modell-Aufruf fehlgeschlagen: {e}"),
            GenerateError::Unparsable(e) => write!(f, "Modell-Antwort unbrauchbar: {e}"),
        }
    }
}

impl std::error::Error for GenerateError {}

/// Standard-Anwendungsfall dieses Clients in der gemeinsamen Anbieterauswahl.
const USE_CASE: &str = "engagement";

impl From<tb_llm::LlmError> for GenerateError {
    fn from(error: tb_llm::LlmError) -> Self {
        match error {
            tb_llm::LlmError::Unavailable(detail) => {
                Self::Unavailable(LlmProviderUnavailable(detail))
            }
            tb_llm::LlmError::Timeout(detail) => Self::Timeout(detail),
            tb_llm::LlmError::Transport(detail) => Self::Transport(detail),
            tb_llm::LlmError::Unparsable(detail) => Self::Unparsable(detail),
            http @ tb_llm::LlmError::Http { .. } => Self::Http(http.to_string()),
        }
    }
}

/// Chat-Client der Engagement-Schicht.
///
/// Der HTTP-Weg liegt in [`tb_llm::complete`]; hier bleibt nur, was fachlich
/// zum Engagement gehört: Verlauf falten, Temperatur, Nachbehandlung des Textes
/// und die Fehlerform, die die Aufrufer kennen.
///
/// Anbieter und Modell kommen ausschließlich aus der gemeinsamen Auswahl unter
/// dem jeweiligen Anwendungsfall. Frühere KI-Variablen und Modell-
/// Overrides werden nicht verwendet.
pub struct EngagementLlmClient {
    use_case: &'static str,
    endpoint: tb_llm::LlmEndpoint,
    /// Expliziter Schlüssel oder eine Basis-URL nageln den Endpunkt für Tests
    /// fest. Der Modellparameter bleibt nur aus API-Kompatibilität bestehen und
    /// wird bewusst ignoriert.
    festgenagelt: bool,
    timeout: Duration,
}

impl EngagementLlmClient {
    /// Baut den Client. Ohne Test-Endpunkt entscheidet ausschließlich die
    /// gemeinsame Auswahl ([`tb_llm::endpoint_for`]).
    pub fn new(
        api_key: Option<String>,
        base_url: Option<String>,
        model: Option<String>,
        timeout: Option<Duration>,
    ) -> Self {
        Self::build(USE_CASE, api_key, base_url, model, timeout)
    }

    /// Wie [`Self::new`] ohne explizite Parameter, aber unter einem eigenen
    /// Anwendungsfall für Logs und Ledger. Anbieter und Modell bleiben gleich.
    pub fn for_use_case(use_case: &'static str, timeout: Option<Duration>) -> Self {
        Self::build(use_case, None, None, None, timeout)
    }

    fn build(
        use_case: &'static str,
        api_key: Option<String>,
        base_url: Option<String>,
        _model: Option<String>,
        timeout: Option<Duration>,
    ) -> Self {
        let mut endpoint = tb_llm::endpoint_for(use_case);
        let mut festgenagelt = false;
        if let Some(api_key) = api_key.filter(|k| !k.is_empty()) {
            endpoint.api_key = Some(api_key);
            festgenagelt = true;
        }
        if let Some(base_url) = base_url.filter(|u| !u.is_empty()) {
            endpoint.base_url = base_url;
            festgenagelt = true;
        }
        Self {
            use_case,
            endpoint,
            festgenagelt,
            timeout: timeout.unwrap_or_else(|| Duration::from_secs(30)),
        }
    }

    /// Das gelockte Modell (für Logging/Persistenz).
    pub fn model(&self) -> &str {
        &self.endpoint.model
    }

    /// Generiert eine Chat-Antwort. `messages = [system] + history` (Sprecher
    /// wird in den Content gefaltet, s. Insight), `temperature=0.7`. Die
    /// Rohantwort läuft durch [`process_response_text`]; `text == None` =
    /// Schweigen.
    pub async fn generate(
        &self,
        system_prompt: &str,
        history: &[ChatMessage],
        max_output_tokens: i64,
        max_answer_len: usize,
    ) -> Result<ChatResponse, GenerateError> {
        self.generate_with_processor(
            system_prompt,
            history,
            max_output_tokens,
            max_answer_len,
            process_response_text,
        )
        .await
    }

    /// Generiert eine längere Antwort außerhalb des Twitch-Chats.
    pub async fn generate_answer(
        &self,
        system_prompt: &str,
        history: &[ChatMessage],
        max_output_tokens: i64,
        max_answer_len: usize,
    ) -> Result<ChatResponse, GenerateError> {
        self.generate_with_processor(
            system_prompt,
            history,
            max_output_tokens,
            max_answer_len,
            process_answer_text,
        )
        .await
    }

    async fn generate_with_processor(
        &self,
        system_prompt: &str,
        history: &[ChatMessage],
        max_output_tokens: i64,
        max_answer_len: usize,
        process: fn(&str, usize) -> Option<String>,
    ) -> Result<ChatResponse, GenerateError> {
        let messages = history
            .iter()
            .map(|turn| {
                // Sprecher in den Content falten statt ein anbieterspezifisches
                // `name`-Feld zu verwenden.
                let content = match &turn.name {
                    Some(name) => format!("{name}: {}", turn.content),
                    None => turn.content.clone(),
                };
                tb_llm::Message {
                    role: turn.role.clone(),
                    content,
                }
            })
            .collect();

        // Verbrauch ins gemeinsame Usage-Ledger (Parität zu Pythons
        // `llm_usage.record(...)`-Seiteneffekt). Tokens auch bei `<silent>`
        // verbuchen, denn verbraucht sind sie ohnehin.
        let response = self
            .call(
                tb_llm::Request::history(messages)
                    .system(system_prompt)
                    .max_tokens(max_output_tokens)
                    .temperature(0.7),
            )
            .await?;

        let review_text = prepare_response_text(&response.text);
        let text = process(&response.text, max_answer_len);
        Ok(ChatResponse {
            text,
            raw_text: review_text,
            model: response.model,
            prompt_tokens: response.prompt_tokens,
            completion_tokens: response.completion_tokens,
            latency_ms: response.latency_ms,
        })
    }

    /// Roher Completion-Call (system + user) → getrimmter Antwort-Text OHNE
    /// [`process_response_text`] — für Jobs wie die Soul-Reflexion.
    ///
    /// Fachliche Aufrufer geben ihren Zweck über
    /// [`Self::raw_completion_tracked`] ausdrücklich an.
    pub async fn raw_completion(
        &self,
        system: &str,
        user: &str,
        max_output_tokens: i64,
        temperature: f64,
    ) -> Result<String, GenerateError> {
        self.raw_completion_with_model(
            system,
            user,
            max_output_tokens,
            temperature,
            "raw_completion",
        )
        .await
        .map(|(text, _)| text)
    }

    /// Wie [`Self::raw_completion`], liefert zusätzlich das tatsächlich
    /// aufgelöste Modell für Herkunftsangaben und Persistenz.
    pub async fn raw_completion_with_model(
        &self,
        system: &str,
        user: &str,
        max_output_tokens: i64,
        temperature: f64,
        purpose: &str,
    ) -> Result<(String, String), GenerateError> {
        let response = self
            .call(
                tb_llm::Request::simple(system, user)
                    .max_tokens(max_output_tokens)
                    .temperature(temperature)
                    .ledger_purpose(purpose),
            )
            .await?;
        Ok((response.text, response.model))
    }

    /// Wie [`Self::raw_completion`], verbucht aber zusätzlich den echten
    /// Token-Verbrauch im geteilten Usage-Ledger unter dem gegebenen `purpose`
    /// (z. B. `purpose="chat-deep-analysis"`).
    pub async fn raw_completion_tracked(
        &self,
        system: &str,
        user: &str,
        max_output_tokens: i64,
        temperature: f64,
        purpose: &str,
    ) -> Result<String, GenerateError> {
        let response = self
            .call(
                tb_llm::Request::simple(system, user)
                    .max_tokens(max_output_tokens)
                    .temperature(temperature)
                    .ledger_purpose(purpose),
            )
            .await?;
        Ok(response.text)
    }

    /// Completion über ein vollständiges `messages`-Array (system + history +
    /// user) → getrimmter Antwort-Text. Für Multi-Turn-Calls wie den
    /// KI-Folgechat.
    pub async fn messages_completion(
        &self,
        messages: serde_json::Value,
        max_output_tokens: i64,
        temperature: f64,
        purpose: &str,
    ) -> Result<String, GenerateError> {
        let response = self
            .call(
                request_from_messages(&messages)
                    .max_tokens(max_output_tokens)
                    .temperature(temperature)
                    .ledger_purpose(purpose),
            )
            .await?;
        Ok(response.text)
    }

    /// Completion über ein vollständiges `messages`-Array ohne `max_tokens`
    /// im Request.
    pub async fn messages_completion_uncapped(
        &self,
        messages: serde_json::Value,
        temperature: f64,
        purpose: &str,
    ) -> Result<String, GenerateError> {
        let response = self
            .call(
                request_from_messages(&messages)
                    .temperature(temperature)
                    .ledger_purpose(purpose),
            )
            .await?;
        Ok(response.text)
    }

    /// Einziger Weg nach draußen: Zeitgrenze dieses Clients an den gemeinsamen
    /// Eingang, Fehler in die Form bringen, die die Aufrufer kennen.
    ///
    /// Ohne explizite Testparameter läuft der Aufruf über die zentrale Kette,
    /// die genau den freigegebenen Fireworks-Endpunkt enthält.
    async fn call(&self, request: tb_llm::Request) -> Result<tb_llm::Response, GenerateError> {
        let request = request.timeout(self.timeout).total_deadline(self.timeout);
        let request = if self.festgenagelt {
            request.endpoint(self.endpoint.clone())
        } else {
            request.failover()
        };
        tb_llm::complete(self.use_case, request)
            .await
            .map_err(GenerateError::from)
    }
}

/// Macht aus einem fertigen `messages`-Array eine Anfrage. Eine führende
/// `system`-Nachricht wandert ins System-Feld: nur so kommt sie auch bei einem
/// Anbieter an, der `system` getrennt erwartet.
fn request_from_messages(messages: &serde_json::Value) -> tb_llm::Request {
    let mut system = None;
    let mut turns = Vec::new();
    for message in messages.as_array().map(Vec::as_slice).unwrap_or_default() {
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("user");
        let content = message
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if role == "system" && system.is_none() && turns.is_empty() {
            system = Some(content.to_string());
            continue;
        }
        turns.push(tb_llm::Message {
            role: role.to_string(),
            content: content.to_string(),
        });
    }
    let request = tb_llm::Request::history(turns);
    match system {
        Some(system) => request.system(system),
        None => request,
    }
}

/// Neutralisiert das geteilte KI-Usage-Ledger für die Lib-Unit-Tests: entfernt
/// den zentralen DSN (und die alte SQLite-Variable) aus der Prozess-Umgebung, sodass
/// der best-effort-`record()` keinen Pool baut und zum No-op wird — KEIN Unit-Test
/// darf die echte zentrale DB anfassen.
///
/// Die eigentliche Ledger-Seiteneffekt-Verifikation läuft bewusst PROZESS-ISOLIERT
/// im Integrationstest `tests/ledger_side_effects.rs` (eigenes Test-Binary, genau ein
/// langlebiges Runtime): der prozessweite `OnceCell`-Pool von tb-llm bindet seine
/// (reaktor-gebundenen) PG-Verbindungen an das Runtime des ersten `record()`; über
/// viele kurzlebige `#[tokio::test]`-Runtimes im selben Prozess hinweg ist er daher
/// NICHT verlässlich nutzbar (Acquire hängt bis zum Timeout, Zeilen gehen verloren).
#[cfg(test)]
pub(crate) fn redirect_ledger_for_tests() {
    std::env::remove_var("TWITCH_ANALYTICS_DSN");
    std::env::remove_var("DATABASE_URL");
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(server: &MockServer) -> EngagementLlmClient {
        EngagementLlmClient::new(Some("test-key".to_string()), Some(server.uri()), None, None)
    }

    /// Serialisiert die Env-Mutationen der Provider-Tests.
    static PROVIDER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_provider_env() {
        for v in [
            "TB_LLM_PROVIDER_DEFAULT",
            "TB_LLM_PROVIDER_ENGAGEMENT",
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
            "FIREWORKS_MODEL",
            "FIREWORK_MODEL",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "MINMAX",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "TB_LLM_MODEL_ENGAGEMENT",
        ] {
            std::env::remove_var(v);
        }
    }

    /// Env-Sperre plus Aufraeumen beim Verlassen, auch bei Panik.
    struct ProviderEnvGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl Drop for ProviderEnvGuard {
        fn drop(&mut self) {
            clear_provider_env();
        }
    }

    fn provider_env() -> ProviderEnvGuard {
        let guard = PROVIDER_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_provider_env();
        ProviderEnvGuard(guard)
    }

    #[test]
    fn fireworks_key_zieht_client_komplett_auf_deepseek() {
        let _g = PROVIDER_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_provider_env();
        std::env::set_var("FIREWORK_API_KEY", "fireworks-key");

        let client = EngagementLlmClient::new(None, None, None, None);
        assert_eq!(client.endpoint.provider, "fireworks");
        assert_eq!(client.endpoint.api_key.as_deref(), Some("fireworks-key"));
        assert!(
            client.endpoint.base_url.contains("fireworks.ai"),
            "falsche Adresse: {}",
            client.endpoint.base_url
        );
        assert!(
            client.model().contains("deepseek"),
            "falsches Modell: {}",
            client.model()
        );
        clear_provider_env();
    }

    /// Das freigegebene Modell ist fest und kann nicht pro Anwendungsfall
    /// umgestellt werden.
    #[test]
    fn modellvariable_des_anwendungsfalls_wird_ignoriert() {
        let _g = PROVIDER_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_provider_env();
        std::env::set_var("FIREWORK_API_KEY", "fireworks-key");
        assert!(EngagementLlmClient::new(None, None, None, None)
            .model()
            .contains("deepseek"));

        std::env::set_var(
            "TB_LLM_MODEL_ENGAGEMENT",
            "accounts/fireworks/models/anders",
        );
        let client = EngagementLlmClient::new(None, None, None, None);
        assert_eq!(client.endpoint.provider, "fireworks");
        assert_eq!(
            client.model(),
            tb_llm::selection::configured_fireworks_model()
        );
        clear_provider_env();
    }

    #[test]
    fn ohne_fireworks_key_gibt_es_keinen_altanbieter_fallback() {
        let _g = PROVIDER_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_provider_env();

        let client = EngagementLlmClient::new(None, None, None, None);
        assert_eq!(client.endpoint.api_key, None);
        assert_eq!(client.endpoint.provider, "fireworks");
        assert_eq!(
            client.model(),
            tb_llm::selection::configured_fireworks_model()
        );
        clear_provider_env();
    }

    #[test]
    fn expliziter_altanbieter_provider_wird_ignoriert() {
        let _g = PROVIDER_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_provider_env();
        std::env::set_var("FIREWORK_API_KEY", "fireworks-key");
        std::env::set_var("TB_LLM_PROVIDER_ENGAGEMENT", "llm");

        let client = EngagementLlmClient::new(None, None, None, None);
        assert_eq!(client.endpoint.api_key.as_deref(), Some("fireworks-key"));
        assert_eq!(client.endpoint.provider, "fireworks");
        clear_provider_env();
    }

    #[test]
    fn sanitize_kurzen_text_und_bestehende_guards() {
        assert_eq!(
            sanitize_chat_text("hallo   welt\nzeile", 120),
            Some("hallo welt zeile".to_string())
        );
        assert_eq!(
            sanitize_chat_text("/me winkt", 120),
            Some("me winkt".to_string())
        );
        assert_eq!(
            sanitize_chat_text("..punkt", 120),
            Some("punkt".to_string())
        );
        assert_eq!(
            sanitize_chat_text("hi @everyone du", 120),
            Some("hi everyone du".to_string())
        );
    }

    #[test]
    fn sanitize_entfernt_emoji_ausrufezeichen_und_gedankenstriche() {
        assert_eq!(
            sanitize_chat_text("wow 😭 das war wild! !clip — echt – stark", 120),
            Some("wow das war wild !clip, echt stark".to_string())
        );
        assert_eq!(
            sanitize_chat_text("ne!! echt jetzt!!!", 120),
            Some("ne echt jetzt".to_string())
        );
        assert_eq!(
            sanitize_chat_text("🇩🇪 sieg ⭐ easy 🀄", 120),
            Some("sieg easy".to_string())
        );
    }

    #[test]
    fn sanitize_begradigt_typografische_anfuehrungszeichen() {
        assert_eq!(
            sanitize_chat_text("„wow“ ‚ok‘ »ja«", 120),
            Some("\"wow\" 'ok' \"ja\"".to_string())
        );
    }

    #[test]
    fn sanitize_verwirft_mehr_als_120_zeichen() {
        assert_eq!(sanitize_chat_text(&"a".repeat(121), 120), None);
        assert_eq!(process_response_text(&"a".repeat(121), 2_000), None);
    }

    #[test]
    fn testmodus_filter_entfernt_emoji_und_ausrufezeichen_wie_bisher() {
        assert_eq!(
            sanitize_test_mode_text("wow 😭 stark! !clip"),
            Ok("wow stark !clip".to_string())
        );
        assert_eq!(
            sanitize_test_mode_text("was?!"),
            Ok("was?".to_string()),
            "Ausrufezeichen werden vor den Verwerfregeln entfernt"
        );
        assert_eq!(
            sanitize_test_mode_text("😭⭐"),
            Err(TestModeRejectReason::Empty)
        );
    }

    /// Der Filter misst Bot-Tells, er darf normale Chat-Schreibe nicht
    /// wegwerfen. Zwei Faelle waren zu streng und haetten die Messung in
    /// Richtung "Bot faellt auf" verfaelscht: `:)` ist im Stilvertrag
    /// ausdruecklich das einzige erlaubte Emoticon, und der Apostroph in
    /// "hab's" ist kein Anfuehrungszeichen, sondern deutsche Umgangssprache.
    #[test]
    fn testmodus_filter_laesst_normale_chat_schreibe_durch() {
        for text in [":) laeuft", "ez :)", "hab's gesehen", "warte...", "ja, ne"] {
            assert!(
                sanitize_test_mode_text(text).is_ok(),
                "{text:?} ist normale Chat-Schreibe und darf nicht verworfen werden: {:?}",
                sanitize_test_mode_text(text)
            );
        }
    }

    #[test]
    fn testmodus_filter_verwirft_bot_tells_mit_stabilem_grund() {
        let cases = [
            ("das — ist wild", TestModeRejectReason::Dash),
            ("das – ist wild", TestModeRejectReason::Dash),
            ("\"wild\"", TestModeRejectReason::Quote),
            ("'wild'", TestModeRejectReason::Quote),
            ("wild ' wild", TestModeRejectReason::Quote),
            ("„wild“", TestModeRejectReason::Quote),
            ("“wild”", TestModeRejectReason::Quote),
            ("»wild«", TestModeRejectReason::Quote),
            ("‚wild‘", TestModeRejectReason::Quote),
            ("’wild’", TestModeRejectReason::Quote),
            ("- erster punkt", TestModeRejectReason::List),
            ("* erster punkt", TestModeRejectReason::List),
            ("• erster punkt", TestModeRejectReason::List),
            ("ok\n  1. zweiter punkt", TestModeRejectReason::List),
            ("was??", TestModeRejectReason::RepeatedPunctuation),
            ("zu....lang", TestModeRejectReason::RepeatedPunctuation),
            (&"a".repeat(121), TestModeRejectReason::TooLong),
            ("komm auf Discord", TestModeRejectReason::OfferOrLink),
            ("unsere Community", TestModeRejectReason::OfferOrLink),
            ("neuer Partner gesucht", TestModeRejectReason::OfferOrLink),
            ("unser Netzwerk", TestModeRejectReason::OfferOrLink),
            ("das Dashboard", TestModeRejectReason::OfferOrLink),
            ("unsere Website", TestModeRejectReason::OfferOrLink),
            ("www.example.org", TestModeRejectReason::OfferOrLink),
            ("https://example.org", TestModeRejectReason::OfferOrLink),
            ("deadlock-deutsch.gg", TestModeRejectReason::OfferOrLink),
            ("example.com", TestModeRejectReason::OfferOrLink),
            ("example.de", TestModeRejectReason::OfferOrLink),
        ];

        for (text, expected) in cases {
            assert_eq!(sanitize_test_mode_text(text), Err(expected), "{text}");
        }
        assert_eq!(
            sanitize_test_mode_text("warte... gleich"),
            Ok("warte... gleich".to_string())
        );
    }

    #[test]
    fn testmodus_grundwerte_sind_db_stabil() {
        assert_eq!(TestModeRejectReason::Dash.as_str(), "dash");
        assert_eq!(TestModeRejectReason::Quote.as_str(), "quote");
        assert_eq!(TestModeRejectReason::List.as_str(), "list");
        assert_eq!(
            TestModeRejectReason::RepeatedPunctuation.as_str(),
            "repeated_punctuation"
        );
        assert_eq!(TestModeRejectReason::TooLong.as_str(), "too_long");
        assert_eq!(TestModeRejectReason::OfferOrLink.as_str(), "offer_or_link");
        assert_eq!(TestModeRejectReason::Empty.as_str(), "empty");
    }

    #[test]
    fn process_silent_und_think() {
        // <think> wird entfernt, danach bleibt <silent> → None.
        assert_eq!(
            process_response_text("<think>hmm</think> <silent>", 480),
            None
        );
        // leer → None.
        assert_eq!(process_response_text("   ", 480), None);
        // echter Text bleibt (think-Block raus).
        assert_eq!(
            process_response_text("<think>reasoning</think> bebop ist mid", 480),
            Some("bebop ist mid".to_string())
        );
        // <silent> case-insensitive.
        assert_eq!(process_response_text("<SILENT>", 480), None);
        assert_eq!(process_answer_text("<SILENT>", 2_000), None);
    }

    #[test]
    fn persona_mode_parst_und_faellt_auf_veteran_zurueck() {
        assert_eq!(PersonaMode::parse("rookie"), PersonaMode::Rookie);
        assert_eq!(PersonaMode::parse(" Neuling "), PersonaMode::Rookie);
        assert_eq!(PersonaMode::parse("veteran"), PersonaMode::Veteran);
        assert_eq!(PersonaMode::parse("quatsch"), PersonaMode::Veteran);
        assert_eq!(PersonaMode::parse(""), PersonaMode::Veteran);
        assert_eq!(PersonaMode::default(), PersonaMode::Veteran);
    }

    fn history() -> Vec<ChatMessage> {
        vec![ChatMessage {
            role: "user".to_string(),
            content: "bebop auf der lane?".to_string(),
            name: Some("chatter1".to_string()),
        }]
    }

    #[tokio::test]
    async fn generate_parst_antwort_und_tokens() {
        redirect_ledger_for_tests();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            // Sprecher in den Content gefaltet:
            .and(body_string_contains("chatter1: bebop auf der lane?"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "klar, bebop ist stark"}}],
                "usage": {"prompt_tokens": 42, "completion_tokens": 7}
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let resp = client
            .generate("system", &history(), 500, 480)
            .await
            .unwrap();
        assert_eq!(resp.text.as_deref(), Some("klar, bebop ist stark"));
        assert_eq!(resp.prompt_tokens, Some(42));
        assert_eq!(resp.completion_tokens, Some(7));
        assert_eq!(resp.model, tb_llm::selection::configured_fireworks_model());
    }

    #[tokio::test]
    async fn generate_answer_laesst_217_zeichen_durch() {
        redirect_ledger_for_tests();
        let server = MockServer::start().await;
        let answer = "a".repeat(217);
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": format!("<think>reasoning</think>{answer}")}}],
                "usage": {"prompt_tokens": 42, "completion_tokens": 60}
            })))
            .mount(&server)
            .await;

        let resp = client_for(&server)
            .generate_answer("system", &history(), 500, 2_000)
            .await
            .unwrap();

        assert_eq!(resp.text.as_deref(), Some(answer.as_str()));
    }

    #[tokio::test]
    async fn generate_silent_marker_gibt_none() {
        redirect_ledger_for_tests();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "<think>nix</think> <silent>"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 1}
            })))
            .mount(&server)
            .await;

        let resp = client_for(&server)
            .generate("system", &history(), 500, 480)
            .await
            .unwrap();
        assert_eq!(resp.text, None);
        assert_eq!(resp.completion_tokens, Some(1)); // Tokens trotzdem da
    }

    // Die Ledger-Seiteneffekte aller Aufrufwege werden prozessisoliert geprüft in
    // `tests/ledger_side_effects.rs` — siehe [`super::redirect_ledger_for_tests`] zur
    // Begründung (geteilter OnceCell-PG-Pool über viele `#[tokio::test]`-Runtimes).

    #[test]
    fn explizites_modell_wird_ignoriert() {
        let _g = PROVIDER_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_provider_env();
        let client = EngagementLlmClient::new(
            Some("fw-key".to_string()),
            Some("http://127.0.0.1:1".to_string()),
            Some("nicht-freigegebenes-modell".to_string()),
            None,
        );
        assert_eq!(
            client.model(),
            tb_llm::selection::configured_fireworks_model()
        );

        clear_provider_env();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn generate_ohne_key_unavailable() {
        // Die Env-Sperre haelt parallele Tests davon ab, waehrenddessen einen
        // Fireworks-Key zu setzen; sonst wird aus "kein Key"
        // ein Transportfehler gegen 127.0.0.1:1.
        let _g = provider_env();
        let client = EngagementLlmClient::new(
            Some(String::new()), // leer → kein Key
            Some("http://127.0.0.1:1".to_string()),
            Some("deepseek-v4-flash".to_string()),
            None,
        );
        match client.generate("s", &[], 500, 480).await {
            Err(GenerateError::Unavailable(_)) => {}
            other => panic!("erwartete Unavailable, war {other:?}"),
        }
    }
}
