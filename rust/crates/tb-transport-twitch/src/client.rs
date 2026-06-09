//! HelixClient — dünner reqwest-Wrapper für Twitch Helix.
//!
//! Hält einen geteilten Arc<reqwest::Client> und erneuert den App-Token
//! automatisch bei Bedarf (in-memory, kein persistenter Cache).

use crate::token::{fetch_app_token, AppToken, TokenError};
use reqwest::{Client, RequestBuilder};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

/// Fehlertyp für Helix-Operationen.
#[derive(Debug, Error)]
pub enum HelixError {
    #[error("Token-Fehler: {0}")]
    Token(#[from] TokenError),
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Helix-Status {status}")]
    Status { status: u16 },
}

/// Konfiguration für den HelixClient.
#[derive(Debug, Clone)]
pub struct HelixConfig {
    /// Twitch Client-ID.
    pub client_id: String,
    /// Twitch Client-Secret.
    pub client_secret: String,
    /// OAuth-Token-URL (überschreibbar für Tests).
    pub token_url: String,
    /// Helix-Basis-URL (überschreibbar für Tests).
    pub helix_base: String,
}

impl HelixConfig {
    /// Erstellt eine HelixConfig mit den Standard-Twitch-URLs.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token_url: "https://id.twitch.tv/oauth2/token".to_string(),
            helix_base: "https://api.twitch.tv/helix".to_string(),
        }
    }
}

/// HTTP-Client für die Twitch Helix API.
///
/// Verwaltet den App-Token intern (auto-refresh bei Ablauf).
#[derive(Clone)]
pub struct HelixClient {
    http: Arc<Client>,
    config: HelixConfig,
    token: Arc<Mutex<Option<AppToken>>>,
    /// Kategorie-Name (lowercase) → game_id (Python: `_category_cache`).
    pub(crate) category_cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl HelixClient {
    /// Erstellt einen neuen HelixClient.
    pub fn new(config: HelixConfig) -> Result<Self, reqwest::Error> {
        let http = Client::builder().build()?;
        Ok(Self {
            http: Arc::new(http),
            config,
            token: Arc::new(Mutex::new(None)),
            category_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Gibt den aktuellen Access-Token zurück, holt bei Bedarf einen neuen.
    pub async fn access_token(&self) -> Result<String, HelixError> {
        let mut guard = self.token.lock().await;
        let now = crate::token::unix_now();

        if let Some(ref t) = *guard {
            if !t.needs_refresh(now) {
                return Ok(t.access_token.clone());
            }
        }

        let fresh = fetch_app_token(
            &self.http,
            &self.config.token_url,
            &self.config.client_id,
            &self.config.client_secret,
        )
        .await?;

        let token_str = fresh.access_token.clone();
        *guard = Some(fresh);
        Ok(token_str)
    }

    /// Erstellt einen vorbereiteten GET-Request an einen Helix-Endpunkt.
    ///
    /// `path` — z. B. `"/streams"` (ohne Basis-URL).
    pub async fn get(&self, path: &str) -> Result<RequestBuilder, HelixError> {
        let token = self.access_token().await?;
        let url = format!("{}{}", self.config.helix_base, path);
        Ok(self
            .http
            .get(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {token}")))
    }

    /// Erstellt einen vorbereiteten POST-Request (App-Token; per
    /// `.header("Authorization", …)` überschreibbar).
    pub async fn post(&self, path: &str) -> Result<RequestBuilder, HelixError> {
        let token = self.access_token().await?;
        let url = format!("{}{}", self.config.helix_base, path);
        Ok(self
            .http
            .post(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {token}")))
    }

    /// Erstellt einen vorbereiteten DELETE-Request.
    pub async fn delete(&self, path: &str) -> Result<RequestBuilder, HelixError> {
        let token = self.access_token().await?;
        let url = format!("{}{}", self.config.helix_base, path);
        Ok(self
            .http
            .delete(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {token}")))
    }

    /// Holt Twitch-User-Infos für eine Liste von Logins.
    ///
    /// Gibt eine Map `login (lowercase) → TwitchUser` zurück.
    /// Leere Logins werden ignoriert. Unbekannte Logins erscheinen nicht in der Map.
    pub async fn get_users(
        &self,
        logins: &[&str],
    ) -> Result<std::collections::HashMap<String, TwitchUser>, HelixError> {
        if logins.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let params: Vec<(&str, &str)> = logins.iter().map(|l| ("login", *l)).collect();
        let builder = self.get("/users").await?;
        let resp = builder.query(&params).send().await?;
        let body: HelixUsersResponse = resp.json().await?;
        Ok(body
            .data
            .into_iter()
            .map(|u| (u.login.to_lowercase(), u))
            .collect())
    }
}

/// Twitch-User-Daten aus der Helix-API.
#[derive(Debug, serde::Deserialize)]
pub struct TwitchUser {
    pub id: String,
    pub login: String,
    pub display_name: String,
}

#[derive(Debug, serde::Deserialize)]
struct HelixUsersResponse {
    pub data: Vec<TwitchUser>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(token_url: &str, helix_base: &str) -> HelixConfig {
        HelixConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            token_url: token_url.to_string(),
            helix_base: helix_base.to_string(),
        }
    }

    #[tokio::test]
    async fn token_fetch_sendet_form_encoded_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=test-client-id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-abc",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let token = client.access_token().await.unwrap();
        assert_eq!(token, "tok-abc");
    }

    #[tokio::test]
    async fn helix_request_setzt_korrekte_header() {
        let server = MockServer::start().await;

        // Token-Endpunkt
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-helix",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        // Helix-Endpunkt
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(header("Client-Id", "test-client-id"))
            .and(header("Authorization", "Bearer tok-helix"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let builder = client.get("/streams").await.unwrap();
        let resp = builder.send().await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn token_wird_nicht_neu_geholt_wenn_noch_gueltig() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-cached",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let t1 = client.access_token().await.unwrap();
        let t2 = client.access_token().await.unwrap();
        assert_eq!(t1, t2);
        server.verify().await;
    }
}
