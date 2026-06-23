//! Anthropic-Claude-Client (Premium/`ai_full`) über die Messages API.
//!
//! Port von `get_anthropic_client` (`bot/core/llm_providers.py`) + den
//! `client.messages.create`-Aufrufen. Antwort = `content`-Block-Array → Text wird
//! über [`extract_text`] aggregiert (Python `_extract_text_response`).
//!
//! **Ledger-Hinweis:** Python-Paritaet: Anthropic schreibt keinen Eintrag in
//! das MiniMax-Ledger; nur MiniMax verbucht Tokens dort.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::{CompletionRequest, CompletionResponse, LlmError, LlmProvider};

/// Default-Endpunkt (Messages API).
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1/messages";
/// Default-Modell (Premium/Opus).
pub const DEFAULT_MODEL: &str = "claude-opus-4-6";
/// Anthropic-API-Versionsheader.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Default-Timeout pro Call (Opus-Antworten sind langsam).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(240);

/// Async-Client für die Anthropic Messages API.
pub struct AnthropicClient {
    api_key: Option<String>,
    base_url: String,
    model: String,
    timeout: Duration,
}

impl AnthropicClient {
    /// Baut den Client; `None`-Parameter ziehen aus Env bzw. Defaults. Key via
    /// [`crate::keys::anthropic_api_key`]. Base-URL: `ANTHROPIC_BASE_URL` →
    /// [`DEFAULT_BASE_URL`]. Modell: `ANTHROPIC_MODEL` → [`DEFAULT_MODEL`].
    pub fn new(
        api_key: Option<String>,
        base_url: Option<String>,
        model: Option<String>,
        timeout: Option<Duration>,
    ) -> Self {
        let api_key = api_key
            .filter(|k| !k.is_empty())
            .or_else(crate::keys::anthropic_api_key);
        let base_url = base_url
            .filter(|u| !u.is_empty())
            .or_else(|| nonempty_env("ANTHROPIC_BASE_URL"))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = model
            .filter(|m| !m.is_empty())
            .or_else(|| nonempty_env("ANTHROPIC_MODEL"))
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

    /// Baut das Messages-Array (ohne system — das geht ins top-level Feld).
    fn build_messages(request: &CompletionRequest) -> Value {
        let msgs: Vec<Value> = request
            .messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect();
        Value::Array(msgs)
    }
}

/// Aggregiert den Text aus dem Anthropic-`content`-Block-Array.
/// Python-Parität: `_extract_text_response` — Text-Blöcke werden mit `\n` verbunden.
pub fn extract_text(content: &Value) -> String {
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
                    item.get("content").and_then(Value::as_str).map(str::to_string)
                })
                .filter(|p| !p.is_empty())
                .collect();
            parts.join("\n").trim().to_string()
        }
        _ => String::new(),
    }
}

#[async_trait]
impl LlmProvider for AnthropicClient {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(
        &self,
        request: &CompletionRequest,
        _purpose: &str,
    ) -> Result<CompletionResponse, LlmError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| LlmError::Unavailable("ANTHROPIC_API_KEY not set".to_string()))?;

        // KEINE temperature (Anthropic-Default, 1:1 Python).
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "messages": Self::build_messages(request),
        });
        if let Some(system) = &request.system {
            body["system"] = Value::String(system.clone());
        }

        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| LlmError::Http(e.to_string()))?;

        let started = std::time::Instant::now();
        let resp = client
            .post(&self.base_url)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            // Body mitnehmen → Aufrufer kann Anthropic-Fehlertexte auswerten.
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Http(format!("HTTP {status}: {text}")));
        }
        let payload: Value = resp.json().await.map_err(|e| LlmError::Http(e.to_string()))?;
        let latency_ms = started.elapsed().as_millis() as i64;

        let text = extract_text(payload.get("content").unwrap_or(&Value::Null));

        // Anthropic-Usage-Schema: input_tokens / output_tokens.
        let usage = payload.get("usage");
        let prompt_tokens = usage
            .and_then(|u| u.get("input_tokens"))
            .and_then(Value::as_i64);
        let completion_tokens = usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(Value::as_i64);

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
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn extract_text_aggregiert_block_array() {
        let content = json!([
            {"type": "text", "text": "Teil eins"},
            {"type": "text", "text": "Teil zwei"}
        ]);
        assert_eq!(extract_text(&content), "Teil eins\nTeil zwei");
        assert_eq!(extract_text(&json!("direkt")), "direkt");
        assert_eq!(extract_text(&Value::Null), "");
    }

    #[tokio::test]
    async fn complete_parst_text_und_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(header("x-api-key", "k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "Antwort"}],
                "usage": {"input_tokens": 5, "output_tokens": 3}
            })))
            .mount(&server)
            .await;

        let client = AnthropicClient::new(
            Some("k".into()),
            Some(format!("{}/messages", server.uri())),
            None,
            None,
        );
        let req = CompletionRequest::simple("sys", "Hi", 100);
        let resp = client.complete(&req, "analytics").await.unwrap();
        assert_eq!(resp.text, "Antwort");
        assert_eq!(resp.prompt_tokens, Some(5));
        assert_eq!(resp.completion_tokens, Some(3));
        assert_eq!(resp.model, "claude-opus-4-6");
        assert_eq!(client.name(), "anthropic");
    }

    #[tokio::test]
    async fn complete_http_fehler_traegt_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_string("credit balance is too low"))
            .mount(&server)
            .await;
        let client = AnthropicClient::new(
            Some("k".into()),
            Some(format!("{}/messages", server.uri())),
            None,
            None,
        );
        let req = CompletionRequest::simple("s", "u", 10);
        match client.complete(&req, "analytics").await {
            Err(LlmError::Http(msg)) => assert!(msg.contains("credit balance is too low")),
            other => panic!("erwartete Http, war {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_ohne_key_unavailable() {
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            return;
        }
        let client = AnthropicClient::new(
            Some(String::new()),
            Some("http://127.0.0.1:1/messages".into()),
            None,
            None,
        );
        let req = CompletionRequest::simple("s", "u", 10);
        match client.complete(&req, "analytics").await {
            Err(LlmError::Unavailable(_)) => {}
            other => panic!("erwartete Unavailable, war {other:?}"),
        }
    }
}
