//! Der eine Eingang fuer jeden Sprachmodell-Aufruf des Bots.
//!
//! Aufrufer nennen ihren Anwendungsfall und schicken einen [`Request`]; alles
//! andere passiert hier: Fireworks-Auswahl ueber [`crate::selection`],
//! Zeitgrenze, Wiederholung bei 429, Verbuchung im Ledger, `<think>`-Strip und
//! die Einordnung des Fehlers. Wer hier vorbeigeht und selbst HTTP spricht,
//! umgeht damit den Modell-Lock und die Kostenerfassung; deshalb gibt es genau
//! diese eine Tuer.
//! Der einzige Transport ist Fireworks' OpenAI-kompatibles `/chat/completions`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::ledger;
use crate::selection::{endpoint_chain, endpoint_for, LlmEndpoint};

/// Zeitgrenze, wenn der Aufrufer keine nennt.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(240);
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
    /// `None` lässt `max_tokens` im Fireworks-Body weg.
    pub max_tokens: Option<i64>,
    /// `None` laesst `temperature` im Body weg.
    pub temperature: Option<f64>,
    /// Setzt `response_format: {"type": "json_object"}`.
    pub json_object: bool,
    pub timeout: Option<Duration>,
    /// Gesamtfrist ueber die ganze Kette: alle Glieder, alle
    /// 429-Wiederholungen samt Wartezeiten. `None` heisst: Summe der
    /// Einzelfristen der Glieder. Der Engagement-Client setzt das Doppelte
    /// seiner Einzelfrist, damit ein haengendes erstes Glied das zweite nicht
    /// verdraengt und die Antwort trotzdem nicht ewig dauert.
    pub total_deadline: Option<Duration>,
    /// `None` bedeutet: unter dem Namen des Anwendungsfalls verbuchen.
    pub ledger: Option<Ledger>,
    /// Entfernt `<think>`-Bloecke aus dem Antworttext.
    pub strip_think: bool,
    /// Erlaubt den Rueckgriff auf `reasoning_content`, wenn `content` leer
    /// ist. Standard aus: Denktext ist keine Antwort und darf nicht im
    /// Twitch-Chat landen. Nur Aufrufer, die den Text ohnehin selbst parsen
    /// (der Spam-Judge), schalten das ein.
    pub allow_reasoning_content: bool,
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
    pub fn total_deadline(mut self, total: Duration) -> Self {
        self.total_deadline = Some(total);
        self
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
    pub fn allow_reasoning_content(mut self) -> Self {
        self.allow_reasoning_content = true;
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

/// Fehler samt Absender: welcher Anbieter und welches Modell zuletzt
/// geantwortet (oder nicht geantwortet) hat. Der Aufrufer muss den Absender
/// so nicht aus der Kette raten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmFailure {
    pub provider: String,
    pub model: String,
    pub error: LlmError,
}

impl std::fmt::Display for LlmFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}): {}", self.provider, self.model, self.error)
    }
}

impl std::error::Error for LlmFailure {}

/// Fuehrt einen Aufruf fuer einen Anwendungsfall aus.
///
/// Ohne `failover` zaehlt nur der gewaehlte Anbieter. Mit `failover` wird die
/// Kette aus [`endpoint_chain`] abgearbeitet, bis einer eine brauchbare Antwort
/// liefert; der Fehler des letzten Versuchs kommt zurueck.
pub async fn complete(use_case: &str, request: Request) -> Result<Response, LlmError> {
    complete_detailed(use_case, request)
        .await
        .map_err(|failure| failure.error)
}

/// Wie [`complete`], liefert im Fehlerfall aber auch Anbieter und Modell des
/// letzten Versuchs mit. Jeder fehlgeschlagene Versuch wird hier einmal mit
/// `warn!` geloggt (inklusive Anbieter-Body); Aufrufer sollen nicht erneut
/// warnen, sondern hoechstens auf `debug!` ergaenzen.
pub async fn complete_detailed(use_case: &str, request: Request) -> Result<Response, LlmFailure> {
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
    if let Some(endpoint) = chain.iter().find(|endpoint| {
        endpoint.provider != "fireworks"
            || endpoint.model != crate::selection::FIREWORKS_DEFAULT_MODEL
    })
    {
        tracing::warn!(
            use_case,
            provider = endpoint.provider,
            "nicht freigegebener LLM-Anbieter abgewiesen"
        );
        return Err(LlmFailure {
            provider: endpoint.provider.to_string(),
            model: endpoint.model.clone(),
            error: LlmError::Unavailable(
                "der Twitch-Bot darf ausschließlich das freigegebene Fireworks-Modell verwenden"
                    .to_string(),
            ),
        });
    }
    complete_chain(use_case, request, chain).await
}

/// Mindestabstand zwischen zwei Warnungen "kein Anbieter konfiguriert" je
/// Anwendungsfall. Ohne Drossel stuende die Zeile bei jedem Chat-Event im
/// Journal, mit Drossel faellt sie trotzdem auf.
const KEIN_ANBIETER_WARNABSTAND: Duration = Duration::from_secs(300);

/// Ob fuer diesen Anwendungsfall gerade wieder gewarnt werden darf.
fn kein_anbieter_warnung_faellig(use_case: &str, jetzt: Instant) -> bool {
    static ZULETZT: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let mut zuletzt = ZULETZT
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    match zuletzt.get(use_case) {
        Some(letzte) if jetzt.duration_since(*letzte) < KEIN_ANBIETER_WARNABSTAND => false,
        _ => {
            zuletzt.insert(use_case.to_string(), jetzt);
            true
        }
    }
}

/// Arbeitet eine fertige Kette ab. Getrennt von [`complete_detailed`], damit
/// Tests eine Kette vorgeben koennen, ohne an Umgebungsvariablen zu drehen.
async fn complete_chain(
    use_case: &str,
    request: Request,
    chain: Vec<LlmEndpoint>,
) -> Result<Response, LlmFailure> {
    if chain.is_empty() {
        // Die Aufrufer haben ihre eigene Warnung abgegeben, weil der Hub pro
        // Fehlversuch warnt. Eine leere Kette ist auch ein Fehlversuch.
        if kein_anbieter_warnung_faellig(use_case, Instant::now()) {
            tracing::warn!(use_case, "kein LLM-Anbieter konfiguriert");
        }
        return Err(LlmFailure {
            provider: "keiner".to_string(),
            model: String::new(),
            error: LlmError::Unavailable(format!("kein Schluessel fuer {use_case}")),
        });
    }

    let purpose = match &request.ledger {
        Some(Ledger::Off) => None,
        Some(Ledger::Purpose(p)) => Some(p.clone()),
        None => Some(use_case.to_string()),
    };

    // Gesamtfrist: ohne Angabe die Summe der Einzelfristen, also das
    // bisherige Verhalten. Mit Angabe bekommt jedes weitere Glied nur noch,
    // was uebrig ist, und die Kette bricht ab, wenn nichts uebrig ist.
    let einzelfrist = request.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let gesamtfrist = request
        .total_deadline
        .unwrap_or_else(|| einzelfrist.saturating_mul(chain.len() as u32));
    let start = Instant::now();

    let mut last: Option<LlmFailure> = None;
    for endpoint in &chain {
        let verbraucht = start.elapsed();
        if verbraucht >= gesamtfrist {
            tracing::warn!(
                use_case,
                provider = endpoint.provider,
                gesamtfrist_ms = gesamtfrist.as_millis() as u64,
                "LLM-Kette abgebrochen: Gesamtfrist erschoepft"
            );
            // Der Fehler des letzten echten Versuchs bleibt der Absender;
            // nur ohne einen solchen steht das uebersprungene Glied drin.
            last.get_or_insert_with(|| LlmFailure {
                provider: endpoint.provider.to_string(),
                model: endpoint.model.clone(),
                error: LlmError::Timeout(format!(
                    "Gesamtfrist von {} ms erschoepft",
                    gesamtfrist.as_millis()
                )),
            });
            break;
        }
        let frist = einzelfrist.min(gesamtfrist - verbraucht);
        match call_endpoint(endpoint, &request, purpose.as_deref(), frist).await {
            Ok(response) => return Ok(response),
            Err(error) => {
                // Die Warnung traegt Klasse, Status und Body-Laenge; der
                // Anbieter-Body selbst (bis 500 Zeichen) nur auf debug.
                let (status, body_len) = match &error {
                    LlmError::Http { status, body } => (Some(*status), body.chars().count()),
                    _ => (None, 0),
                };
                tracing::warn!(
                    use_case,
                    provider = endpoint.provider,
                    model = %endpoint.model,
                    code = error.code(),
                    status,
                    body_len,
                    "LLM-Aufruf fehlgeschlagen"
                );
                tracing::debug!(
                    use_case,
                    provider = endpoint.provider,
                    fehler = %error,
                    "LLM-Aufruf fehlgeschlagen (Detail)"
                );
                last = Some(LlmFailure {
                    provider: endpoint.provider.to_string(),
                    model: endpoint.model.clone(),
                    error,
                });
            }
        }
    }
    Err(last.expect("Kette ist nicht leer, also gab es mindestens einen Versuch"))
}

/// Ein Anbieter, inklusive Wiederholung bei 429.
async fn call_endpoint(
    endpoint: &LlmEndpoint,
    request: &Request,
    purpose: Option<&str>,
    frist: Duration,
) -> Result<Response, LlmError> {
    let Some(api_key) = endpoint.api_key.as_deref() else {
        return Err(LlmError::Unavailable(format!(
            "kein Schluessel fuer {}",
            endpoint.provider
        )));
    };
    let client = http_client()?;

    // `frist` ist das Budget dieses Glieds inklusive aller 429-Wiederholungen
    // und der Wartezeiten dazwischen; so bleibt die Gesamtfrist der Kette
    // wirklich eine Gesamtfrist.
    let ende = Instant::now() + frist;
    let mut versuch = 0u8;
    let raw = loop {
        let started = Instant::now();
        let rest = ende.saturating_duration_since(started);
        // Die Frist liegt um den ganzen Request (Senden, Warten, Body lesen),
        // nicht im Client: so reicht ein einziger Client fuer alle Fristen.
        let senden = send_openai_compatible(&client, endpoint, api_key, request);
        let outcome = match tokio::time::timeout(rest, senden).await {
            Ok(outcome) => outcome,
            Err(_) => {
                return Err(LlmError::Timeout(format!(
                    "{} antwortete nicht innerhalb von {} ms",
                    endpoint.provider,
                    frist.as_millis()
                )))
            }
        };
        match outcome {
            Ok(payload) => break (payload, started.elapsed().as_millis() as i64),
            Err(RawError::TooManyRequests { retry_after, body })
                if versuch < request.retry_on_429 =>
            {
                versuch += 1;
                let wartezeit = Duration::from_secs(
                    retry_after
                        .unwrap_or(1u64 << (versuch - 1))
                        .min(MAX_RETRY_AFTER_SECS),
                );
                // Reicht die Frist nicht mehr fuer Warten plus Versuch, ist
                // die Wiederholung sinnlos: 429 zurueckgeben statt ueberziehen.
                if Instant::now() + wartezeit >= ende {
                    return Err(LlmError::Http { status: 429, body });
                }
                tokio::time::sleep(wartezeit).await;
            }
            Err(RawError::TooManyRequests { body, .. }) => {
                return Err(LlmError::Http { status: 429, body })
            }
            Err(RawError::Fehler(error)) => return Err(error),
        }
    };
    let (payload, latency_ms) = raw;

    let usage = payload.get("usage");
    let text = extract_openai_text(&payload, request.allow_reasoning_content);
    let prompt_tokens = usage_field(usage, &["prompt_tokens", "tokens_in"]);
    let completion_tokens = usage_field(usage, &["completion_tokens", "tokens_out"]);

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
            return Err(LlmError::Unparsable(
                "keine verwertbare Antwort".to_string(),
            ));
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
    /// Der Body kommt mit, damit er bei erschoepften Wiederholungen im
    /// Fehler steht wie bei jedem anderen 4xx.
    TooManyRequests {
        retry_after: Option<u64>,
        body: String,
    },
    Fehler(LlmError),
}

/// Request-Body fuer OpenAI-kompatible Anbieter. System-Prompt und
/// Nutzerdaten bleiben getrennte Nachrichten: der System-Prompt ist die erste
/// `system`-Nachricht, Nutzerdaten stehen nur in ihren eigenen Rollen.
fn openai_compatible_body(endpoint: &LlmEndpoint, request: &Request) -> Value {
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
    body
}

async fn send_openai_compatible(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
    request: &Request,
) -> Result<Value, RawError> {
    let body = openai_compatible_body(endpoint, request);
    let url = format!(
        "{}/chat/completions",
        endpoint.base_url.trim_end_matches('/')
    );
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
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
        let body = response.text().await.unwrap_or_default();
        return Err(RawError::TooManyRequests {
            retry_after,
            body: kurz(&body),
        });
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(RawError::Fehler(LlmError::Http {
            status: status.as_u16(),
            body: kurz(&body),
        }));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| RawError::Fehler(transport_error(&error)))
}

/// Anbieter-Body auf 500 Zeichen gekuerzt: genug fuer "credit balance is too
/// low", zu wenig fuer eine ganze HTML-Fehlerseite im Log.
fn kurz(body: &str) -> String {
    body.chars().take(500).collect()
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

/// Token-Zahl aus dem `usage`-Block; der erste vorhandene Name gewinnt.
/// Manche Anbieter schreiben `tokens_in`/`tokens_out` statt der
/// OpenAI-Namen, so wie es frueher `title_ai::usage_i64` schon abfing.
fn usage_field(usage: Option<&Value>, names: &[&str]) -> Option<i64> {
    let usage = usage?;
    names
        .iter()
        .find_map(|name| usage.get(name).and_then(Value::as_i64))
        .map(|v| v.max(0))
}

/// Antworttext aus einer OpenAI-kompatiblen Antwort.
///
/// Denkende Modelle legen das Ergebnis gelegentlich in `reasoning_content`
/// statt in `content`. Der Rueckgriff darauf ist Opt-in
/// (`Request::allow_reasoning_content`): fuer Chat-Antworten ist Denktext
/// keine Antwort, sondern Muell im Kanal.
fn extract_openai_text(payload: &Value, allow_reasoning_content: bool) -> String {
    let message = &payload["choices"][0]["message"];
    let content = message["content"].as_str().unwrap_or("").trim();
    if !content.is_empty() || !allow_reasoning_content {
        return content.to_string();
    }
    message["reasoning_content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Entfernt geschlossene `<think>...</think>`-Bloecke. Ein offener Block ohne
/// Schluss bleibt stehen: bei einer abgeschnittenen Antwort steht das JSON
/// oft nach dem offenen Tag, und der Aufrufer (Spam-Judge) schneidet sich das
/// letzte flache JSON-Objekt selbst heraus.
///
/// Bewusst per Regex statt per Kleinschreibungs-Suche: `to_lowercase`
/// veraendert bei manchen Zeichen (z. B. `İ`) die Byte-Laenge, damit stimmen
/// die Indizes nicht mehr und ein Schnitt kann mitten in ein UTF-8-Zeichen
/// fallen. Die Regex arbeitet auf Zeichengrenzen und ist dabei
/// schreibungsunabhaengig.
pub fn strip_think(raw: &str) -> String {
    static THINK: OnceLock<regex::Regex> = OnceLock::new();
    let re = THINK.get_or_init(|| {
        regex::Regex::new(r"(?si)<think>.*?</think>").expect("think-Regex ist konstant")
    });
    re.replace_all(raw, "").trim().to_string()
}

/// Ein HTTP-Client fuer alle Aufrufe (ein Verbindungspool). Die Zeitgrenze
/// liegt nicht im Client, sondern um jeden Request, deshalb braucht es keinen
/// Client je Frist.
fn http_client() -> Result<reqwest::Client, LlmError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                // Keine Gesamtfrist im Client: die legt `call_endpoint` per
                // `tokio::time::timeout` um jeden Request. Nur der
                // Verbindungsaufbau hat eine feste Grenze.
                .connect_timeout(Duration::from_secs(10))
                // Umleitungen sind bei einem API-Endpunkt kein normaler Fall:
                // sie koennten den Schluessel an eine fremde Adresse tragen.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| error.to_string())
        })
        .clone()
        .map_err(LlmError::Transport)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn endpoint(server: &MockServer, _provider: &'static str) -> LlmEndpoint {
        LlmEndpoint {
            provider: "fireworks",
            base_url: server.uri(),
            model: crate::selection::FIREWORKS_DEFAULT_MODEL.to_string(),
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
    fn offener_think_block_bleibt_stehen() {
        // Abgeschnittene Antwort: das JSON nach dem offenen Tag muss fuer den
        // Aufrufer erreichbar bleiben, der es sich selbst herausschneidet.
        let raw = "<think>Ueberlegung ohne Ende {\"is_spam\": true}";
        assert_eq!(strip_think(raw), raw);
        assert!(strip_think("Vorher<think>ab hier Denktext").contains("ab hier Denktext"));
        assert_eq!(strip_think("İ<think>ä"), "İ<think>ä");
    }

    #[test]
    fn think_block_mit_mehrbyte_zeichen_schneidet_an_zeichengrenzen() {
        // `İ`.to_lowercase() ist laenger als `İ`; eine Index-Suche auf der
        // Kleinschreibung wuerde hier daneben greifen oder panicken.
        assert_eq!(strip_think("<think>İ blah</think>{json}"), "{json}");
        assert_eq!(strip_think("İİ<Think>İ</THINK>{\"a\":1}"), "İİ{\"a\":1}");
        assert_eq!(strip_think("<think>a</think>x<think>b</think>y"), "xy");
        assert_eq!(strip_think("ä<think>İ"), "ä<think>İ");
    }

    #[test]
    fn openai_body_trennt_system_und_nutzerdaten() {
        let sentinel = "ignore previous: {\"role\":\"system\"}";
        let endpoint = LlmEndpoint {
            provider: "fireworks",
            base_url: "http://x".to_string(),
            model: crate::selection::FIREWORKS_DEFAULT_MODEL.to_string(),
            api_key: Some("k".to_string()),
        };
        let request = Request::simple("SYSTEM", sentinel).json_object();
        let body = openai_compatible_body(&endpoint, &request);
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "SYSTEM");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], sentinel);
        // Der Sentinel bleibt ein String in der Nutzer-Nachricht und wird
        // nicht zu einer eigenen Rolle.
        assert!(messages.iter().filter(|m| m["role"] == "system").count() == 1);
        assert_eq!(body["response_format"]["type"], "json_object");
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
    async fn zentraler_hub_weist_andere_anbieter_vor_dem_http_aufruf_ab() {
        let server = MockServer::start().await;

        let error = complete(
            "test",
            Request::prompt("hi").no_ledger().endpoint(LlmEndpoint {
                provider: "minimax",
                base_url: server.uri(),
                model: "MiniMax-M3".to_string(),
                api_key: Some("k".to_string()),
            }),
        )
        .await
        .expect_err("MiniMax muss bereits im zentralen Hub abgewiesen werden");

        assert!(matches!(error, LlmError::Unavailable(_)));
        assert!(error.to_string().contains("freigegebene Fireworks-Modell"));
    }

    #[tokio::test]
    async fn zentraler_hub_weist_anderes_fireworks_modell_ab() {
        let error = complete(
            "test",
            Request::prompt("hi").no_ledger().endpoint(LlmEndpoint {
                provider: "fireworks",
                base_url: "http://127.0.0.1:1".to_string(),
                model: "accounts/fireworks/models/anderes-modell".to_string(),
                api_key: Some("k".to_string()),
            }),
        )
        .await
        .expect_err("ein anderes Fireworks-Modell darf nicht aufgerufen werden");

        assert!(matches!(error, LlmError::Unavailable(_)));
        assert!(error.to_string().contains("freigegebene Fireworks-Modell"));
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
    async fn reasoning_content_greift_nur_mit_opt_in() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "", "reasoning_content": "Urteil"}}]
            })))
            .mount(&server)
            .await;

        // Ohne Opt-in bleibt Denktext Denktext: die Antwort ist leer.
        let response = complete(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .endpoint(endpoint(&server, "minimax")),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.text, "");

        // Mit Opt-in (Spam-Judge) kommt der Text durch.
        let response = complete(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .allow_reasoning_content()
                .endpoint(endpoint(&server, "minimax")),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.text, "Urteil");
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
                .endpoint(endpoint(&server, "fireworks")),
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
                model: crate::selection::FIREWORKS_DEFAULT_MODEL.to_string(),
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

    #[tokio::test]
    async fn erschoepfte_429_wiederholung_traegt_body_und_absender() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "0")
                    .set_body_string("rate limit: quota exhausted"),
            )
            .mount(&server)
            .await;

        let failure = complete_detailed(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .retry_on_429(1)
                .endpoint(endpoint(&server, "fireworks")),
        )
        .await
        .expect_err("429 ohne Erfolg");
        assert_eq!(failure.provider, "fireworks");
        assert_eq!(
            failure.model,
            crate::selection::FIREWORKS_DEFAULT_MODEL
        );
        assert_eq!(
            failure.error,
            LlmError::Http {
                status: 429,
                body: "rate limit: quota exhausted".to_string()
            }
        );
    }

    #[test]
    fn token_zahlen_mit_alternativnamen() {
        let usage = json!({"tokens_in": 12, "tokens_out": 3});
        assert_eq!(
            usage_field(Some(&usage), &["prompt_tokens", "tokens_in"]),
            Some(12)
        );
        assert_eq!(
            usage_field(Some(&usage), &["completion_tokens", "tokens_out"]),
            Some(3)
        );
        // Der OpenAI-Name gewinnt, wenn beide da sind.
        let beide = json!({"prompt_tokens": 5, "tokens_in": 99});
        assert_eq!(
            usage_field(Some(&beide), &["prompt_tokens", "tokens_in"]),
            Some(5)
        );
        assert_eq!(usage_field(None, &["prompt_tokens"]), None);
    }

    #[test]
    fn warnung_ohne_anbieter_ist_je_anwendungsfall_gedrosselt() {
        let t0 = Instant::now();
        assert!(kein_anbieter_warnung_faellig("drossel_a", t0));
        assert!(!kein_anbieter_warnung_faellig(
            "drossel_a",
            t0 + Duration::from_secs(10)
        ));
        // Anderer Anwendungsfall, eigene Drossel.
        assert!(kein_anbieter_warnung_faellig("drossel_b", t0));
        // Nach dem Abstand darf wieder gewarnt werden.
        assert!(kein_anbieter_warnung_faellig(
            "drossel_a",
            t0 + KEIN_ANBIETER_WARNABSTAND + Duration::from_secs(1)
        ));
    }

    #[tokio::test]
    async fn leere_kette_liefert_unavailable() {
        let failure = complete_chain("ohne_anbieter", Request::prompt("hi").no_ledger(), vec![])
            .await
            .expect_err("keine Kette");
        assert_eq!(failure.provider, "keiner");
        assert!(matches!(failure.error, LlmError::Unavailable(_)));
    }

    #[tokio::test]
    async fn gesamtfrist_bricht_die_kette_ab() {
        // Erstes Glied braucht laenger als die Gesamtfrist; das zweite darf
        // dann nicht mehr drankommen.
        let langsam = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(3))
                    .set_body_json(json!({"choices": [{"message": {"content": "spaet"}}]})),
            )
            .mount(&langsam)
            .await;
        let schnell = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "schnell"}}]
            })))
            .expect(0)
            .mount(&schnell)
            .await;

        let kette = vec![
            endpoint(&langsam, "fireworks"),
            endpoint(&schnell, "minimax"),
        ];
        let failure = complete_chain(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .timeout(Duration::from_secs(5))
                .total_deadline(Duration::from_millis(150)),
            kette.clone(),
        )
        .await
        .expect_err("Gesamtfrist reisst");
        assert!(matches!(failure.error, LlmError::Timeout(_)), "{failure}");
        assert_eq!(failure.provider, "fireworks");

        // Die interne Kettenfunktion hält auch bei zwei Fireworks-Testendpunkten
        // die Gesamtfrist ein; produktiv liefert `endpoint_chain` nur einen.
        let schnell2 = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "schnell"}}]
            })))
            .mount(&schnell2)
            .await;
        let response = complete_chain(
            "test",
            Request::prompt("hi")
                .no_ledger()
                .timeout(Duration::from_millis(150))
                .accept(|text| text == "schnell"),
            vec![
                endpoint(&langsam, "fireworks"),
                endpoint(&schnell2, "minimax"),
            ],
        )
        .await
        .expect("zweites Glied antwortet");
        assert_eq!(response.text, "schnell");
        assert_eq!(response.provider, "fireworks");
    }
}
