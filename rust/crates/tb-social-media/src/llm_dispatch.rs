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

const MINIMAX_DEFAULT_BASE: &str = "https://api.minimax.io/v1";
const MINIMAX_DEFAULT_MODEL: &str = "MiniMax-M3";
const CLAUDE_DEFAULT_BASE: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const EXTERNAL_TIMEOUT_SECONDS: u64 = 60;

fn external_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(EXTERNAL_TIMEOUT_SECONDS))
        .build()
        .unwrap_or_default()
}

/// MiniMax-Provider (extern, OpenAI-kompatibles chat/completions).
pub struct MiniMaxProvider {
    model: String,
    base_url: String,
    api_key: String,
    temperature: f64,
    http: reqwest::Client,
}

impl MiniMaxProvider {
    /// Aus Env; `MINIMAX_API_KEY` Pflicht → sonst Unavailable.
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = nonempty_env("MINIMAX_API_KEY").ok_or_else(|| LlmError::ProviderUnavailable("MINIMAX_API_KEY not set".to_string()))?;
        Ok(Self {
            model: nonempty_env("MINIMAX_MODEL").unwrap_or_else(|| MINIMAX_DEFAULT_MODEL.to_string()),
            base_url: nonempty_env("MINIMAX_BASE_URL").unwrap_or_else(|| MINIMAX_DEFAULT_BASE.to_string()),
            api_key,
            temperature: 0.4,
            http: external_http(),
        })
    }

    pub fn new(model: String, base_url: String, api_key: String, temperature: f64) -> Self {
        Self { model, base_url, api_key, temperature, http: external_http() }
    }
}

#[async_trait]
impl LlmProvider for MiniMaxProvider {
    fn name(&self) -> &str {
        "minimax"
    }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let prompt = render_user_prompt(request);
        let text = self.generate_text(SYSTEM_PROMPT, &prompt, 600, self.temperature).await?;
        parse_llm_payload(&text.content, "minimax", &self.model, text.cost_usd_estimate)
    }

    async fn generate_text(&self, system_prompt: &str, user_prompt: &str, max_tokens: i64, temperature: f64) -> Result<LlmTextResponse, LlmError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
            "temperature": temperature,
            "max_tokens": max_tokens,
        });
        if system_prompt.to_lowercase().contains("strict json") {
            body["response_format"] = serde_json::json!({"type": "json_object"});
        }
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::ProviderError(format!("MiniMax error: {e}")))?
            .error_for_status()
            .map_err(|e| LlmError::ProviderError(format!("MiniMax error: {e}")))?;
        let payload: Value = resp.json().await.map_err(|e| LlmError::ProviderError(format!("MiniMax JSON: {e}")))?;
        let content = payload
            .get("choices").and_then(Value::as_array).and_then(|a| a.first())
            .and_then(|c| c.get("message")).and_then(|m| m.get("content")).and_then(Value::as_str)
            .unwrap_or("").to_string();
        if content.trim().is_empty() {
            return Err(LlmError::ProviderError("MiniMax returned empty content".to_string()));
        }
        let usage = payload.get("usage");
        let pt = usage.and_then(|u| u.get("prompt_tokens")).and_then(Value::as_i64).unwrap_or(0);
        let ct = usage.and_then(|u| u.get("completion_tokens")).and_then(Value::as_i64).unwrap_or(0);
        let cost = estimate_cost(pt, ct, env_rate("MINIMAX_PRICE_INPUT_PER_1K", 0.0006), env_rate("MINIMAX_PRICE_OUTPUT_PER_1K", 0.0024));
        Ok(LlmTextResponse { content, provider: "minimax".to_string(), model: self.model.clone(), cost_usd_estimate: Some(cost) })
    }
}

/// Claude-Haiku-Provider (extern, Anthropic Messages-API).
pub struct ClaudeHaikuProvider {
    model: String,
    base_url: String,
    api_key: String,
    temperature: f64,
    http: reqwest::Client,
}

impl ClaudeHaikuProvider {
    /// Aus Env; `ANTHROPIC_API_KEY` Pflicht → sonst Unavailable.
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = nonempty_env("ANTHROPIC_API_KEY").ok_or_else(|| LlmError::ProviderUnavailable("ANTHROPIC_API_KEY not set".to_string()))?;
        Ok(Self {
            model: nonempty_env("ANTHROPIC_HAIKU_MODEL").unwrap_or_else(|| CLAUDE_DEFAULT_MODEL.to_string()),
            base_url: CLAUDE_DEFAULT_BASE.to_string(),
            api_key,
            temperature: 0.4,
            http: external_http(),
        })
    }

    pub fn new(model: String, base_url: String, api_key: String, temperature: f64) -> Self {
        Self { model, base_url, api_key, temperature, http: external_http() }
    }
}

#[async_trait]
impl LlmProvider for ClaudeHaikuProvider {
    fn name(&self) -> &str {
        "claude_haiku"
    }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let prompt = render_user_prompt(request);
        let text = self.generate_text(SYSTEM_PROMPT, &prompt, 600, self.temperature).await?;
        parse_llm_payload(&text.content, "claude_haiku", &self.model, text.cost_usd_estimate)
    }

    async fn generate_text(&self, system_prompt: &str, user_prompt: &str, max_tokens: i64, temperature: f64) -> Result<LlmTextResponse, LlmError> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "system": system_prompt,
            "messages": [{"role": "user", "content": user_prompt}],
        });
        let resp = self
            .http
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::ProviderError(format!("Claude error: {e}")))?
            .error_for_status()
            .map_err(|e| LlmError::ProviderError(format!("Claude error: {e}")))?;
        let payload: Value = resp.json().await.map_err(|e| LlmError::ProviderError(format!("Claude JSON: {e}")))?;
        // content ist ein Array von Blocks; Text-Blocks konkatenieren.
        let content: String = payload
            .get("content").and_then(Value::as_array)
            .map(|blocks| blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<String>())
            .unwrap_or_default();
        if content.trim().is_empty() {
            return Err(LlmError::ProviderError("Claude returned empty content".to_string()));
        }
        let usage = payload.get("usage");
        let pt = usage.and_then(|u| u.get("input_tokens")).and_then(Value::as_i64).unwrap_or(0);
        let ct = usage.and_then(|u| u.get("output_tokens")).and_then(Value::as_i64).unwrap_or(0);
        let cost = estimate_cost(pt, ct, env_rate("CLAUDE_HAIKU_PRICE_INPUT_PER_1K", 0.001), env_rate("CLAUDE_HAIKU_PRICE_OUTPUT_PER_1K", 0.005));
        Ok(LlmTextResponse { content, provider: "claude_haiku".to_string(), model: self.model.clone(), cost_usd_estimate: Some(cost) })
    }
}

/// Env-Var nur wenn gesetzt + nicht leer.
fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
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
            "minimax" => Ok(Box::new(MiniMaxProvider::from_env()?)),
            "claude_haiku" => Ok(Box::new(ClaudeHaikuProvider::from_env()?)),
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
        let p = MiniMaxProvider::new("MiniMax-M3".into(), server.uri(), "k".into(), 0.4);
        let resp = p.generate(&LlmRequest::default()).await.unwrap();
        assert_eq!(resp.provider, "minimax");
        assert_eq!(resp.youtube.title.as_deref(), Some("YT"));
        // Cost: 1000/1000*0.0006 + 1000/1000*0.0024 = 0.003 (Default-Raten, env-frei).
        if std::env::var("MINIMAX_PRICE_INPUT_PER_1K").is_err() {
            assert_eq!(resp.cost_usd_estimate, Some(0.003));
        }
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
        let p = ClaudeHaikuProvider::new("claude-haiku".into(), format!("{}/messages", server.uri()), "k".into(), 0.4);
        let resp = p.generate(&LlmRequest::default()).await.unwrap();
        assert_eq!(resp.provider, "claude_haiku");
        assert_eq!(resp.youtube.title.as_deref(), Some("YT"));
        assert!(resp.cost_usd_estimate.is_some());
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
