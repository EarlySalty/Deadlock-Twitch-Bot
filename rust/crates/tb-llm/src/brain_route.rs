use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use brain_client::{AsyncBrainClient, ClientError};
pub use brain_client::{
    BotCapability, BotIdentity, BotTask, BotTaskRequest, BotTaskResponse, BotTaskResult,
    TaskFailure, TaskUsage, TwitchTitlePurpose, BOT_TASK_VERSION,
};

use crate::hub::{
    strip_think, transport_error, Ledger, LlmError, LlmFailure, Request, Response, DEFAULT_TIMEOUT,
    MAX_RETRY_AFTER_SECS,
};
use crate::ledger::Completion;

pub const PROVIDER: &str = "brain";
pub const TITLE_AI_USE_CASE: &str = "title_ai";
pub const MAX_TITLE_TOKENS: u32 = 1500;
const UNKNOWN_MODEL: &str = "unknown";
const PROVIDER_RATE_LIMITED: &str = "provider_rate_limited";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    Overloaded {
        retry_after: Option<u64>,
        body: String,
    },
    Rejected {
        status: u16,
        body: String,
    },
    Timeout(String),
    Unreachable(String),
    InvalidRequest(String),
    InvalidResponse(String),
}

#[async_trait]
pub trait BrainTaskTransport: Send + Sync {
    async fn task(&self, request: &BotTaskRequest) -> Result<BotTaskResponse, TransportError>;
}

pub struct BrainClientTransport {
    client: AsyncBrainClient,
}

impl BrainClientTransport {
    pub fn new(endpoint: &str, token: &str, timeout: Option<Duration>) -> Result<Self, String> {
        AsyncBrainClient::new_local(endpoint, token, timeout.unwrap_or(DEFAULT_TIMEOUT))
            .map(|client| Self { client })
            .map_err(|error| match error {
                ClientError::InvalidBaseUrl => {
                    "Brain-Endpunkt muss lokal erreichbar sein und eine gültige Frist haben".into()
                }
                ClientError::InvalidToken => "Brain-Dienstschlüssel fehlt oder ist ungültig".into(),
                other => format!("Brain-Client nicht nutzbar: {other}"),
            })
    }
}

#[async_trait]
impl BrainTaskTransport for BrainClientTransport {
    async fn task(&self, request: &BotTaskRequest) -> Result<BotTaskResponse, TransportError> {
        self.client.task(request).await.map_err(client_error)
    }
}

fn client_error(error: ClientError) -> TransportError {
    match error {
        ClientError::HttpStatus { status, body } if status.as_u16() == 429 => {
            TransportError::Overloaded {
                retry_after: None,
                body,
            }
        }
        ClientError::HttpStatus { status, body } => TransportError::Rejected {
            status: status.as_u16(),
            body,
        },
        ClientError::Http(error) => match transport_error(&error) {
            LlmError::Timeout(detail) => TransportError::Timeout(detail),
            LlmError::Unparsable(detail) => TransportError::InvalidResponse(detail),
            other => TransportError::Unreachable(other.to_string()),
        },
        ClientError::InvalidBaseUrl
        | ClientError::InvalidToken
        | ClientError::InvalidQuery(_)
        | ClientError::RequestTooLarge => TransportError::InvalidRequest(error.to_string()),
        ClientError::Contract(_)
        | ClientError::InvalidResponse
        | ClientError::ResponseTooLarge
        | ClientError::BodyRead(_) => TransportError::InvalidResponse(error.to_string()),
    }
}

#[async_trait]
pub(crate) trait AttemptLedger: Send + Sync {
    async fn begin(&self, purpose: &str) -> Result<i64, LlmError>;
    async fn finish(&self, completion: Completion, deadline: Instant);
}

struct PostgresLedger;

#[async_trait]
impl AttemptLedger for PostgresLedger {
    async fn begin(&self, purpose: &str) -> Result<i64, LlmError> {
        crate::ledger::begin(purpose, UNKNOWN_MODEL, PROVIDER).await
    }

    async fn finish(&self, completion: Completion, deadline: Instant) {
        crate::ledger::journal_completion(&completion);
        let finish = tokio::spawn(crate::ledger::finish(completion));
        if matches!(
            tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), finish).await,
            Ok(Err(_))
        ) {
            tracing::error!(
                "LLM_USAGE_RECOVERY: Abschlussaufgabe abgebrochen, Start bleibt ungeklärt"
            );
        }
    }
}

pub struct BrainRoute {
    transport: Result<Arc<dyn BrainTaskTransport>, String>,
    ledger: Arc<dyn AttemptLedger>,
}

impl BrainRoute {
    pub fn new(transport: Arc<dyn BrainTaskTransport>) -> Self {
        Self {
            transport: Ok(transport),
            ledger: Arc::new(PostgresLedger),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            transport: Err(reason.into()),
            ledger: Arc::new(PostgresLedger),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_ledger(
        transport: Arc<dyn BrainTaskTransport>,
        ledger: Arc<dyn AttemptLedger>,
    ) -> Self {
        Self {
            transport: Ok(transport),
            ledger,
        }
    }

    pub(crate) async fn complete(
        &self,
        use_case: &str,
        request: &Request,
    ) -> Result<Response, LlmFailure> {
        let result = self.run(request).await;
        if let Err(error) = &result {
            tb_observability::warning_budget::warn(
                error.code(),
                "KI-Aufruf fehlgeschlagen; gleiche Fehler werden zusammengefasst",
            );
            tracing::debug!(use_case, provider = PROVIDER, code = error.code(), fehler = %error, "Brain-Aufgabe fehlgeschlagen");
        }
        result.map_err(|error| LlmFailure {
            provider: PROVIDER.to_string(),
            model: String::new(),
            error,
        })
    }

    async fn run(&self, request: &Request) -> Result<Response, LlmError> {
        let transport = self
            .transport
            .as_ref()
            .map_err(|reason| LlmError::Unavailable(reason.clone()))?;
        let broadcaster_id = verified_broadcaster(request)?;
        let (purpose, task) = title_task(request)?;
        let einzelfrist = request.timeout.unwrap_or(DEFAULT_TIMEOUT);
        let frist = request
            .total_deadline
            .map_or(einzelfrist, |total| total.min(einzelfrist));
        let ende = Instant::now() + frist;
        let mut versuch = 0u8;
        loop {
            let started = Instant::now();
            let rest = ende.saturating_duration_since(started);
            if rest.is_zero() {
                return Err(LlmError::Timeout(
                    "Aufruffrist vor dem Versuch abgelaufen".into(),
                ));
            }
            let id = tokio::time::timeout(rest, self.ledger.begin(purpose))
                .await
                .map_err(|_| {
                    LlmError::Timeout("Verbrauchsstart überschreitet Aufruffrist".into())
                })??;
            let body = BotTaskRequest {
                contract_version: BOT_TASK_VERSION.to_string(),
                request_id: uuid::Uuid::new_v4().to_string(),
                identity: BotIdentity::Twitch {
                    broadcaster_id: broadcaster_id.clone(),
                },
                task: task.clone(),
            };
            let outcome = attempt(transport.as_ref(), &body, ende).await;
            self.ledger
                .finish(completion(id, &body.request_id, &outcome, started), ende)
                .await;
            match outcome {
                Ok(answer) => return answer_response(request, answer, started),
                Err(error) => match error.rate_limited() {
                    Some(retry_after) if versuch < request.retry_on_429 => {
                        versuch += 1;
                        let wartezeit = Duration::from_secs(
                            retry_after
                                .unwrap_or(1u64 << (versuch - 1))
                                .min(MAX_RETRY_AFTER_SECS),
                        );
                        if Instant::now() + wartezeit >= ende {
                            return Err(error.llm_error());
                        }
                        tokio::time::sleep(wartezeit).await;
                    }
                    _ => return Err(error.llm_error()),
                },
            }
        }
    }
}

struct Answer {
    text: String,
    model: String,
    usage: TaskUsage,
}

enum Attempt {
    Transport(TransportError),
    Failed(TaskFailure),
}

impl Attempt {
    fn rate_limited(&self) -> Option<Option<u64>> {
        match self {
            Self::Transport(TransportError::Overloaded { retry_after, .. }) => Some(*retry_after),
            Self::Failed(TaskFailure::ProviderRateLimited) => Some(None),
            _ => None,
        }
    }

    fn llm_error(&self) -> LlmError {
        match self {
            Self::Transport(TransportError::Overloaded { body, .. }) => LlmError::Http {
                status: 429,
                body: body.clone(),
            },
            Self::Transport(TransportError::Rejected { status, body }) => LlmError::Http {
                status: *status,
                body: body.clone(),
            },
            Self::Transport(TransportError::Timeout(detail)) => LlmError::Timeout(detail.clone()),
            Self::Transport(TransportError::Unreachable(detail)) => {
                LlmError::Transport(detail.clone())
            }
            Self::Transport(TransportError::InvalidRequest(detail)) => {
                LlmError::Unavailable(format!("Brain-Aufgabe ungültig: {detail}"))
            }
            Self::Transport(TransportError::InvalidResponse(detail)) => {
                LlmError::Unparsable(detail.clone())
            }
            Self::Failed(TaskFailure::ProviderUnavailable) => {
                LlmError::Unavailable("Brain: Modellanbieter nicht verfügbar".into())
            }
            Self::Failed(TaskFailure::ProviderRateLimited) => LlmError::Http {
                status: 429,
                body: PROVIDER_RATE_LIMITED.into(),
            },
            Self::Failed(TaskFailure::Timeout) => {
                LlmError::Timeout("Brain: Modellanbieter antwortete nicht rechtzeitig".into())
            }
            Self::Failed(TaskFailure::InvalidOutput) => {
                LlmError::Unparsable("Brain: Modellantwort unbrauchbar".into())
            }
        }
    }

    fn http_status(&self) -> Option<i32> {
        match self {
            Self::Transport(TransportError::Overloaded { .. }) => Some(429),
            Self::Transport(TransportError::Rejected { status, .. }) => Some(i32::from(*status)),
            Self::Transport(TransportError::InvalidResponse(_)) | Self::Failed(_) => Some(200),
            Self::Transport(_) => None,
        }
    }
}

async fn attempt(
    transport: &dyn BrainTaskTransport,
    body: &BotTaskRequest,
    ende: Instant,
) -> Result<Answer, Attempt> {
    let rest = ende.saturating_duration_since(Instant::now());
    let timeout = || {
        Attempt::Transport(TransportError::Timeout(
            "Brain antwortete nicht innerhalb der Aufruffrist".into(),
        ))
    };
    if rest.is_zero() {
        return Err(timeout());
    }
    let response = tokio::time::timeout(rest, transport.task(body))
        .await
        .map_err(|_| timeout())?
        .map_err(Attempt::Transport)?;
    validated_answer(body, response)
}

fn validated_answer(body: &BotTaskRequest, response: BotTaskResponse) -> Result<Answer, Attempt> {
    let invalid = |detail: &str| Attempt::Transport(TransportError::InvalidResponse(detail.into()));
    if response.contract_version != BOT_TASK_VERSION {
        return Err(invalid("Brain-Antwort hat eine fremde Vertragsversion"));
    }
    if response.request_id != body.request_id {
        return Err(invalid("Brain-Antwort gehört zu einer anderen Anfrage"));
    }
    match response.result {
        BotTaskResult::TwitchTitle { text, model, usage } => {
            if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
                return Err(invalid("Brain-Antwort nennt kein gültiges Modell"));
            }
            Ok(Answer { text, model, usage })
        }
        BotTaskResult::Failed {
            capability: BotCapability::TwitchTitle,
            failure,
        } => Err(Attempt::Failed(failure)),
        BotTaskResult::Failed { .. } => Err(invalid("Brain-Fehler betrifft eine andere Aufgabe")),
        BotTaskResult::DiscordAnswer { .. } | BotTaskResult::CapabilityStatus { .. } => {
            Err(invalid("Brain-Antwort passt nicht zur Titelaufgabe"))
        }
    }
}

fn answer_response(
    request: &Request,
    answer: Answer,
    started: Instant,
) -> Result<Response, LlmError> {
    let text = answer.text.trim().to_string();
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
        provider: PROVIDER.to_string(),
        model: answer.model,
        prompt_tokens: known_tokens(answer.usage.input_tokens),
        completion_tokens: known_tokens(answer.usage.output_tokens),
        latency_ms: started.elapsed().as_millis() as i64,
    })
}

fn known_tokens(value: Option<u64>) -> Option<i64> {
    value.and_then(|tokens| i64::try_from(tokens).ok())
}

fn completion(
    id: i64,
    request_id: &str,
    outcome: &Result<Answer, Attempt>,
    started: Instant,
) -> Completion {
    let latency_ms = started.elapsed().as_millis() as i64;
    match outcome {
        Ok(answer) => {
            let tokens_in = known_tokens(answer.usage.input_tokens);
            let tokens_out = known_tokens(answer.usage.output_tokens);
            Completion {
                id,
                tokens_in,
                tokens_out,
                total: tokens_in
                    .zip(tokens_out)
                    .map(|(input, output)| input.saturating_add(output)),
                request_id: Some(request_id.to_string()),
                success: true,
                error_code: None,
                http_status: Some(200),
                latency_ms,
                model: Some(answer.model.clone()),
                social_accounting: None,
            }
        }
        Err(error) => Completion {
            id,
            tokens_in: None,
            tokens_out: None,
            total: None,
            request_id: Some(request_id.to_string()),
            success: false,
            error_code: Some(error.llm_error().code().to_string()),
            http_status: error.http_status(),
            latency_ms,
            model: None,
            social_accounting: None,
        },
    }
}

fn verified_broadcaster(request: &Request) -> Result<String, LlmError> {
    let id = request.twitch_broadcaster.as_deref().ok_or_else(|| {
        LlmError::Unavailable("Brain-Aufgabe ohne geprüfte Twitch-Kanal-ID".into())
    })?;
    let valid = !id.is_empty()
        && id.len() <= 20
        && !id.starts_with('0')
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && id.parse::<u64>().is_ok();
    if !valid {
        return Err(LlmError::Unavailable(
            "Brain-Aufgabe mit ungültiger Twitch-Kanal-ID".into(),
        ));
    }
    Ok(id.to_string())
}

fn title_task(request: &Request) -> Result<(&'static str, BotTask), LlmError> {
    let unsupported =
        |detail: &str| LlmError::Unavailable(format!("Brain-Aufgabe nicht abbildbar: {detail}"));
    let (ledger_purpose, purpose) = match &request.ledger {
        Some(Ledger::Purpose(purpose)) if purpose == "title" => {
            ("title", TwitchTitlePurpose::Title)
        }
        Some(Ledger::Purpose(purpose)) if purpose == "title-insight" => {
            ("title-insight", TwitchTitlePurpose::Insight)
        }
        _ => return Err(unsupported("Zweck")),
    };
    if request.system.is_some() || request.json_object || request.allow_reasoning_content {
        return Err(unsupported("Anfrageform"));
    }
    let prompt = match request.messages.as_slice() {
        [message] if message.role == "user" && !message.content.trim().is_empty() => {
            message.content.clone()
        }
        _ => return Err(unsupported("Nachrichten")),
    };
    let temperature = request
        .temperature
        .filter(|value| value.is_finite() && (0.0..=2.0).contains(value))
        .map(|value| value as f32)
        .ok_or_else(|| unsupported("Temperatur"))?;
    let max_tokens = request
        .max_tokens
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| (1..=MAX_TITLE_TOKENS).contains(value))
        .ok_or_else(|| unsupported("Tokengrenze"))?;
    Ok((
        ledger_purpose,
        BotTask::TwitchTitle {
            purpose,
            prompt,
            temperature,
            max_tokens,
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrainRouteError {
    UnsupportedUseCase,
    AlreadyInstalled,
}

impl std::fmt::Display for BrainRouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedUseCase => f.write_str("Anwendungsfall hat keine Brain-Route"),
            Self::AlreadyInstalled => f.write_str("Brain-Route ist bereits eingerichtet"),
        }
    }
}

impl std::error::Error for BrainRouteError {}

type Routes = RwLock<HashMap<&'static str, Arc<BrainRoute>>>;

fn routes() -> &'static Routes {
    static ROUTES: OnceLock<Routes> = OnceLock::new();
    ROUTES.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn install(use_case: &'static str, route: BrainRoute) -> Result<(), BrainRouteError> {
    if use_case != TITLE_AI_USE_CASE {
        return Err(BrainRouteError::UnsupportedUseCase);
    }
    let mut routes = routes()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if routes.contains_key(use_case) {
        return Err(BrainRouteError::AlreadyInstalled);
    }
    routes.insert(use_case, Arc::new(route));
    Ok(())
}

pub fn install_title_ai(
    endpoint: Option<&str>,
    token: Option<&str>,
    timeout: Option<Duration>,
) -> Result<(), BrainRouteError> {
    let transport = match (endpoint, token) {
        (Some(endpoint), Some(token)) => BrainClientTransport::new(endpoint, token, timeout),
        (None, _) => Err("Brain-Endpunkt fehlt".to_string()),
        (_, None) => Err("Brain-Dienstschlüssel fehlt".to_string()),
    };
    let route = match transport {
        Ok(transport) => BrainRoute::new(Arc::new(transport)),
        Err(reason) => {
            tracing::error!(
                use_case = TITLE_AI_USE_CASE,
                grund = %reason,
                "Brain-Route für Titel ist aktiv, aber nicht nutzbar; Titelaufrufe schlagen fehl"
            );
            BrainRoute::unavailable(reason)
        }
    };
    install(TITLE_AI_USE_CASE, route)
}

pub fn is_installed(use_case: &str) -> bool {
    installed(use_case).is_some()
}

pub(crate) fn installed(use_case: &str) -> Option<Arc<BrainRoute>> {
    routes()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(use_case)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::complete_routed;
    use crate::selection::LlmEndpoint;
    use serde_json::{json, Value};
    use std::sync::Mutex;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Default)]
    struct RecordingLedger {
        events: Arc<Mutex<Vec<String>>>,
        completions: Mutex<Vec<Completion>>,
        purposes: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AttemptLedger for RecordingLedger {
        async fn begin(&self, purpose: &str) -> Result<i64, LlmError> {
            self.events.lock().unwrap().push("ledger_begin".into());
            self.purposes.lock().unwrap().push(purpose.into());
            Ok(41)
        }

        async fn finish(&self, completion: Completion, _deadline: Instant) {
            self.events.lock().unwrap().push("ledger_finish".into());
            self.completions.lock().unwrap().push(completion);
        }
    }

    fn fireworks(server: &MockServer) -> LlmEndpoint {
        LlmEndpoint {
            provider: "fireworks",
            base_url: server.uri(),
            model: crate::selection::configured_fireworks_model(),
            api_key: Some("fireworks-fixture".into()),
        }
    }

    fn title_request(fireworks_server: &MockServer, purpose: &str) -> Request {
        Request::prompt("Titel-Prompt äöü")
            .temperature(0.8)
            .max_tokens(900)
            .denken_aus()
            .strip_think()
            .retry_on_429(2)
            .ledger_purpose(purpose)
            .endpoint(fireworks(fireworks_server))
    }

    async fn silent_fireworks() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        server
    }

    fn route(brain: &MockServer, ledger: Arc<RecordingLedger>) -> Arc<BrainRoute> {
        let transport = BrainClientTransport::new(&brain.uri(), "brain-fixture", None)
            .expect("lokaler Brain-Endpunkt");
        Arc::new(BrainRoute::with_ledger(Arc::new(transport), ledger))
    }

    fn echo_title(text: &'static str, events: Arc<Mutex<Vec<String>>>) -> impl wiremock::Respond {
        move |request: &wiremock::Request| {
            events.lock().unwrap().push("brain".into());
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            ResponseTemplate::new(200).set_body_json(json!({
                "contract_version": BOT_TASK_VERSION,
                "request_id": body["request_id"],
                "result": {
                    "kind": "twitch_title",
                    "text": text,
                    "model": "accounts/fireworks/models/brain-fixture",
                    "usage": {"input_tokens": 120, "output_tokens": 30}
                }
            }))
        }
    }

    #[tokio::test]
    async fn aktive_route_sendet_genau_den_vertrag_und_nie_an_fireworks() {
        for (purpose, contract_purpose) in [("title", "title"), ("title-insight", "insight")] {
            let fireworks_server = silent_fireworks().await;
            let brain = MockServer::start().await;
            let ledger = Arc::new(RecordingLedger::default());
            Mock::given(method("POST"))
                .and(path("/v1/task"))
                .and(header("authorization", "Bearer brain-fixture"))
                .respond_with(echo_title(
                    "<think>intern</think> {\"primary_title\":\"X\"} ",
                    Arc::clone(&ledger.events),
                ))
                .expect(1)
                .mount(&brain)
                .await;

            let response = complete_routed(
                "title_ai",
                title_request(&fireworks_server, purpose).twitch_broadcaster("123456789"),
                Some(route(&brain, Arc::clone(&ledger))),
            )
            .await
            .expect("Brain-Antwort");

            let received = brain.received_requests().await.expect("Requests");
            let body: Value = serde_json::from_slice(&received[0].body).expect("JSON");
            let request_id = body["request_id"].as_str().expect("request_id").to_string();
            assert!(!request_id.is_empty());
            assert_eq!(
                body,
                json!({
                    "contract_version": "brain.bot_tasks.v1",
                    "request_id": request_id,
                    "identity": {"kind": "twitch", "broadcaster_id": "123456789"},
                    "task": {
                        "kind": "twitch_title",
                        "purpose": contract_purpose,
                        "prompt": "Titel-Prompt äöü",
                        "temperature": 0.8,
                        "max_tokens": 900
                    }
                })
            );
            assert_eq!(response.text, "{\"primary_title\":\"X\"}");
            assert_eq!(response.provider, "brain");
            assert_eq!(response.model, "accounts/fireworks/models/brain-fixture");
            assert_eq!(response.prompt_tokens, Some(120));
            assert_eq!(response.completion_tokens, Some(30));
            assert_eq!(
                *ledger.events.lock().unwrap(),
                vec!["ledger_begin", "brain", "ledger_finish"]
            );
            assert_eq!(*ledger.purposes.lock().unwrap(), vec![purpose.to_string()]);
            let completions = ledger.completions.lock().unwrap();
            assert_eq!(completions[0].id, 41);
            assert_eq!(completions[0].tokens_in, Some(120));
            assert_eq!(completions[0].tokens_out, Some(30));
            assert_eq!(completions[0].total, Some(150));
            assert_eq!(
                completions[0].request_id.as_deref(),
                Some(request_id.as_str())
            );
            assert_eq!(
                completions[0].model.as_deref(),
                Some("accounts/fireworks/models/brain-fixture")
            );
            assert!(completions[0].success);
        }
    }

    #[tokio::test]
    async fn fehlende_token_zahlen_bleiben_unbekannt() {
        let fireworks_server = silent_fireworks().await;
        let brain = MockServer::start().await;
        let ledger = Arc::new(RecordingLedger::default());
        Mock::given(method("POST"))
            .respond_with(|request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                ResponseTemplate::new(200).set_body_json(json!({
                    "contract_version": BOT_TASK_VERSION,
                    "request_id": body["request_id"],
                    "result": {"kind": "twitch_title", "text": "T", "model": "m", "usage": {"output_tokens": 30}}
                }))
            })
            .mount(&brain)
            .await;
        let response = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title").twitch_broadcaster("42"),
            Some(route(&brain, Arc::clone(&ledger))),
        )
        .await
        .expect("Antwort");
        assert_eq!(response.prompt_tokens, None);
        assert_eq!(response.completion_tokens, Some(30));
        let completions = ledger.completions.lock().unwrap();
        assert_eq!(
            (
                completions[0].tokens_in,
                completions[0].tokens_out,
                completions[0].total
            ),
            (None, Some(30), None)
        );
    }

    #[tokio::test]
    async fn brain_fehler_werden_llm_fehler_ohne_fireworks_aufruf() {
        let cases = [
            (
                ResponseTemplate::new(200),
                Some("provider_unavailable"),
                LlmError::Unavailable("Brain: Modellanbieter nicht verfügbar".into()),
            ),
            (
                ResponseTemplate::new(200),
                Some("timeout"),
                LlmError::Timeout("Brain: Modellanbieter antwortete nicht rechtzeitig".into()),
            ),
            (
                ResponseTemplate::new(200),
                Some("invalid_output"),
                LlmError::Unparsable("Brain: Modellantwort unbrauchbar".into()),
            ),
            (
                ResponseTemplate::new(403).set_body_json(
                    json!({"error": {"code": "forbidden", "message": "Kanal nicht freigegeben"}}),
                ),
                None,
                LlmError::Http {
                    status: 403,
                    body: "forbidden".into(),
                },
            ),
            (
                ResponseTemplate::new(502).set_body_string("upstream"),
                None,
                LlmError::Http {
                    status: 502,
                    body: "http_error".into(),
                },
            ),
        ];
        for (template, failure, expected) in cases {
            let fireworks_server = silent_fireworks().await;
            let brain = MockServer::start().await;
            let ledger = Arc::new(RecordingLedger::default());
            let respond = move |request: &wiremock::Request| match failure {
                Some(failure) => {
                    let body: Value = serde_json::from_slice(&request.body).unwrap();
                    ResponseTemplate::new(200).set_body_json(json!({
                        "contract_version": BOT_TASK_VERSION,
                        "request_id": body["request_id"],
                        "result": {"kind": "failed", "capability": "twitch_title", "failure": failure}
                    }))
                }
                None => template.clone(),
            };
            Mock::given(method("POST"))
                .respond_with(respond)
                .expect(1)
                .mount(&brain)
                .await;
            let failure_result = complete_routed(
                "title_ai",
                title_request(&fireworks_server, "title").twitch_broadcaster("42"),
                Some(route(&brain, Arc::clone(&ledger))),
            )
            .await
            .expect_err("Brain-Fehler");
            assert_eq!(failure_result.provider, "brain");
            assert_eq!(failure_result.error, expected);
            let completions = ledger.completions.lock().unwrap();
            assert_eq!(completions.len(), 1);
            assert!(!completions[0].success);
            assert_eq!(completions[0].model, None);
        }
    }

    #[tokio::test]
    async fn fremde_antworten_sind_unbrauchbar() {
        for result in [
            json!({"contract_version": "brain.bot_tasks.v0", "request_id": "SAME", "result": {"kind": "twitch_title", "text": "T", "model": "m", "usage": {}}}),
            json!({"contract_version": BOT_TASK_VERSION, "request_id": "andere", "result": {"kind": "twitch_title", "text": "T", "model": "m", "usage": {}}}),
            json!({"contract_version": BOT_TASK_VERSION, "request_id": "SAME", "result": {"kind": "failed", "capability": "public_guide", "failure": "timeout"}}),
            json!({"contract_version": BOT_TASK_VERSION, "request_id": "SAME", "result": {"kind": "twitch_title", "text": "T", "model": "", "usage": {}}}),
            json!({"contract_version": BOT_TASK_VERSION, "request_id": "SAME", "result": {"kind": "twitch_title", "text": "T", "model": "m", "usage": {}, "extra": 1}}),
        ] {
            let fireworks_server = silent_fireworks().await;
            let brain = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(move |request: &wiremock::Request| {
                    let body: Value = serde_json::from_slice(&request.body).unwrap();
                    let text = result
                        .to_string()
                        .replace("\"SAME\"", &body["request_id"].to_string());
                    ResponseTemplate::new(200).set_body_raw(text, "application/json")
                })
                .mount(&brain)
                .await;
            let failure = complete_routed(
                "title_ai",
                title_request(&fireworks_server, "title").twitch_broadcaster("42"),
                Some(route(&brain, Arc::new(RecordingLedger::default()))),
            )
            .await
            .expect_err("Vertragsbruch");
            assert!(
                matches!(failure.error, LlmError::Unparsable(_)),
                "{failure}"
            );
        }
    }

    #[tokio::test]
    async fn brain_429_nutzt_die_bestehende_wiederholung() {
        let fireworks_server = silent_fireworks().await;
        let brain = MockServer::start().await;
        let ledger = Arc::new(RecordingLedger::default());
        Mock::given(method("POST"))
            .respond_with(overloaded())
            .up_to_n_times(1)
            .mount(&brain)
            .await;
        Mock::given(method("POST"))
            .respond_with(echo_title("Titel", Arc::new(Mutex::new(Vec::new()))))
            .mount(&brain)
            .await;
        let response = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title").twitch_broadcaster("42"),
            Some(route(&brain, Arc::clone(&ledger))),
        )
        .await
        .expect("zweiter Versuch");
        assert_eq!(response.text, "Titel");
        {
            let completions = ledger.completions.lock().unwrap();
            assert_eq!(completions.len(), 2);
            assert_eq!(completions[0].http_status, Some(429));
            assert!(completions[1].success);
        }

        let exhausted = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(overloaded())
            .expect(2)
            .mount(&exhausted)
            .await;
        let failure = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title")
                .retry_on_429(1)
                .twitch_broadcaster("42"),
            Some(route(&exhausted, Arc::new(RecordingLedger::default()))),
        )
        .await
        .expect_err("429 erschöpft");
        assert_eq!(
            failure.error,
            LlmError::Http {
                status: 429,
                body: "overloaded".into()
            }
        );
    }

    fn overloaded() -> ResponseTemplate {
        ResponseTemplate::new(429)
            .set_body_json(json!({"error": {"code": "overloaded", "message": "ausgelastet"}}))
    }

    fn rate_limited() -> impl wiremock::Respond {
        |request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            ResponseTemplate::new(200).set_body_json(json!({
                "contract_version": BOT_TASK_VERSION,
                "request_id": body["request_id"],
                "result": {"kind": "failed", "capability": "twitch_title", "failure": "provider_rate_limited"}
            }))
        }
    }

    #[tokio::test]
    async fn anbieter_ratenlimit_nutzt_die_bestehende_wiederholung() {
        let fireworks_server = silent_fireworks().await;
        let brain = MockServer::start().await;
        let ledger = Arc::new(RecordingLedger::default());
        Mock::given(method("POST"))
            .respond_with(rate_limited())
            .up_to_n_times(1)
            .mount(&brain)
            .await;
        Mock::given(method("POST"))
            .respond_with(echo_title("Titel", Arc::new(Mutex::new(Vec::new()))))
            .mount(&brain)
            .await;
        let response = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title").twitch_broadcaster("42"),
            Some(route(&brain, Arc::clone(&ledger))),
        )
        .await
        .expect("zweiter Versuch");
        assert_eq!(response.text, "Titel");
        {
            let completions = ledger.completions.lock().unwrap();
            assert_eq!(completions.len(), 2);
            assert_eq!(completions[0].error_code.as_deref(), Some("http_status"));
            assert!(completions[1].success);
        }

        let exhausted = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(rate_limited())
            .expect(2)
            .mount(&exhausted)
            .await;
        let failure = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title")
                .retry_on_429(1)
                .twitch_broadcaster("42"),
            Some(route(&exhausted, Arc::new(RecordingLedger::default()))),
        )
        .await
        .expect_err("Ratenlimit erschöpft");
        assert_eq!(
            failure.error,
            LlmError::Http {
                status: 429,
                body: "provider_rate_limited".into()
            }
        );
    }

    #[tokio::test]
    async fn ohne_gepruefte_kanal_id_bleibt_alles_geschlossen() {
        for broadcaster in [None, Some("abc"), Some("0123"), Some(""), Some("1 2")] {
            let fireworks_server = silent_fireworks().await;
            let brain = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200))
                .expect(0)
                .mount(&brain)
                .await;
            let ledger = Arc::new(RecordingLedger::default());
            let mut request = title_request(&fireworks_server, "title");
            if let Some(id) = broadcaster {
                request = request.twitch_broadcaster(id);
            }
            let failure = complete_routed(
                "title_ai",
                request,
                Some(route(&brain, Arc::clone(&ledger))),
            )
            .await
            .expect_err("ohne Identität");
            assert!(matches!(failure.error, LlmError::Unavailable(_)));
            assert!(ledger.events.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn nicht_nutzbare_route_ruft_niemanden_auf() {
        let fireworks_server = silent_fireworks().await;
        let failure = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title").twitch_broadcaster("42"),
            Some(Arc::new(BrainRoute::unavailable(
                "Brain-Dienstschlüssel fehlt",
            ))),
        )
        .await
        .expect_err("fehlender Schlüssel");
        assert_eq!(
            failure.error,
            LlmError::Unavailable("Brain-Dienstschlüssel fehlt".into())
        );
    }

    #[tokio::test]
    async fn unbekannter_zweck_wird_nicht_umgeleitet() {
        let fireworks_server = silent_fireworks().await;
        let brain = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&brain)
            .await;
        let failure = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "andere").twitch_broadcaster("42"),
            Some(route(&brain, Arc::new(RecordingLedger::default()))),
        )
        .await
        .expect_err("fremder Zweck");
        assert!(matches!(failure.error, LlmError::Unavailable(_)));
    }

    #[tokio::test]
    async fn zu_grosse_tokengrenze_wird_nicht_umgeleitet() {
        let fireworks_server = silent_fireworks().await;
        let brain = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&brain)
            .await;
        let failure = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title")
                .max_tokens(1501)
                .twitch_broadcaster("42"),
            Some(route(&brain, Arc::new(RecordingLedger::default()))),
        )
        .await
        .expect_err("Tokengrenze");
        assert!(matches!(failure.error, LlmError::Unavailable(_)));
    }

    #[tokio::test]
    async fn inaktive_route_schickt_den_unveraenderten_fireworks_request() {
        let fireworks_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "<think>x</think>Titel"}}]
            })))
            .expect(1)
            .mount(&fireworks_server)
            .await;
        let response = complete_routed(
            "title_ai",
            title_request(&fireworks_server, "title").twitch_broadcaster("42"),
            None,
        )
        .await
        .expect("Fireworks-Antwort");
        assert_eq!(response.text, "Titel");
        assert_eq!(response.provider, "fireworks");
        let received = fireworks_server
            .received_requests()
            .await
            .expect("Requests");
        let body: Value = serde_json::from_slice(&received[0].body).expect("JSON");
        assert_eq!(
            body,
            json!({
                "model": crate::selection::configured_fireworks_model(),
                "messages": [{"role": "user", "content": "Titel-Prompt äöü"}],
                "max_tokens": 900,
                "temperature": 0.8,
                "reasoning_effort": "none"
            })
        );
    }

    #[test]
    fn nur_title_ai_hat_eine_brain_route() {
        assert_eq!(
            install("ai_chat", BrainRoute::unavailable("x")).err(),
            Some(BrainRouteError::UnsupportedUseCase)
        );
        assert!(installed("ai_chat").is_none());
    }

    #[test]
    fn brain_endpunkt_muss_lokal_sein() {
        assert!(BrainClientTransport::new("https://example.org", "t", None).is_err());
        assert!(BrainClientTransport::new("http://127.0.0.1:8789", " ", None).is_err());
        assert!(BrainClientTransport::new(
            "http://127.0.0.1:8789",
            "t",
            Some(Duration::from_secs(606))
        )
        .is_err());
        assert!(BrainClientTransport::new("http://127.0.0.1:8789/", "t", None).is_ok());
    }
}
