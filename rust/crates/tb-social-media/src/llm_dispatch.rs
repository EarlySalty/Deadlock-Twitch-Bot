//! LLM-Provider + Dispatcher (Port von `social_media/llm/ollama.py` +
//! `dispatcher.py`).
//!
//! Default-Provider ist das **lokale Ollama** (`/api/generate`, format=json).
//! Externe Provider (minimax/claude_haiku) sind consent-gated
//! (`social_media_settings.external_llm_consent`) und folgen in einer weiteren
//! Slice — bis dahin fallen sie via Fallback-Chain auf Ollama zurück.

use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;

use crate::llm::{parse_llm_payload, render_user_prompt, LlmError, LlmRequest, LlmResponse, LlmTextResponse, SYSTEM_PROMPT};
use crate::settings;

const DEFAULT_LOCAL: &str = "ollama";
const EXTERNAL_PROVIDERS: [&str; 2] = ["minimax", "claude_haiku"];
const OLLAMA_DEFAULT_MODEL: &str = "qwen2.5:7b-instruct-q4_K_M";
const OLLAMA_DEFAULT_HOST: &str = "127.0.0.1:11434";
const OLLAMA_TIMEOUT_SECONDS: u64 = 240;

/// Gemeinsame Provider-Schnittstelle (Python `LLMProvider`-Protocol).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError>;
    async fn generate_text(&self, system_prompt: &str, user_prompt: &str, max_tokens: i64, temperature: f64) -> Result<LlmTextResponse, LlmError>;
}

/// Lokaler Ollama-Provider (Default).
pub struct OllamaProvider {
    model: String,
    host: String,
    temperature: f64,
    http: reqwest::Client,
}

impl OllamaProvider {
    /// Aus Env: `OLLAMA_MODEL` / `OLLAMA_HOST` (Default 127.0.0.1:11434).
    pub fn from_env() -> Self {
        let model = std::env::var("OLLAMA_MODEL").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| OLLAMA_DEFAULT_MODEL.to_string());
        let raw_host = std::env::var("OLLAMA_HOST").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| OLLAMA_DEFAULT_HOST.to_string());
        Self::new(model, raw_host, 0.4)
    }

    pub fn new(model: String, host: impl Into<String>, temperature: f64) -> Self {
        let host = host.into();
        let host = if host.contains("://") { host } else { format!("http://{host}") };
        let host = host.trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(OLLAMA_TIMEOUT_SECONDS))
            .build()
            .unwrap_or_default();
        Self { model, host, temperature, http }
    }

    fn url(&self) -> String {
        format!("{}/api/generate", self.host)
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let prompt = render_user_prompt(request);
        let text = self.generate_text(SYSTEM_PROMPT, &prompt, 600, self.temperature).await?;
        parse_llm_payload(&text.content, "ollama", &self.model, text.cost_usd_estimate)
    }

    async fn generate_text(&self, system_prompt: &str, user_prompt: &str, max_tokens: i64, temperature: f64) -> Result<LlmTextResponse, LlmError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "prompt": user_prompt,
            "system": system_prompt,
            "stream": false,
            "options": { "temperature": temperature, "num_predict": max_tokens, "top_p": 0.9 },
        });
        // format=json nur wenn der System-Prompt strikten JSON verlangt.
        if system_prompt.to_lowercase().contains("strict json") {
            body["format"] = Value::String("json".to_string());
        }

        let resp = self
            .http
            .post(self.url())
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    LlmError::ProviderUnavailable(format!("Ollama nicht erreichbar unter {}", self.host))
                } else if e.is_timeout() {
                    LlmError::ProviderError("Ollama generate timeout".to_string())
                } else {
                    LlmError::ProviderError(format!("Ollama client error: {e}"))
                }
            })?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ProviderError(format!("Ollama HTTP {status}: {}", text.chars().take(200).collect::<String>())));
        }
        let payload: Value = resp.json().await.map_err(|e| LlmError::ProviderError(format!("Ollama JSON: {e}")))?;
        let content = payload.get("response").and_then(Value::as_str).unwrap_or("").to_string();
        if content.trim().is_empty() {
            return Err(LlmError::ProviderError("Ollama returned empty response".to_string()));
        }
        // Lokales LLM hat keine Per-Call-Kosten (0.0 als Marker).
        Ok(LlmTextResponse { content, provider: "ollama".to_string(), model: self.model.clone(), cost_usd_estimate: Some(0.0) })
    }
}

/// Token-basierte USD-Kostenschätzung (gerundet auf 6 Nachkommastellen).
fn estimate_cost(prompt_tokens: i64, completion_tokens: i64, input_rate: f64, output_rate: f64) -> f64 {
    let raw = (prompt_tokens as f64 / 1000.0) * input_rate + (completion_tokens as f64 / 1000.0) * output_rate;
    (raw * 1_000_000.0).round() / 1_000_000.0
}

fn env_rate(var: &str, default: f64) -> f64 {
    std::env::var(var).ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|r| *r >= 0.0).unwrap_or(default)
}

/// Anwendungsfaelle der beiden externen Anbieter in der gemeinsamen Auswahl.
/// Modell und Adresse stehen in `tb-llm`, nicht hier.
const USE_CASE_MINIMAX: &str = "social_media";
const USE_CASE_CLAUDE: &str = "social_media_claude";
const EXTERNAL_TIMEOUT_SECONDS: u64 = 60;

/// Uebersetzt den Fehler des gemeinsamen Eingangs in die Fehlerform des
/// Dispatchers. `ProviderUnavailable` ist wichtig: nur darauf schaltet die
/// Kette weiter, ohne den Fehler zu melden.
fn dispatch_error(provider: &str, error: tb_llm::LlmError) -> LlmError {
    match error {
        tb_llm::LlmError::Unavailable(detail) => LlmError::ProviderUnavailable(detail),
        other => LlmError::ProviderError(format!("{provider} error: {other}")),
    }
}

/// Ein externer Anbieter der Clip-Anreicherung.
///
/// Beide Anbieter unterscheiden sich nur noch in Name, Anwendungsfall und
/// Preisschluesseln; der HTTP-Weg liegt in [`tb_llm::complete`].
pub struct ExternalProvider {
    /// Name der Bahn in den Einstellungen (`minimax`, `claude_haiku`). Welcher
    /// Anbieter dahinter wirklich antwortet, entscheidet die zentrale Auswahl;
    /// gespeichert und bepreist wird der antwortende Anbieter, nicht die Bahn.
    name: &'static str,
    use_case: &'static str,
    endpoint: tb_llm::LlmEndpoint,
    temperature: f64,
}

/// Preisschluessel und Standardrate (USD je 1000 Tokens) eines Anbieters:
/// Umgebungsvariable fuer Eingang, Standard Eingang, Variable Ausgang,
/// Standard Ausgang.
type Preise = ((&'static str, f64), (&'static str, f64));

const MINIMAX_PREISE: Preise = (
    ("MINIMAX_PRICE_INPUT_PER_1K", 0.0006),
    ("MINIMAX_PRICE_OUTPUT_PER_1K", 0.0024),
);
/// DeepSeek ueber Fireworks; Listenpreis, per Env ueberschreibbar.
const FIREWORKS_PREISE: Preise = (
    ("FIREWORKS_PRICE_INPUT_PER_1K", 0.0009),
    ("FIREWORKS_PRICE_OUTPUT_PER_1K", 0.0009),
);
const CLAUDE_HAIKU_PREISE: Preise = (
    ("CLAUDE_HAIKU_PRICE_INPUT_PER_1K", 0.001),
    ("CLAUDE_HAIKU_PRICE_OUTPUT_PER_1K", 0.005),
);

/// Preistabelle je antwortendem Anbieter. Die Bahn `minimax` landet ueber die
/// zentrale Auswahl live meist auf Fireworks/DeepSeek; mit MiniMax-Raten
/// gerechnet waere die Kostenschaetzung falsch.
fn preise_fuer_anbieter(provider: &str) -> Preise {
    match provider {
        "minimax" => MINIMAX_PREISE,
        "anthropic" => CLAUDE_HAIKU_PREISE,
        _ => FIREWORKS_PREISE,
    }
}

impl ExternalProvider {
    /// MiniMax-Bahn; ohne Schluessel `Unavailable`, damit die Kette weiterschaltet.
    pub fn minimax_from_env() -> Result<Self, LlmError> {
        Self::from_env("minimax", USE_CASE_MINIMAX)
    }

    /// Claude-Haiku-Bahn; ohne Schluessel `Unavailable`.
    pub fn claude_haiku_from_env() -> Result<Self, LlmError> {
        Self::from_env("claude_haiku", USE_CASE_CLAUDE)
    }

    fn from_env(name: &'static str, use_case: &'static str) -> Result<Self, LlmError> {
        let endpoint = tb_llm::endpoint_for(use_case);
        if endpoint.api_key.is_none() {
            return Err(LlmError::ProviderUnavailable(format!(
                "kein Schluessel fuer {name}"
            )));
        }
        Ok(Self {
            name,
            use_case,
            endpoint,
            temperature: 0.4,
        })
    }

    /// Ledger-Zweck dieses Aufrufs: Bahn plus tatsaechlich antwortender
    /// Anbieter, zum Beispiel `social-media-claude_haiku-anthropic`.
    ///
    /// Die Bahn allein reicht nicht: hinter `social_media` kann Fireworks oder
    /// MiniMax stehen, und in der Kostenauswertung waeren beide dann eine
    /// Summe. Das Schema bleibt unberuehrt, der Zweck traegt die Unterscheidung.
    fn ledger_purpose(&self) -> String {
        format!("social-media-{}-{}", self.name, self.endpoint.provider)
    }

    /// Expliziter Endpunkt fuer Tests. Nicht `pub`: produktiv kommt der
    /// Endpunkt immer aus `from_env`.
    #[cfg(test)]
    pub(crate) fn for_test(
        name: &'static str,
        use_case: &'static str,
        endpoint: tb_llm::LlmEndpoint,
        temperature: f64,
    ) -> Self {
        Self {
            name,
            use_case,
            endpoint,
            temperature,
        }
    }
}

#[async_trait]
impl LlmProvider for ExternalProvider {
    fn name(&self) -> &str {
        self.name
    }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let prompt = render_user_prompt(request);
        let text = self
            .generate_text(SYSTEM_PROMPT, &prompt, 600, self.temperature)
            .await?;
        parse_llm_payload(
            &text.content,
            &text.provider,
            &text.model,
            text.cost_usd_estimate,
        )
    }

    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: i64,
        temperature: f64,
    ) -> Result<LlmTextResponse, LlmError> {
        let mut request = tb_llm::Request::simple(system_prompt, user_prompt)
            .max_tokens(max_tokens)
            .temperature(temperature)
            .timeout(std::time::Duration::from_secs(EXTERNAL_TIMEOUT_SECONDS))
            .ledger_purpose(self.ledger_purpose())
            .endpoint(self.endpoint.clone());
        // format=json nur, wenn der System-Prompt strikten JSON verlangt.
        if system_prompt.to_lowercase().contains("strict json") {
            request = request.json_object();
        }

        let response = tb_llm::complete(self.use_case, request)
            .await
            .map_err(|error| dispatch_error(self.name, error))?;
        if response.text.trim().is_empty() {
            return Err(LlmError::ProviderError(format!(
                "{} returned empty content",
                self.name
            )));
        }
        // Achtung Vokabular: `clip_enrichments.llm_provider` traegt seit der
        // Zentralisierung den antwortenden Anbieter (`fireworks`, `minimax`,
        // `anthropic`), vorher den Bahn-Namen (`minimax`, `claude_haiku`).
        // Altzeilen werden nicht nachgezogen; Auswertungen muessen beide
        // Schreibweisen kennen (siehe llm-zentral.md, Verhaltensaenderungen).
        // Anbieter und Preis vom tatsaechlich antwortenden Endpunkt, nicht
        // von der Bahn: so steht in `clip_enrichments.llm_provider` der echte
        // Absender und die Kostenschaetzung passt zu ihm.
        let (preis_eingang, preis_ausgang) = preise_fuer_anbieter(&response.provider);
        let cost = estimate_cost(
            response.prompt_tokens.unwrap_or(0),
            response.completion_tokens.unwrap_or(0),
            env_rate(preis_eingang.0, preis_eingang.1),
            env_rate(preis_ausgang.0, preis_ausgang.1),
        );
        Ok(LlmTextResponse {
            content: response.text,
            provider: response.provider,
            model: response.model,
            cost_usd_estimate: Some(cost),
        })
    }
}

/// Wählt + ruft den passenden Provider (Python `LLMDispatcher`).
pub struct LlmDispatcher {
    pool: PgPool,
    provider_override: Option<String>,
    consent_override: Option<bool>,
}

impl LlmDispatcher {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, provider_override: None, consent_override: None }
    }

    pub fn with_provider(mut self, name: impl Into<String>) -> Self {
        self.provider_override = Some(name.into());
        self
    }

    pub fn with_consent(mut self, consent: bool) -> Self {
        self.consent_override = Some(consent);
        self
    }

    async fn resolve_consent(&self) -> bool {
        match self.consent_override {
            Some(c) => c,
            None => settings::external_llm_consent(&self.pool).await,
        }
    }

    fn resolve_provider_name(&self) -> String {
        if let Some(p) = self.provider_override.as_deref().filter(|s| !s.is_empty()) {
            return p.trim().to_lowercase();
        }
        std::env::var("SOCIAL_MEDIA_LLM_PROVIDER")
            .ok()
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_LOCAL.to_string())
    }

    /// Instanziiert einen Provider; fehlende Keys → Unavailable (→ Fallback).
    fn instantiate(&self, name: &str) -> Result<Box<dyn LlmProvider>, LlmError> {
        match name {
            "ollama" => Ok(Box::new(OllamaProvider::from_env())),
            "minimax" => Ok(Box::new(ExternalProvider::minimax_from_env()?)),
            "claude_haiku" => Ok(Box::new(ExternalProvider::claude_haiku_from_env()?)),
            other => Err(LlmError::ProviderUnavailable(format!("Unknown LLM provider: {other}"))),
        }
    }

    /// Anreicherung erzeugen (mit Consent-Gate + Fallback-Chain).
    pub async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let mut chosen = self.resolve_provider_name();
        if EXTERNAL_PROVIDERS.contains(&chosen.as_str()) && !self.resolve_consent().await {
            tracing::warn!(provider = %chosen, "Externer LLM-Provider ohne Consent → Fallback auf Ollama");
            chosen = DEFAULT_LOCAL.to_string();
        }

        let mut last_error = None;
        for candidate in candidate_chain(&chosen) {
            let provider = match self.instantiate(&candidate) {
                Ok(p) => p,
                Err(e) => {
                    tracing::info!(provider = %candidate, error = %e, "Provider nicht verfügbar");
                    last_error = Some(e);
                    continue;
                }
            };
            match provider.generate(request).await {
                Ok(resp) => return Ok(resp),
                Err(LlmError::ProviderUnavailable(e)) => {
                    last_error = Some(LlmError::ProviderUnavailable(e));
                    continue;
                }
                Err(e) => {
                    tracing::warn!(provider = %candidate, error = %e, "Provider-Fehler");
                    last_error = Some(e);
                    continue;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| LlmError::ProviderError("Alle LLM-Provider fehlgeschlagen".to_string())))
    }

    /// Freitext erzeugen (Consent-Gate + Fallback-Chain wie `generate`); für
    /// Report-Markdown genutzt.
    pub async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: i64,
        temperature: f64,
    ) -> Result<LlmTextResponse, LlmError> {
        let mut chosen = self.resolve_provider_name();
        if EXTERNAL_PROVIDERS.contains(&chosen.as_str()) && !self.resolve_consent().await {
            tracing::warn!(provider = %chosen, "Externer LLM-Provider ohne Consent → Fallback auf Ollama");
            chosen = DEFAULT_LOCAL.to_string();
        }

        let mut last_error = None;
        for candidate in candidate_chain(&chosen) {
            let provider = match self.instantiate(&candidate) {
                Ok(p) => p,
                Err(e) => {
                    tracing::info!(provider = %candidate, error = %e, "Provider nicht verfügbar");
                    last_error = Some(e);
                    continue;
                }
            };
            match provider.generate_text(system_prompt, user_prompt, max_tokens, temperature).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    tracing::warn!(provider = %candidate, error = %e, "Provider-Textfehler");
                    last_error = Some(e);
                    continue;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| LlmError::ProviderError("Alle LLM-Provider fehlgeschlagen".to_string())))
    }
}

/// Kandidaten-Kette: primär, dann Ollama-Fallback (Python `_candidate_chain`).
fn candidate_chain(primary: &str) -> Vec<String> {
    if primary == DEFAULT_LOCAL {
        vec![primary.to_string()]
    } else {
        vec![primary.to_string(), DEFAULT_LOCAL.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok_payload() -> serde_json::Value {
        serde_json::json!({
            "response": "{\"youtube\":{\"title\":\"YT\",\"description\":\"d\",\"hashtags\":[\"haze\"]},\"tiktok\":{\"title\":\"TK\",\"description\":\"d\",\"hashtags\":[]},\"instagram\":{\"title\":\"IG\",\"description\":\"d\",\"hashtags\":[]}}",
            "eval_count": 10, "prompt_eval_count": 20
        })
    }

    #[tokio::test]
    async fn ollama_generate_parst_antwort() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/generate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_payload()))
            .mount(&server).await;
        let provider = OllamaProvider::new("llama3".into(), server.uri(), 0.4);
        let req = LlmRequest { transcript: "haze".into(), ..Default::default() };
        let resp = provider.generate(&req).await.unwrap();
        assert_eq!(resp.youtube.title.as_deref(), Some("YT"));
        assert_eq!(resp.provider, "ollama");
        assert!(resp.youtube.hashtags.contains(&"#Deadlock".to_string()));
    }

    #[tokio::test]
    async fn ollama_leere_antwort_ist_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/generate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"response": "  "})))
            .mount(&server).await;
        let provider = OllamaProvider::new("m".into(), server.uri(), 0.4);
        assert!(matches!(provider.generate(&LlmRequest::default()).await, Err(LlmError::ProviderError(_))));
    }

    fn inner_json() -> &'static str {
        "{\"youtube\":{\"title\":\"YT\",\"description\":\"d\",\"hashtags\":[\"haze\"]},\"tiktok\":{\"title\":\"TK\",\"description\":\"d\",\"hashtags\":[]},\"instagram\":{\"title\":\"IG\",\"description\":\"d\",\"hashtags\":[]}}"
    }

    #[tokio::test]
    async fn minimax_generate_und_cost() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": inner_json()}}],
                "usage": {"prompt_tokens": 1000, "completion_tokens": 1000}
            })))
            .mount(&server).await;
        let p = ExternalProvider::for_test(
            "minimax",
            USE_CASE_MINIMAX,
            tb_llm::LlmEndpoint {
                provider: "minimax",
                base_url: server.uri(),
                model: "MiniMax-M3".into(),
                api_key: Some("k".into()),
            },
            0.4,
        );
        let resp = p.generate(&LlmRequest::default()).await.unwrap();
        assert_eq!(resp.provider, "minimax");
        assert_eq!(resp.youtube.title.as_deref(), Some("YT"));
        // 1000 Eingangs- und 1000 Ausgangstokens zu den produktiven
        // MiniMax-Standardraten 0.0006 / 0.0024 je 1000 Tokens.
        let kosten = resp.cost_usd_estimate.expect("Kostenschaetzung");
        assert!((kosten - 0.003).abs() < 1e-9, "kosten={kosten}");
    }

    #[tokio::test]
    async fn minimax_bahn_auf_fireworks_nennt_fireworks_und_rechnet_fireworks_preis() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": inner_json()}}],
                "usage": {"prompt_tokens": 1000, "completion_tokens": 1000}
            })))
            .mount(&server).await;
        // Bahn `minimax`, aber die zentrale Auswahl hat Fireworks/DeepSeek
        // ergeben: llm_provider und Preis muessen Fireworks folgen.
        let p = ExternalProvider::for_test(
            "minimax",
            USE_CASE_MINIMAX,
            tb_llm::LlmEndpoint {
                provider: "fireworks",
                base_url: server.uri(),
                model: "accounts/fireworks/models/deepseek-v4-flash".into(),
                api_key: Some("k".into()),
            },
            0.4,
        );
        let resp = p.generate(&LlmRequest::default()).await.unwrap();
        assert_eq!(resp.provider, "fireworks");
        assert_eq!(resp.model, "accounts/fireworks/models/deepseek-v4-flash");
        let kosten = resp.cost_usd_estimate.expect("Kostenschaetzung");
        // 1000/1000 Tokens zu den Fireworks-Raten 0.0009 / 0.0009.
        assert!((kosten - 0.0018).abs() < 1e-9, "kosten={kosten}");
        assert!((kosten - 0.003).abs() > 1e-6, "darf nicht mit MiniMax-Raten rechnen");
    }

    #[tokio::test]
    async fn claude_generate_konkateniert_textblocks() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{"type": "text", "text": inner_json()}],
                "usage": {"input_tokens": 100, "output_tokens": 50}
            })))
            .mount(&server).await;
        let p = ExternalProvider::for_test(
            "claude_haiku",
            USE_CASE_CLAUDE,
            tb_llm::LlmEndpoint {
                provider: "anthropic",
                base_url: format!("{}/messages", server.uri()),
                model: "claude-haiku".into(),
                api_key: Some("k".into()),
            },
            0.4,
        );
        let resp = p.generate(&LlmRequest::default()).await.unwrap();
        // Gespeichert wird der antwortende Anbieter, nicht der Bahn-Name.
        assert_eq!(resp.provider, "anthropic");
        assert_eq!(resp.youtube.title.as_deref(), Some("YT"));
        // 100 Eingangs- und 50 Ausgangstokens zu den Haiku-Standardraten
        // 0.001 / 0.005 je 1000 Tokens.
        let kosten = resp.cost_usd_estimate.expect("Kostenschaetzung");
        assert!((kosten - 0.00035).abs() < 1e-9, "kosten={kosten}");
    }

    #[test]
    fn ledger_zweck_nennt_bahn_und_antwortenden_anbieter() {
        // Hinter der Bahn `social_media` kann Fireworks oder MiniMax stehen; in
        // der Kostenauswertung muessen beide auseinanderzuhalten sein.
        let p = ExternalProvider::for_test(
            "minimax",
            USE_CASE_MINIMAX,
            tb_llm::LlmEndpoint {
                provider: "fireworks",
                base_url: "http://127.0.0.1:1".into(),
                model: "m".into(),
                api_key: Some("k".into()),
            },
            0.4,
        );
        assert_eq!(p.ledger_purpose(), "social-media-minimax-fireworks");

        let p = ExternalProvider::for_test(
            "claude_haiku",
            USE_CASE_CLAUDE,
            tb_llm::LlmEndpoint {
                provider: "anthropic",
                base_url: "http://127.0.0.1:1".into(),
                model: "m".into(),
                api_key: Some("k".into()),
            },
            0.4,
        );
        assert_eq!(p.ledger_purpose(), "social-media-claude_haiku-anthropic");
    }

    #[test]
    fn candidate_chain_fallback() {
        assert_eq!(candidate_chain("ollama"), vec!["ollama"]);
        assert_eq!(candidate_chain("minimax"), vec!["minimax", "ollama"]);
    }

    #[test]
    fn host_normalisierung() {
        assert_eq!(OllamaProvider::new("m".into(), "127.0.0.1:11434", 0.4).host, "http://127.0.0.1:11434");
        assert_eq!(OllamaProvider::new("m".into(), "https://x/", 0.4).host, "https://x");
    }
}
