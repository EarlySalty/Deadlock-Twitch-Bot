use axum::{
    extract::{Extension, Query},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use url::Url;

use crate::auth::{
    level::cookie_values,
    oauth_login::build_login_authorize_url,
    session::{session_lookup_key, DashboardAuthState, OAuthLoginState},
};
use crate::handlers::auth_login::OAuthLoginConfig;

pub const ROOT: &str = "/twitch/guild-onboarding";
pub const COOKIE: &str = "tb_guild_onboarding";
const CONTEXT_COOKIE: &str = "tb_guild_onboarding_oauth";
const HOME: i64 = 1_289_721_245_281_292_288;
const TTL: u64 = 1200;

#[derive(Clone)]
pub struct DiscordConfig {
    pub client_id: String,
    pub client_secret: String,
    pub http: reqwest::Client,
    pub api_base: String,
}

impl DiscordConfig {
    fn from_env() -> Option<Self> {
        Some(Self {
            client_id: std::env::var("DISCORD_HUB_CLIENT_ID").ok()?,
            client_secret: std::env::var("DISCORD_HUB_CLIENT_SECRET").ok()?,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok()?,
            api_base: "https://discord.com/api/v10".into(),
        })
    }

    async fn identity(&self, code: &str, redirect: &str) -> Result<(String, String, Vec<i64>), ()> {
        let response = self
            .http
            .post(format!("{}/oauth2/token", self.api_base))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect),
            ])
            .send()
            .await
            .map_err(|_| ())?
            .error_for_status()
            .map_err(|_| ())?;
        let token: Value = response.json().await.map_err(|_| ())?;
        let access = token
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(())?;
        let user: Value = self
            .http
            .get(format!("{}/users/@me", self.api_base))
            .bearer_auth(access)
            .send()
            .await
            .map_err(|_| ())?
            .error_for_status()
            .map_err(|_| ())?
            .json()
            .await
            .map_err(|_| ())?;
        let id = user
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_id(id))
            .ok_or(())?
            .to_string();
        let name = user
            .get("global_name")
            .and_then(Value::as_str)
            .or_else(|| user.get("username").and_then(Value::as_str))
            .unwrap_or("")
            .chars()
            .take(200)
            .collect();
        let mut ids = Vec::new();
        let mut after = String::new();
        loop {
            let request = self
                .http
                .get(format!("{}/users/@me/guilds", self.api_base))
                .bearer_auth(access)
                .query(&[("limit", "200")]);
            let request = if after.is_empty() {
                request
            } else {
                request.query(&[("after", after.as_str())])
            };
            let guilds: Vec<Value> = request
                .send()
                .await
                .map_err(|_| ())?
                .error_for_status()
                .map_err(|_| ())?
                .json()
                .await
                .map_err(|_| ())?;
            for guild in &guilds {
                let guild_id = guild
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|id| *id > 0)
                    .ok_or(())?;
                ids.push(guild_id);
            }
            if guilds.len() < 200 {
                break;
            }
            let next = ids.last().ok_or(())?.to_string();
            if next == after || ids.len() >= 2000 {
                return Err(());
            }
            after = next;
        }
        Ok((id, name, ids))
    }
}

pub fn router() -> Router {
    let router = Router::new()
        .route(
            ROOT,
            get(crate::handlers::spa::main_domain_spa_shell_handler),
        )
        .route("/twitch/guild-onboarding/twitch/login", get(twitch_login))
        .route(
            "/twitch/guild-onboarding/twitch/callback",
            get(twitch_callback),
        )
        .route("/twitch/guild-onboarding/discord/login", get(discord_login))
        .route(
            "/twitch/guild-onboarding/discord/callback",
            get(discord_callback),
        )
        .route("/twitch/guild-onboarding/session", get(status))
        .route("/twitch/guild-onboarding/select", post(select))
        .route("/twitch/guild-onboarding/logout", post(logout));
    match DiscordConfig::from_env() {
        Some(config) => router.layer(Extension(config)),
        None => router,
    }
}

fn origin(headers: &HeaderMap) -> Option<&'static str> {
    match headers.get(header::HOST)?.to_str().ok()? {
        "dead-lock.de" | "dead-lock.de:443" => Some("https://dead-lock.de"),
        "www.dead-lock.de" | "www.dead-lock.de:443" => Some("https://www.dead-lock.de"),
        "deutsche-deadlock-community.de" | "deutsche-deadlock-community.de:443" => {
            Some("https://deutsche-deadlock-community.de")
        }
        "www.deutsche-deadlock-community.de" | "www.deutsche-deadlock-community.de:443" => {
            Some("https://www.deutsche-deadlock-community.de")
        }
        _ => None,
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 20
        && id.bytes().all(|c| c.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|id| id > 0)
}

fn token(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = cookie_values(headers, name);
    if cookies.len() != 1 || !(32..=128).contains(&cookies[0].len()) {
        return None;
    }
    Some(cookies[0].to_string())
}

fn cookie(name: &str, token: &str, ttl: u64) -> String {
    format!("{name}={token}; Path={ROOT}; Max-Age={ttl}; HttpOnly; Secure; SameSite=Lax")
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn with_cookie(mut response: Response, name: &str, value: &str, ttl: u64) -> Response {
    if let Ok(value) = HeaderValue::from_str(&cookie(name, value, ttl)) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    no_store(response)
}

fn error(status: StatusCode, code: &str) -> Response {
    no_store((status, Json(json!({"error": code}))).into_response())
}

fn unavailable() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "onboarding_unavailable")
}
fn unauthorized() -> Response {
    error(StatusCode::UNAUTHORIZED, "onboarding_login_required")
}

#[derive(Default, Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn twitch_login(
    Extension(auth): Extension<DashboardAuthState>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
) -> Response {
    let (Some(base), Some(Extension(config))) = (origin(&headers), config) else {
        return unavailable();
    };
    let state = tb_crypto::random_urlsafe_token(32);
    let context = tb_crypto::random_urlsafe_token(32);
    let redirect = format!("{base}{ROOT}/twitch/callback");
    let saved = OAuthLoginState {
        next_path: ROOT.into(),
        redirect_uri: redirect.clone(),
        context_token: session_lookup_key(&context),
    };
    if auth.save_oauth_login_state(&state, &saved).await.is_err() {
        return unavailable();
    }
    with_cookie(
        Redirect::to(&build_login_authorize_url(
            &config.client_id,
            &redirect,
            &state,
        ))
        .into_response(),
        CONTEXT_COOKIE,
        &context,
        600,
    )
}

async fn twitch_callback(
    Extension(auth): Extension<DashboardAuthState>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Response {
    let (Some(base), Some(Extension(config)), Some(context), Some(state), Some(code)) = (
        origin(&headers),
        config,
        token(&headers, CONTEXT_COOKIE),
        query.state,
        query.code,
    ) else {
        return unauthorized();
    };
    if query.error.is_some() || state.len() > 128 || code.len() > 2048 {
        return unauthorized();
    }
    let saved = match auth.consume_oauth_login_state(&state).await {
        Ok(Some(saved)) => saved,
        _ => return unauthorized(),
    };
    if saved.next_path != ROOT
        || saved.redirect_uri != format!("{base}{ROOT}/twitch/callback")
        || !tb_crypto::constant_time_eq(
            saved.context_token.as_bytes(),
            session_lookup_key(&context).as_bytes(),
        )
    {
        return unauthorized();
    }
    let identity = match config
        .client
        .exchange_code_for_identity(&code, &saved.redirect_uri)
        .await
    {
        Ok(identity) if valid_id(&identity.twitch_user_id) && !identity.twitch_login.is_empty() => {
            identity
        }
        _ => return unauthorized(),
    };
    match tb_raid::signup_denylist::lookup_or_fail_closed(
        auth.pool(),
        Some(&identity.twitch_user_id),
        &identity.twitch_login,
        "guild_onboarding",
    )
    .await
    {
        Ok(None) => {}
        _ => return error(StatusCode::FORBIDDEN, "onboarding_blocked"),
    }
    let raw = tb_crypto::random_urlsafe_token(32);
    let result = sqlx::query("INSERT INTO guild_onboarding_sessions (session_hash, twitch_user_id, twitch_login) VALUES ($1, $2, $3)")
        .bind(session_lookup_key(&raw)).bind(identity.twitch_user_id).bind(identity.twitch_login).execute(auth.pool()).await;
    if result.is_err() {
        return unavailable();
    }
    let _ = sqlx::query("DELETE FROM guild_onboarding_sessions WHERE expires_at <= now()")
        .execute(auth.pool())
        .await;
    with_cookie(
        with_cookie(Redirect::to(ROOT).into_response(), COOKIE, &raw, TTL),
        CONTEXT_COOKIE,
        "",
        0,
    )
}

#[derive(sqlx::FromRow)]
struct Session {
    session_hash: String,
    twitch_user_id: String,
    twitch_login: String,
    discord_user_id: Option<String>,
    discord_name: Option<String>,
    guild_ids: Vec<i64>,
}

async fn load(pool: &PgPool, headers: &HeaderMap) -> Result<Session, Response> {
    let raw = token(headers, COOKIE).ok_or_else(unauthorized)?;
    sqlx::query_as::<_, Session>("SELECT session_hash, twitch_user_id, twitch_login, discord_user_id, discord_name, guild_ids FROM guild_onboarding_sessions WHERE session_hash = $1 AND expires_at > now()")
        .bind(session_lookup_key(&raw)).fetch_optional(pool).await.map_err(|_| unavailable())?.ok_or_else(unauthorized)
}

async fn discord_login(
    Extension(auth): Extension<DashboardAuthState>,
    config: Option<Extension<DiscordConfig>>,
    headers: HeaderMap,
) -> Response {
    let session = match load(auth.pool(), &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let (Some(base), Some(Extension(config))) = (origin(&headers), config) else {
        return unavailable();
    };
    let raw = tb_crypto::random_urlsafe_token(32);
    let redirect = format!("{base}{ROOT}/discord/callback");
    if sqlx::query("UPDATE guild_onboarding_sessions SET oauth_hash = $2, oauth_redirect_uri = $3, oauth_expires_at = now() + interval '5 minutes' WHERE session_hash = $1 AND expires_at > now()")
        .bind(&session.session_hash).bind(session_lookup_key(&raw)).bind(&redirect).execute(auth.pool()).await.is_err() { return unavailable(); }
    let Ok(mut url) = Url::parse("https://discord.com/oauth2/authorize") else {
        return unavailable();
    };
    url.query_pairs_mut()
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", "identify guilds")
        .append_pair("state", &raw);
    no_store(Redirect::to(url.as_str()).into_response())
}

async fn discord_callback(
    Extension(auth): Extension<DashboardAuthState>,
    config: Option<Extension<DiscordConfig>>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Response {
    let session = match load(auth.pool(), &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let (Some(base), Some(Extension(config)), Some(state), Some(code)) =
        (origin(&headers), config, query.state, query.code)
    else {
        return unauthorized();
    };
    if query.error.is_some() || state.len() > 128 || code.len() > 2048 {
        return unauthorized();
    }
    let redirect = format!("{base}{ROOT}/discord/callback");
    let consumed = sqlx::query("UPDATE guild_onboarding_sessions SET oauth_hash = NULL, oauth_expires_at = NULL WHERE session_hash = $1 AND oauth_hash = $2 AND oauth_redirect_uri = $3 AND oauth_expires_at > now() AND expires_at > now()")
        .bind(&session.session_hash).bind(session_lookup_key(&state)).bind(&redirect).execute(auth.pool()).await;
    if !matches!(consumed, Ok(result) if result.rows_affected() == 1) {
        return unauthorized();
    }
    let (id, name, guilds) = match config.identity(&code, &redirect).await {
        Ok(identity) => identity,
        Err(()) => return unavailable(),
    };
    let raw = tb_crypto::random_urlsafe_token(32);
    let updated = sqlx::query("UPDATE guild_onboarding_sessions SET session_hash = $2, discord_user_id = $3, discord_name = $4, guild_ids = $5, discord_verified_at = now() WHERE session_hash = $1 AND expires_at > now()")
        .bind(session.session_hash).bind(session_lookup_key(&raw)).bind(id).bind(name).bind(&guilds).execute(auth.pool()).await;
    if !matches!(updated, Ok(result) if result.rows_affected() == 1) {
        return unavailable();
    }
    with_cookie(Redirect::to(ROOT).into_response(), COOKIE, &raw, TTL)
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct Guild {
    guild_id: String,
    name: Option<String>,
    language: String,
    auto_partner_default: bool,
}

async fn status(Extension(auth): Extension<DashboardAuthState>, headers: HeaderMap) -> Response {
    let session = match load(auth.pool(), &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let guilds = sqlx::query_as::<_, Guild>("SELECT guild_id::text, name, language, auto_partner_default FROM discord_guilds WHERE guild_id = ANY($1) AND status = 'active' AND removed_at IS NULL ORDER BY name, guild_id")
        .bind(&session.guild_ids).fetch_all(auth.pool()).await;
    let memberships = sqlx::query_as::<_, (String, bool, bool)>("SELECT sg.guild_id::text, sg.active AND g.status = 'active' AND g.removed_at IS NULL AND (NOT sg.require_discord_link OR (sg.is_on_discord AND i.discord_user_id IS NOT NULL)), (sg.active AND sg.require_discord_link AND (NOT sg.is_on_discord OR i.discord_user_id IS NULL)) OR (NOT sg.active AND sg.source <> 'admin' AND COALESCE(sg.deactivated_by IN ('discord_leave','guild_removed','identity_change'), false)) FROM streamer_guilds sg JOIN discord_guilds g USING (guild_id) LEFT JOIN twitch_streamer_identities i ON i.twitch_user_id = sg.twitch_user_id WHERE sg.twitch_user_id = $1")
        .bind(&session.twitch_user_id).fetch_all(auth.pool()).await;
    match (guilds, memberships) {
        (Ok(guilds), Ok(memberships)) => no_store(Json(json!({"twitch_login": session.twitch_login, "discord_linked": session.discord_user_id.is_some(), "guilds": guilds,
            "memberships": memberships.into_iter().map(|(guild_id, active, can_rejoin)| json!({"guild_id":guild_id,"active":active,"can_rejoin":can_rejoin})).collect::<Vec<_>>()})).into_response()),
        _ => unavailable(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    guild_id: String,
}

fn same_origin(headers: &HeaderMap) -> bool {
    origin(headers).is_some_and(|expected| {
        headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            == Some(expected)
    })
}

async fn select(
    Extension(auth): Extension<DashboardAuthState>,
    headers: HeaderMap,
    Json(body): Json<Selection>,
) -> Response {
    if !same_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "invalid_csrf");
    }
    let session = match load(auth.pool(), &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(guild_id) = body.guild_id.parse::<i64>().ok().filter(|id| *id > 0) else {
        return error(StatusCode::BAD_REQUEST, "invalid_guild");
    };
    match assign(auth.pool(), &session.session_hash, guild_id).await {
        Ok(Some(active)) => no_store(Json(json!({"active":active})).into_response()),
        Ok(None) => error(StatusCode::FORBIDDEN, "guild_not_available"),
        Err(_) => unavailable(),
    }
}

async fn assign(pool: &PgPool, hash: &str, guild: i64) -> Result<Option<bool>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let session = sqlx::query_as::<_, Session>("SELECT session_hash, twitch_user_id, twitch_login, discord_user_id, discord_name, guild_ids FROM guild_onboarding_sessions WHERE session_hash = $1 AND expires_at > now() AND discord_verified_at > now() - interval '2 minutes' FOR UPDATE")
        .bind(hash).fetch_optional(&mut *tx).await?;
    let Some(session) = session
        .filter(|session| session.guild_ids.contains(&guild) && session.discord_user_id.is_some())
    else {
        return Ok(None);
    };
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&session.twitch_user_id)
        .execute(&mut *tx)
        .await?;
    let allowed: bool = sqlx::query_scalar("WITH current_partner AS (SELECT manual_partner_opt_out, admin_archived_at FROM twitch_partners WHERE twitch_user_id = $1 ORDER BY CASE COALESCE(status, '') WHEN 'active' THEN 0 WHEN 'archived' THEN 1 WHEN 'departnered' THEN 2 ELSE 3 END, COALESCE(departnered_at, admin_archived_at, partnered_at) DESC NULLS LAST LIMIT 1) SELECT NOT EXISTS (SELECT 1 FROM twitch_exclusions WHERE twitch_user_id = $1 AND reactivated_at IS NULL) AND NOT EXISTS (SELECT 1 FROM twitch_partner_signup_denylist WHERE twitch_user_id = $1 OR lower(twitch_login) = lower($2)) AND NOT EXISTS (SELECT 1 FROM twitch_partners WHERE twitch_user_id = $1 AND lower(btrim(coalesce(technical_pause_reason, ''))) IN ('blocked','bot_banned')) AND NOT EXISTS (SELECT 1 FROM current_partner WHERE coalesce(manual_partner_opt_out, 0) = 1 OR admin_archived_at IS NOT NULL)")
        .bind(&session.twitch_user_id).bind(&session.twitch_login).fetch_one(&mut *tx).await?;
    if !allowed {
        return Ok(None);
    }
    let enabled: bool = sqlx::query_scalar("SELECT enabled FROM guild_runtime WHERE singleton")
        .fetch_one(&mut *tx)
        .await?;
    if !enabled && guild != HOME {
        return Ok(None);
    }
    let automatic = sqlx::query_scalar::<_, bool>("SELECT auto_partner_default FROM discord_guilds WHERE guild_id = $1 AND status = 'active' AND removed_at IS NULL FOR UPDATE")
        .bind(guild).fetch_optional(&mut *tx).await?;
    let Some(automatic) = automatic else {
        return Ok(None);
    };
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 1))")
        .bind(&session.discord_user_id)
        .execute(&mut *tx)
        .await?;
    let conflict: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM twitch_streamer_identities WHERE (discord_user_id = $1 AND twitch_user_id <> $2) OR (twitch_user_id = $2 AND discord_user_id IS NOT NULL AND discord_user_id <> $1))")
        .bind(&session.discord_user_id).bind(&session.twitch_user_id).fetch_one(&mut *tx).await?;
    if conflict {
        return Ok(None);
    }
    sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord) VALUES ($1,$2,$3,$4,CASE WHEN $5 THEN 1 ELSE 0 END) ON CONFLICT (twitch_user_id) DO UPDATE SET twitch_login = EXCLUDED.twitch_login, discord_user_id = EXCLUDED.discord_user_id, discord_display_name = EXCLUDED.discord_display_name, is_on_discord = CASE WHEN $5 THEN 1 ELSE twitch_streamer_identities.is_on_discord END")
        .bind(&session.twitch_user_id).bind(&session.twitch_login).bind(&session.discord_user_id).bind(&session.discord_name).bind(guild == HOME).execute(&mut *tx).await?;
    let active: bool = sqlx::query_scalar("INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, deactivated_at, source, is_on_discord, require_discord_link) VALUES ($1,$2,$3,CASE WHEN $3 THEN NULL ELSE now() END,'onboarding',true,true) ON CONFLICT (twitch_user_id,guild_id) DO UPDATE SET is_on_discord = true, active = CASE WHEN streamer_guilds.deactivated_by = 'admin' OR streamer_guilds.source = 'admin' OR streamer_guilds.active THEN streamer_guilds.active ELSE $3 END, deactivated_at = CASE WHEN streamer_guilds.deactivated_by = 'admin' OR streamer_guilds.source = 'admin' OR streamer_guilds.active THEN streamer_guilds.deactivated_at WHEN $3 THEN NULL ELSE now() END RETURNING active")
        .bind(&session.twitch_user_id).bind(guild).bind(automatic).fetch_one(&mut *tx).await?;
    if active {
        if guild != HOME {
            sqlx::query("SET LOCAL tb.foreign_guild_admission = 'true'")
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE twitch_partners SET status = 'active', departnered_at = NULL, require_discord_link = CASE WHEN $2 THEN 1 ELSE require_discord_link END, technical_pause_reason = CASE WHEN lower(btrim(COALESCE(technical_pause_reason, ''))) LIKE 'token_error%' THEN NULL ELSE technical_pause_reason END WHERE id = (SELECT id FROM twitch_partners WHERE twitch_user_id = $1 ORDER BY CASE COALESCE(status, '') WHEN 'active' THEN 0 WHEN 'archived' THEN 1 WHEN 'departnered' THEN 2 ELSE 3 END, COALESCE(departnered_at, admin_archived_at, partnered_at) DESC NULLS LAST LIMIT 1 FOR UPDATE) AND (status <> 'active' OR $2) AND admin_archived_at IS NULL AND COALESCE(manual_partner_opt_out, 0) = 0 AND lower(btrim(COALESCE(technical_pause_reason, ''))) NOT IN ('blocked','bot_banned')")
            .bind(&session.twitch_user_id).bind(guild == HOME).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, added_by, require_discord_link) SELECT $1,$2,'active','guild_onboarding',CASE WHEN $3 THEN 1 ELSE 0 END WHERE NOT EXISTS (SELECT 1 FROM twitch_partners WHERE twitch_user_id = $1)")
            .bind(&session.twitch_user_id).bind(&session.twitch_login).bind(guild == HOME).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Some(active))
}

async fn logout(Extension(auth): Extension<DashboardAuthState>, headers: HeaderMap) -> Response {
    if !same_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "invalid_csrf");
    }
    if let Some(raw) = token(&headers, COOKIE) {
        if sqlx::query("DELETE FROM guild_onboarding_sessions WHERE session_hash = $1")
            .bind(session_lookup_key(&raw))
            .execute(auth.pool())
            .await
            .is_err()
        {
            return unavailable();
        }
    }
    with_cookie(StatusCode::NO_CONTENT.into_response(), COOKIE, "", 0)
}

#[cfg(test)]
pub(crate) mod tests;
