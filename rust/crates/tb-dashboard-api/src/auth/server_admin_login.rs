//! Discord-Login des Server-Admin-Dashboards (Multi-Server Paket A).
//!
//! Läuft über die Discord-Anwendung des Discord-Hubs (PLAN.md 4.1/4.3), nicht
//! über den Broker des Haupt-Bots: Der Broker liefert keine Guild-Liste, und
//! Deadlock-Bots bleibt laut E6 unangetastet. Scopes `identify guilds`.
//!
//! Server-Admin ist, wer auf einer Guild Eigentümer ist oder das Recht
//! „Server verwalten“ (`MANAGE_GUILD`) bzw. `ADMINISTRATOR` hat. Das prüft
//! [`can_manage_guild`] anhand der von Discord gelieferten Rechte. Der
//! Discord-Access-Token wird nur für die zwei Abfragen im Speicher gehalten und
//! nie gespeichert (docs/token-storage-db.md).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    extract::{Extension, Query},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use url::Url;

use super::level::cookie_values;
use super::session::{
    build_session_cookie, clear_session_cookie, DashboardAuthState, SameSite,
    SERVER_ADMIN_COOKIE_NAME, SERVER_ADMIN_OAUTH_STATE_TTL_SECS, SERVER_ADMIN_SESSION_TTL_SECS,
};

pub const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const DISCORD_SCOPES: &str = "identify guilds";
const OAUTH_COOKIE_NAME: &str = "tb_server_admin_oauth";
const OAUTH_COOKIE_PATH: &str = "/twitch/server-admin/auth";
pub const SERVER_ADMIN_PAGE_PATH: &str = "/twitch/server-admin";
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);

/// Discord-Rechtebits (https://discord.com/developers/docs/topics/permissions).
pub const PERMISSION_ADMINISTRATOR: u64 = 1 << 3;
pub const PERMISSION_MANAGE_GUILD: u64 = 1 << 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordUserGuild {
    pub id: i64,
    pub name: String,
    pub owner: bool,
    pub permissions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordGuildAdminIdentity {
    pub user_id: u64,
    pub display_name: String,
    pub guilds: Vec<DiscordUserGuild>,
}

#[derive(Debug, Clone)]
pub struct ServerAdminLoginError;

/// Code → Identität + Guild-Liste. Im Test durch einen Fake ersetzt.
#[async_trait]
pub trait ServerAdminDiscordClient: Send + Sync {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<DiscordGuildAdminIdentity, ServerAdminLoginError>;
}

#[derive(Clone)]
pub struct ServerAdminLoginConfig {
    pub client_id: String,
    pub redirect_uri: String,
    pub cookie_secure: bool,
    pub client: Arc<dyn ServerAdminDiscordClient>,
}

/// Eigentümer, `ADMINISTRATOR` oder `MANAGE_GUILD` („Server verwalten“).
pub fn can_manage_guild(guild: &DiscordUserGuild) -> bool {
    guild.owner || guild.permissions & (PERMISSION_ADMINISTRATOR | PERMISSION_MANAGE_GUILD) != 0
}

pub fn managed_guild_ids(guilds: &[DiscordUserGuild]) -> Vec<i64> {
    let mut ids: Vec<i64> = guilds
        .iter()
        .filter(|guild| guild.id > 0 && can_manage_guild(guild))
        .map(|guild| guild.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

pub fn build_authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let mut url = Url::parse(DISCORD_AUTHORIZE_URL).expect("statische Discord-URL");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", DISCORD_SCOPES)
        .append_pair("state", state)
        .append_pair("prompt", "none");
    url.to_string()
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Konfiguration aus dem Secret-Manager (Infisical → Env). Ohne vollständige
/// Werte bleibt der Login aus und die Routen antworten 503.
pub fn server_admin_login_config_from_env() -> Option<ServerAdminLoginConfig> {
    let client_id = non_empty_env("DISCORD_HUB_CLIENT_ID")?;
    let client_secret = non_empty_env("DISCORD_HUB_CLIENT_SECRET")?;
    let redirect_uri = non_empty_env("TB_SERVER_ADMIN_OAUTH_REDIRECT_URI")?;
    let parsed = Url::parse(&redirect_uri).ok()?;
    if parsed.scheme() != "https" && parsed.host_str() != Some("localhost") {
        return None;
    }
    let cookie_secure = std::env::var("TB_DASHBOARD_COOKIE_INSECURE").as_deref() != Ok("1");
    let client = DiscordRestClient::new(client_id.clone(), client_secret)?;
    Some(ServerAdminLoginConfig {
        client_id,
        redirect_uri,
        cookie_secure,
        client: Arc::new(client),
    })
}

struct DiscordRestClient {
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
}

impl DiscordRestClient {
    fn new(client_id: String, client_secret: String) -> Option<Self> {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()?;
        Some(Self {
            client_id,
            client_secret,
            http,
        })
    }

    async fn get_json(
        &self,
        path: &str,
        access_token: &str,
    ) -> Result<serde_json::Value, ServerAdminLoginError> {
        let response = self
            .http
            .get(format!("{DISCORD_API_BASE}{path}"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(%error, path, "Server-Admin-Login: Discord-Abfrage fehlgeschlagen");
                ServerAdminLoginError
            })?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), path, "Server-Admin-Login: Discord non-200");
            return Err(ServerAdminLoginError);
        }
        response.json().await.map_err(|_| ServerAdminLoginError)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[async_trait]
impl ServerAdminDiscordClient for DiscordRestClient {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<DiscordGuildAdminIdentity, ServerAdminLoginError> {
        let response = self
            .http
            .post(format!("{DISCORD_API_BASE}/oauth2/token"))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(%error, "Server-Admin-Login: Code-Tausch fehlgeschlagen");
                ServerAdminLoginError
            })?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "Server-Admin-Login: Code-Tausch non-200");
            return Err(ServerAdminLoginError);
        }
        let token: TokenResponse = response.json().await.map_err(|_| ServerAdminLoginError)?;
        let user = self.get_json("/users/@me", &token.access_token).await?;
        let guilds = self
            .get_json("/users/@me/guilds", &token.access_token)
            .await?;
        // Token fällt hier aus dem Gültigkeitsbereich und wird nirgends abgelegt.
        parse_identity(&user, &guilds).ok_or(ServerAdminLoginError)
    }
}

/// Parst `/users/@me` und `/users/@me/guilds`. `permissions` liefert Discord als
/// Dezimal-String.
pub fn parse_identity(
    user: &serde_json::Value,
    guilds: &serde_json::Value,
) -> Option<DiscordGuildAdminIdentity> {
    let user_id = user
        .get("id")?
        .as_str()?
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)?;
    let display_name = user
        .get("global_name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| user.get("username").and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .trim()
        .chars()
        .take(100)
        .collect::<String>();
    let guilds = guilds
        .as_array()?
        .iter()
        .filter_map(|guild| {
            let id = guild
                .get("id")?
                .as_str()?
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)?;
            let permissions = match guild.get("permissions")? {
                serde_json::Value::String(raw) => raw.parse::<u64>().ok()?,
                serde_json::Value::Number(raw) => raw.as_u64()?,
                _ => return None,
            };
            Some(DiscordUserGuild {
                id,
                name: guild
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect(),
                owner: guild
                    .get("owner")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                permissions,
            })
        })
        .collect();
    Some(DiscordGuildAdminIdentity {
        user_id,
        display_name,
        guilds,
    })
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn unavailable() -> Response {
    no_store(
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Server-Admin-Login ist nicht konfiguriert.",
        )
            .into_response(),
    )
}

fn redirect_with_cookies(target: &str, cookies: &[String]) -> Response {
    let mut response = no_store(Redirect::to(target).into_response());
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

fn oauth_cookie(value: &str, secure: bool, max_age: u64) -> String {
    let mut cookie = format!(
        "{OAUTH_COOKIE_NAME}={value}; Path={OAUTH_COOKIE_PATH}; Max-Age={max_age}; HttpOnly; SameSite=Lax"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// `GET /twitch/server-admin/auth/login`
pub async fn login_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<ServerAdminLoginConfig>>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return unavailable();
    };
    let state_token = tb_crypto::random_urlsafe_token(32);
    if let Err(error) = state
        .save_server_admin_oauth_state(&state_token, &config.redirect_uri)
        .await
    {
        tracing::error!(%error, "Server-Admin-Login: State konnte nicht gespeichert werden");
        return unavailable();
    }
    let target = build_authorize_url(&config.client_id, &config.redirect_uri, &state_token);
    redirect_with_cookies(
        &target,
        &[oauth_cookie(
            &state_token,
            config.cookie_secure,
            SERVER_ADMIN_OAUTH_STATE_TTL_SECS,
        )],
    )
}

#[derive(Debug, Deserialize, Default)]
pub struct CallbackQuery {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

fn login_failed(status: StatusCode, message: &'static str) -> Response {
    no_store((status, message).into_response())
}

/// `GET /twitch/server-admin/auth/callback`
pub async fn callback_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<ServerAdminLoginConfig>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return unavailable();
    };
    let state_token = query.state.as_deref().map(str::trim).unwrap_or("");
    let cookies = cookie_values(&headers, OAUTH_COOKIE_NAME);
    // Login-CSRF-Schutz: der State muss zu dem Browser gehören, der den Login
    // gestartet hat.
    if state_token.is_empty()
        || state_token.len() > 200
        || cookies.len() != 1
        || !tb_crypto::constant_time_eq(cookies[0].as_bytes(), state_token.as_bytes())
    {
        return login_failed(StatusCode::BAD_REQUEST, "Ungültiger OAuth-State.");
    }
    let saved = match state.consume_server_admin_oauth_state(state_token).await {
        Ok(Some(saved)) => saved,
        Ok(None) => return login_failed(StatusCode::BAD_REQUEST, "OAuth-State abgelaufen."),
        Err(error) => {
            tracing::error!(%error, "Server-Admin-Login: State-Lookup fehlgeschlagen");
            return unavailable();
        }
    };
    if query.error.as_deref().is_some_and(|e| !e.trim().is_empty()) {
        return login_failed(StatusCode::UNAUTHORIZED, "Discord-Login abgebrochen.");
    }
    let code = query.code.as_deref().map(str::trim).unwrap_or("");
    if code.is_empty() || code.len() > 200 {
        return login_failed(StatusCode::BAD_REQUEST, "Fehlender OAuth-Code.");
    }
    let identity = match config.client.exchange_code(code, &saved.redirect_uri).await {
        Ok(identity) => identity,
        Err(_) => {
            return login_failed(
                StatusCode::UNAUTHORIZED,
                "Discord-Nutzer konnte nicht geladen werden.",
            )
        }
    };
    let managed = managed_guild_ids(&identity.guilds);
    // Nur Guilds, auf denen der Bot installiert ist, landen in der Session.
    let installed: Vec<i64> = match sqlx::query_scalar(
        "SELECT guild_id FROM discord_guilds WHERE status = 'active' AND guild_id = ANY($1)",
    )
    .bind(&managed)
    .fetch_all(state.pool())
    .await
    {
        Ok(ids) => ids,
        Err(error) => {
            tracing::error!(%error, "Server-Admin-Login: Guild-Abgleich fehlgeschlagen");
            return unavailable();
        }
    };
    let display_name = if identity.display_name.is_empty() {
        format!("User {}", identity.user_id)
    } else {
        identity.display_name.clone()
    };
    let created = match state
        .create_server_admin_session(identity.user_id, &display_name, &installed)
        .await
    {
        Ok(created) => created,
        Err(error) => {
            tracing::error!(%error, "Server-Admin-Session konnte nicht gespeichert werden");
            return unavailable();
        }
    };
    tracing::info!(
        discord_user_id = identity.user_id,
        guilds = installed.len(),
        "AUDIT server-admin login success"
    );
    redirect_with_cookies(
        SERVER_ADMIN_PAGE_PATH,
        &[
            build_session_cookie(
                SERVER_ADMIN_COOKIE_NAME,
                &created.session_id,
                config.cookie_secure,
                SameSite::Lax,
                SERVER_ADMIN_SESSION_TTL_SECS,
            ),
            oauth_cookie("", config.cookie_secure, 0),
        ],
    )
}

/// `GET /twitch/server-admin/auth/logout`
pub async fn logout_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<ServerAdminLoginConfig>>,
    headers: HeaderMap,
) -> Response {
    let secure = config.map(|c| c.0.cookie_secure).unwrap_or(true);
    if let Some(Extension(state)) = state {
        for session_id in cookie_values(&headers, SERVER_ADMIN_COOKIE_NAME) {
            if !session_id.is_empty() {
                state.invalidate_session(session_id).await;
            }
        }
    }
    redirect_with_cookies(
        SERVER_ADMIN_PAGE_PATH,
        &[clear_session_cookie(
            SERVER_ADMIN_COOKIE_NAME,
            secure,
            SameSite::Lax,
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guild(id: i64, owner: bool, permissions: u64) -> DiscordUserGuild {
        DiscordUserGuild {
            id,
            name: format!("g{id}"),
            owner,
            permissions,
        }
    }

    #[test]
    fn nur_server_verwalten_admin_oder_owner_zaehlen() {
        assert!(can_manage_guild(&guild(1, false, PERMISSION_MANAGE_GUILD)));
        assert!(can_manage_guild(&guild(1, false, PERMISSION_ADMINISTRATOR)));
        assert!(can_manage_guild(&guild(1, true, 0)));
        // Kanäle verwalten (1<<4), Rollen verwalten (1<<28), Nachrichten senden: kein Admin.
        assert!(!can_manage_guild(&guild(
            1,
            false,
            (1 << 4) | (1 << 28) | (1 << 11)
        )));
        assert!(!can_manage_guild(&guild(1, false, 0)));
    }

    #[test]
    fn managed_ids_sind_sortiert_und_gefiltert() {
        let ids = managed_guild_ids(&[
            guild(30, false, PERMISSION_MANAGE_GUILD),
            guild(10, true, 0),
            guild(20, false, 1 << 11),
            guild(10, false, PERMISSION_ADMINISTRATOR),
        ]);
        assert_eq!(ids, vec![10, 30]);
    }

    #[test]
    fn identity_parst_discord_antworten() {
        let identity = parse_identity(
            &json!({"id": "42", "username": "x", "global_name": "Mod X"}),
            &json!([
                {"id": "100", "name": "A", "owner": false, "permissions": "32"},
                {"id": "200", "name": "B", "owner": false, "permissions": "2048"},
                {"id": "kaputt", "permissions": "8"},
            ]),
        )
        .expect("identity");
        assert_eq!(identity.user_id, 42);
        assert_eq!(identity.display_name, "Mod X");
        assert_eq!(identity.guilds.len(), 2);
        assert_eq!(managed_guild_ids(&identity.guilds), vec![100]);
    }

    #[test]
    fn authorize_url_fordert_nur_identify_und_guilds() {
        let url = build_authorize_url(
            "cid",
            "https://x.test/twitch/server-admin/auth/callback",
            "st",
        );
        assert!(url.starts_with(DISCORD_AUTHORIZE_URL));
        assert!(url.contains("scope=identify+guilds"));
        assert!(url.contains("state=st"));
        assert!(!url.contains("bot"));
    }
}
