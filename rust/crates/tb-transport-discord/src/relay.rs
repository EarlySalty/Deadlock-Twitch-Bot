//! BrokerRelay — HTTP-Client für den Master-Broker (Discord-Bridge).
//!
//! Sendet Nachrichten via POST mit deterministischem Idempotency-Key
//! und einfacher Retry-Logik (max. 2 Versuche bei Timeout).

use crate::backend::{
    DeleteMessage, DiscordBackend, DiscordError, EditRichMessage, SendAlertEmbed, SendResult,
    SendRichMessage, SendUserDm,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tb_config::BrokerConfig;

const SEND_PATH: &str = "/internal/master/v1/discord/send-rich-message";
const EDIT_PATH: &str = "/internal/master/v1/discord/edit-rich-message";
const DELETE_PATH: &str = "/internal/master/v1/discord/delete-message";
const RESOLVE_USER_PATH: &str = "/internal/master/v1/discord/resolve-user";
const ADD_ROLE_PATH: &str = "/internal/master/v1/discord/member/add-role";
const REMOVE_ROLE_PATH: &str = "/internal/master/v1/discord/member/remove-role";
const CREATE_ROLE_PATH: &str = "/internal/master/v1/discord/role/create";
const CREATE_INVITE_PATH: &str = "/internal/master/v1/discord/create-invite";
const SEND_DM_PATH: &str = "/internal/master/v1/discord/send-dm";
const MEMBERS_PATH: &str = "/internal/master/v1/discord/members";
const ROLES_PATH: &str = "/internal/master/v1/discord/roles";
const MESSAGE_REACTIONS_PATH: &str = "/internal/master/v1/discord/message-reactions";
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
    #[serde(default, deserialize_with = "deserialize_option_u64_flexible")]
    pub guild_id: Option<u64>,
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

/// Ergebnis von `POST /discord/create-invite`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InviteInfo {
    pub invite_url: String,
    pub code: String,
    #[serde(deserialize_with = "deserialize_u64_flexible")]
    pub channel_id: u64,
    #[serde(deserialize_with = "deserialize_u64_flexible")]
    pub guild_id: u64,
}

/// Reaktionszähler einer Discord-Nachricht.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct MessageReaction {
    pub emoji: String,
    pub count: i32,
}

/// Antwort von `GET /discord/message-reactions`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct MessageReactions {
    pub found: bool,
    #[serde(default)]
    pub reactions: Vec<MessageReaction>,
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

#[derive(serde::Serialize)]
struct CreateRoleRequest {
    guild_id: String,
    name: String,
    mentionable: bool,
    reason: String,
}

#[derive(serde::Serialize)]
struct CreateInviteRequest {
    channel_id: u64,
    reason: String,
}

/// Antwort auf `POST /discord/role/create`. `role_id` kann je nach Broker-
/// Serialisierung als JSON-Number ODER als String (große Snowflakes)
/// ankommen — beides wird auf u64 normalisiert.
#[derive(serde::Deserialize)]
struct CreateRoleResponse {
    #[serde(deserialize_with = "deserialize_u64_flexible")]
    role_id: u64,
}

#[derive(Debug, serde::Deserialize)]
struct RoleInfo {
    #[serde(deserialize_with = "deserialize_u64_flexible")]
    id: u64,
    name: String,
}

#[derive(Debug, serde::Deserialize)]
struct RolesResponse {
    #[serde(default)]
    roles: Vec<RoleInfo>,
}

/// Akzeptiert eine u64 sowohl als JSON-Number als auch als dezimalen String.
fn deserialize_u64_flexible<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    use serde::Deserialize as _;
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| D::Error::custom("role_id ist keine gültige u64")),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<u64>()
            .map_err(|_| D::Error::custom("role_id-String ist keine gültige u64")),
        _ => Err(D::Error::custom("role_id hat unerwarteten Typ")),
    }
}

fn deserialize_option_u64_flexible<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    use serde::Deserialize as _;
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .map(Some)
            .ok_or_else(|| D::Error::custom("guild_id ist keine gültige u64")),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed
                    .parse::<u64>()
                    .map(Some)
                    .map_err(|_| D::Error::custom("guild_id-String ist keine gültige u64"))
            }
        }
        Some(_) => Err(D::Error::custom("guild_id hat unerwarteten Typ")),
    }
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
        Ok(envelope.result.filter(|user| envelope.ok && user.found))
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

    /// Entfernt einem Guild-Mitglied eine Rolle
    /// (`POST /discord/member/remove-role`, idempotent auf Broker-Seite).
    /// Gleiche Payload-Form wie [`add_member_role`](Self::add_member_role).
    pub async fn remove_member_role(
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
        let key = Self::idempotency_key("remove-role", &payload);
        let resp = self
            .post_with_retry(REMOVE_ROLE_PATH, &payload, &key)
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        Ok(())
    }

    /// Erstellt einen permanenten Discord-Invite für den angegebenen Kanal.
    pub async fn create_invite(
        &self,
        channel_id: u64,
        reason: &str,
    ) -> Result<InviteInfo, DiscordError> {
        let payload = CreateInviteRequest {
            channel_id,
            reason: reason.to_string(),
        };
        let key = Self::idempotency_key("create-invite", &payload);
        let resp = self
            .post_with_retry(CREATE_INVITE_PATH, &payload, &key)
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        let envelope: BrokerEnvelope<InviteInfo> = resp.json().await?;
        envelope.result.filter(|_| envelope.ok).ok_or_else(|| {
            DiscordError::BrokerError {
                status: 502,
                body: "missing create-invite result".to_string(),
            }
        })
    }

    /// Legt eine Discord-Rolle über den Broker an
    /// (`POST /discord/role/create`) und liefert die neue `role_id`.
    pub async fn create_role(
        &self,
        guild_id: u64,
        name: &str,
        mentionable: bool,
        reason: &str,
    ) -> Result<u64, DiscordError> {
        let payload = CreateRoleRequest {
            guild_id: guild_id.to_string(),
            name: name.to_string(),
            mentionable,
            reason: reason.to_string(),
        };
        let key = Self::idempotency_key("create-role", &payload);
        let resp = self
            .post_with_retry(CREATE_ROLE_PATH, &payload, &key)
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        let envelope: BrokerEnvelope<CreateRoleResponse> = resp.json().await?;
        let parsed = envelope.result.filter(|_| envelope.ok).ok_or(DiscordError::BrokerError {
            status: 502,
            body: "missing create-role result".to_string(),
        })?;
        Ok(parsed.role_id)
    }

    /// Sucht eine bestehende Rolle über den read-only Diagnose-Endpunkt.
    /// Unterstützt den laufenden Python-Broker, der Rollen lesen, aber noch
    /// nicht selbst erstellen kann.
    pub async fn find_role_by_name(
        &self,
        guild_id: u64,
        name: &str,
    ) -> Result<Option<u64>, DiscordError> {
        let url = format!("{}{}", self.base_url, ROLES_PATH);
        let resp = self
            .client
            .get(&url)
            .query(&[("guild_id", guild_id.to_string())])
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        let parsed: RolesResponse = resp.json().await?;
        Ok(parsed
            .roles
            .into_iter()
            .find(|role| role.name.eq_ignore_ascii_case(name.trim()))
            .map(|role| role.id))
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

    /// Liest die Reaktionszähler einer Discord-Nachricht über den Master-Broker.
    pub async fn get_message_reactions(
        &self,
        channel_id: &str,
        message_id: &str,
    ) -> Result<MessageReactions, DiscordError> {
        let url = format!("{}{}", self.base_url, MESSAGE_REACTIONS_PATH);
        let resp = self
            .client
            .get(&url)
            .header("X-Internal-Token", &self.token)
            .query(&[("channel_id", channel_id), ("message_id", message_id)])
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        Ok(resp.json().await?)
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

    async fn delete_message(&self, payload: DeleteMessage) -> Result<(), DiscordError> {
        let key = Self::idempotency_key("delete", &payload);
        let resp = self.post_with_retry(DELETE_PATH, &payload, &key).await?;

        if resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }

        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Err(DiscordError::BrokerError { status, body })
    }

    async fn send_user_dm(&self, payload: SendUserDm) -> Result<SendResult, DiscordError> {
        let key = Self::idempotency_key("send-dm", &payload);
        let resp = self.post_with_retry(SEND_DM_PATH, &payload, &key).await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        let result: SendResult = resp.json().await?;
        Ok(result)
    }

    async fn send_alert_embed(&self, payload: SendAlertEmbed) -> Result<SendResult, DiscordError> {
        let key = Self::idempotency_key("alert-embed", &payload);
        let resp = self.post_with_retry(SEND_PATH, &payload, &key).await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiscordError::BrokerError { status, body });
        }
        let result: SendResult = resp.json().await?;
        Ok(result)
    }

    async fn remove_member_role(
        &self,
        guild_id: u64,
        user_id: u64,
        role_id: u64,
        reason: &str,
    ) -> Result<(), DiscordError> {
        BrokerRelay::remove_member_role(self, guild_id, user_id, role_id, reason).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeleteMessage;
    use tb_config::BrokerConfig;
    use wiremock::matchers::{header, method, path, query_param};
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
            components: None,
            allowed_role_ids: vec![99],
            view_spec: None,
        }
    }

    #[tokio::test]
    async fn delete_message_sendet_auth_payload_und_idempotency_key() {
        let server = MockServer::start().await;
        let payload = DeleteMessage {
            channel_id: 123,
            message_id: "123456789012345678".to_string(),
            reason: "Shadow-Review verworfen".to_string(),
        };
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/delete-message"))
            .and(header("X-Internal-Token", "test-token"))
            .and(header(
                "X-Idempotency-Key",
                "delete-b327ea2ca5251b1edb1c53de25e8137505f21173d785600b",
            ))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        relay.delete_message(payload).await.unwrap();

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["channel_id"], 123);
        assert_eq!(body["message_id"], "123456789012345678");
        assert_eq!(body["reason"], "Shadow-Review verworfen");
    }

    #[tokio::test]
    async fn delete_message_behandelt_404_als_bereits_geloescht() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/delete-message"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let result = relay
            .delete_message(DeleteMessage {
                channel_id: 123,
                message_id: "123456789012345679".to_string(),
                reason: "Shadow-Review verworfen".to_string(),
            })
            .await;

        assert!(result.is_ok());
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
    async fn message_reactions_sendet_ids_und_parst_antwort() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/master/v1/discord/message-reactions"))
            .and(query_param("channel_id", "1374364800817303632"))
            .and(query_param("message_id", "1402558159123456789"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "found": true,
                "reactions": [
                    {"emoji": "👍", "count": 2},
                    {"emoji": "👎", "count": 1}
                ]
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let result = relay
            .get_message_reactions("1374364800817303632", "1402558159123456789")
            .await
            .unwrap();

        assert!(result.found);
        assert_eq!(result.reactions.len(), 2);
        assert_eq!(result.reactions[0].emoji, "👍");
        assert_eq!(result.reactions[0].count, 2);
    }

    #[tokio::test]
    async fn message_reactions_502_ist_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/master/v1/discord/message-reactions"))
            .respond_with(ResponseTemplate::new(502).set_body_string("discord unavailable"))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let error = relay.get_message_reactions("1", "2").await.unwrap_err();

        assert!(matches!(
            error,
            DiscordError::BrokerError { status: 502, .. }
        ));
    }

    #[tokio::test]
    async fn message_reactions_parst_nicht_gefundene_nachricht() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/master/v1/discord/message-reactions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "found": false,
                "reactions": []
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let result = relay.get_message_reactions("1", "2").await.unwrap();

        assert!(!result.found);
        assert!(result.reactions.is_empty());
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
    async fn create_role_parst_role_id_als_string() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/role/create"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "role_id": "1313624729466441769" }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let role_id = relay
            .create_role(1, "foo ist live", true, "Auto-created")
            .await
            .unwrap();
        assert_eq!(role_id, 1313624729466441769);

        let received = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["guild_id"], "1");
        assert_eq!(body["name"], "foo ist live");
        assert_eq!(body["mentionable"], true);
        assert_eq!(body["reason"], "Auto-created");
    }

    #[tokio::test]
    async fn create_role_parst_role_id_als_number() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/role/create"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "role_id": 42u64 }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let role_id = relay
            .create_role(1, "bar ist live", true, "Auto-created")
            .await
            .unwrap();
        assert_eq!(role_id, 42);
    }

    #[tokio::test]
    async fn find_role_by_name_findet_python_broker_shape() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/master/v1/discord/roles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "guild_id": "1",
                "roles": [
                    {"id": "42", "name": "nani ist live", "position": 4, "member_count": 0}
                ]
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        assert_eq!(
            relay.find_role_by_name(1, "NANI IST LIVE").await.unwrap(),
            Some(42)
        );
    }

    #[tokio::test]
    async fn send_user_dm_sendet_payload_und_idempotency_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-dm"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "user_id": 42, "channel_id": 7, "message_id": "dm-1" }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let payload = SendUserDm {
            user_id: 42,
            content: "Token-Fehler — bitte neu verbinden.".to_string(),
        };
        let result = relay.send_user_dm(payload).await.unwrap();
        assert!(result.ok);
        assert_eq!(result.result.message_id, "dm-1");

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        assert!(received[0].headers.contains_key("x-idempotency-key"));
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["user_id"], 42);
        assert_eq!(body["content"], "Token-Fehler — bitte neu verbinden.");
    }

    #[tokio::test]
    async fn send_alert_embed_sendet_rich_message_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-rich-message"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": "alert-9" }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let payload = SendAlertEmbed {
            channel_id: 555,
            content: Some("@here".to_string()),
            embed: serde_json::json!({"title": "Alert", "description": "Etwas ist kaputt"}),
            allowed_role_ids: vec![],
        };
        let result = relay.send_alert_embed(payload).await.unwrap();
        assert_eq!(result.result.message_id, "alert-9");

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        assert!(received[0].headers.contains_key("x-idempotency-key"));
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["channel_id"], 555);
        assert_eq!(body["content"], "@here");
        assert_eq!(body["embed"]["title"], "Alert");
        // Leere Allowlist wird weggelassen (skip_serializing_if).
        assert!(body.get("allowed_role_ids").is_none());
    }

    #[tokio::test]
    async fn remove_member_role_sendet_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/member/remove-role"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "guild_id": 1, "user_id": 2, "role_id": 3 }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        DiscordBackend::remove_member_role(&relay, 1, 2, 3, "Partner deautorisiert")
            .await
            .unwrap();
        let received = server.received_requests().await.unwrap();
        assert!(received[0].headers.contains_key("x-idempotency-key"));
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["guild_id"], 1);
        assert_eq!(body["user_id"], 2);
        assert_eq!(body["role_id"], 3);
        assert_eq!(body["reason"], "Partner deautorisiert");
    }

    #[test]
    fn add_und_remove_role_haben_unterschiedliche_idempotency_keys() {
        // Gleiche Felder, aber unterschiedliche Aktion → unterschiedlicher Key,
        // damit der Broker add/remove nicht als denselben idempotenten Vorgang
        // dedupliziert.
        let payload = AddRoleRequest {
            guild_id: 1,
            user_id: 2,
            role_id: 3,
            reason: "r".to_string(),
        };
        let add = BrokerRelay::idempotency_key("add-role", &payload);
        let remove = BrokerRelay::idempotency_key("remove-role", &payload);
        assert_ne!(add, remove);
    }

    #[tokio::test]
    async fn create_invite_sendet_payload_und_parst_antwort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/create-invite"))
            .and(header("X-Internal-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {
                    "invite_url": "https://discord.gg/abc",
                    "code": "abc",
                    "channel_id": "123",
                    "guild_id": "456"
                }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let invite = relay.create_invite(123, "streamer-invite:test").await.unwrap();
        assert_eq!(invite.invite_url, "https://discord.gg/abc");
        assert_eq!(invite.code, "abc");
        assert_eq!(invite.channel_id, 123);
        assert_eq!(invite.guild_id, 456);
        let received = server.received_requests().await.unwrap();
        assert!(received[0].headers.contains_key("x-idempotency-key"));
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["channel_id"], 123);
        assert_eq!(body["reason"], "streamer-invite:test");
    }

    #[tokio::test]
    async fn alert_embed_loest_keinen_dm_aus() {
        // Negativ-Guard: Nicht-DM-Aktionen (hier: Alert-Embed, stellvertretend
        // für die bewusst gedroppten Approval-/Social-/Clip-DMs) dürfen NIE den
        // send-dm-Endpunkt treffen. send-dm ist allein dem Token-Lifecycle-DM
        // vorbehalten (`send_user_dm`).
        let server = MockServer::start().await;
        // send-dm explizit verboten: 0 erwartete Treffer.
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-dm"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-rich-message"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": "x" }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        relay
            .send_alert_embed(SendAlertEmbed {
                channel_id: 1,
                content: None,
                embed: serde_json::json!({"title": "Approval"}),
                allowed_role_ids: vec![],
            })
            .await
            .unwrap();
        // server.verify() bestätigt: send-dm-Mock wurde 0-mal getroffen.
        server.verify().await;
    }

    #[tokio::test]
    async fn headless_noop_neue_aktionen_geben_ok() {
        use crate::noop::HeadlessNoop;
        let noop = HeadlessNoop;
        let dm = noop
            .send_user_dm(SendUserDm {
                user_id: 1,
                content: "x".to_string(),
            })
            .await
            .unwrap();
        assert!(dm.ok);
        let alert = noop
            .send_alert_embed(SendAlertEmbed {
                channel_id: 1,
                content: None,
                embed: serde_json::Value::Null,
                allowed_role_ids: vec![],
            })
            .await
            .unwrap();
        assert!(alert.ok);
        DiscordBackend::remove_member_role(&noop, 1, 2, 3, "x")
            .await
            .unwrap();
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
            components: None,
            view_spec: None,
        };
        noop.edit_rich_message(edit).await.unwrap();
    }
}
