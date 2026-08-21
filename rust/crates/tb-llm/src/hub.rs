//! Der eine Eingang fuer jeden Sprachmodell-Aufruf des Bots.
//!
//! Aufrufer nennen ihren Anwendungsfall und schicken einen [`Request`]; alles
//! andere passiert hier: Anbieterwahl ueber [`crate::selection`], Ausweichkette,
//! Zeitgrenze, Wiederholung bei 429, Verbuchung im Ledger, `<think>`-Strip und
//! die Einordnung des Fehlers. Wer hier vorbeigeht und selbst HTTP spricht,
//! umgeht damit die Anbieterwahl und die Kostenerfassung; deshalb gibt es genau
//! diese eine Tuer.
//!
//! Zwei Transportformen stecken dahinter, nach Anbieter ausgewaehlt:
//! OpenAI-kompatibles `/chat/completions` (Fireworks, MiniMax) und die
//! Anthropic-Messages-API. Der Aufrufer merkt den Unterschied nicht.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

use crate::ledger;
use crate::selection::{endpoint_chain, endpoint_for, LlmEndpoint};

/// Zeitgrenze, wenn der Aufrufer keine nennt.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(240);
/// Versionskopf der Anthropic-Messages-API.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Obergrenze fuer eine vom Anbieter genannte Wartezeit.
const MAX_RETRY_AFTER_SECS: u64 = 5;

/// Eine Nachricht im Verlauf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// `system`, `user` oder `assistant`.
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// Ob und unter welchem Zweck der Verbrauch verbucht wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ledger {
    /// Keine Verbuchung. Fuer Aufrufer, die selbst verbuchen, und fuer Pfade,
    /// die es noch nie getan haben.
    Off,
    /// Verbuchung unter diesem Zweck.
    Purpose(String),
}

/// Praedikat auf dem Antworttext. Liefert es `false`, gilt die Antwort als
/// unbrauchbar und der naechste Anbieter der Kette kommt dran.
pub type Accept = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Ein Aufruf an ein Sprachmodell.
#[derive(Clone, Default)]
pub struct Request {
    pub system: Option<String>,
    pub messages: Vec<Message>,
    /// `None` laesst `max_tokens` im Body weg.
    pub max_tokens: Option<i64>,
    /// `None` laesst `temperature` im Body weg.
    pub temperature: Option<f64>,
    /// Setzt `response_format: {"type": "json_object"}`.
    pub json_object: bool,
    pub timeout: Option<Duration>,
    /// `None` bedeutet: unter dem Namen des Anwendungsfalls verbuchen.
    pub ledger: Option<Ledger>,
    /// Entfernt `<think>`-Bloecke aus dem Antworttext.
    pub strip_think: bool,
    pub accept: Option<Accept>,
    /// Wiederholungen bei HTTP 429.
    pub retry_on_429: u8,
    /// Ausweichkette abarbeiten statt beim ersten Anbieter aufzugeben.
    pub failover: bool,
    /// Fester Endpunkt statt Anbieterwahl. Fuer Tests und fuer Aufrufer, die
    /// aus fachlichen Gruenden an genau einer Adresse haengen.
    pub endpoint: Option<LlmEndpoint>,
}

impl Request {
    /// Der haeufige Fall: ein System-Prompt und eine Nutzernachricht.
    pub fn simple(system: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            system: Some(system.into()),
            messages: vec![Message::user(user)],
            ..Self::default()
        }
    }

    /// Nur eine Nutzernachricht, ohne System-Prompt.
    pub fn prompt(user: impl Into<String>) -> Self {
        Self {
            messages: vec![Message::user(user)],
            ..Self::default()
        }
    }

    /// Vollstaendiger Verlauf, System-Prompt darin oder separat.
    pub fn history(messages: Vec<Message>) -> Self {
        Self {
            messages,
            ..Self::default()
        }
    }

    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }
    pub fn max_tokens(mut self, max_tokens: i64) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }
    pub fn json_object(mut self) -> Self {
        self.json_object = true;
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    pub fn timeout_secs(self, secs: u64) -> Self {
        self.timeout(Duration::from_secs(secs))
    }
    /// Verbucht unter einem anderen Zweck als dem Namen des Anwendungsfalls.
    pub fn ledger_purpose(mut self, purpose: impl Into<String>) -> Self {
        self.ledger = Some(Ledger::Purpose(purpose.into()));
        self
    }
    /// Keine Verbuchung.
    pub fn no_ledger(mut self) -> Self {
        self.ledger = Some(Ledger::Off);
        self
    }
    pub fn strip_think(mut self) -> Self {
        self.strip_think = true;
        self
    }
    pub fn accept(mut self, accept: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.accept = Some(Arc::new(accept));
        self
    }
    pub fn retry_on_429(mut self, retries: u8) -> Self {
        self.retry_on_429 = retries;
        self
    }
    pub fn failover(mut self) -> Self {
        self.failover = true;
        self
    }
    pub fn endpoint(mut self, endpoint: LlmEndpoint) -> Self {
        self.endpoint = Some(endpoint);
        self
    }
}

/// Was zurueckkommt.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub text: String,
    /// Der Anbieter, der geantwortet hat.
    pub provider: String,
    pub model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub latency_ms: i64,
}

/// Warum ein Aufruf nicht geklappt hat. Fein genug, dass die Aufrufer ihre
/// bisherigen Fehlerklassen daraus bilden koennen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// Kein Anbieter mit Schluessel vorhanden.
    Unavailable(String),
    /// Zeitgrenze gerissen.
    Timeout(String),
    /// Antwort kam, war aber kein Erfolg. Der Body haengt dran, damit der
    /// Aufrufer Anbietertexte wie "credit balance is too low" lesen kann.
    Http { status: u16, body: String },
    /// Verbindung, TLS, Abbruch.
    Transport(String),
    /// Antwort kam an, taugte aber nicht: leerer Text oder abgelehnt vom
    /// `accept`-Praedikat des Aufrufers.
    Unparsable(String),
}

impl LlmError {
    /// Kurzname fuer Logs und Metriken.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "unavailable",
            Self::Timeout(_) => "timeout",
            Self::Http { .. } => "http_status",
            Self::Transport(_) => "transport",
            Self::Unparsable(_) => "unparsable",
        }
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "LLM-Anbieter nicht verfuegbar: {e}"),
            Self::Timeout(e) => write!(f, "LLM-Aufruf: Zeitgrenze gerissen: {e}"),
            Self::Http { status, body } if body.is_empty() => {
                write!(f, "LLM-Aufruf fehlgeschlagen: HTTP {status}")
            }
            Self::Http { status, body } => {
                write!(f, "LLM-Aufruf fehlgeschlagen: HTTP {status}: {body}")
            }
            Self::Transport(e) => write!(f, "LLM-Aufruf fehlgeschlagen: {e}"),
            Self::Unparsable(e) => write!(f, "LLM-Antwort unbrauchbar: {e}"),
        }
    }
}

impl std::error::Error for LlmError {}

/// Fuehrt einen Aufruf fuer einen Anwendungsfall aus.
///
/// Ohne `failover` zaehlt nur der gewaehlte Anbieter. Mit `failover` wird die
/// Kette aus [`endpoint_chain`] abgearbeitet, bis einer eine brauchbare Antwort
/// liefert; der Fehler des letzten Versuchs kommt zurueck.
pub async fn complete(use_case: &str, request: Request) -> Result<Response, LlmError> {
    let chain: Vec<LlmEndpoint> = match &request.endpoint {
        Some(endpoint) => vec![endpoint.clone()],
        None if request.failover => endpoint_chain(use_case),
        None => {
            let endpoint = endpoint_for(use_case);
            if endpoint.api_key.is_none() {
                Vec::new()
            } else {
                vec![endpoint]
            }
        }
    };
    if chain.is_empty() {
        return Err(LlmError::Unavailable(format!(
            "kein Schluessel fuer {use_case}"
        )));
    }

    let purpose = match &request.ledger {
        Some(Ledger::Off) => None,
        Some(Ledger::Purpose(p)) => Some(p.clone()),
        None => Some(use_case.to_string()),
    };

    let mut last = LlmError::Unavailable(format!("kein Schluessel fuer {use_case}"));
    for endpoint in &chain {
        match call_endpoint(endpoint, &request, purpose.as_deref()).await {
            Ok(response) => return Ok(response),
            Err(error) => {
                tracing::warn!(
                    use_case,
                    provider = endpoint.provider,
                    model = %endpoint.model,
                    code = error.code(),
                    fehler = %error,
                    "LLM-Aufruf fehlgeschlagen"
                );
                last = error;
            }
        }
    }
    Err(last)
}

/// Ein Anbieter, inklusive Wiederholung bei 429.
async fn call_endpoint(
    endpoint: &LlmEndpoint,
    request: &Request,
    purpose: Option<&str>,
) -> Result<Response, LlmError> {
    let Some(api_key) = endpoint.api_key.as_deref() else {
        return Err(LlmError::Unavailable(format!(
            "kein Schluessel fuer {}",
            endpoint.provider
        )));
    };
    let timeout = request.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let client = http_client(timeout)?;

    let mut versuch = 0u8;
    let raw = loop {
        let started = std::time::Instant::now();
        let outcome = if endpoint.provider == "anthropic" {
            send_anthropic(&client, endpoint, api_key, request).await
        } else {
            send_openai_compatible(&client, endpoint, api_key, request).await
        };
        match outcome {
            Ok(payload) => break (payload, started.elapsed().as_millis() as i64),
            Err(RawError::TooManyRequests { retry_after }) if versuch < request.retry_on_429 => {
                versuch += 1;
                let wartezeit = retry_after
                    .unwrap_or(1u64 << (versuch - 1))
                    .min(MAX_RETRY_AFTER_SECS);
                tokio::time::sleep(Duration::from_secs(wartezeit)).await;
            }
            Err(RawError::TooManyRequests { .. }) => {
                return Err(LlmError::Http {
                    status: 429,
                    body: String::new(),
                })
            }
            Err(RawError::Fehler(error)) => return Err(error),
        }
    };
    let (payload, latency_ms) = raw;

    let (text, prompt_tokens, completion_tokens) = if endpoint.provider == "anthropic" {
        let usage = payload.get("usage");
        (
            extract_anthropic_text(payload.get("content").unwrap_or(&Value::Null)),
            usage_field(usage, "input_tokens"),
            usage_field(usage, "output_tokens"),
        )
    } else {
        let usage = payload.get("usage");
        (
            extract_openai_text(&payload),
            usage_field(usage, "prompt_tokens"),
            usage_field(usage, "completion_tokens"),
        )
    };

    // Verbucht direkt nach der Antwort, damit der Verbrauch auch dann zaehlt,
    // wenn der Text unten am `accept`-Praedikat scheitert. Verbraucht sind die
    // Tokens ohnehin. `record` verschluckt jeden DB-Fehler und kippt den Aufruf
    // nie.
    if let Some(purpose) = purpose {
        ledger::record(
            purpose,
            &endpoint.model,
            prompt_tokens.unwrap_or(0),
            completion_tokens.unwrap_or(0),
            true,
        )
        .await;
    }

    let text = if request.strip_think {
        strip_think(&text)
    } else {
        text
    };

    if let Some(accept) = &request.accept {
        if !accept(&text) {
            return Err(LlmError::Unparsable(format!(
                "{} lieferte keine verwertbare Antwort",
                endpoint.model
            )));
        }
    }

    Ok(Response {
        text,
        provider: endpoint.provider.to_string(),
        model: endpoint.model.clone(),
        prompt_tokens,
        completion_tokens,
        latency_ms,
    })
}

/// Transportfehler mit gesondertem 429-Fall, damit die Wiederholung oben
/// entscheiden kann.
enum RawError {
    TooManyRequests { retry_after: Option<u64> },
    Fehler(LlmError),
}

async fn send_openai_compatible(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
    request: &Request,
) -> Result<Value, RawError> {
    let mut messages: Vec<Value> = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system) = &request.system {
        messages.push(serde_json::json!({"role": "system", "content": system}));
    }
    for message in &request.messages {
        messages.push(serde_json::json!({"role": message.role, "content": message.content}));
    }
    let mut body = serde_json::json!({
        "model": endpoint.model,
        "messages": Value::Array(messages),
    });
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = serde_json::json!(temperature);
    }
    if request.json_object {
        body["response_format"] = serde_json::json!({"type": "json_object"});
    }

    let url = format!("{}/chat/completions", endpoint.base_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await
        .map_err(|error| RawError::Fehler(transport_error(&error)))?;
    finish(response).await
}

async fn send_anthropic(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
    request: &Request,
) -> Result<Value, RawError> {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
        .collect();
    let mut body = serde_json::json!({
        "model": endpoint.model,
        "messages": Value::Array(messages),
    });
    // Anthropic verlangt `max_tokens`. Fehlt es, faellt der Aufruf sonst mit
    // einem 400 auf, das nach einem Konfigurationsfehler aussieht.
    body["max_tokens"] = serde_json::json!(request.max_tokens.unwrap_or(4096));
    if let Some(temperature) = request.temperature {
        body["temperature"] = serde_json::json!(temperature);
    }
    if let Some(system) = &request.system {
        body["system"] = Value::String(system.clone());
    }

    let response = client
        .post(&endpoint.base_url)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|error| RawError::Fehler(transport_error(&error)))?;
    finish(response).await
}

/// Status pruefen, Body lesen, JSON parsen.
async fn finish(response: reqwest::Response) -> Result<Value, RawError> {
    let status = response.status();
    if status.as_u16() == 429 {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        return Err(RawError::TooManyRequests { retry_after });
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(RawError::Fehler(LlmError::Http {
            status: status.as_u16(),
            body: body.chars().take(500).collect(),
        }));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| RawError::Fehler(transport_error(&error)))
}

fn transport_error(error: &reqwest::Error) -> LlmError {
    if error.is_timeout() {
        LlmError::Timeout(error.to_string())
    } else if error.is_decode() {
        LlmError::Unparsable(error.to_string())
    } else {
        LlmError::Transport(error.to_string())
    }
}

fn usage_field(usage: Option<&Value>, name: &str) -> Option<i64> {
    usage
        .and_then(|u| u.get(name))
        .and_then(Value::as_i64)
        .map(|v| v.max(0))
}

/// Antworttext aus einer OpenAI-kompatiblen Antwort.
///
/// Denkende Modelle legen das Ergebnis gelegentlich in `reasoning_content`
/// statt in `content`. Ohne diesen Rueckgriff sieht so eine Antwort aus wie
/// eine leere.
fn extract_openai_text(payload: &Value) -> String {
    let message = &payload["choices"][0]["message"];
    let content = message["content"].as_str().unwrap_or("").trim();
    if !content.is_empty() {
        return content.to_string();
    }
    message["reasoning_content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Text aus dem `content`-Block-Array der Anthropic-Antwort. Text-Bloecke
/// werden mit Zeilenumbruch verbunden.
pub fn extract_anthropic_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.trim().to_string(),
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| {
                    if let Some(s) = item.as_str() {
                        return Some(s.to_string());
                    }
                    if let Some(t) = item.get("text").and_then(Value::as_str) {
                        return Some(t.to_string());
                    }
                    item.get("content")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .filter(|p| !p.is_empty())
                .collect();
            parts.join("\n").trim().to_string()
        }
        _ => String::new(),
    }
}

/// Entfernt `<think>`-Bloecke. Ein offener Block ohne Schluss faellt bis zum
/// Ende weg: alles danach gehoert zum Denktext, nicht zur Antwort.
pub fn strip_think(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    loop {
        let Some(start) = find_ignore_case(rest, "<think>") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let nach_start = &rest[start + "<think>".len()..];
        match find_ignore_case(nach_start, "</think>") {
            Some(ende) => rest = &nach_start[ende + "</think>".len()..],
            None => break,
        }
    }
    out.trim().to_string()
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let haystack_lower = haystack.to_lowercase();
    // Kleinschreibung kann die Byte-Laenge aendern; deshalb ueber die
    // Zeichen-Position zurueckrechnen statt den Index direkt zu uebernehmen.
    let treffer = haystack_lower.find(needle)?;
    let zeichen = haystack_lower[..treffer].chars().count();
    haystack
        .char_indices()
        .nth(zeichen)
        .map(|(i, _)| i)
        .or(Some(haystack.len()))
}

/// HTTP-Clients nach Zeitgrenze zwischengespeichert.
///
/// Ein Client je Aufruf hiesse ein neuer Verbindungspool je Aufruf. Die
/// Zeitgrenze steckt im Client, deshalb ist sie der Schluessel.
fn http_client(timeout: Duration) -> Result<reqwest::Client, LlmError> {
    static CACHE: OnceLock<Mutex<HashMap<u64, reqwest::Client>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = timeout.as_millis() as u64;
    let mut guard = cache.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(client) = guard.get(&key) {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .timeout(timeout)
        // Umleitungen sind bei einem API-Endpunkt kein normaler Fall: sie
        // koennten den Schluessel an eine fremde Adresse tragen.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| LlmError::Transport(error.to_string()))?;
    guard.insert(key, client.clone());
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn endpoint(server: &MockServer, provider: &'static str) -> LlmEndpoint {
        LlmEndpoint {
            provider,
            base_url: if provider == "anthropic" {
                format!("{}/v1/messages", server.uri())
            } else {
                server.uri()
            },
            model: "test-model".to_string(),
            api_key: Some("k".to_string()),
        }
    }

    #[test]
    fn think_block_faellt_weg() {
        assert_eq!(strip_think("<think>egal</think>Antwort"), "Antwort");
        assert_eq!(strip_think("a<THINK>x</THINK>b"), "ab");
        assert_eq!(strip_think("Antwort ohne Block"), "Antwort ohne Block");
    }

    #[test]
    fn offener_think_block_verschluckt_den_rest() {
        // Ein Denktext ohne Schluss ist kein halb brauchbarer Text: was danach
        // kommt, gehoert noch zum Denken.
        assert_eq!(strip_think("Vorher<think>ab hier Denktext"), "Vorher");
    }

    #[test]
    fn anthropic_text_verbindet_bloecke() {
        let content = json!([{"type": "text", "text": "eins"}, {"type": "text", "text": "zwei"}]);
        assert_eq!(extract_anthropic_text(&content), "eins\nzwei");
    }

    #[tokio::test]
    async fn openai_pfad_liefert_text_und_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer k"))
            .and(body_string_contains("\"temperature\":0.25"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": " Hallo "}}],
                "usage": {"prompt_tokens": 7, "completion_tokens": 3}
            })))
            .mount(&server)
            .await;

        let response = complete(
            "test",
            Request::simple("sys", "hi")
                .temperature(0.25)
                .no_ledger()
                .endpoint(endpoint(&server, "fireworks")),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.text, "Hallo");
        assert_eq!(response.prompt_tokens, Some(7));
        assert_eq!(response.completion_tokens, Some(3));
        assert_eq!(response.provider, "fireworks");
    }

    #[tokio::test]
    async fn negative_und_fehlende_token_zahlen_werden_geklemmt() {
        // Ein Anbieter, der Unsinn meldet, darf keine negativen Zeilen ins
        // Ledger schreiben.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "ok"}}],
                "usage": {"prompt_tokens": -7}
            })))
            .mount(&server)
            .await;

        let response = complete(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .endpoint(endpoint(&server, "fireworks")),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.prompt_tokens, Some(0));
        assert_eq!(response.completion_tokens, None);
    }

    #[tokio::test]
    async fn reasoning_content_greift_wenn_content_leer_ist() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "", "reasoning_content": "Urteil"}}]
            })))
            .mount(&server)
            .await;

        let response = complete(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .endpoint(endpoint(&server, "minimax")),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.text, "Urteil");
    }

    #[tokio::test]
    async fn anthropic_pfad_setzt_kopfzeilen_und_system_feld() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "k"))
            .and(header("anthropic-version", ANTHROPIC_VERSION))
            .and(body_string_contains("\"system\":\"sys\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "Antwort"}],
                "usage": {"input_tokens": 4, "output_tokens": 2}
            })))
            .mount(&server)
            .await;

        let response = complete(
            "test",
            Request::simple("sys", "hi")
                .max_tokens(100)
                .no_ledger()
                .endpoint(endpoint(&server, "anthropic")),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.text, "Antwort");
        assert_eq!(response.prompt_tokens, Some(4));
    }

    #[tokio::test]
    async fn fehlerbody_kommt_beim_aufrufer_an() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string("credit balance is too low"))
            .mount(&server)
            .await;

        let error = complete(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .endpoint(endpoint(&server, "anthropic")),
        )
        .await
        .expect_err("Fehler");
        match error {
            LlmError::Http { status, body } => {
                assert_eq!(status, 400);
                assert!(body.contains("credit balance is too low"));
            }
            other => panic!("erwartete Http, war {other:?}"),
        }
    }

    #[tokio::test]
    async fn abgelehnte_antwort_gilt_als_unbrauchbar() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "kein JSON"}}]
            })))
            .mount(&server)
            .await;

        let error = complete(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .accept(|text| text.starts_with('{'))
                .endpoint(endpoint(&server, "fireworks")),
        )
        .await
        .expect_err("Fehler");
        assert!(matches!(error, LlmError::Unparsable(_)));
    }

    #[tokio::test]
    async fn ohne_schluessel_ist_der_aufruf_nicht_verfuegbar() {
        let error = complete(
            "test",
            Request::prompt("hi").no_ledger().endpoint(LlmEndpoint {
                provider: "fireworks",
                base_url: "http://127.0.0.1:1".to_string(),
                model: "m".to_string(),
                api_key: None,
            }),
        )
        .await
        .expect_err("Fehler");
        assert!(matches!(error, LlmError::Unavailable(_)));
    }

    #[tokio::test]
    async fn wiederholung_nach_429_holt_die_antwort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "Titel"}}]
            })))
            .mount(&server)
            .await;

        let response = complete(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .retry_on_429(2)
                .endpoint(endpoint(&server, "fireworks")),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.text, "Titel");
    }
}
