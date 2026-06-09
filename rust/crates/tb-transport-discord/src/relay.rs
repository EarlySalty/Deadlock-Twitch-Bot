//! BrokerRelay — HTTP-Client für den Master-Broker (Discord-Bridge).
//!
//! Sendet Nachrichten via POST mit deterministischem Idempotency-Key
//! und einfacher Retry-Logik (max. 2 Versuche bei Timeout).

use crate::backend::{DiscordBackend, DiscordError, EditRichMessage, SendResult, SendRichMessage};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tb_config::BrokerConfig;

const SEND_PATH: &str = "/internal/master/v1/discord/send-rich-message";
const EDIT_PATH: &str = "/internal/master/v1/discord/edit-rich-message";
const TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_WAIT: Duration = Duration::from_secs(2);
const MAX_ATTEMPTS: u32 = 2;

/// HTTP-Client für den Master-Broker. Hält eine geteilte `reqwest::Client`-Instanz.
#[derive(Clone)]
pub struct BrokerRelay {
    client: Arc<Client>,
    base_url: String,
    token: String,
}

impl BrokerRelay {
    /// Erstellt einen neuen BrokerRelay aus der übergebenen Konfiguration.
    pub fn new(config: &BrokerConfig) -> Result<Self, reqwest::Error> {
        let client = Client::builder().timeout(TIMEOUT).build()?;
        Ok(Self {
            client: Arc::new(client),
            base_url: config.base_url.clone(),
            token: config.token.clone(),
        })
    }

    /// Berechnet den deterministischen Idempotency-Key.
    ///
    /// Format: `<prefix>-<sha256hex[..48]>`
    pub fn idempotency_key<T: serde::Serialize>(prefix: &str, payload: &T) -> String {
        let json = serde_json::to_string(payload).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let digest = hex::encode(hasher.finalize());
        format!("{}-{}", prefix, &digest[..48])
    }

    /// Sendet eine POST-Anfrage an den Broker mit Retry-Logik.
    ///
    /// Retry: max. 2 Versuche; bei Timeout 2 s warten; HTTP ≥ 400 → kein Retry;
    /// Netzwerkfehler → kein Retry.
    async fn post_with_retry<T: serde::Serialize>(
        &self,
        path: &str,
        payload: &T,
        idempotency_key: &str,
    ) -> Result<reqwest::Response, DiscordError> {
        let url = format!("{}{}", self.base_url, path);
        let mut last_err: Option<DiscordError> = None;

        for attempt in 0..MAX_ATTEMPTS {
            let result = self
                .client
                .post(&url)
                .header("X-Internal-Token", &self.token)
                .header("X-Idempotency-Key", idempotency_key)
                .json(payload)
                .send()
                .await;

            match result {
                Ok(resp) => return Ok(resp),
                Err(e) if e.is_timeout() => {
                    last_err = Some(DiscordError::Http(e));
                    if attempt + 1 < MAX_ATTEMPTS {
                        tokio::time::sleep(RETRY_WAIT).await;
                    }
                    // Timeout-Versuch → kein weiterer Retry nach dem letzten
                }
                Err(e) => return Err(DiscordError::Http(e)),
            }
        }
        Err(last_err.unwrap())
    }
}

#[async_trait::async_trait]
impl DiscordBackend for BrokerRelay {
    async fn send_rich_message(
        &self,
        payload: SendRichMessage,
    ) -> Result<SendResult, DiscordError> {
        let key = Self::idempotency_key("send", &payload);
        let resp = self.post_with_retry(SEND_PATH, &payload, &key).await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }

        let result: SendResult = resp.json().await?;
        Ok(result)
    }

    async fn edit_rich_message(&self, payload: EditRichMessage) -> Result<(), DiscordError> {
        let key = Self::idempotency_key("edit", &payload);
        let resp = self.post_with_retry(EDIT_PATH, &payload, &key).await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tb_config::BrokerConfig;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: &str) -> BrokerConfig {
        BrokerConfig {
            base_url: base_url.to_string(),
            token: "test-token".to_string(),
        }
    }

    fn sample_send_payload() -> SendRichMessage {
        SendRichMessage {
            channel_id: 12345,
            content: Some("Hallo".to_string()),
            embed: serde_json::json!({"title": "Test"}),
            allowed_role_ids: vec![99],
            view_spec: None,
        }
    }

    #[tokio::test]
    async fn sendet_korrekte_header_und_parst_antwort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-rich-message"))
            .and(header("X-Internal-Token", "test-token"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": "msg-42" }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let result = relay
            .send_rich_message(sample_send_payload())
            .await
            .unwrap();
        assert!(result.ok);
        assert_eq!(result.result.message_id, "msg-42");
    }

    #[tokio::test]
    async fn idempotency_key_header_vorhanden() {
        let server = MockServer::start().await;
        // Prüft, dass der Header gesetzt ist (Wert deterministisch — kein exakter Match nötig)
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-rich-message"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": "msg-99" }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        relay
            .send_rich_message(sample_send_payload())
            .await
            .unwrap();

        // Wenn der Request ankam und kein Panic — Header wurde korrekt gesetzt
        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        assert!(received[0].headers.contains_key("x-idempotency-key"));
    }

    #[test]
    fn idempotency_key_deterministisch_gleicher_payload() {
        let p = sample_send_payload();
        let k1 = BrokerRelay::idempotency_key("send", &p);
        let k2 = BrokerRelay::idempotency_key("send", &p);
        assert_eq!(k1, k2);
        assert!(k1.starts_with("send-"));
        assert_eq!(k1.trim_start_matches("send-").len(), 48);
    }

    #[tokio::test]
    async fn http_400_kein_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .expect(1) // genau 1 Versuch — 4xx löst keinen Retry aus
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let err = relay
            .send_rich_message(sample_send_payload())
            .await
            .unwrap_err();
        assert!(matches!(err, DiscordError::BrokerError { status: 400, .. }));
        server.verify().await;
    }

    #[tokio::test]
    async fn headless_noop_gibt_immer_ok() {
        use crate::noop::HeadlessNoop;
        let noop = HeadlessNoop;
        let result = noop.send_rich_message(sample_send_payload()).await.unwrap();
        assert!(result.ok);
        let edit = EditRichMessage {
            channel_id: 1,
            message_id: "x".to_string(),
            content: None,
            embed: serde_json::Value::Null,
            view_spec: None,
        };
        noop.edit_rich_message(edit).await.unwrap();
    }
}
