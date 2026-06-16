//! HelixClient — dünner reqwest-Wrapper für Twitch Helix.
//!
//! Hält einen geteilten Arc<reqwest::Client> und erneuert den App-Token
//! automatisch bei Bedarf (in-memory, kein persistenter Cache).

use crate::token::{AppTokenManager, TokenError};
use reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Mutex;

/// Request-Timeout für alle Helix-Aufrufe (wie discord relay).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Fehlertyp für Helix-Operationen.
#[derive(Debug, Error)]
pub enum HelixError {
    #[error("Token-Fehler: {0}")]
    Token(#[from] TokenError),
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Helix-Status {status}")]
    Status { status: u16 },
    /// 403 auf `/chat/chatters` — der angefragte Account ist kein Moderator
    /// (bzw. der Token hat `moderator:read:chatters` nicht). Eigener Zweig,
    /// damit der Poller einen Mod-Self-Heal anstoßen kann (Block-6).
    #[error("kein Moderator im Ziel-Channel (403)")]
    NotModerator,
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
    /// Zentraler App-Token-Manager: Cache + In-Flight-Dedupe (B18-5) +
    /// `invalid_client`-Circuit-Breaker (B18-3).
    token: AppTokenManager,
    /// Kategorie-Name (lowercase) → game_id (Python: `_category_cache`).
    pub(crate) category_cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl HelixClient {
    /// Erstellt einen neuen HelixClient.
    pub fn new(config: HelixConfig) -> Result<Self, reqwest::Error> {
        let http = Arc::new(Client::builder().timeout(REQUEST_TIMEOUT).build()?);
        let token = AppTokenManager::new(
            http.clone(),
            config.token_url.clone(),
            config.client_id.clone(),
            config.client_secret.clone(),
        );
        Ok(Self {
            http,
            config,
            token,
            category_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Gibt den aktuellen Access-Token zurück, holt bei Bedarf einen neuen.
    pub async fn access_token(&self) -> Result<String, HelixError> {
        Ok(self.token.access_token().await?)
    }

    /// Ist die App-Auth nach einer `invalid_client`-Ablehnung gesperrt
    /// (15-Min-Cooldown)? Synchron + lock-frei, damit der `tb-bot`-Adapter es
    /// direkt im synchronen `StreamSource::is_auth_blocked`-Trait-Gate des
    /// Pollers (monitoring.py:1207) verdrahten kann, sodass der Tick
    /// Helix-Requests überspringt.
    pub fn is_auth_blocked(&self) -> bool {
        self.token.is_auth_blocked()
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

    /// Roh-Zugriff für Schwester-Module (z. B. den User-Token-Refresh).
    pub(crate) fn http_client(&self) -> &Client {
        &self.http
    }

    pub(crate) fn helix_config(&self) -> &HelixConfig {
        &self.config
    }

    /// POST mit einem **User-Token** (Client-Id + dieser Bearer, KEIN App-Token).
    /// Für Endpoints, die die Identität des Broadcasters brauchen (z. B. `/raids`).
    pub fn post_with_user_token(&self, path: &str, user_token: &str) -> RequestBuilder {
        let url = format!("{}{}", self.config.helix_base, path);
        self.http
            .post(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {user_token}"))
    }

    /// GET mit einem **User-Token** (Client-Id + dieser Bearer, KEIN App-Token).
    pub fn get_with_user_token(&self, path: &str, user_token: &str) -> RequestBuilder {
        let url = format!("{}{}", self.config.helix_base, path);
        self.http
            .get(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {user_token}"))
    }

    /// DELETE mit einem **User-Token** (analog [`Self::post_with_user_token`]).
    pub fn delete_with_user_token(&self, path: &str, user_token: &str) -> RequestBuilder {
        let url = format!("{}{}", self.config.helix_base, path);
        self.http
            .delete(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {user_token}"))
    }

    /// Erstellt einen Clip aus dem aktuellen Stream-Buffer (Helix `POST /clips`).
    ///
    /// Braucht ein **User-Token mit `clips:edit`-Scope** (i. d. R. der Broadcaster).
    /// Titel/Dauer akzeptiert Helix hier nicht — der Clip kommt aus dem Live-Buffer
    /// (~letzte 30 s); der Titel wird nur in der Chat-Antwort verwendet.
    /// `Ok(None)` = Twitch lieferte kein Clip-Objekt zurück.
    pub async fn create_clip(
        &self,
        broadcaster_id: &str,
        user_token: &str,
    ) -> Result<Option<ClipInfo>, HelixError> {
        let path = format!("/clips?broadcaster_id={broadcaster_id}&has_delay=false");
        let resp = self.post_with_user_token(&path, user_token).send().await?;
        let body: HelixClipsResponse = check_status_and_json(resp).await?;
        Ok(body.data.into_iter().next())
    }

    /// Holt Twitch-User-Infos für eine Liste von Logins.
    ///
    /// Gibt eine Map `login (lowercase) → TwitchUser` zurück.
    /// Leere Logins werden ignoriert. Unbekannte Logins erscheinen nicht in der Map.
    pub async fn get_users(
        &self,
        logins: &[&str],
    ) -> Result<std::collections::HashMap<String, TwitchUser>, HelixError> {
        let mut out = std::collections::HashMap::new();
        // Helix /users akzeptiert max. 100 login-Parameter pro Request — größere
        // Listen müssen gechunkt werden (Python batcht ebenso), sonst lehnt Twitch ab.
        for chunk in logins.chunks(100) {
            let params: Vec<(&str, &str)> = chunk
                .iter()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| ("login", l))
                .collect();
            if params.is_empty() {
                continue;
            }
            let builder = self.get("/users").await?;
            let resp = builder.query(&params).send().await?;
            let body: HelixUsersResponse = check_status_and_json(resp).await?;
            for u in body.data {
                out.insert(u.login.to_lowercase(), u);
            }
        }
        Ok(out)
    }
}

/// Prüft den HTTP-Status einer Helix-Response und deserialisiert den Body.
///
/// Bei non-2xx wird `HelixError::Status` zurückgegeben, bevor JSON geparst wird.
/// So entsteht bei 429/5xx ein klarer Status-Fehler statt eines serde-Parse-Fehlers.
pub(crate) async fn check_status_and_json<T: DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, HelixError> {
    let status = resp.status();
    if !status.is_success() {
        return Err(HelixError::Status {
            status: status.as_u16(),
        });
    }
    Ok(resp.json::<T>().await?)
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

/// Clip-Daten aus `POST /clips` (Helix liefert id + edit_url).
#[derive(Debug, serde::Deserialize)]
pub struct ClipInfo {
    pub id: String,
    #[serde(default)]
    pub edit_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct HelixClipsResponse {
    data: Vec<ClipInfo>,
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
