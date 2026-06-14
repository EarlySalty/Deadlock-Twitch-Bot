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
const RESOLVE_USER_PATH: &str = "/internal/master/v1/discord/resolve-user";
const ADD_ROLE_PATH: &str = "/internal/master/v1/discord/member/add-role";
const MEMBERS_PATH: &str = "/internal/master/v1/discord/members";
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

/// Ein Guild-Member aus `GET /discord/members`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GuildMember {
    pub id: String,
    pub name: String,
    pub global_name: Option<String>,
    pub nick: Option<String>,
}

/// Aufgelöster Discord-User aus `POST /discord/resolve-user`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ResolvedDiscordUser {
    pub found: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

impl ResolvedDiscordUser {
    /// Anzeigename nach Python-Präzedenz (`resolve_discord_display_name`):
    /// global_name → display_name → name; leere Strings zählen nicht.
    pub fn preferred_display_name(&self) -> Option<String> {
        [&self.global_name, &self.display_name, &self.name]
            .into_iter()
            .filter_map(|v| v.as_deref())
            .map(str::trim)
            .find(|s| !s.is_empty())
            .map(str::to_string)
    }
}

/// Broker-Antwort-Envelope (`_success_response`): `{"ok": .., "result": ..}`.
#[derive(Debug, serde::Deserialize)]
struct BrokerEnvelope<T> {
    ok: bool,
    #[serde(default = "Option::default")]
    result: Option<T>,
}

#[derive(serde::Serialize)]
struct ResolveUserRequest {
    user_id: u64,
}

#[derive(serde::Serialize)]
struct AddRoleRequest {
    guild_id: u64,
    user_id: u64,
    role_id: u64,
    reason: String,
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

    /// Löst einen Discord-User über den Broker auf (read-only).
    /// `Ok(None)` = User nicht gefunden; Fehler nur bei Transport/HTTP-Problemen.
    pub async fn resolve_user(
        &self,
        user_id: u64,
    ) -> Result<Option<ResolvedDiscordUser>, DiscordError> {
        let payload = ResolveUserRequest { user_id };
        let key = Self::idempotency_key("resolve-user", &payload);
        let resp = self
            .post_with_retry(RESOLVE_USER_PATH, &payload, &key)
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        let envelope: BrokerEnvelope<ResolvedDiscordUser> = resp.json().await?;
        Ok(envelope
            .result
            .filter(|user| envelope.ok && user.found))
    }

    /// Fügt einem Guild-Mitglied eine Rolle hinzu
    /// (`POST /discord/member/add-role`, idempotent auf Broker-Seite).
    pub async fn add_member_role(
        &self,
        guild_id: u64,
        user_id: u64,
        role_id: u64,
        reason: &str,
    ) -> Result<(), DiscordError> {
        let payload = AddRoleRequest {
            guild_id,
            user_id,
            role_id,
            reason: reason.to_string(),
        };
        let key = Self::idempotency_key("add-role", &payload);
        let resp = self.post_with_retry(ADD_ROLE_PATH, &payload, &key).await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        Ok(())
    }
    /// Holt alle nicht-Bot-Guild-Member vom Broker (loopback, kein Token).
    /// `GET /internal/master/v1/discord/members`
    pub async fn list_members(&self) -> Result<Vec<GuildMember>, DiscordError> {
        let url = format!("{}{}", self.base_url, MEMBERS_PATH);
        let resp = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(DiscordError::Http)?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        #[derive(serde::Deserialize)]
        struct Envelope {
            members: Vec<GuildMember>,
        }
        let env: Envelope = resp.json().await.map_err(DiscordError::Http)?;
        Ok(env.members)
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
    async fn akzeptiert_numerische_message_id_vom_rust_broker() {
        // Regression: Der Rust-dl-broker liefert message_id als u64-Zahl (nicht
        // als String wie das alte Python-master_broker). Ohne tolerantes Decoding
        // scheiterte resp.json() mit „error decoding response body" → announce_live
        // gab None zurück → message_id nie persistiert → Doppel-Live-Ping.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-rich-message"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "request_id": "r-1",
                "idempotency_key": "send-abc",
                "cached": false,
                "result": { "channel_id": 123_i64, "message_id": 1_402_558_159_123_456_789_i64 },
                "error": serde_json::Value::Null
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let result = relay
            .send_rich_message(sample_send_payload())
            .await
            .unwrap();
        assert_eq!(result.result.message_id, "1402558159123456789");
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
    async fn resolve_user_parst_treffer_und_praeferenz() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/resolve-user"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {
                    "found": true,
                    "name": "rawname",
                    "global_name": "Globaler Name",
                    "display_name": "Server-Name"
                }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let user = relay.resolve_user(123).await.unwrap().unwrap();
        assert_eq!(
            user.preferred_display_name().as_deref(),
            Some("Globaler Name"),
            "global_name hat Vorrang"
        );
    }

    #[tokio::test]
    async fn resolve_user_nicht_gefunden_gibt_none() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/resolve-user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "found": false }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        assert!(relay.resolve_user(123).await.unwrap().is_none());
    }

    #[test]
    fn preferred_display_name_fallback_kette() {
        let user = ResolvedDiscordUser {
            found: true,
            name: Some("rawname".to_string()),
            global_name: Some("  ".to_string()),
            display_name: None,
        };
        assert_eq!(user.preferred_display_name().as_deref(), Some("rawname"));
    }

    #[tokio::test]
    async fn add_member_role_sendet_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/member/add-role"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "changed": true }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        relay
            .add_member_role(1, 2, 3, "Twitch-Bot erfolgreich autorisiert")
            .await
            .unwrap();
        let received = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["guild_id"], 1);
        assert_eq!(body["user_id"], 2);
        assert_eq!(body["role_id"], 3);
        assert_eq!(body["reason"], "Twitch-Bot erfolgreich autorisiert");
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
