//! Bot-Token-Ownership — Port von `bot/chat/tokens.py` (`TwitchBotTokenManager`)
//! nach dem Vertrag `/tmp/welle-b-vertraege/token_ownership.md`.
//!
//! # Boot-Pfad (am 12.6. live bewiesen)
//!
//! Der Access-Token-Snapshot in Infisical (`TWITCH_BOT_TOKEN`) veraltet nach
//! dem ersten Refresh des laufenden Prozesses — der lebende Token existiert
//! nur in-memory. Der **Refresh-Token** (`TWITCH_BOT_REFRESH_TOKEN`) bleibt
//! über Refreshes hinweg gültig (der Python-Worker bootet nach jedem Restart
//! erfolgreich darüber). Dieser Manager macht es genauso: beim Start wird der
//! Env-Access-Token validiert; ist er tot, wird sofort mit dem Refresh-Token
//! ein frischer geholt.
//!
//! # Ownership-Regel (Dual-Refresh-Race)
//!
//! Es darf zu jeder Zeit nur EIN Prozess den Bot-Token refreshen. Der Flip
//! ist deshalb atomar: Python-Chat aus (Takeover-Gate) → tb-bot mit Chat an.
//!
//! # Auto-Refresh
//!
//! Python: Loop alle 30 Minuten, Refresh wenn < 1 h Restlaufzeit. Identisch
//! hier (`spawn_refresh_loop`). Bei 401 auf einem Helix-Call erzwingen die
//! Aufrufer `force_refresh()` und wiederholen einmal (2-Attempt-Muster).

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::sync::RwLock;

const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
/// Python: Refresh-Schwelle 1 h vor Ablauf.
const REFRESH_THRESHOLD: chrono::Duration = chrono::Duration::hours(1);
/// Python: Loop-Intervall 30 min.
const REFRESH_LOOP_INTERVAL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    #[serde(default)]
    login: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: i64,
}

#[derive(Debug, Clone)]
struct TokenState {
    access_token: String,
    refresh_token: String,
    expires_at: DateTime<Utc>,
    scopes: Vec<String>,
}

/// Fehler des Token-Managers.
#[derive(Debug)]
pub enum TokenError {
    Http(reqwest::Error),
    Rejected { status: u16, body: String },
    NotInitialized,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "token http error: {e}"),
            Self::Rejected { status, body } => write!(f, "token rejected: HTTP {status}: {body}"),
            Self::NotInitialized => write!(f, "token manager not initialized"),
        }
    }
}

impl std::error::Error for TokenError {}

impl From<reqwest::Error> for TokenError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

/// Verwaltet den Bot-User-Token (deutschedeadlockcommunity) mit Auto-Refresh.
pub struct BotTokenManager {
    client: reqwest::Client,
    client_id: String,
    client_secret: String,
    bot_user_id: RwLock<String>,
    bot_login: RwLock<String>,
    state: RwLock<Option<TokenState>>,
    /// URLs für Tests überschreibbar.
    validate_url: String,
    token_url: String,
}

impl BotTokenManager {
    pub fn new(client_id: String, client_secret: String) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()?,
            client_id,
            client_secret,
            bot_user_id: RwLock::new(String::new()),
            bot_login: RwLock::new(String::new()),
            state: RwLock::new(None),
            validate_url: VALIDATE_URL.to_string(),
            token_url: TOKEN_URL.to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_urls(mut self, validate_url: String, token_url: String) -> Self {
        self.validate_url = validate_url;
        self.token_url = token_url;
        self
    }

    /// Initialisiert aus Seed-Tokens (Env): validiert den Access-Token; tot →
    /// sofortiger Refresh über den Refresh-Token (Boot-Pfad).
    pub async fn initialize(
        &self,
        seed_access_token: Option<&str>,
        seed_refresh_token: &str,
    ) -> Result<(), TokenError> {
        if let Some(access) = seed_access_token.map(str::trim).filter(|s| !s.is_empty()) {
            match self.validate(access).await {
                Ok(v) => {
                    tracing::info!(
                        login = %v.login,
                        expires_in = v.expires_in,
                        "Bot-Token aus Env gültig"
                    );
                    *self.bot_user_id.write().await = v.user_id.clone();
                    *self.bot_login.write().await = v.login.clone();
                    *self.state.write().await = Some(TokenState {
                        access_token: access.to_string(),
                        refresh_token: seed_refresh_token.trim().to_string(),
                        expires_at: Utc::now() + chrono::Duration::seconds(v.expires_in),
                        scopes: v.scopes,
                    });
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Env-Access-Token ungültig ({e}) — boote über Refresh-Token");
                }
            }
        }
        self.refresh_with(seed_refresh_token.trim()).await
    }

    /// Aktueller Access-Token; refresht lazy wenn < 1 h Restlaufzeit.
    pub async fn access_token(&self) -> Result<String, TokenError> {
        {
            let guard = self.state.read().await;
            if let Some(ref s) = *guard {
                if s.expires_at - Utc::now() > REFRESH_THRESHOLD {
                    return Ok(s.access_token.clone());
                }
            }
        }
        self.force_refresh().await?;
        let guard = self.state.read().await;
        guard
            .as_ref()
            .map(|s| s.access_token.clone())
            .ok_or(TokenError::NotInitialized)
    }

    /// Erzwingt einen Refresh (z. B. nach Helix-401).
    pub async fn force_refresh(&self) -> Result<(), TokenError> {
        let refresh_token = {
            let guard = self.state.read().await;
            guard
                .as_ref()
                .map(|s| s.refresh_token.clone())
                .ok_or(TokenError::NotInitialized)?
        };
        self.refresh_with(&refresh_token).await
    }

    /// Bot-User-ID (aus validate; leer bis `initialize`).
    pub async fn bot_user_id(&self) -> String {
        self.bot_user_id.read().await.clone()
    }

    pub async fn bot_login(&self) -> String {
        self.bot_login.read().await.clone()
    }

    /// Adapter für den 2-Attempt-Loop in [`HelixChatClient`]:
    /// `force=true` → erst `force_refresh()`, dann `access_token()`;
    /// `force=false` → direkt `access_token()` (lazy Refresh wenn nötig).
    pub async fn get_valid_token(&self, force: bool) -> Result<String, String> {
        if force {
            self.force_refresh().await.map_err(|e| e.to_string())?;
        }
        self.access_token().await.map_err(|e| e.to_string())
    }

    /// Gewährte Scopes (aus validate/refresh — Python hält sie dynamisch).
    pub async fn scopes(&self) -> Vec<String> {
        self.state
            .read()
            .await
            .as_ref()
            .map(|s| s.scopes.clone())
            .unwrap_or_default()
    }

    /// Startet den 30-min-Auto-Refresh-Loop (Python-Parität).
    pub fn spawn_refresh_loop(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(REFRESH_LOOP_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let needs_refresh = {
                    let guard = manager.state.read().await;
                    match *guard {
                        Some(ref s) => s.expires_at - Utc::now() <= REFRESH_THRESHOLD,
                        None => false,
                    }
                };
                if needs_refresh {
                    if let Err(e) = manager.force_refresh().await {
                        tracing::error!("Bot-Token-Auto-Refresh fehlgeschlagen: {e}");
                    }
                }
            }
        });
    }

    async fn validate(&self, access_token: &str) -> Result<ValidateResponse, TokenError> {
        let resp = self
            .client
            .get(&self.validate_url)
            .header("Authorization", format!("OAuth {access_token}"))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(TokenError::Rejected {
                status,
                body: body.chars().take(200).collect(),
            });
        }
        Ok(resp.json().await?)
    }

    async fn refresh_with(&self, refresh_token: &str) -> Result<(), TokenError> {
        let resp = self
            .client
            .post(&self.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(TokenError::Rejected {
                status,
                body: body.chars().take(200).collect(),
            });
        }
        let refreshed: RefreshResponse = resp.json().await?;
        let validate = self.validate(&refreshed.access_token).await?;
        *self.bot_user_id.write().await = validate.user_id.clone();
        *self.bot_login.write().await = validate.login.clone();
        let new_refresh = refreshed
            .refresh_token
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| refresh_token.to_string());
        *self.state.write().await = Some(TokenState {
            access_token: refreshed.access_token,
            refresh_token: new_refresh,
            expires_at: Utc::now() + chrono::Duration::seconds(refreshed.expires_in.max(60)),
            scopes: validate.scopes,
        });
        tracing::info!(
            login = %validate.login,
            expires_in = refreshed.expires_in,
            "Bot-Token refresht"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn manager(server: &MockServer) -> BotTokenManager {
        BotTokenManager::new("cid".into(), "csec".into())
            .unwrap()
            .with_urls(
                format!("{}/validate", server.uri()),
                format!("{}/token", server.uri()),
            )
    }

    fn validate_ok(expires_in: i64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "login": "deutschedeadlockcommunity",
            "user_id": "1422558159",
            "scopes": ["user:bot", "user:read:chat", "user:write:chat"],
            "expires_in": expires_in
        }))
    }

    #[tokio::test]
    async fn initialize_mit_gueltigem_seed_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/validate"))
            .and(header("Authorization", "OAuth seed-token"))
            .respond_with(validate_ok(14000))
            .mount(&server)
            .await;
        let m = manager(&server).await;
        m.initialize(Some("seed-token"), "refresh-seed").await.unwrap();
        assert_eq!(m.access_token().await.unwrap(), "seed-token");
        assert_eq!(m.bot_user_id().await, "1422558159");
    }

    #[tokio::test]
    async fn initialize_mit_totem_seed_bootet_via_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/validate"))
            .and(header("Authorization", "OAuth dead-token"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=refresh-seed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fresh-token",
                "refresh_token": "fresh-refresh",
                "expires_in": 14000
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/validate"))
            .and(header("Authorization", "OAuth fresh-token"))
            .respond_with(validate_ok(14000))
            .mount(&server)
            .await;

        let m = manager(&server).await;
        m.initialize(Some("dead-token"), "refresh-seed").await.unwrap();
        assert_eq!(m.access_token().await.unwrap(), "fresh-token");
    }

    #[tokio::test]
    async fn access_token_refresht_lazy_bei_kurzer_restlaufzeit() {
        let server = MockServer::start().await;
        // Seed gültig, aber expires_in unter der 1h-Schwelle → lazy Refresh.
        Mock::given(method("GET"))
            .and(path("/validate"))
            .and(header("Authorization", "OAuth seed-token"))
            .respond_with(validate_ok(120))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "rotated",
                "refresh_token": "rotated-refresh",
                "expires_in": 14000
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/validate"))
            .and(header("Authorization", "OAuth rotated"))
            .respond_with(validate_ok(14000))
            .mount(&server)
            .await;

        let m = manager(&server).await;
        m.initialize(Some("seed-token"), "refresh-seed").await.unwrap();
        assert_eq!(m.access_token().await.unwrap(), "rotated");
    }

    #[tokio::test]
    async fn refresh_behaelt_alten_refresh_token_wenn_keiner_kommt() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/validate"))
            .respond_with(validate_ok(14000))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fresh",
                "expires_in": 14000
            })))
            .mount(&server)
            .await;
        let m = manager(&server).await;
        m.initialize(None, "stable-refresh").await.unwrap();
        // Zweiter Refresh nutzt weiterhin den alten Refresh-Token.
        m.force_refresh().await.unwrap();
        let state = m.state.read().await;
        assert_eq!(state.as_ref().unwrap().refresh_token, "stable-refresh");
    }
}
