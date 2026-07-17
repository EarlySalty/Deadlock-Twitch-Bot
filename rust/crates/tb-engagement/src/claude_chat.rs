//! Anthropic-Claude-Client (Messages API) für die KI-Analyse-Endpunkte.
//!
//! Port von `core/llm_providers.py:get_anthropic_client` + den
//! `client.messages.create`-Aufrufen in `api_ai.py` (Opus-Pfad von
//! ai/analysis + ai/chat). Minimal: `model`/`max_tokens`/optionales `system`/
//! `messages`. Gibt das `content`-Feld der Antwort zurück (Block-Array), das der
//! Aufrufer via `tb_analytics::ai_analysis::extract_text_response` auswertet.

use std::time::Duration;

use serde_json::Value;

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1/messages";
pub const DEFAULT_MODEL: &str = "claude-opus-4-6";

#[derive(Debug)]
pub enum ClaudeError {
    /// Kein API-Key konfiguriert.
    Unavailable(String),
    /// Transport-/HTTP-Fehler (Message enthält den Response-Body, damit der
    /// Aufrufer z. B. „credit balance is too low" erkennen kann).
    Http(String),
}

impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeError::Unavailable(e) => write!(f, "{e}"),
            ClaudeError::Http(e) => write!(f, "Claude-Call fehlgeschlagen: {e}"),
        }
    }
}
impl std::error::Error for ClaudeError {}

/// Async-Client für die Anthropic Messages API.
pub struct ClaudeClient {
    api_key: Option<String>,
    base_url: String,
    model: String,
    timeout: Duration,
}

impl ClaudeClient {
    /// Baut den Client; `None`-Parameter ziehen aus Env bzw. Defaults. Key:
    /// `ANTHROPIC_API_KEY`. Base-URL: `ANTHROPIC_BASE_URL` → [`DEFAULT_BASE_URL`].
    /// Modell: `ANTHROPIC_MODEL` → [`DEFAULT_MODEL`] (`claude-opus-4-6`).
    pub fn new(
        api_key: Option<String>,
        base_url: Option<String>,
        model: Option<String>,
        timeout: Option<Duration>,
    ) -> Self {
        let api_key = api_key
            .filter(|k| !k.is_empty())
            .or_else(|| nonempty_env("ANTHROPIC_API_KEY"));
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
            timeout: timeout.unwrap_or_else(|| Duration::from_secs(240)),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Ruft die Messages API. `messages` = JSON-Array, `system` optional (wie in
    /// Python: ai/analysis ohne, ai/chat mit). KEINE temperature (Anthropic-
    /// Default, 1:1 Python). Rückgabe = `content`-Feld (Block-Array) oder Null.
    pub async fn create_message(
        &self,
        system: Option<&str>,
        messages: Value,
        max_tokens: i64,
    ) -> Result<Value, ClaudeError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| ClaudeError::Unavailable("ANTHROPIC_API_KEY not set".to_string()))?;
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": messages,
        });
        if let Some(sys) = system {
            body["system"] = Value::String(sys.to_string());
        }
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| ClaudeError::Http(e.to_string()))?;
        let resp = client
            .post(&self.base_url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| ClaudeError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            // Body mitnehmen → Aufrufer kann Anthropic-Fehlertexte auswerten.
            let text = resp.text().await.unwrap_or_default();
            return Err(ClaudeError::Http(format!("HTTP {status}: {text}")));
        }
        let payload: Value = resp
            .json()
            .await
            .map_err(|e| ClaudeError::Http(e.to_string()))?;
        Ok(payload.get("content").cloned().unwrap_or(Value::Null))
    }
}

fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn create_message_liefert_content_array() {
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

        let client = ClaudeClient::new(
            Some("k".into()),
            Some(format!("{}/messages", server.uri())),
            None,
            None,
        );
        let content = client
            .create_message(None, json!([{"role": "user", "content": "Hi"}]), 100)
            .await
            .unwrap();
        assert_eq!(content, json!([{"type": "text", "text": "Antwort"}]));
    }

    #[tokio::test]
    async fn ohne_key_unavailable() {
        // Expliziter leerer Key + kein Env → Unavailable. (Base-URL gesetzt, damit
        // kein echter Call passiert, falls Env-Key existiert.)
        let client = ClaudeClient::new(
            Some(String::new()),
            Some("http://127.0.0.1:1/messages".into()),
            None,
            None,
        );
        if client.api_key.is_some() {
            // ANTHROPIC_API_KEY ist in der Umgebung gesetzt → Test überspringen.
            return;
        }
        let err = client
            .create_message(None, json!([]), 10)
            .await
            .unwrap_err();
        assert!(matches!(err, ClaudeError::Unavailable(_)));
    }

    #[tokio::test]
    async fn http_fehler_traegt_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_string("credit balance is too low"))
            .mount(&server)
            .await;
        let client = ClaudeClient::new(
            Some("k".into()),
            Some(format!("{}/messages", server.uri())),
            None,
            None,
        );
        let err = client
            .create_message(None, json!([]), 10)
            .await
            .unwrap_err();
        match err {
            ClaudeError::Http(msg) => assert!(msg.contains("credit balance is too low")),
            other => panic!("erwartete Http, war {other:?}"),
        }
    }
}
