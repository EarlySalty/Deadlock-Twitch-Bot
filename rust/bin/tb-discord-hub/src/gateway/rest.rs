//! Schmaler Discord-REST-Client für das Gateway (Gateway-URL, Befehls-
//! Registrierung, Interaction-Antworten, Rollen-Abfrage).

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::permissions::{Role, MANAGE_GUILD};
use super::protocol::snowflake;

/// Standard-Basisadresse der Discord-API.
pub const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

/// Ephemeral-Flag für Interaction-Antworten.
pub const FLAG_EPHEMERAL: u64 = 1 << 6;

/// Fehler bei REST-Aufrufen.
#[derive(Debug, thiserror::Error)]
pub enum RestError {
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Discord antwortet {status}: {body}")]
    Status { status: u16, body: String },
    #[error("unerwartete Antwort: {0}")]
    Decode(String),
}

impl RestError {
    /// 401/403: Token ungültig bzw. keine Berechtigung – nicht wiederholen.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, RestError::Status { status: 401, .. })
    }
}

/// Antwort von `GET /gateway/bot`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GatewayBot {
    pub url: String,
    #[serde(default)]
    pub session_start_limit: Option<SessionStartLimit>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionStartLimit {
    pub remaining: u64,
    /// Millisekunden bis zum Zurücksetzen.
    pub reset_after: u64,
}

/// Grunddaten einer Guild für `/setup`.
#[derive(Debug, Clone)]
pub struct GuildInfo {
    pub owner_id: Option<u64>,
    pub name: String,
    pub roles: Vec<Role>,
}

#[derive(Clone)]
pub struct DiscordRest {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl std::fmt::Debug for DiscordRest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordRest")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl DiscordRest {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!(
                "DiscordBot (https://github.com/EarlySalty/Deadlock-Twitch-Bot, ",
                env!("CARGO_PKG_VERSION"),
                ")"
            ))
            .build()
            .unwrap_or_default();
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    /// Führt eine Anfrage aus; bei 429 wird einmal nach `retry_after` wiederholt.
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        auth: bool,
    ) -> Result<Value, RestError> {
        let url = format!("{}{}", self.base, path);
        for attempt in 0..2 {
            let mut req = self.http.request(method.clone(), &url);
            if auth {
                req = req.header("Authorization", format!("Bot {}", self.token));
            }
            if let Some(b) = body {
                req = req.json(b);
            }
            let resp = req.send().await?;
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            if status == 429 && attempt == 0 {
                let wait = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("retry_after").and_then(Value::as_f64))
                    .unwrap_or(1.0)
                    .clamp(0.0, 10.0);
                tracing::warn!(path, wait, "Discord-Rate-Limit, warte vor Wiederholung");
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                continue;
            }
            if !(200..300).contains(&status) {
                return Err(RestError::Status {
                    status,
                    body: text.chars().take(500).collect(),
                });
            }
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&text).map_err(|e| RestError::Decode(e.to_string()));
        }
        Err(RestError::Status {
            status: 429,
            body: "Rate-Limit nach Wiederholung".to_string(),
        })
    }

    /// `GET /gateway/bot`
    pub async fn gateway_bot(&self) -> Result<GatewayBot, RestError> {
        let v = self
            .send(reqwest::Method::GET, "/gateway/bot", None, true)
            .await?;
        serde_json::from_value(v).map_err(|e| RestError::Decode(e.to_string()))
    }

    /// Registriert `/setup` global (`PUT /applications/{id}/commands`).
    pub async fn register_commands(&self, application_id: u64) -> Result<(), RestError> {
        let body = setup_command_definition();
        self.send(
            reqwest::Method::PUT,
            &format!("/applications/{application_id}/commands"),
            Some(&body),
            true,
        )
        .await
        .map(|_| ())
    }

    /// Owner und Rollen einer Guild (`GET /guilds/{id}`).
    pub async fn guild(&self, guild_id: u64) -> Result<GuildInfo, RestError> {
        let v = self
            .send(
                reqwest::Method::GET,
                &format!("/guilds/{guild_id}"),
                None,
                true,
            )
            .await?;
        let roles: Vec<Role> = match v.get("roles") {
            Some(r) => {
                serde_json::from_value(r.clone()).map_err(|e| RestError::Decode(e.to_string()))?
            }
            None => {
                // Rückfall: Rollen separat laden.
                let r = self
                    .send(
                        reqwest::Method::GET,
                        &format!("/guilds/{guild_id}/roles"),
                        None,
                        true,
                    )
                    .await?;
                serde_json::from_value(r).map_err(|e| RestError::Decode(e.to_string()))?
            }
        };
        Ok(GuildInfo {
            owner_id: snowflake(v.get("owner_id")),
            name: v
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect(),
            roles,
        })
    }

    /// Rollen-IDs eines Mitglieds (`GET /guilds/{id}/members/{user}`).
    pub async fn member_role_ids(
        &self,
        guild_id: u64,
        user_id: u64,
    ) -> Result<Vec<u64>, RestError> {
        let v = self
            .send(
                reqwest::Method::GET,
                &format!("/guilds/{guild_id}/members/{user_id}"),
                None,
                true,
            )
            .await?;
        let roles = v
            .get("roles")
            .and_then(Value::as_array)
            .ok_or_else(|| RestError::Decode("Mitglied ohne roles".to_string()))?;
        Ok(roles.iter().filter_map(|r| snowflake(Some(r))).collect())
    }

    /// Antwortet sofort auf eine Interaction (`POST /interactions/{id}/{token}/callback`).
    pub async fn interaction_callback(
        &self,
        interaction_id: u64,
        token: &str,
        body: &Value,
    ) -> Result<(), RestError> {
        self.send(
            reqwest::Method::POST,
            &format!("/interactions/{interaction_id}/{token}/callback"),
            Some(body),
            false,
        )
        .await
        .map(|_| ())
    }

    /// Bearbeitet die (aufgeschobene) Originalantwort einer Interaction.
    pub async fn edit_original_response(
        &self,
        application_id: u64,
        token: &str,
        content: &str,
    ) -> Result<(), RestError> {
        self.send(
            reqwest::Method::PATCH,
            &format!("/webhooks/{application_id}/{token}/messages/@original"),
            Some(&json!({ "content": content, "allowed_mentions": { "parse": [] } })),
            false,
        )
        .await
        .map(|_| ())
    }
}

/// Definition des globalen Befehls `/setup` (Bulk-Overwrite-Liste).
pub fn setup_command_definition() -> Value {
    json!([{
        "name": "setup",
        "type": 1,
        "description": "Check permissions and save this server's Deadlock Hub channels and roles",
        "default_member_permissions": MANAGE_GUILD.to_string(),
        "dm_permission": false,
        "contexts": [0],
        "options": [
            {
                "name": "language",
                "description": "Language for bot messages on this server",
                "type": 3,
                "required": false,
                "choices": [
                    {"name": "German", "value": "de"},
                    {"name": "English", "value": "en"}
                ]
            },
            {
                "name": "announce_channel",
                "description": "Channel for go-live announcements",
                "type": 7,
                "required": false,
                "channel_types": [0]
            },
            {
                "name": "alert_channel",
                "description": "Channel for moderation alerts",
                "type": 7,
                "required": false,
                "channel_types": [0]
            },
            {
                "name": "streamer_role",
                "description": "Role assigned to partnered streamers",
                "type": 8,
                "required": false
            },
            {
                "name": "ping_role",
                "description": "Mentionable role for live pings",
                "type": 8,
                "required": false
            },
            {
                "name": "invite",
                "description": "Discord invite URL used in promotions",
                "type": 3,
                "required": false
            },
            {
                "name": "auto_partner",
                "description": "New streamers become partners of this server automatically",
                "type": 5,
                "required": false
            }
        ]
    }])
}

/// Sofortantwort (type 4, ephemeral).
pub fn ephemeral_message(content: &str) -> Value {
    json!({
        "type": 4,
        "data": {
            "content": content,
            "flags": FLAG_EPHEMERAL,
            "allowed_mentions": { "parse": [] }
        }
    })
}

/// Aufgeschobene Antwort (type 5, ephemeral) – Inhalt folgt per Edit.
pub fn ephemeral_deferred() -> Value {
    json!({ "type": 5, "data": { "flags": FLAG_EPHEMERAL } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn befehlsdefinition() {
        let v = setup_command_definition();
        assert_eq!(v[0]["name"], "setup");
        assert_eq!(v[0]["default_member_permissions"], "32");
        assert_eq!(v[0]["dm_permission"], false);
        let names: Vec<&str> = v[0]["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "language",
                "announce_channel",
                "alert_channel",
                "streamer_role",
                "ping_role",
                "invite",
                "auto_partner"
            ]
        );
    }

    #[test]
    fn antworten_sind_ephemeral() {
        let v = ephemeral_message("hi");
        assert_eq!(v["type"], 4);
        assert_eq!(v["data"]["flags"], 64);
        assert_eq!(ephemeral_deferred()["data"]["flags"], 64);
    }

    #[test]
    fn debug_ohne_token() {
        let r = DiscordRest::new("http://x", "geheim");
        assert!(!format!("{r:?}").contains("geheim"));
    }

    #[tokio::test]
    async fn gateway_bot_mit_auth() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gateway/bot"))
            .and(header("Authorization", "Bot tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "url": "wss://gateway.discord.gg", "shards": 1,
                "session_start_limit": {"total": 1000, "remaining": 999, "reset_after": 1000, "max_concurrency": 1}
            })))
            .mount(&server)
            .await;
        let rest = DiscordRest::new(server.uri(), "tok");
        let gb = rest.gateway_bot().await.unwrap();
        assert_eq!(gb.url, "wss://gateway.discord.gg");
        assert_eq!(gb.session_start_limit.unwrap().remaining, 999);
    }

    #[tokio::test]
    async fn gateway_bot_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gateway/bot"))
            .respond_with(ResponseTemplate::new(401).set_body_string("401: Unauthorized"))
            .mount(&server)
            .await;
        let err = DiscordRest::new(server.uri(), "tok")
            .gateway_bot()
            .await
            .unwrap_err();
        assert!(err.is_unauthorized());
    }

    #[tokio::test]
    async fn rate_limit_wird_wiederholt() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/applications/7/commands"))
            .respond_with(
                ResponseTemplate::new(429)
                    .set_body_json(json!({"retry_after": 0.01, "global": false})),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applications/7/commands"))
            .and(body_partial_json(json!([{"name": "setup"}])))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;
        DiscordRest::new(server.uri(), "tok")
            .register_commands(7)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn guild_und_member() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/guilds/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "1", "owner_id": "2",
                "roles": [{"id":"1","name":"@everyone","permissions":"1024","position":0}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/guilds/1/members/50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"roles": ["10", "11"]})))
            .mount(&server)
            .await;
        let rest = DiscordRest::new(server.uri(), "tok");
        let g = rest.guild(1).await.unwrap();
        assert_eq!(g.owner_id, Some(2));
        assert_eq!(g.roles.len(), 1);
        assert_eq!(rest.member_role_ids(1, 50).await.unwrap(), vec![10, 11]);
    }
}
