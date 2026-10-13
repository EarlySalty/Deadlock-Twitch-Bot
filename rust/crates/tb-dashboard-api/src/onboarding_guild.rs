use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use url::Url;

use crate::auth::level::DashboardAuthLevel;
use tb_raid::partner_setup::{
    attach_streamer_guild, promote_streamer_to_partner, PromotePartnerArgs,
    STREAMER_GUILD_SOURCE_ONBOARDING,
};

pub const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const DISCORD_SCOPES: &str = "identify guilds";
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_RETURN_PATH: &str = "/twitch/onboarding";

const RETURN_HOSTS: &[&str] = &[
    "deutsche-deadlock-community.de",
    "www.deutsche-deadlock-community.de",
    "dead-lock.de",
    "www.dead-lock.de",
    "localhost",
    "127.0.0.1",
];

#[derive(Clone)]
pub struct OnboardingDiscordConfig {
    pub client_id: String,
    pub redirect_uri: String,
    pub client: Arc<dyn OnboardingDiscordClient>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordUserGuild {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordOnboardingIdentity {
    pub user_id: String,
    pub display_name: String,
    pub guilds: Vec<DiscordUserGuild>,
}

#[derive(Debug, Clone)]
pub struct OnboardingDiscordError;

#[async_trait]
pub trait OnboardingDiscordClient: Send + Sync {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<DiscordOnboardingIdentity, OnboardingDiscordError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledGuild {
    pub guild_id: i64,
    pub name: String,
    pub language: String,
    pub auto_partner_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuildDecision {
    None,
    Auto(InstalledGuild),
    Pick(Vec<InstalledGuild>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignError {
    Foreign,
    MissingGuild,
}

pub fn decide_guilds(user_guild_ids: &[i64], installed: Vec<InstalledGuild>) -> GuildDecision {
    let user: HashSet<i64> = user_guild_ids
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .collect();
    let mut matches: Vec<InstalledGuild> = installed
        .into_iter()
        .filter(|guild| user.contains(&guild.guild_id))
        .collect();
    matches.sort_by_key(|guild| guild.guild_id);
    match matches.len() {
        0 => GuildDecision::None,
        1 => GuildDecision::Auto(matches.remove(0)),
        _ => GuildDecision::Pick(matches),
    }
}

pub fn sanitize_return_url(raw: Option<&str>) -> String {
    let Some(candidate) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return DEFAULT_RETURN_PATH.to_string();
    };
    if candidate.starts_with('/') && !candidate.starts_with("//") && !candidate.contains('\\') {
        if candidate.chars().any(char::is_control) {
            return DEFAULT_RETURN_PATH.to_string();
        }
        return candidate
            .split('#')
            .next()
            .unwrap_or(DEFAULT_RETURN_PATH)
            .to_string();
    }
    let Ok(parsed) = Url::parse(candidate) else {
        return DEFAULT_RETURN_PATH.to_string();
    };
    let Some(host) = parsed.host_str() else {
        return DEFAULT_RETURN_PATH.to_string();
    };
    let host = host.to_ascii_lowercase();
    let https_ok = parsed.scheme() == "https";
    let local_ok = parsed.scheme() == "http" && (host == "localhost" || host == "127.0.0.1");
    if !(https_ok || local_ok) || !RETURN_HOSTS.contains(&host.as_str()) {
        return DEFAULT_RETURN_PATH.to_string();
    }
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path = "/".into();
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    if parsed.scheme() == "https" {
        format!("https://{host}{path}")
    } else {
        format!("http://{host}{path}")
    }
}

pub fn build_authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let mut url = match Url::parse(DISCORD_AUTHORIZE_URL) {
        Ok(url) => url,
        Err(_) => return DISCORD_AUTHORIZE_URL.to_string(),
    };
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", DISCORD_SCOPES)
        .append_pair("state", state);
    url.to_string()
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn onboarding_discord_config_from_env() -> Option<OnboardingDiscordConfig> {
    let client_id = non_empty_env("DISCORD_HUB_CLIENT_ID")?;
    let client_secret = non_empty_env("DISCORD_HUB_CLIENT_SECRET")?;
    let redirect_uri = non_empty_env("TB_ONBOARDING_DISCORD_REDIRECT_URI")?;
    let parsed = Url::parse(&redirect_uri).ok()?;
    if parsed.scheme() != "https"
        && parsed.host_str() != Some("localhost")
        && parsed.host_str() != Some("127.0.0.1")
    {
        return None;
    }
    let client = DiscordRestClient::new(client_id.clone(), client_secret)?;
    Some(OnboardingDiscordConfig {
        client_id,
        redirect_uri,
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
    ) -> Result<serde_json::Value, OnboardingDiscordError> {
        let response = self
            .http
            .get(format!("{DISCORD_API_BASE}{path}"))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| OnboardingDiscordError)?;
        if !response.status().is_success() {
            return Err(OnboardingDiscordError);
        }
        response.json().await.map_err(|_| OnboardingDiscordError)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[async_trait]
impl OnboardingDiscordClient for DiscordRestClient {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<DiscordOnboardingIdentity, OnboardingDiscordError> {
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
            .map_err(|_| OnboardingDiscordError)?;
        if !response.status().is_success() {
            return Err(OnboardingDiscordError);
        }
        let token: TokenResponse = response.json().await.map_err(|_| OnboardingDiscordError)?;
        let user = self.get_json("/users/@me", &token.access_token).await?;
        let guilds = self
            .get_json("/users/@me/guilds", &token.access_token)
            .await?;
        parse_identity(&user, &guilds).ok_or(OnboardingDiscordError)
    }
}

pub fn parse_identity(
    user: &serde_json::Value,
    guilds: &serde_json::Value,
) -> Option<DiscordOnboardingIdentity> {
    let user_id = user
        .get("id")?
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))?
        .to_string();
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
            let name = guild
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>();
            Some(DiscordUserGuild { id, name })
        })
        .collect();
    Some(DiscordOnboardingIdentity {
        user_id,
        display_name,
        guilds,
    })
}

pub fn router(pool: PgPool) -> Router {
    let mut router = Router::new()
        .route("/twitch/onboarding/discord/login", get(login_handler))
        .route("/twitch/onboarding/discord/callback", get(callback_handler))
        .route(
            "/twitch/api/v2/streamer/onboarding/guilds",
            get(list_guilds_handler).post(assign_guild_handler),
        )
        .with_state(pool);
    if let Some(config) = onboarding_discord_config_from_env() {
        router = router.layer(Extension(config));
    }
    router
}

#[derive(Debug, Deserialize, Default)]
pub struct LoginQuery {
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default, rename = "return")]
    pub return_to: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct AssignBody {
    pub guild_id: String,
}

fn partner_ids(auth: &DashboardAuthLevel) -> Option<(String, String)> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
            ..
        } if !twitch_user_id.trim().is_empty()
            && twitch_user_id.bytes().all(|c| c.is_ascii_digit())
            && !twitch_login.trim().is_empty() =>
        {
            Some((twitch_user_id.clone(), twitch_login.clone()))
        }
        _ => None,
    }
}

fn unauthorized_login() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":"Bitte melde dich erneut mit Twitch an."})),
    )
        .into_response()
}

fn locale_of(raw: Option<&str>) -> String {
    match raw.map(str::trim) {
        Some("en") => "en".into(),
        _ => "de".into(),
    }
}

fn append_query(base: &str, key: &str, value: &str) -> String {
    let mut url = match Url::parse(base) {
        Ok(url) => url,
        Err(_) => {
            let separator = if base.contains('?') { '&' } else { '?' };
            return format!("{base}{separator}{key}={value}");
        }
    };
    url.query_pairs_mut().append_pair(key, value);
    url.to_string()
}

pub async fn login_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    config: Option<Extension<OnboardingDiscordConfig>>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let Some((twitch_user_id, twitch_login)) = partner_ids(&auth) else {
        return Redirect::to(&format!(
            "/twitch/auth/login?next={}",
            encoded_path("/twitch/onboarding/discord/login")
        ))
        .into_response();
    };
    let Some(Extension(config)) = config else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"Discord-Login ist gerade nicht erreichbar."})),
        )
            .into_response();
    };
    let return_to = sanitize_return_url(query.return_to.as_deref());
    let locale = locale_of(query.locale.as_deref());
    let state = uuid::Uuid::new_v4().to_string();
    if let Err(error) = sqlx::query(
        r#"
        INSERT INTO onboarding_discord_states (
            state_token, twitch_user_id, twitch_login, redirect_uri, locale, expires_at
        ) VALUES ($1, $2, $3, $4, $5, NOW() + INTERVAL '10 minutes')
        "#,
    )
    .bind(&state)
    .bind(&twitch_user_id)
    .bind(&twitch_login)
    .bind(&return_to)
    .bind(&locale)
    .execute(&pool)
    .await
    {
        tracing::warn!(%error, "Onboarding-Discord-State konnte nicht gespeichert werden");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"Bitte versuche es erneut."})),
        )
            .into_response();
    }
    let url = build_authorize_url(&config.client_id, &config.redirect_uri, &state);
    Redirect::to(&url).into_response()
}

fn encoded_path(path: &str) -> String {
    percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC).to_string()
}

pub async fn callback_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<OnboardingDiscordConfig>>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(Extension(config)) = config else {
        return Redirect::to(DEFAULT_RETURN_PATH).into_response();
    };
    let Some(state) = query
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| value.len() >= 20 && value.len() <= 200)
    else {
        return Redirect::to(DEFAULT_RETURN_PATH).into_response();
    };
    let row = sqlx::query_as::<_, StateRow>(
        r#"
        SELECT state_token, twitch_user_id, twitch_login, redirect_uri,
               discord_user_id, guild_ids
          FROM onboarding_discord_states
         WHERE state_token = $1
           AND consumed_at IS NULL
           AND expires_at > NOW()
        "#,
    )
    .bind(state)
    .fetch_optional(&pool)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return Redirect::to(DEFAULT_RETURN_PATH).into_response(),
        Err(error) => {
            tracing::warn!(%error, "Onboarding-Discord-State konnte nicht gelesen werden");
            return Redirect::to(DEFAULT_RETURN_PATH).into_response();
        }
    };
    if query.error.as_deref().is_some_and(|e| !e.trim().is_empty()) {
        return Redirect::to(&append_query(&row.redirect_uri, "onboarding", "error"))
            .into_response();
    }
    let identity = if row.discord_user_id.is_some() {
        DiscordOnboardingIdentity {
            user_id: row.discord_user_id.clone().unwrap_or_default(),
            display_name: String::new(),
            guilds: row
                .guild_ids
                .iter()
                .copied()
                .map(|id| DiscordUserGuild {
                    id,
                    name: String::new(),
                })
                .collect(),
        }
    } else {
        let Some(code) = query
            .code
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            return Redirect::to(&append_query(&row.redirect_uri, "onboarding", "error"))
                .into_response();
        };
        match config
            .client
            .exchange_code(code, &config.redirect_uri)
            .await
        {
            Ok(identity) => identity,
            Err(_) => {
                return Redirect::to(&append_query(&row.redirect_uri, "onboarding", "error"))
                    .into_response();
            }
        }
    };
    let guild_ids: Vec<i64> = identity.guilds.iter().map(|g| g.id).collect();
    if let Err(error) = sqlx::query(
        r#"
        UPDATE onboarding_discord_states
           SET discord_user_id = $2,
               guild_ids = $3
         WHERE state_token = $1
        "#,
    )
    .bind(state)
    .bind(&identity.user_id)
    .bind(&guild_ids)
    .execute(&pool)
    .await
    {
        tracing::warn!(%error, "Onboarding-Discord-State konnte nicht aktualisiert werden");
        return Redirect::to(&append_query(&row.redirect_uri, "onboarding", "error"))
            .into_response();
    }
    if let Err(error) = upsert_identity(
        &pool,
        &row.twitch_user_id,
        &row.twitch_login,
        Some(&identity.user_id),
        Some(&identity.display_name),
    )
    .await
    {
        tracing::warn!(%error, "Discord-Identität konnte nicht gespeichert werden");
    }
    let installed = match load_installed_guilds(&pool).await {
        Ok(guilds) => guilds,
        Err(error) => {
            tracing::warn!(%error, "Installierte Guilds konnten nicht gelesen werden");
            return Redirect::to(&append_query(&row.redirect_uri, "onboarding", "error"))
                .into_response();
        }
    };
    match decide_guilds(&guild_ids, installed) {
        GuildDecision::Auto(guild) => {
            match assign_streamer_to_guild(
                &pool,
                &row.twitch_user_id,
                &row.twitch_login,
                Some(&identity.user_id),
                Some(&identity.display_name),
                guild.guild_id,
                &guild_ids,
            )
            .await
            {
                Ok(outcome) => {
                    let _ = mark_consumed(&pool, state).await;
                    let mut dest = append_query(&row.redirect_uri, "onboarding", "assigned");
                    if !outcome.active {
                        dest = append_query(&dest, "inactive", "1");
                    }
                    Redirect::to(&dest).into_response()
                }
                Err(AssignError::Foreign) => {
                    Redirect::to(&append_query(&row.redirect_uri, "onboarding", "foreign"))
                        .into_response()
                }
                Err(_) => Redirect::to(&append_query(&row.redirect_uri, "onboarding", "error"))
                    .into_response(),
            }
        }
        GuildDecision::Pick(_) => {
            Redirect::to(&append_query(&row.redirect_uri, "onboarding", "pick")).into_response()
        }
        GuildDecision::None => {
            Redirect::to(&append_query(&row.redirect_uri, "onboarding", "none")).into_response()
        }
    }
}

pub async fn list_guilds_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    let Some((twitch_user_id, _)) = partner_ids(&auth) else {
        return unauthorized_login();
    };
    let state = sqlx::query_as::<_, StateRow>(
        r#"
        SELECT state_token, twitch_user_id, twitch_login, redirect_uri,
               discord_user_id, guild_ids
          FROM onboarding_discord_states
         WHERE twitch_user_id = $1
           AND consumed_at IS NULL
           AND expires_at > NOW()
         ORDER BY expires_at DESC
         LIMIT 1
        "#,
    )
    .bind(&twitch_user_id)
    .fetch_optional(&pool)
    .await;
    let state = match state {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, "Onboarding-Guilds konnten nicht gelesen werden");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"Bitte versuche es erneut."})),
            )
                .into_response();
        }
    };
    let Some(state) = state.filter(|row| row.discord_user_id.is_some()) else {
        return Json(json!({
            "needs_discord": true,
            "decision": "login",
            "guilds": [],
            "assigned": serde_json::Value::Null
        }))
        .into_response();
    };
    let installed = match load_installed_guilds(&pool).await {
        Ok(guilds) => guilds,
        Err(error) => {
            tracing::warn!(%error, "Installierte Guilds konnten nicht gelesen werden");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"Bitte versuche es erneut."})),
            )
                .into_response();
        }
    };
    let decision = decide_guilds(&state.guild_ids, installed);
    Json(decision_json(&decision)).into_response()
}

pub async fn assign_guild_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<AssignBody>,
) -> Response {
    let Some((twitch_user_id, twitch_login)) = partner_ids(&auth) else {
        return unauthorized_login();
    };
    let Ok(guild_id) = body.guild_id.trim().parse::<i64>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"unknown_guild"})),
        )
            .into_response();
    };
    let state = sqlx::query_as::<_, StateRow>(
        r#"
        SELECT state_token, twitch_user_id, twitch_login, redirect_uri,
               discord_user_id, guild_ids
          FROM onboarding_discord_states
         WHERE twitch_user_id = $1
           AND consumed_at IS NULL
           AND expires_at > NOW()
         ORDER BY expires_at DESC
         LIMIT 1
        "#,
    )
    .bind(&twitch_user_id)
    .fetch_optional(&pool)
    .await;
    let state = match state {
        Ok(Some(state)) => state,
        Ok(None) => {
            return (StatusCode::CONFLICT, Json(json!({"error":"needs_discord"}))).into_response();
        }
        Err(error) => {
            tracing::warn!(%error, "Onboarding-State konnte nicht gelesen werden");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"Bitte versuche es erneut."})),
            )
                .into_response();
        }
    };
    match assign_streamer_to_guild(
        &pool,
        &twitch_user_id,
        &twitch_login,
        state.discord_user_id.as_deref(),
        None,
        guild_id,
        &state.guild_ids,
    )
    .await
    {
        Ok(outcome) => {
            let _ = mark_consumed(&pool, &state.state_token).await;
            Json(json!({
                "ok": true,
                "guild_id": outcome.guild_id.to_string(),
                "name": outcome.name,
                "active": outcome.active
            }))
            .into_response()
        }
        Err(AssignError::Foreign) => (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"foreign_guild"})),
        )
            .into_response(),
        Err(AssignError::MissingGuild) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"unknown_guild"})),
        )
            .into_response(),
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct StateRow {
    state_token: String,
    twitch_user_id: String,
    twitch_login: String,
    redirect_uri: String,
    discord_user_id: Option<String>,
    guild_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignOutcome {
    pub guild_id: i64,
    pub name: String,
    pub active: bool,
}

pub async fn assign_streamer_to_guild(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
    discord_user_id: Option<&str>,
    discord_display_name: Option<&str>,
    guild_id: i64,
    allowed_guild_ids: &[i64],
) -> Result<AssignOutcome, AssignError> {
    if guild_id <= 0 || !allowed_guild_ids.contains(&guild_id) {
        return Err(AssignError::Foreign);
    }
    let guild = sqlx::query_as::<_, InstalledGuildRow>(
        r#"
        SELECT guild_id, name, language, auto_partner_default
          FROM discord_guilds
         WHERE guild_id = $1 AND status = 'active'
        "#,
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| AssignError::MissingGuild)?;
    let Some(guild) = guild else {
        return Err(AssignError::MissingGuild);
    };
    let mut tx = pool.begin().await.map_err(|_| AssignError::MissingGuild)?;
    if guild.auto_partner_default {
        promote_streamer_to_partner(
            &mut tx,
            &PromotePartnerArgs {
                twitch_login: twitch_login.to_string(),
                twitch_user_id: twitch_user_id.to_string(),
                discord_user_id: discord_user_id.map(str::to_string),
                discord_display_name: discord_display_name.map(str::to_string),
                is_on_discord: i32::from(discord_user_id.is_some()),
                activate_partner_features: true,
                clear_source: true,
                guild_id: Some(guild.guild_id),
            },
            chrono::Utc::now(),
        )
        .await
        .map_err(|_| AssignError::MissingGuild)?;
    } else {
        attach_streamer_guild(
            &mut tx,
            twitch_user_id,
            guild.guild_id,
            false,
            STREAMER_GUILD_SOURCE_ONBOARDING,
        )
        .await
        .map_err(|_| AssignError::MissingGuild)?;
    }
    tx.commit().await.map_err(|_| AssignError::MissingGuild)?;
    if let Err(error) = upsert_identity(
        pool,
        twitch_user_id,
        twitch_login,
        discord_user_id,
        discord_display_name,
    )
    .await
    {
        tracing::warn!(%error, "Discord-Identität nach Guild-Zuordnung nicht gespeichert");
    }
    Ok(AssignOutcome {
        guild_id: guild.guild_id,
        name: guild.name,
        active: guild.auto_partner_default,
    })
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct InstalledGuildRow {
    guild_id: i64,
    name: String,
    language: String,
    auto_partner_default: bool,
}

async fn load_installed_guilds(pool: &PgPool) -> Result<Vec<InstalledGuild>, sqlx::Error> {
    let rows = sqlx::query_as::<_, InstalledGuildRow>(
        r#"
        SELECT guild_id, name, language, auto_partner_default
          FROM discord_guilds
         WHERE status = 'active'
         ORDER BY guild_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| InstalledGuild {
            guild_id: row.guild_id,
            name: row.name,
            language: row.language,
            auto_partner_default: row.auto_partner_default,
        })
        .collect())
}

async fn upsert_identity(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
    discord_user_id: Option<&str>,
    discord_display_name: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO twitch_streamer_identities (
            twitch_user_id, twitch_login, discord_user_id, discord_display_name,
            is_on_discord, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            twitch_login = EXCLUDED.twitch_login,
            discord_user_id = COALESCE(EXCLUDED.discord_user_id, twitch_streamer_identities.discord_user_id),
            discord_display_name = COALESCE(EXCLUDED.discord_display_name, twitch_streamer_identities.discord_display_name),
            is_on_discord = CASE
                WHEN EXCLUDED.discord_user_id IS NOT NULL THEN 1
                ELSE twitch_streamer_identities.is_on_discord
            END,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .bind(discord_user_id)
    .bind(discord_display_name)
    .bind(i32::from(discord_user_id.is_some()))
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_consumed(pool: &PgPool, state: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE onboarding_discord_states SET consumed_at = NOW() WHERE state_token = $1")
        .bind(state)
        .execute(pool)
        .await?;
    Ok(())
}

fn decision_json(decision: &GuildDecision) -> serde_json::Value {
    match decision {
        GuildDecision::None => json!({
            "needs_discord": false,
            "decision": "none",
            "guilds": [],
            "assigned": serde_json::Value::Null
        }),
        GuildDecision::Auto(guild) => json!({
            "needs_discord": false,
            "decision": "auto",
            "guilds": [guild_json(guild)],
            "assigned": guild_json(guild)
        }),
        GuildDecision::Pick(guilds) => json!({
            "needs_discord": false,
            "decision": "pick",
            "guilds": guilds.iter().map(guild_json).collect::<Vec<_>>(),
            "assigned": serde_json::Value::Null
        }),
    }
}

fn guild_json(guild: &InstalledGuild) -> serde_json::Value {
    json!({
        "guild_id": guild.guild_id.to_string(),
        "name": guild.name,
        "language": guild.language,
        "auto_partner_default": guild.auto_partner_default
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(id: i64, name: &str, auto: bool) -> InstalledGuild {
        InstalledGuild {
            guild_id: id,
            name: name.into(),
            language: "en".into(),
            auto_partner_default: auto,
        }
    }

    #[test]
    fn decide_zero_one_many() {
        let installed = vec![
            guild(1, "A", true),
            guild(2, "B", false),
            guild(3, "C", true),
        ];
        assert_eq!(decide_guilds(&[], installed.clone()), GuildDecision::None);
        assert_eq!(decide_guilds(&[9], installed.clone()), GuildDecision::None);
        match decide_guilds(&[2], installed.clone()) {
            GuildDecision::Auto(chosen) => assert_eq!(chosen.guild_id, 2),
            other => panic!("{other:?}"),
        }
        match decide_guilds(&[1, 2, 9], installed) {
            GuildDecision::Pick(list) => {
                assert_eq!(
                    list.iter().map(|g| g.guild_id).collect::<Vec<_>>(),
                    vec![1, 2]
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn return_url_allowlist() {
        assert_eq!(sanitize_return_url(None), DEFAULT_RETURN_PATH);
        assert_eq!(
            sanitize_return_url(Some("/twitch/onboarding")),
            "/twitch/onboarding"
        );
        assert_eq!(
            sanitize_return_url(Some("https://dead-lock.de/twitch/onboarding")),
            "https://dead-lock.de/twitch/onboarding"
        );
        assert_eq!(
            sanitize_return_url(Some(
                "https://deutsche-deadlock-community.de/twitch/onboarding"
            )),
            "https://deutsche-deadlock-community.de/twitch/onboarding"
        );
        assert_eq!(
            sanitize_return_url(Some("https://evil.test/phish")),
            DEFAULT_RETURN_PATH
        );
        assert_eq!(
            sanitize_return_url(Some("http://localhost/twitch/onboarding")),
            "http://localhost/twitch/onboarding"
        );
    }

    #[test]
    fn authorize_url_uses_identify_guilds() {
        let url = build_authorize_url("cid", "https://x.test/cb", "state-token-value");
        assert!(url.contains("scope=identify+guilds") || url.contains("scope=identify%20guilds"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("response_type=code"));
    }

    #[test]
    fn parse_identity_liest_user_und_guilds() {
        let user = json!({"id":"99","username":"nani","global_name":"Nani"});
        let guilds =
            json!([{"id":"1","name":"One"},{"id":"0","name":"bad"},{"id":"2","name":"Two"}]);
        let identity = parse_identity(&user, &guilds).expect("identity");
        assert_eq!(identity.user_id, "99");
        assert_eq!(identity.display_name, "Nani");
        assert_eq!(identity.guilds.len(), 2);
        assert_eq!(identity.guilds[0].id, 1);
    }

    #[tokio::test]
    async fn auto_partner_aus_bleibt_inaktiv_und_fremd_wird_abgelehnt() {
        let db = crate::test_database::Database::new().await;
        for ddl in [
            r#"CREATE TABLE discord_guilds (
                guild_id BIGINT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                language TEXT NOT NULL DEFAULT 'en',
                auto_partner_default BOOLEAN NOT NULL DEFAULT TRUE,
                status TEXT NOT NULL DEFAULT 'active'
            )"#,
            r#"CREATE TABLE streamer_guilds (
                twitch_user_id TEXT NOT NULL,
                guild_id BIGINT NOT NULL REFERENCES discord_guilds(guild_id),
                active BOOLEAN NOT NULL DEFAULT TRUE,
                deactivated_by TEXT,
                deactivated_at TIMESTAMPTZ,
                joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                source TEXT NOT NULL DEFAULT 'onboarding',
                PRIMARY KEY (twitch_user_id, guild_id)
            )"#,
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                discord_user_id TEXT,
                discord_display_name TEXT,
                is_on_discord INTEGER,
                created_at TEXT,
                updated_at TEXT
            )"#,
            r#"INSERT INTO discord_guilds (guild_id, name, language, auto_partner_default, status)
            VALUES (99, 'Fremd', 'en', FALSE, 'active')"#,
        ] {
            sqlx::query(ddl).execute(&db.pool).await.unwrap();
        }
        let foreign = assign_streamer_to_guild(&db.pool, "55", "login", None, None, 99, &[1]).await;
        assert_eq!(foreign, Err(AssignError::Foreign));
        let ok = assign_streamer_to_guild(&db.pool, "55", "login", Some("8"), None, 99, &[99])
            .await
            .expect("assign");
        assert!(!ok.active);
        let active: bool =
            sqlx::query_scalar("SELECT active FROM streamer_guilds WHERE twitch_user_id = '55'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(!active);
        db.close().await;
    }
}
