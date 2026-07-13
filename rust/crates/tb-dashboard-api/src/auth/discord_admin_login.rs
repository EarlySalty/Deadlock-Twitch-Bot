//! Nativer Discord-Admin-Login für `master_dash_session`.
//!
//! Python-Referenz:
//! - `bot/dashboard/auth/auth_mixin.py:263-288` (`/callback/discord`)
//! - `bot/dashboard/auth/auth_mixin.py:1439-1650`
//!   (`/twitch/auth/discord/login|complete|logout`)
//! - `bot/dashboard/auth/fingerprint_mixin.py` (`/twitch/auth/fingerprint`)
//!
//! Der Discord-Code→Token-Tausch läuft wie in Python nicht in diesem Prozess,
//! sondern über den lokalen Discord-OAuth-Broker:
//! `POST /internal/v1/discord/initiate` und
//! `POST /internal/v1/discord/consume-result`.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use axum::{
    body::Bytes,
    extract::{Extension, Query},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::session::{
    build_passive_fp, clear_session_cookie, DashboardAuthState, SameSite, ADMIN_COOKIE_NAME,
    ADMIN_SESSION_TTL_SECS,
};

const DEFAULT_ADMIN_BASE_URL: &str = "https://admin.deutsche-deadlock-community.de";
const DEFAULT_COOKIE_DOMAIN: &str = "deutsche-deadlock-community.de";
const DEFAULT_DASHBOARD_MODERATOR_ROLE_ID: u64 = 1_337_518_124_647_579_661;
const BROKER_BASE_URL: &str = "http://127.0.0.1:8766";
const BROKER_INITIATE_PATH: &str = "/internal/v1/discord/initiate";
const BROKER_CONSUME_PATH: &str = "/internal/v1/discord/consume-result";
const BROKER_VALIDATE_SESSION_PATH: &str = "/internal/twitch/v1/discord/validate-session";
const BROKER_IMPORT_SESSION_PATH: &str = "/internal/twitch/v1/discord/import-session";
// Gegenroute: Deadlock-Bots d2558e19, enthalten in origin/main ab 7fc95051.
const BROKER_REVOKE_SESSION_PATH: &str = "/internal/twitch/v1/discord/revoke-session";
const BROKER_TOKEN_HEADER: &str = "X-Internal-Token";
const BROKER_TIMEOUT: Duration = Duration::from_secs(20);
const FINGERPRINT_PATH: &str = "/twitch/auth/fingerprint";
const ADMIN_LOGIN_PATH: &str = "/twitch/auth/discord/login";
const ADMIN_COMPLETE_PATH: &str = "/twitch/auth/discord/complete";
const ADMIN_FALLBACK_PATH: &str = "/twitch/admin";

#[derive(Debug, Deserialize, Default)]
pub struct AdminLoginQuery {
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SharedCallbackQuery {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub state_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CompleteQuery {
    #[serde(default)]
    pub state_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordAuthorize {
    pub authorize_url: String,
    pub state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordAdminSession {
    pub discord_id: String,
    pub discord_name: String,
    pub discord_roles: Vec<String>,
    pub service_metadata: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedAdminSession {
    pub user_id: u64,
    pub username: String,
    pub display_name: String,
    pub expires_at: f64,
}

#[derive(Debug, Clone)]
pub struct DiscordAdminOAuthError;

impl std::fmt::Display for DiscordAdminOAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("discord admin oauth failed")
    }
}

impl std::error::Error for DiscordAdminOAuthError {}

#[async_trait]
pub trait DiscordAdminOAuthClient: Send + Sync {
    async fn initiate(
        &self,
        scope: &str,
        redirect_after: &str,
        requesting_service: &str,
        metadata: Value,
    ) -> Result<DiscordAuthorize, DiscordAdminOAuthError>;

    async fn consume_result(
        &self,
        state_id: &str,
    ) -> Result<DiscordAdminSession, DiscordAdminOAuthError>;

    async fn validate_session(
        &self,
        session_id: &str,
    ) -> Result<ValidatedAdminSession, DiscordAdminOAuthError>;

    async fn import_session(
        &self,
        session_id: &str,
        session: &ValidatedAdminSession,
    ) -> Result<(), DiscordAdminOAuthError>;

    async fn revoke_session(&self, session_id: &str) -> Result<(), DiscordAdminOAuthError>;
}

#[derive(Clone)]
pub struct DiscordAdminLoginConfig {
    pub admin_base_url: String,
    pub cookie_secure: bool,
    pub cookie_domain: Option<String>,
    pub owner_user_id: Option<u64>,
    pub moderator_role_id: u64,
    pub admin_guild_ids: Vec<u64>,
    pub client: Arc<dyn DiscordAdminOAuthClient>,
}

struct BrokerDiscordAdminOAuthClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl BrokerDiscordAdminOAuthClient {
    fn new(base_url: String, token: String) -> Option<Self> {
        let http = reqwest::Client::builder()
            .timeout(BROKER_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()?;
        Some(Self { base_url, token, http })
    }

    async fn post(&self, path: &str, payload: &Value) -> Result<Value, DiscordAdminOAuthError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let response = self
            .http
            .post(url)
            .header(BROKER_TOKEN_HEADER, &self.token)
            .json(payload)
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(%error, path, "Discord-OAuth-Broker request failed");
                DiscordAdminOAuthError
            })?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), path, "Discord-OAuth-Broker non-200");
            return Err(DiscordAdminOAuthError);
        }
        response.json::<Value>().await.map_err(|error| {
            tracing::warn!(%error, path, "Discord-OAuth-Broker JSON invalid");
            DiscordAdminOAuthError
        })
    }
}

#[async_trait]
impl DiscordAdminOAuthClient for BrokerDiscordAdminOAuthClient {
    async fn initiate(
        &self,
        scope: &str,
        redirect_after: &str,
        requesting_service: &str,
        metadata: Value,
    ) -> Result<DiscordAuthorize, DiscordAdminOAuthError> {
        let data = self
            .post(
                BROKER_INITIATE_PATH,
                &json!({
                    "scope": scope,
                    "redirect_after": redirect_after,
                    "requesting_service": requesting_service,
                    "metadata": metadata,
                }),
            )
            .await?;
        let authorize_url = data
            .get("authorize_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let state_id = data
            .get("state_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if authorize_url.is_empty() || state_id.is_empty() {
            return Err(DiscordAdminOAuthError);
        }
        Ok(DiscordAuthorize { authorize_url, state_id })
    }

    async fn consume_result(
        &self,
        state_id: &str,
    ) -> Result<DiscordAdminSession, DiscordAdminOAuthError> {
        let data = self
            .post(BROKER_CONSUME_PATH, &json!({ "state_id": state_id }))
            .await?;
        parse_discord_admin_session(data).ok_or(DiscordAdminOAuthError)
    }

    async fn validate_session(
        &self,
        session_id: &str,
    ) -> Result<ValidatedAdminSession, DiscordAdminOAuthError> {
        let data = self
            .post(
                BROKER_VALIDATE_SESSION_PATH,
                &json!({ "session_id": session_id }),
            )
            .await?;
        if data.get("valid").and_then(Value::as_bool) != Some(true) {
            return Err(DiscordAdminOAuthError);
        }
        let user_id = data
            .get("user_id")
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
            })
            .ok_or(DiscordAdminOAuthError)?;
        let username = data
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let display_name = data
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(&username)
            .trim()
            .to_string();
        let expires_at = data
            .get("expires_at")
            .and_then(Value::as_f64)
            .ok_or(DiscordAdminOAuthError)?;
        Ok(ValidatedAdminSession {
            user_id,
            username,
            display_name,
            expires_at,
        })
    }

    async fn import_session(
        &self,
        session_id: &str,
        session: &ValidatedAdminSession,
    ) -> Result<(), DiscordAdminOAuthError> {
        let data = self
            .post(
                BROKER_IMPORT_SESSION_PATH,
                &json!({
                    "session_id": session_id,
                    "user_id": session.user_id.to_string(),
                    "username": session.username,
                    "display_name": session.display_name,
                    "expires_at": session.expires_at,
                }),
            )
            .await?;
        (data.get("ok").and_then(Value::as_bool) == Some(true))
            .then_some(())
            .ok_or(DiscordAdminOAuthError)
    }

    async fn revoke_session(&self, session_id: &str) -> Result<(), DiscordAdminOAuthError> {
        let data = self
            .post(
                BROKER_REVOKE_SESSION_PATH,
                &json!({ "session_id": session_id }),
            )
            .await?;
        (data.get("ok").and_then(Value::as_bool) == Some(true))
            .then_some(())
            .ok_or(DiscordAdminOAuthError)
    }
}

pub fn discord_admin_login_config_from_env() -> Option<DiscordAdminLoginConfig> {
    let token = internal_token_from_env()?;
    let base_url = non_empty_env("DISCORD_OAUTH_INTERNAL_API_BASE_URL")
        .unwrap_or_else(|| BROKER_BASE_URL.to_string());
    let admin_base_url = admin_base_url_from_env()?;
    let cookie_secure = std::env::var("TB_DASHBOARD_COOKIE_INSECURE").as_deref() != Ok("1");
    let cookie_domain = shared_admin_cookie_domain_from_env();
    let owner_user_id = optional_u64_env(&["TWITCH_ADMIN_OWNER_USER_ID", "DISCORD_ADMIN_OWNER_USER_ID"]);
    let admin_guild_ids = parse_u64_csv_env(&["TWITCH_ADMIN_DISCORD_GUILD_IDS", "DISCORD_ADMIN_GUILD_IDS"]);
    let client = BrokerDiscordAdminOAuthClient::new(base_url, token)?;
    Some(DiscordAdminLoginConfig {
        admin_base_url,
        cookie_secure,
        cookie_domain,
        owner_user_id,
        moderator_role_id: DEFAULT_DASHBOARD_MODERATOR_ROLE_ID,
        admin_guild_ids,
        client: Arc::new(client),
    })
}

/// `GET /twitch/auth/discord/login`
pub async fn login_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<DiscordAdminLoginConfig>>,
    headers: HeaderMap,
    Query(query): Query<AdminLoginQuery>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return discord_unconfigured();
    };
    if is_public_host_admin_route(&headers, &config) {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    let next_path = normalize_discord_admin_next_path(query.next.as_deref());
    // Ein vorhandenes Cookie gilt nur dann als „schon eingeloggt", wenn die
    // Session-Bindung es trägt — also mit demselben Maßstab, den der Forward-Auth
    // anlegt (`handlers::forward_auth::validate_admin_session`). Prüfte der Login
    // nur Existenz + TTL, während der Forward-Auth zusätzlich IP/Passive-FP/
    // fp_pending verlangt, schickten sich beide gegenseitig im Kreis: Panel → 401 →
    // Login → Panel, bis das Rate-Limit greift (Vorfall 2026-07-10). Die zentrale
    // Session-Prüfung gehört wie im Auth-Level-Extractor ebenfalls zum Maßstab.
    let mut unbrauchbares_admin_cookie = false;
    if let Some(session_id) = cookie_from_headers(&headers, ADMIN_COOKIE_NAME) {
        if !session_id.is_empty() {
            match state.load_admin_session_fingerprint(&session_id).await {
                Ok(Some(fingerprint))
                    if fingerprint.verify(
                        &client_ip_from_headers(&headers),
                        &passive_fp_from_headers(&headers),
                    ) =>
                {
                    if config.client.validate_session(&session_id).await.is_ok() {
                        let destination = safe_internal_redirect(
                            &canonical_discord_admin_post_login_path(Some(&next_path)),
                            ADMIN_FALLBACK_PATH,
                        );
                        let cookie = build_admin_cookie(&config, &session_id);
                        return redirect_with_cookie(&destination, &cookie);
                    }
                    unbrauchbares_admin_cookie = true;
                }
                // Session da, Bindung trägt nicht (IP-Wechsel, neuer Passive-FP nach
                // Browser-Update, oder Fingerprint-Schritt offen). Cookie räumen und
                // frisch anmelden lassen. Die Session bleibt serverseitig bestehen:
                // sie hier zu löschen, hieße, dass jeder mit einem alten Cookie die
                // laufende Sitzung des echten Admins abschießen kann.
                Ok(Some(_)) => unbrauchbares_admin_cookie = true,
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, "Discord-Admin-Session-Lookup beim Login fehlgeschlagen");
                    return no_store((
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Admin-Session konnte gerade nicht geprüft werden.",
                    ).into_response());
                }
            }
        }
    }

    let complete_url = admin_route_url(&config.admin_base_url, ADMIN_COMPLETE_PATH, &[]);
    let auth = match config
        .client
        .initiate(
            "identify",
            &complete_url,
            "twitch-admin",
            json!({ "next_path": next_path }),
        )
        .await
    {
        Ok(auth) => auth,
        Err(_) => {
            return no_store((
                StatusCode::SERVICE_UNAVAILABLE,
                "Discord Admin OAuth ist nicht konfiguriert. Bitte internen API-Token setzen.",
            ).into_response());
        }
    };

    let safe_auth_url = safe_discord_admin_login_redirect(&auth.authorize_url, &config);
    if unbrauchbares_admin_cookie {
        let cookie = clear_admin_cookie(config.cookie_secure, config.cookie_domain.as_deref());
        return redirect_with_cookie(&safe_auth_url, &cookie);
    }
    no_store(Redirect::to(&safe_auth_url).into_response())
}

/// `GET /callback/discord`
pub async fn shared_callback_handler(
    config: Option<Extension<DiscordAdminLoginConfig>>,
    Query(query): Query<SharedCallbackQuery>,
) -> Response {
    let base_url = config
        .as_ref()
        .map(|Extension(config)| config.admin_base_url.clone())
        .or_else(admin_base_url_from_env)
        .unwrap_or_else(|| DEFAULT_ADMIN_BASE_URL.to_string());
    let state_id = query
        .state
        .as_deref()
        .or(query.state_id.as_deref())
        .map(str::trim)
        .unwrap_or("");
    if state_id.is_empty() {
        return no_store((StatusCode::BAD_REQUEST, "Fehlender OAuth-State.").into_response());
    }

    let mut params = vec![("state_id", state_id)];
    if let Some(error) = query.error.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("error", error));
    }
    let target = admin_route_url(&base_url, ADMIN_COMPLETE_PATH, &params);
    no_store(Redirect::to(&target).into_response())
}

/// `GET /twitch/auth/discord/complete` und Alias `/twitch/auth/discord/callback`.
pub async fn complete_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<DiscordAdminLoginConfig>>,
    headers: HeaderMap,
    Query(query): Query<CompleteQuery>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return discord_unconfigured();
    };
    if is_public_host_admin_route(&headers, &config) {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    let state_id = query.state_id.as_deref().map(str::trim).unwrap_or("");
    if state_id.is_empty() {
        return no_store((StatusCode::BAD_REQUEST, "Fehlender state_id.").into_response());
    }
    if let Some(error) = query.error.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return no_store((
            StatusCode::UNAUTHORIZED,
            format!("Discord OAuth Fehler: {}", sanitize_inline(error)),
        ).into_response());
    }

    let session = match config.client.consume_result(state_id).await {
        Ok(session) => session,
        Err(_) => {
            return no_store((
                StatusCode::UNAUTHORIZED,
                "Discord User konnte nicht geladen werden.",
            ).into_response());
        }
    };
    let discord_id = session.discord_id.trim();
    if discord_id.is_empty() || !discord_id.chars().all(|c| c.is_ascii_digit()) {
        return no_store((
            StatusCode::UNAUTHORIZED,
            "Discord User konnte nicht geladen werden.",
        ).into_response());
    }
    let Ok(user_id) = discord_id.parse::<u64>() else {
        return no_store((
            StatusCode::UNAUTHORIZED,
            "Discord User konnte nicht geladen werden.",
        ).into_response());
    };

    let (allowed, reason) = discord_admin_privilege_reason(user_id, &session.discord_roles, &config);
    if !allowed {
        tracing::warn!(
            user_id = %user_id,
            reason = reason,
            "AUDIT twitch-dashboard discord login denied"
        );
        return no_store((
            StatusCode::FORBIDDEN,
            "Kein Zugriff. Es wird Administrator-Recht oder die Moderator-Rolle benötigt.",
        ).into_response());
    }

    let username = session.discord_name.trim();
    let display_name = if username.is_empty() {
        format!("User {user_id}")
    } else {
        username.to_string()
    };
    let next_path = session
        .service_metadata
        .get("next_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let destination = safe_internal_redirect(
        &canonical_discord_admin_post_login_path(Some(&next_path)),
        ADMIN_FALLBACK_PATH,
    );
    let client_ip = client_ip_from_headers(&headers);
    let passive_fp = passive_fp_from_headers(&headers);

    let created = match state
        .create_discord_admin_session(
            user_id,
            username,
            &display_name,
            reason,
            &client_ip,
            &passive_fp,
            &destination,
        )
        .await
    {
        Ok(created) => created,
        Err(error) => {
            tracing::error!(%error, "Discord-Admin-Session konnte nicht gespeichert werden");
            return no_store((
                StatusCode::SERVICE_UNAVAILABLE,
                "Die gemeinsame Admin-Session konnte nicht gespeichert werden.",
            ).into_response());
        }
    };

    let central_session = ValidatedAdminSession {
        user_id,
        username: username.to_string(),
        display_name: display_name.clone(),
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            + ADMIN_SESSION_TTL_SECS as f64,
    };
    if config
        .client
        .import_session(&created.session_id, &central_session)
        .await
        .is_err()
    {
        state.invalidate_session(&created.session_id).await;
        return no_store((
            StatusCode::SERVICE_UNAVAILABLE,
            "Die gemeinsame Admin-Session konnte nicht synchronisiert werden.",
        ).into_response());
    }

    tracing::info!(
        user_id = %user_id,
        reason = reason,
        "AUDIT twitch-dashboard discord login success"
    );
    let cookie = build_admin_cookie(&config, &created.session_id);
    let mut response = redirect_with_cookie(FINGERPRINT_PATH, &cookie);
    if config.cookie_domain.is_some() {
        let legacy_cookie = clear_admin_cookie(config.cookie_secure, None);
        if let Ok(value) = HeaderValue::from_str(&legacy_cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// `GET /twitch/auth/discord/logout`
pub async fn logout_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<DiscordAdminLoginConfig>>,
    headers: HeaderMap,
) -> Response {
    let cookie_secure = config.as_ref().map(|c| c.0.cookie_secure).unwrap_or(true);
    let cookie_domain = config
        .as_ref()
        .and_then(|c| c.0.cookie_domain.clone())
        .or_else(shared_admin_cookie_domain_from_env);
    let session_ids: Vec<String> = crate::auth::level::cookie_values(&headers, ADMIN_COOKIE_NAME)
        .into_iter()
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string)
        .collect();
    for session_id in session_ids {
        if let Some(Extension(config)) = config.as_ref() {
            if config.client.revoke_session(&session_id).await.is_err() {
                tracing::warn!("Zentrale Admin-Session konnte beim Logout nicht widerrufen werden");
            }
        }
        if let Some(Extension(state)) = state.as_ref() {
            state.invalidate_session(&session_id).await;
        }
    }
    let base_url = config
        .as_ref()
        .map(|c| c.0.admin_base_url.clone())
        .or_else(admin_base_url_from_env)
        .unwrap_or_else(|| DEFAULT_ADMIN_BASE_URL.to_string());
    let target = admin_route_url(&base_url, ADMIN_LOGIN_PATH, &[]);
    let cookie = clear_admin_cookie(cookie_secure, cookie_domain.as_deref());
    let mut response = redirect_with_cookie(&target, &cookie);
    if cookie_domain.is_some() {
        let legacy_cookie = clear_admin_cookie(cookie_secure, None);
        if let Ok(value) = HeaderValue::from_str(&legacy_cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// `GET /twitch/auth/fingerprint`
pub async fn fingerprint_page_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<DiscordAdminLoginConfig>>,
    headers: HeaderMap,
) -> Response {
    let Some(Extension(state)) = state else {
        return Redirect::to(ADMIN_LOGIN_PATH).into_response();
    };
    let Some(session_id) = cookie_from_headers(&headers, ADMIN_COOKIE_NAME) else {
        return Redirect::to(ADMIN_LOGIN_PATH).into_response();
    };
    match state.load_admin_session(&session_id).await {
        Ok(Some(_)) => {
            let mut response = (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                FP_COLLECT_HTML,
            )
                .into_response();
            if let Some(Extension(config)) = config {
                let cookie = build_admin_cookie(&config, &session_id);
                if let Ok(value) = HeaderValue::from_str(&cookie) {
                    response.headers_mut().append(header::SET_COOKIE, value);
                }
            }
            no_store(response)
        }
        Ok(None) | Err(_) => Redirect::to(ADMIN_LOGIN_PATH).into_response(),
    }
}

/// `POST /twitch/auth/fingerprint`
pub async fn fingerprint_submit_handler(
    state: Option<Extension<DashboardAuthState>>,
    _config: Option<Extension<DiscordAdminLoginConfig>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(Extension(state)) = state else {
        return Redirect::to(ADMIN_LOGIN_PATH).into_response();
    };
    let Some(session_id) = cookie_from_headers(&headers, ADMIN_COOKIE_NAME) else {
        return Redirect::to(ADMIN_LOGIN_PATH).into_response();
    };
    let raw_fp = form_value(&body, "fp").unwrap_or_default();
    let js_fp = normalize_js_fp(&raw_fp);
    match state
        .complete_admin_session_fingerprint(&session_id, &js_fp)
        .await
    {
        Ok(Some(destination)) => {
            let destination = safe_internal_redirect(&destination, ADMIN_FALLBACK_PATH);
            no_store((StatusCode::SEE_OTHER, [(header::LOCATION, destination)]).into_response())
        }
        Ok(None) | Err(_) => Redirect::to(ADMIN_LOGIN_PATH).into_response(),
    }
}

fn parse_discord_admin_session(data: Value) -> Option<DiscordAdminSession> {
    Some(DiscordAdminSession {
        discord_id: value_to_string(data.get("discord_id")?).trim().to_string(),
        discord_name: value_to_string(data.get("discord_name").unwrap_or(&Value::Null))
            .trim()
            .to_string(),
        discord_roles: data
            .get("discord_roles")
            .and_then(Value::as_array)
            .map(|roles| {
                roles
                    .iter()
                    .filter_map(|role| {
                        let role = value_to_string(role);
                        let role = role.trim();
                        (!role.is_empty()).then(|| role.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        service_metadata: data
            .get("service_metadata")
            .cloned()
            .unwrap_or(Value::Null),
    })
}

fn discord_admin_privilege_reason<'a>(
    user_id: u64,
    role_ids: &[String],
    config: &'a DiscordAdminLoginConfig,
) -> (bool, &'a str) {
    if matches!(config.owner_user_id, Some(owner) if owner > 0 && owner == user_id) {
        return (true, "owner_override");
    }
    let moderator = config.moderator_role_id.to_string();
    if role_ids.iter().any(|role| role.trim() == moderator) {
        return (true, "moderator_role:delegated");
    }
    if config.admin_guild_ids.is_empty() {
        return (false, "admin_guild_not_configured");
    }
    (false, "missing_admin_or_moderator_role")
}

fn normalize_discord_admin_next_path(raw: Option<&str>) -> String {
    let fallback = ADMIN_FALLBACK_PATH.to_string();
    let candidate = raw.unwrap_or("").trim();
    if candidate.is_empty()
        || candidate.starts_with("//")
        || !candidate.starts_with('/')
        || !candidate.starts_with("/twitch")
    {
        return fallback;
    }
    if candidate.contains("://") {
        return fallback;
    }
    candidate.split('#').next().unwrap_or(candidate).to_string()
}

fn canonical_discord_admin_post_login_path(raw: Option<&str>) -> String {
    let normalized = normalize_discord_admin_next_path(raw);
    let (path, query) = split_path_query(&normalized);
    let path = path.trim_end_matches('/').to_string();
    let path = if path.is_empty() { "/" } else { path.as_str() };
    let query_suffix = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    match path {
        "/twitch/abo" | "/twitch/abbo" | "/twitch/abos" => "/twitch/pricing".to_string(),
        "/twitch/abbo/stripe-settings" => "/twitch/abbo/stripe-settings".to_string(),
        "/twitch/abbo/rechnungen" => "/twitch/abbo/rechnungen".to_string(),
        "/twitch/abbo/rechnung" => "/twitch/abbo/rechnung".to_string(),
        "/twitch/abbo/kündigen" => "/twitch/abbo/kündigen".to_string(),
        "/twitch/dashboads" | "/twitch/dashboards" | "/twitch/dashboard" => {
            "/twitch/dashboard".to_string()
        }
        "/twitch/admin/announcements" => format!("/twitch/admin/announcements{query_suffix}"),
        "/twitch/admin/legacy" => format!("/twitch/admin/legacy{query_suffix}"),
        _ => ADMIN_FALLBACK_PATH.to_string(),
    }
}

fn split_path_query(value: &str) -> (&str, &str) {
    match value.split_once('?') {
        Some((path, query)) => (path, query),
        None => (value, ""),
    }
}

fn safe_internal_redirect(location: &str, fallback: &str) -> String {
    let candidate = location.trim();
    if candidate.is_empty()
        || candidate.starts_with("//")
        || !candidate.starts_with('/')
        || candidate.contains("://")
    {
        fallback.to_string()
    } else {
        candidate.to_string()
    }
}

fn safe_discord_admin_login_redirect(raw_url: &str, config: &DiscordAdminLoginConfig) -> String {
    let fallback = admin_route_url(&config.admin_base_url, ADMIN_LOGIN_PATH, &[]);
    let candidate = raw_url.trim();
    if candidate.is_empty() {
        return fallback;
    }
    if candidate.starts_with('/') && !candidate.starts_with("//") {
        return if candidate.starts_with(ADMIN_LOGIN_PATH) {
            candidate.to_string()
        } else {
            fallback
        };
    }
    let Ok(url) = url::Url::parse(candidate) else {
        return fallback;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return fallback;
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path();
    if url.scheme() == "https"
        && matches!(host.as_str(), "discord.com" | "www.discord.com")
        && matches!(
            path,
            "/oauth2/authorize" | "/api/oauth2/authorize" | "/api/v10/oauth2/authorize"
        )
    {
        return candidate.to_string();
    }
    let admin_host = url::Url::parse(&config.admin_base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    if url.scheme() == "https" && host == admin_host && path == ADMIN_LOGIN_PATH {
        return candidate.to_string();
    }
    fallback
}

fn admin_route_url(base_url: &str, path: &str, query: &[(&str, &str)]) -> String {
    let mut url = format!(
        "{}{}",
        base_url.trim().trim_end_matches('/'),
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        }
    );
    if !query.is_empty() {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in query {
            serializer.append_pair(key, value);
        }
        url.push('?');
        url.push_str(&serializer.finish());
    }
    url
}

fn build_admin_cookie(config: &DiscordAdminLoginConfig, session_id: &str) -> String {
    let mut cookie = format!(
        "{ADMIN_COOKIE_NAME}={session_id}; Path=/; Max-Age={ADMIN_SESSION_TTL_SECS}; HttpOnly; SameSite=Lax"
    );
    if let Some(domain) = config.cookie_domain.as_deref().filter(|d| !d.trim().is_empty()) {
        cookie.push_str("; Domain=");
        cookie.push_str(domain.trim());
    }
    if config.cookie_secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn clear_admin_cookie(cookie_secure: bool, cookie_domain: Option<&str>) -> String {
    let mut cookie = clear_session_cookie(ADMIN_COOKIE_NAME, cookie_secure, SameSite::Lax);
    if let Some(domain) = cookie_domain.filter(|d| !d.trim().is_empty()) {
        cookie.push_str("; Domain=");
        cookie.push_str(domain.trim());
    }
    cookie
}

fn redirect_with_cookie(location: &str, cookie: &str) -> Response {
    let mut response = Redirect::to(location).into_response();
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    no_store(response)
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    response.headers_mut().insert(
        header::PRAGMA,
        HeaderValue::from_static("no-cache"),
    );
    response
}

fn discord_unconfigured() -> Response {
    no_store((
        StatusCode::SERVICE_UNAVAILABLE,
        "Discord Admin OAuth ist nicht konfiguriert. Bitte internen API-Token setzen.",
    ).into_response())
}

fn cookie_from_headers(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn passive_fp_from_headers(headers: &HeaderMap) -> String {
    let header_value = |name: header::HeaderName| -> &str {
        headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
    };
    let platform = headers
        .get("sec-ch-ua-platform")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    build_passive_fp(
        header_value(header::USER_AGENT),
        header_value(header::ACCEPT_LANGUAGE),
        platform,
    )
}

fn client_ip_from_headers(headers: &HeaderMap) -> String {
    for name in ["x-forwarded-for", "x-real-ip"] {
        if let Some(raw) = headers.get(name).and_then(|v| v.to_str().ok()) {
            for candidate in raw.split(',') {
                let host = host_without_port(candidate);
                if !host.is_empty() {
                    return host;
                }
            }
        }
    }
    String::new()
}

fn host_without_port(raw: &str) -> String {
    let host = raw.split(',').next().unwrap_or("").trim();
    if host.is_empty() {
        return String::new();
    }
    let host = if let Some(stripped) = host.strip_prefix('[') {
        stripped.split(']').next().unwrap_or(stripped)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    host.trim().to_ascii_lowercase()
}

fn is_public_host_admin_route(headers: &HeaderMap, config: &DiscordAdminLoginConfig) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(host_without_port)
        .unwrap_or_default();
    if host.is_empty() || is_loopback_host(&host) {
        return false;
    }
    let admin_host = url::Url::parse(&config.admin_base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    !admin_host.is_empty() && host != admin_host
}

fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

fn value_to_string(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    if let Some(n) = value.as_u64() {
        return n.to_string();
    }
    if let Some(n) = value.as_i64() {
        return n.to_string();
    }
    String::new()
}

fn form_value(body: &Bytes, key: &str) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    for (name, value) in url::form_urlencoded::parse(text.as_bytes()) {
        if name == key {
            return Some(value.into_owned());
        }
    }
    None
}

fn normalize_js_fp(raw: &str) -> String {
    let candidate = raw.trim().to_ascii_lowercase();
    if (8..=64).contains(&candidate.len())
        && candidate.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return candidate;
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(b"fallback");
    digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .chars()
        .take(32)
        .collect()
}

fn sanitize_inline(raw: &str) -> String {
    raw.chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .take(128)
        .collect()
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn internal_token_from_env() -> Option<String> {
    ["TWITCH_INTERNAL_API_TOKEN", "MASTER_BROKER_TOKEN", "MAIN_BOT_INTERNAL_TOKEN"]
        .iter()
        .find_map(|key| non_empty_env(key))
}

fn admin_base_url_from_env() -> Option<String> {
    let raw = non_empty_env("TWITCH_ADMIN_PUBLIC_URL")
        .or_else(|| non_empty_env("MASTER_DASHBOARD_PUBLIC_URL"))
        .unwrap_or_else(|| DEFAULT_ADMIN_BASE_URL.to_string());
    normalize_base_url(&raw)
}

fn normalize_base_url(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw.trim()).ok()?;
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let scheme = url.scheme().to_ascii_lowercase();
    let host = url.host_str()?.to_ascii_lowercase();
    let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1");
    if scheme != "https" && !(scheme == "http" && loopback) {
        return None;
    }
    let origin = match url.port() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    Some(origin.trim_end_matches('/').to_string())
}

fn shared_admin_cookie_domain_from_env() -> Option<String> {
    let domain = non_empty_env("SHARED_ADMIN_COOKIE_DOMAIN")
        .unwrap_or_else(|| DEFAULT_COOKIE_DOMAIN.to_string())
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    (!domain.is_empty()).then_some(domain)
}

fn optional_u64_env(keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| non_empty_env(key))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn parse_u64_csv_env(keys: &[&str]) -> Vec<u64> {
    let Some(raw) = keys.iter().find_map(|key| non_empty_env(key)) else {
        return Vec::new();
    };
    raw.split(',')
        .filter_map(|part| part.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .collect()
}

const FP_COLLECT_HTML: &str = r##"<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<title>Admin-Authentifizierung</title>
<style>
body {
  font-family: system-ui, sans-serif;
  background: #0d1117;
  color: #e6edf3;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100vh;
  margin: 0;
}
.box { text-align: center; }
p { opacity: 0.7; font-size: 0.95rem; }
</style>
</head>
<body>
<div class="box">
  <p>Sicherheitspruefung laeuft...</p>
  <noscript><p style="color:#f85149">JavaScript ist erforderlich.</p></noscript>
</div>
<script>
(function() {
  function canvasHash() {
    try {
      var c = document.createElement("canvas");
      var ctx = c.getContext("2d");
      ctx.textBaseline = "top";
      ctx.font = "14px Arial";
      ctx.fillStyle = "#f60";
      ctx.fillRect(125, 1, 62, 20);
      ctx.fillStyle = "#069";
      ctx.fillText("DDC-Admin-Auth | " + (navigator.language || ""), 2, 15);
      ctx.fillStyle = "rgba(102,204,0,0.7)";
      ctx.fillText("DDC-Admin-Auth | " + (navigator.language || ""), 4, 17);
      return c.toDataURL();
    } catch (err) {
      return "no-canvas";
    }
  }

  function rawFingerprint() {
    var timezone = "";
    try {
      timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || "";
    } catch (err) {}
    return [
      (screen.width || 0) + "x" + (screen.height || 0),
      timezone,
      navigator.language || "",
      String(navigator.hardwareConcurrency || 0),
      canvasHash()
    ].join("||");
  }

  function sha256hex(str) {
    if (window.crypto && window.crypto.subtle) {
      var enc = new TextEncoder();
      return window.crypto.subtle.digest("SHA-256", enc.encode(str)).then(function(buf) {
        return Array.from(new Uint8Array(buf)).map(function(b) {
          return b.toString(16).padStart(2, "0");
        }).join("").slice(0, 32);
      });
    }

    var h = 0;
    for (var i = 0; i < str.length; i++) {
      h = ((h << 5) - h + str.charCodeAt(i)) | 0;
    }
    return Promise.resolve(("00000000" + Math.abs(h).toString(16)).slice(-8).padStart(32, "0"));
  }

  sha256hex(rawFingerprint()).then(function(hash) {
    var form = document.createElement("form");
    form.method = "POST";
    form.action = "/twitch/auth/fingerprint";

    var fp = document.createElement("input");
    fp.type = "hidden";
    fp.name = "fp";
    fp.value = hash;
    form.appendChild(fp);

    document.body.appendChild(form);
    form.submit();
  });
})();
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Arc};

    use tokio::sync::Mutex;

    #[derive(Clone)]
    struct FakeDiscordClient {
        authorize: DiscordAuthorize,
        sessions: Arc<Mutex<HashMap<String, DiscordAdminSession>>>,
        central_session_valid: bool,
        imported: Arc<Mutex<Vec<String>>>,
        revoked: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl DiscordAdminOAuthClient for FakeDiscordClient {
        async fn initiate(
            &self,
            scope: &str,
            redirect_after: &str,
            requesting_service: &str,
            metadata: Value,
        ) -> Result<DiscordAuthorize, DiscordAdminOAuthError> {
            assert_eq!(scope, "identify");
            assert!(redirect_after.ends_with(ADMIN_COMPLETE_PATH));
            assert_eq!(requesting_service, "twitch-admin");
            assert!(metadata.get("next_path").is_some());
            Ok(self.authorize.clone())
        }

        async fn consume_result(
            &self,
            state_id: &str,
        ) -> Result<DiscordAdminSession, DiscordAdminOAuthError> {
            self.sessions
                .lock()
                .await
                .remove(state_id)
                .ok_or(DiscordAdminOAuthError)
        }

        async fn validate_session(
            &self,
            _session_id: &str,
        ) -> Result<ValidatedAdminSession, DiscordAdminOAuthError> {
            if !self.central_session_valid {
                return Err(DiscordAdminOAuthError);
            }
            Ok(ValidatedAdminSession {
                user_id: 42,
                username: "earlysalty".into(),
                display_name: "EarlySalty".into(),
                expires_at: 9_999_999_999.0,
            })
        }

        async fn import_session(
            &self,
            session_id: &str,
            _session: &ValidatedAdminSession,
        ) -> Result<(), DiscordAdminOAuthError> {
            self.imported.lock().await.push(session_id.to_string());
            Ok(())
        }

        async fn revoke_session(&self, session_id: &str) -> Result<(), DiscordAdminOAuthError> {
            self.revoked.lock().await.push(session_id.to_string());
            Ok(())
        }
    }

    fn fake_client(entries: Vec<(&str, DiscordAdminSession)>) -> Arc<FakeDiscordClient> {
        fake_client_with_central_validation(entries, false)
    }

    fn fake_client_with_central_validation(
        entries: Vec<(&str, DiscordAdminSession)>,
        central_session_valid: bool,
    ) -> Arc<FakeDiscordClient> {
        let sessions = entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        Arc::new(FakeDiscordClient {
            authorize: DiscordAuthorize {
                authorize_url: "https://discord.com/oauth2/authorize?client_id=cid&state=s".into(),
                state_id: "broker-state".into(),
            },
            sessions: Arc::new(Mutex::new(sessions)),
            central_session_valid,
            imported: Arc::new(Mutex::new(Vec::new())),
            revoked: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn config(client: Arc<dyn DiscordAdminOAuthClient>) -> DiscordAdminLoginConfig {
        DiscordAdminLoginConfig {
            admin_base_url: "https://admin.test".into(),
            cookie_secure: false,
            cookie_domain: Some(DEFAULT_COOKIE_DOMAIN.into()),
            owner_user_id: None,
            moderator_role_id: DEFAULT_DASHBOARD_MODERATOR_ROLE_ID,
            admin_guild_ids: vec![42],
            client,
        }
    }

    fn admin_session(state_next: &str, roles: Vec<String>) -> DiscordAdminSession {
        DiscordAdminSession {
            discord_id: "123456789012345678".into(),
            discord_name: "AdminUser".into(),
            discord_roles: roles,
            service_metadata: json!({ "next_path": state_next }),
        }
    }

    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    async fn maybe_pool(prefix: &str) -> Option<sqlx::PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name(prefix);
        let admin_pool = sqlx::PgPool::connect(&url).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin_pool)
            .await
            .ok()?;
        admin_pool.close().await;

        let opts: sqlx::postgres::PgConnectOptions = url.parse().ok()?;
        let opts = opts.options([("search_path", schema.as_str())]);
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()
    }

    async fn ensure_sessions_table(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    fn base_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("admin.test"));
        headers.insert(header::USER_AGENT, HeaderValue::from_static("Mozilla/5.0 Test"));
        headers.insert(header::ACCEPT_LANGUAGE, HeaderValue::from_static("de,en;q=0.9"));
        headers.insert("sec-ch-ua-platform", HeaderValue::from_static("\"Linux\""));
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));
        headers
    }

    fn cookies(response: &Response) -> Vec<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .map(str::to_string)
            .collect()
    }

    fn cookie_value(response: &Response, name: &str) -> String {
        let prefix = format!("{name}=");
        cookies(response)
            .into_iter()
            .find_map(|cookie| {
                cookie
                    .strip_prefix(&prefix)
                    .and_then(|rest| rest.split(';').next())
                    .map(str::to_string)
            })
            .unwrap_or_default()
    }

    #[test]
    fn next_und_canonical_blocken_open_redirects() {
        assert_eq!(
            normalize_discord_admin_next_path(Some("/twitch/admin/legacy?tab=x")),
            "/twitch/admin/legacy?tab=x"
        );
        assert_eq!(normalize_discord_admin_next_path(Some("//evil.test")), ADMIN_FALLBACK_PATH);
        assert_eq!(
            normalize_discord_admin_next_path(Some("https://evil.test/twitch/admin")),
            ADMIN_FALLBACK_PATH
        );
        assert_eq!(
            canonical_discord_admin_post_login_path(Some("/twitch/abbo")),
            "/twitch/pricing"
        );
        assert_eq!(
            canonical_discord_admin_post_login_path(Some("/twitch/admin/legacy?x=1")),
            "/twitch/admin/legacy?x=1"
        );
        assert_eq!(
            canonical_discord_admin_post_login_path(Some("/twitch/not-allowed")),
            ADMIN_FALLBACK_PATH
        );
    }

    #[test]
    fn privileg_check_nur_owner_oder_moderator_rolle() {
        let cfg = config(fake_client(Vec::new()));
        assert_eq!(
            discord_admin_privilege_reason(
                10,
                &[DEFAULT_DASHBOARD_MODERATOR_ROLE_ID.to_string()],
                &cfg
            ),
            (true, "moderator_role:delegated")
        );
        assert_eq!(
            discord_admin_privilege_reason(10, &["111".into()], &cfg),
            (false, "missing_admin_or_moderator_role")
        );
        let mut owner_cfg = cfg.clone();
        owner_cfg.owner_user_id = Some(10);
        assert_eq!(
            discord_admin_privilege_reason(10, &[], &owner_cfg),
            (true, "owner_override")
        );
    }

    #[tokio::test]
    async fn login_start_delegiert_mit_identify_scope() {
        let Some(pool) = maybe_pool("discord_admin_login_start").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let cfg = config(fake_client(Vec::new()));

        let response = login_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            base_headers(),
            Query(AdminLoginQuery { next: Some("/twitch/admin/legacy".into()) }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
        assert!(location.starts_with("https://discord.com/oauth2/authorize"));
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store, max-age=0"
        );
    }

    /// Headers mit abweichendem User-Agent → anderer Passive-Fingerprint als
    /// [`base_headers`], gleiche IP.
    fn headers_mit_anderem_user_agent(session_id: &str) -> HeaderMap {
        let mut headers = base_headers();
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 Test Neuer Browser"),
        );
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{ADMIN_COOKIE_NAME}={session_id}")).unwrap(),
        );
        headers
    }

    fn headers_mit_cookie(session_id: &str) -> HeaderMap {
        let mut headers = base_headers();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{ADMIN_COOKIE_NAME}={session_id}")).unwrap(),
        );
        headers
    }

    /// Legt eine Discord-Admin-Session an, die an die Bindung von `headers` gebunden
    /// ist, und schließt den Fingerprint-Schritt ab (`fp_pending = false`).
    async fn gebundene_admin_session(state: &DashboardAuthState, headers: &HeaderMap) -> String {
        let created = state
            .create_discord_admin_session(
                42,
                "earlysalty",
                "EarlySalty",
                "owner",
                &client_ip_from_headers(headers),
                &passive_fp_from_headers(headers),
                ADMIN_FALLBACK_PATH,
            )
            .await
            .expect("Session anlegen");
        state
            .complete_admin_session_fingerprint(&created.session_id, "js-fp-abc")
            .await
            .expect("Fingerprint-Schritt abschließen");
        created.session_id
    }

    async fn lokale_discord_dashboard_session(state: &DashboardAuthState) -> String {
        state
            .create_admin_session("42", "EarlySalty")
            .await
            .expect("lokale Discord-Dashboard-Session anlegen")
            .session_id
    }

    fn geloeschtes_admin_cookie(response: &Response) -> bool {
        cookies(response).iter().any(|c| {
            c.starts_with(&format!("{ADMIN_COOKIE_NAME}=;")) && c.contains("Max-Age=0")
        })
    }

    /// Live-Vorfall 2026-07-10: Der Passive-FP änderte sich (Browser-Update), der
    /// Forward-Auth lehnte die Session mit 401 ab, der Login-Handler hielt sie für
    /// gültig und schickte zurück ins Panel → Endlos-Redirect-Loop bis zum
    /// Rate-Limit. Der Login-Handler muss dieselbe Bindung prüfen wie der
    /// Forward-Auth und den Nutzer in den frischen OAuth-Flow schicken.
    #[tokio::test]
    async fn login_bei_passive_fp_mismatch_startet_oauth_statt_panel_redirect() {
        let Some(pool) = maybe_pool("discord_admin_login_fp_mismatch").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let session_id = gebundene_admin_session(&state, &base_headers()).await;
        let cfg = config(fake_client(Vec::new()));

        let response = login_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            headers_mit_anderem_user_agent(&session_id),
            Query(AdminLoginQuery { next: None }),
        )
        .await;

        let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
        assert!(
            location.starts_with("https://discord.com/oauth2/authorize"),
            "muss frischen OAuth-Flow starten, nicht ins Panel zurueckschicken: {location}"
        );
        assert!(
            geloeschtes_admin_cookie(&response),
            "das nicht mehr nutzbare Cookie muss geraeumt werden, sonst loopt der naechste Aufruf erneut"
        );
    }

    /// `fp_pending` heißt: der JS-Fingerprint-Schritt steht noch aus, der
    /// Forward-Auth antwortet 401. Auch hier darf der Login nicht ins Panel
    /// zurückschicken.
    #[tokio::test]
    async fn login_bei_fp_pending_startet_oauth_statt_panel_redirect() {
        let Some(pool) = maybe_pool("discord_admin_login_fp_pending").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let headers = base_headers();
        let created = state
            .create_discord_admin_session(
                42,
                "earlysalty",
                "EarlySalty",
                "owner",
                &client_ip_from_headers(&headers),
                &passive_fp_from_headers(&headers),
                ADMIN_FALLBACK_PATH,
            )
            .await
            .expect("Session anlegen");
        let cfg = config(fake_client(Vec::new()));

        let response = login_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            headers_mit_cookie(&created.session_id),
            Query(AdminLoginQuery { next: None }),
        )
        .await;

        let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
        assert!(
            location.starts_with("https://discord.com/oauth2/authorize"),
            "fp_pending-Session darf nicht als eingeloggt gelten: {location}"
        );
    }

    #[tokio::test]
    async fn login_bei_zentral_abgelehnter_session_startet_oauth_und_raeumt_cookie() {
        let Some(pool) = maybe_pool("discord_admin_login_central_rejected").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let session_id = lokale_discord_dashboard_session(&state).await;
        let cfg = config(fake_client(Vec::new()));

        let response = login_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            headers_mit_cookie(&session_id),
            Query(AdminLoginQuery { next: None }),
        )
        .await;

        let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
        assert!(
            location.starts_with("https://discord.com/oauth2/authorize"),
            "zentral abgelehnte Session muss frischen OAuth-Flow starten: {location}"
        );
        assert!(
            geloeschtes_admin_cookie(&response),
            "zentral abgelehntes Cookie muss geraeumt werden"
        );
    }

    /// Regression: lokal und zentral gültig → weiterhin „schon eingeloggt",
    /// Redirect ins Panel, Session-Cookie bleibt gesetzt.
    #[tokio::test]
    async fn login_bei_lokal_und_zentral_gueltiger_session_redirectet_ins_panel() {
        let Some(pool) = maybe_pool("discord_admin_login_central_valid").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let session_id = lokale_discord_dashboard_session(&state).await;
        let cfg = config(fake_client_with_central_validation(Vec::new(), true));

        let response = login_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            headers_mit_cookie(&session_id),
            Query(AdminLoginQuery { next: None }),
        )
        .await;

        let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
        assert_eq!(location, ADMIN_FALLBACK_PATH);
        assert!(
            cookies(&response)
                .iter()
                .any(|c| c.starts_with(&format!("{ADMIN_COOKIE_NAME}={session_id}"))),
            "gueltige Session behaelt ihr Cookie"
        );
    }

    #[tokio::test]
    async fn state_ist_nur_einmal_konsumierbar() {
        let Some(pool) = maybe_pool("discord_admin_state_once").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let roles = vec![DEFAULT_DASHBOARD_MODERATOR_ROLE_ID.to_string()];
        let client = fake_client(vec![("once", admin_session("/twitch/admin", roles))]);
        let cfg = config(client);

        let first = complete_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg.clone())),
            base_headers(),
            Query(CompleteQuery { state_id: Some("once".into()), error: None }),
        )
        .await;
        assert_eq!(first.status(), StatusCode::SEE_OTHER);
        assert!(!cookie_value(&first, ADMIN_COOKIE_NAME).is_empty());

        let replay = complete_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            base_headers(),
            Query(CompleteQuery { state_id: Some("once".into()), error: None }),
        )
        .await;
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
        assert!(cookie_value(&replay, ADMIN_COOKIE_NAME).is_empty());
    }

    #[tokio::test]
    async fn callback_mintet_session_fuer_admin_discord_user() {
        let Some(pool) = maybe_pool("discord_admin_mint").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let roles = vec![DEFAULT_DASHBOARD_MODERATOR_ROLE_ID.to_string()];
        let client = fake_client(vec![(
            "admin",
            admin_session("/twitch/admin/legacy?tab=live", roles),
        )]);
        let cfg = config(client.clone());

        let response = complete_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            base_headers(),
            Query(CompleteQuery { state_id: Some("admin".into()), error: None }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), FINGERPRINT_PATH);
        let response_cookies = cookies(&response);
        assert_eq!(response_cookies.len(), 2);
        let set_cookie = response_cookies.join("\n");
        assert!(set_cookie.contains("master_dash_session="));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        assert!(set_cookie.contains("Domain=deutsche-deadlock-community.de"));
        assert!(set_cookie.contains("Max-Age=1209600"));

        let sid = cookie_value(&response, ADMIN_COOKIE_NAME);
        assert_eq!(client.imported.lock().await.as_slice(), std::slice::from_ref(&sid));
        let fp = state
            .load_admin_session_fingerprint(&sid)
            .await
            .unwrap()
            .expect("Session muss in der DB liegen");
        assert_eq!(fp.client_ip, "203.0.113.7");
        assert_eq!(fp.passive_fp, build_passive_fp("Mozilla/5.0 Test", "de", "Linux"));
        assert!(fp.fp_pending);
        assert_eq!(fp.username, "AdminUser");
    }

    #[tokio::test]
    async fn nicht_admin_wird_abgelehnt_ohne_cookie() {
        let Some(pool) = maybe_pool("discord_admin_denied").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let cfg = config(fake_client(vec![(
            "nope",
            admin_session("/twitch/admin", vec!["111".into()]),
        )]));

        let response = complete_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            base_headers(),
            Query(CompleteQuery { state_id: Some("nope".into()), error: None }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(cookie_value(&response, ADMIN_COOKIE_NAME).is_empty());
    }

    #[tokio::test]
    async fn fingerprint_submit_schliesst_pending_session_ab() {
        let Some(pool) = maybe_pool("discord_admin_fp_submit").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let created = state
            .create_discord_admin_session(
                123456789012345678,
                "AdminUser",
                "AdminUser",
                "moderator_role:delegated",
                "203.0.113.7",
                "abc123",
                "/twitch/admin/legacy?tab=live",
            )
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("master_dash_session={}", created.session_id)).unwrap(),
        );

        let response = fingerprint_submit_handler(
            Some(Extension(state.clone())),
            None,
            headers,
            Bytes::from_static(b"fp=abcdef1234567890"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "/twitch/admin/legacy?tab=live"
        );
        let fp = state
            .load_admin_session_fingerprint(&created.session_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!fp.fp_pending);
        assert_eq!(fp.js_fp, "abcdef1234567890");
        assert!(fp.verify("203.0.113.7", "abc123"));
    }

    #[tokio::test]
    async fn logout_loescht_admin_session_und_cookie() {
        let Some(pool) = maybe_pool("discord_admin_logout").await else { return; };
        ensure_sessions_table(&pool).await;
        let state = DashboardAuthState::new(pool, test_fernet_key());
        let created = state
            .create_discord_admin_session(
                123456789012345678,
                "AdminUser",
                "AdminUser",
                "moderator_role:delegated",
                "",
                "",
                "/twitch/admin",
            )
            .await
            .unwrap();
        assert!(state.load_admin_session(&created.session_id).await.unwrap().is_some());
        let client = fake_client(Vec::new());
        let cfg = config(client.clone());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!(
                "master_dash_session=veraltet; master_dash_session={}",
                created.session_id
            ))
            .unwrap(),
        );

        let response = logout_handler(Some(Extension(state.clone())), Some(Extension(cfg)), headers).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "https://admin.test/twitch/auth/discord/login"
        );
        let set_cookie = cookies(&response).join("\n");
        assert!(set_cookie.contains("master_dash_session=;"));
        assert!(set_cookie.contains("Max-Age=0"));
        assert!(set_cookie.contains("Domain=deutsche-deadlock-community.de"));
        assert_eq!(cookies(&response).len(), 2);
        assert!(state.load_admin_session(&created.session_id).await.unwrap().is_none());
        assert_eq!(
            client.revoked.lock().await.as_slice(),
            &["veraltet".to_string(), created.session_id]
        );
    }
}
