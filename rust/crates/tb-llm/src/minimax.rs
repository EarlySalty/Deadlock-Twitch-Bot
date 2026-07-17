//! MiniMax-M3-Client (Primär-Provider) über den OpenAI-kompatiblen
//! `/chat/completions`-Endpunkt.
//!
//! Port von `get_minimax_client` (`bot/core/llm_providers.py`) + dem
//! `_track_minimax_completion`-Ledger-Hook. Jeder erfolgreiche Call verbucht die
//! echten `usage.prompt_tokens`/`usage.completion_tokens` ins gemeinsame Ledger.

use std::time::Duration;

use async_trait::async_trait;

use crate::ledger;
use crate::provider::{CompletionRequest, CompletionResponse, LlmError, LlmProvider};

/// Default-Endpunkt (OpenAI-kompatibel). Python: `MINIMAX_DEFAULT_BASE_URL`.
pub const DEFAULT_BASE_URL: &str = "https://api.minimax.io/v1";
/// Default-Modell-Lock.
pub const DEFAULT_MODEL: &str = "MiniMax-M3";
/// Default-Timeout pro Call.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(240);

/// Async-Client für MiniMax M3.
pub struct MiniMaxClient {
    api_key: Option<String>,
    base_url: String,
    model: String,
    timeout: Duration,
}

impl MiniMaxClient {
    /// Baut den Client; `None`-Parameter ziehen aus Env bzw. Defaults. Key via
    /// [`crate::keys::minimax_api_key`]. Base-URL: `MINIMAX_BASE_URL` →
    /// [`DEFAULT_BASE_URL`]. Modell: `MINIMAX_MODEL` → [`DEFAULT_MODEL`].
    pub fn new(
        api_key: Option<String>,
        base_url: Option<String>,
        model: Option<String>,
        timeout: Option<Duration>,
    ) -> Self {
        let api_key = api_key
            .filter(|k| !k.is_empty())
            .or_else(crate::keys::minimax_api_key);
        let base_url = base_url
            .filter(|u| !u.is_empty())
            .or_else(|| nonempty_env("MINIMAX_BASE_URL"))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = model
            .filter(|m| !m.is_empty())
            .or_else(|| nonempty_env("MINIMAX_MODEL"))
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Self {
            api_key,
            base_url,
            model,
            timeout: timeout.unwrap_or(DEFAULT_TIMEOUT),
        }
    }

    /// Convenience-Default: alles aus Env/Defaults.
    pub fn from_env() -> Self {
        Self::new(None, None, None, None)
    }

    /// Baut das OpenAI-kompatible `messages`-Array aus dem Request (system zuerst).
    fn build_messages(request: &CompletionRequest) -> serde_json::Value {
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        for m in &request.messages {
            messages.push(serde_json::json!({"role": m.role, "content": m.content}));
        }
        serde_json::Value::Array(messages)
    }
}

#[async_trait]
impl LlmProvider for MiniMaxClient {
    fn name(&self) -> &'static str {
        "minimax"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(
        &self,
        request: &CompletionRequest,
        purpose: &str,
    ) -> Result<CompletionResponse, LlmError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| LlmError::Unavailable("MINIMAX_API_KEY not set".to_string()))?;

        let body = serde_json::json!({
            "model": self.model,
            "messages": Self::build_messages(request),
            "max_tokens": request.max_tokens,
            "temperature": request.temperature.unwrap_or(0.5),
        });

        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| LlmError::Http(e.to_string()))?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let started = std::time::Instant::now();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?
            .error_for_status()
            .map_err(|e| LlmError::Http(e.to_string()))?;
        let payload: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;
        let latency_ms = started.elapsed().as_millis() as i64;

        let text = payload
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();

        let usage = payload.get("usage");
        let prompt_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(serde_json::Value::as_i64);
        let completion_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(serde_json::Value::as_i64);

        // Tokens best-effort verbuchen (DB-Fehler ≠ Hard-Fail).
        ledger::record(
            purpose,
            &self.model,
            prompt_tokens.unwrap_or(0),
            completion_tokens.unwrap_or(0),
            true,
        )
        .await;

        Ok(CompletionResponse {
            text,
            model: self.model.clone(),
            prompt_tokens,
            completion_tokens,
            latency_ms,
        })
    }
}

/// Env-Var nur, wenn gesetzt UND nicht leer.
fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::CompletionRequest;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(server: &MockServer) -> MiniMaxClient {
        MiniMaxClient::new(
            Some("test-key".to_string()),
            Some(server.uri()),
            Some("MiniMax-M3".to_string()),
            None,
        )
    }

    #[tokio::test]
    async fn complete_parst_text_und_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .and(body_string_contains("bebop auf der lane"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "klar, bebop ist stark"}}],
                "usage": {"prompt_tokens": 42, "completion_tokens": 7}
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let req = CompletionRequest::simple("system", "bebop auf der lane?", 500);
        let resp = client.complete(&req, "engagement").await.unwrap();

        assert_eq!(resp.text, "klar, bebop ist stark");
        assert_eq!(resp.prompt_tokens, Some(42));
        assert_eq!(resp.completion_tokens, Some(7));
        assert_eq!(resp.model, "MiniMax-M3");
        assert_eq!(client.name(), "minimax");
    }

    #[tokio::test]
    async fn complete_ohne_key_unavailable() {
        // Nur valide, wenn keine Env-Keys gesetzt sind (Testprozess i. d. R. ohne).
        if std::env::var("MINIMAX_TOKEN_PLAN_KEY").is_ok()
            || std::env::var("MINIMAX_API_KEY").is_ok()
            || std::env::var("MINMAX").is_ok()
        {
            return;
        }
        let client = MiniMaxClient::new(
            Some(String::new()),
            Some("http://127.0.0.1:1".to_string()),
            Some("MiniMax-M3".to_string()),
            None,
        );
        let req = CompletionRequest::simple("s", "u", 10);
        match client.complete(&req, "engagement").await {
            Err(LlmError::Unavailable(_)) => {}
            other => panic!("erwartete Unavailable, war {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_http_fehler_propagiert() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;
        let client = client_for(&server);
        let req = CompletionRequest::simple("s", "u", 10);
        match client.complete(&req, "engagement").await {
            Err(LlmError::Http(_)) => {}
            other => panic!("erwartete Http, war {other:?}"),
        }
    }
}
