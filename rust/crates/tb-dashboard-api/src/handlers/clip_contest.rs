//! Monatlicher öffentlicher Clip-Wettbewerb unter /clips.
//!
//! Identität kommt ausschließlich aus bestehenden Wegen:
//! Discord über den lokalen dl-web OAuth-Broker.
//! Twitch über die bestehende Dashboard-Session.

use std::{collections::BTreeMap, time::Duration as StdDuration};

use axum::{
    extract::{Extension, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, PgPool, Postgres, Row, Transaction};
use tb_social_media::{clip::model::ClipRecord, ClipRepository};

use crate::{
    auth::{
        csrf::is_allowed_origin,
        level::{cookie_values, DashboardAuthLevel},
        oauth_login::TwitchIdentity,
        session::{session_lookup_key, DashboardAuthState},
    },
    uplink_config,
};

const SESSION_COOKIE: &str = "ddc_clip_contest_session";
const OAUTH_COOKIE: &str = "ddc_clip_contest_oauth";
const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
const OAUTH_TTL_SECS: i64 = 10 * 60;
const MAX_SUBMISSIONS_PER_MONTH: i64 = 3;
const MAX_VOTES_PER_MONTH: i64 = 5;
const MAX_CLIP_AGE_DAYS: i64 = 60;
const COMMUNITY_GUILD_ID: i64 = 1_289_721_245_281_292_288;
const BROKER_INITIATE_PATH: &str = "/internal/v1/discord/initiate";
const BROKER_CONSUME_PATH: &str = "/internal/v1/discord/consume-result";
const DEADLOCK_GAME_ID_FALLBACK: &str = "2132205352";

/// Existing central read pool, shared with the partner challenge sources.
#[derive(Clone)]
pub struct ContestCentralPool(pub PgPool);

#[derive(Debug, Clone)]
struct DiscordIdentity {
    user_id: String,
}

#[derive(Debug, Clone)]
struct SubmitIdentity {
    provider: &'static str,
    user_id: String,
    person_key: String,
    aliases: Vec<String>,
    display_name: String,
}

#[derive(Debug, Clone)]
struct DiscordEligibility {
    account_age_ok: bool,
    member_age_ok: bool,
    present: bool,
    joined_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct HelixClip {
    clip_id: String,
    url: String,
    title: String,
    thumbnail_url: Option<String>,
    broadcaster_id: String,
    broadcaster_name: String,
    creator_id: Option<String>,
    game_id: String,
    created_at: DateTime<Utc>,
    duration_seconds: f64,
    view_count: i64,
    vod_id: Option<String>,
    vod_offset_s: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Submission,
    Voting,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Submission => "submission",
            Self::Voting => "voting",
        }
    }
}

#[derive(Debug, Clone)]
struct ContestClock {
    month: NaiveDate,
    phase: Phase,
    phase_ends_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SubmitBody {
    clip_url: String,
}

#[derive(Debug, Deserialize)]
pub struct VoteBody {
    submission_id: i64,
}

#[derive(Debug, Deserialize, Default)]
pub struct HideBody {
    submission_id: i64,
    #[serde(default)]
    reason: String,
    #[serde(default = "default_true")]
    hidden: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct DiscordCallbackQuery {
    #[serde(default)]
    state_id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn default_true() -> bool {
    true
}

fn json_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "error": code, "message": message })),
    )
        .into_response()
}

fn no_store_json(value: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

fn contest_clock(now: DateTime<Utc>) -> ContestClock {
    let local = now.with_timezone(&Berlin);
    let month = NaiveDate::from_ymd_opt(local.year(), local.month(), 1).expect("gültiger Monat");
    let phase = if local.day() <= 21 {
        Phase::Submission
    } else {
        Phase::Voting
    };
    let next_boundary = if phase == Phase::Submission {
        Berlin
            .with_ymd_and_hms(local.year(), local.month(), 22, 0, 0, 0)
            .single()
            .expect("gültige Berliner Zeit")
    } else {
        let next = next_month(month);
        Berlin
            .with_ymd_and_hms(next.year(), next.month(), 1, 0, 0, 0)
            .single()
            .expect("gültige Berliner Zeit")
    };
    ContestClock {
        month,
        phase,
        phase_ends_at: next_boundary.with_timezone(&Utc),
    }
}

fn next_month(month: NaiveDate) -> NaiveDate {
    if month.month() == 12 {
        NaiveDate::from_ymd_opt(month.year() + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(month.year(), month.month() + 1, 1).unwrap()
    }
}

fn month_label(month: NaiveDate) -> String {
    const NAMES: [&str; 12] = [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ];
    format!("{} {}", NAMES[(month.month() - 1) as usize], month.year())
}

fn secure_request(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

fn cookie(name: &str, value: &str, path: &str, max_age: i64, secure: bool) -> String {
    let mut out = format!("{name}={value}; Path={path}; Max-Age={max_age}; HttpOnly; SameSite=Lax");
    if secure {
        out.push_str("; Secure");
    }
    out
}

fn clear_cookie(name: &str, path: &str, secure: bool) -> String {
    cookie(name, "", path, 0, secure)
}

async fn contest_session(
    state: Option<&DashboardAuthState>,
    headers: &HeaderMap,
) -> Option<(String, String, String)> {
    let values = cookie_values(headers, SESSION_COOKIE);
    if values.len() != 1 {
        return None;
    }
    state?
        .load_clip_contest_session(values[0])
        .await
        .ok()
        .flatten()
}

async fn discord_session(
    state: Option<&DashboardAuthState>,
    headers: &HeaderMap,
) -> Option<DiscordIdentity> {
    let (provider, user_id, _) = contest_session(state, headers).await?;
    (provider == "discord").then_some(DiscordIdentity { user_id })
}

/// Only verified platform IDs participate in identity resolution. Login names
/// can change ownership and are never identity evidence.
async fn identity_aliases(
    pool: &PgPool,
    provider: &str,
    user_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let mut aliases = vec![format!("{provider}:{user_id}")];
    let discord_ids: Vec<String> = if provider == "discord" {
        vec![user_id.to_string()]
    } else {
        sqlx::query_scalar(
            "SELECT DISTINCT discord_user_id FROM twitch_streamer_identities
            WHERE twitch_user_id = $1 AND NULLIF(TRIM(discord_user_id), '') IS NOT NULL",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?
    };
    for id in discord_ids {
        aliases.push(format!("discord:{id}"));
        let linked: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT twitch_user_id FROM twitch_streamer_identities
            WHERE discord_user_id = $1 AND NULLIF(TRIM(twitch_user_id), '') IS NOT NULL",
        )
        .bind(id)
        .fetch_all(pool)
        .await?;
        aliases.extend(linked.into_iter().map(|id| format!("twitch:{id}")));
    }
    aliases.sort();
    aliases.dedup();
    Ok(aliases)
}

async fn submit_identity(
    pool: &PgPool,
    headers: &HeaderMap,
    auth: &DashboardAuthLevel,
    state: Option<&DashboardAuthState>,
) -> Option<SubmitIdentity> {
    if cookie_values(headers, SESSION_COOKIE).contains(&"signed-out") {
        return None;
    }
    let (provider, user_id, display_name) = match contest_session(state, headers).await {
        Some((provider, id, name)) => (
            if provider == "discord" {
                "discord"
            } else {
                "twitch"
            },
            id,
            name,
        ),
        None => match auth {
            DashboardAuthLevel::Partner {
                twitch_user_id,
                twitch_login,
                display_name,
            } if !twitch_user_id.is_empty() => (
                "twitch",
                twitch_user_id.clone(),
                if display_name.is_empty() {
                    twitch_login.clone()
                } else {
                    display_name.clone()
                },
            ),
            DashboardAuthLevel::Admin { actor: Some(actor) }
                if !actor.twitch_user_id.is_empty() =>
            {
                (
                    "twitch",
                    actor.twitch_user_id.clone(),
                    actor.twitch_login.clone(),
                )
            }
            _ => return None,
        },
    };
    let aliases = identity_aliases(pool, provider, &user_id).await.ok()?;
    let person_key = aliases.first()?.clone();
    Some(SubmitIdentity {
        provider,
        user_id,
        person_key,
        display_name,
        aliases,
    })
}

pub async fn complete_twitch_login(
    state: &DashboardAuthState,
    identity: &TwitchIdentity,
    secure: bool,
) -> Response {
    match state
        .create_clip_contest_session("twitch", &identity.twitch_user_id, &identity.display_name)
        .await
    {
        Ok(session) => session_redirect(&session.session_id, secure),
        Err(_) => json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "login_unavailable",
            "Die Anmeldung konnte nicht gespeichert werden.",
        ),
    }
}

fn session_redirect(token: &str, secure: bool) -> Response {
    let mut response = Redirect::to("/clips").into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(value) = HeaderValue::from_str(&cookie(
        SESSION_COOKIE,
        token,
        "/clips",
        SESSION_TTL_SECS,
        secure,
    )) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

fn clip_id_from_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some_and(|port| port != 443)
    {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    let candidate = match (host.as_str(), segments.as_slice()) {
        ("clips.twitch.tv", [clip]) => *clip,
        ("twitch.tv" | "www.twitch.tv" | "m.twitch.tv", [_, "clip", clip]) => *clip,
        _ => return None,
    };
    let candidate = candidate.trim();
    if candidate.len() < 3
        || candidate.len() > 120
        || !candidate
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return None;
    }
    Some(candidate.to_string())
}

fn unavailable() -> Response {
    json_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "contest_unavailable",
        "Der Clip-Wettbewerb ist gerade nicht erreichbar. Bitte erneut versuchen.",
    )
}

fn valid_write_origin(headers: &HeaderMap) -> bool {
    (headers.contains_key(header::ORIGIN) || headers.contains_key(header::REFERER))
        && headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) != Some("cross-site")
        && is_allowed_origin(headers)
}

async fn fetch_helix_clip(clip_id: &str) -> Result<HelixClip, Response> {
    tokio::time::timeout(StdDuration::from_secs(8), fetch_helix_clip_inner(clip_id))
        .await
        .map_err(|_| unavailable())?
}

async fn fetch_helix_clip_inner(clip_id: &str) -> Result<HelixClip, Response> {
    let client = uplink_config::runtime()
        .ok()
        .and_then(|runtime| runtime.helix.as_ref())
        .ok_or_else(unavailable)?;
    let req = client.get("/clips").await.map_err(|_| {
        json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "twitch_unavailable",
            "Die Twitch-Prüfung ist gerade nicht verfügbar.",
        )
    })?;
    let response = req.query(&[("id", clip_id)]).send().await.map_err(|_| {
        json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "twitch_unavailable",
            "Die Twitch-Prüfung ist gerade nicht verfügbar.",
        )
    })?;
    if !response.status().is_success() {
        return Err(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "twitch_unavailable",
            "Die Twitch-Prüfung ist gerade nicht verfügbar.",
        ));
    }
    let value: Value = response.json().await.map_err(|_| {
        json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "twitch_unavailable",
            "Die Twitch-Antwort konnte nicht geprüft werden.",
        )
    })?;
    let Some(clip) = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|clips| {
            clips
                .iter()
                .find(|clip| clip.get("id").and_then(Value::as_str) == Some(clip_id))
        })
    else {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "clip_not_found",
            "Diesen Twitch-Clip gibt es nicht oder er ist nicht mehr verfügbar.",
        ));
    };
    let text = |name: &str| {
        clip.get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let created_raw = text("created_at").ok_or_else(|| {
        json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "clip_invalid",
            "Twitch hat keinen gültigen Clip-Zeitpunkt geliefert.",
        )
    })?;
    let created_at = DateTime::parse_from_rfc3339(&created_raw)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|_| {
            json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "clip_invalid",
                "Twitch hat keinen gültigen Clip-Zeitpunkt geliefert.",
            )
        })?;
    Ok(HelixClip {
        clip_id: text("id").unwrap_or_else(|| clip_id.to_string()),
        url: format!("https://clips.twitch.tv/{clip_id}"),
        title: text("title").unwrap_or_else(|| "Deadlock-Clip".to_string()),
        thumbnail_url: text("thumbnail_url"),
        broadcaster_id: text("broadcaster_id").ok_or_else(|| {
            json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "clip_invalid",
                "Der Kanal des Clips konnte nicht geprüft werden.",
            )
        })?,
        broadcaster_name: text("broadcaster_name").unwrap_or_default(),
        creator_id: text("creator_id"),
        game_id: text("game_id").unwrap_or_default(),
        created_at,
        duration_seconds: clip.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
        view_count: clip.get("view_count").and_then(Value::as_i64).unwrap_or(0),
        vod_id: text("video_id"),
        vod_offset_s: clip
            .get("vod_offset")
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok()),
    })
}

async fn deadlock_game_id(pool: &PgPool) -> Result<String, Response> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT twitch_game_id
           FROM social_media_category
          WHERE category_key = 'deadlock'
            AND NULLIF(TRIM(twitch_game_id), '') IS NOT NULL
          LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| unavailable())?
    .unwrap_or_else(|| DEADLOCK_GAME_ID_FALLBACK.to_string()))
}

fn discord_account_created_at(discord_id: &str) -> Option<DateTime<Utc>> {
    const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
    let id = discord_id.parse::<u64>().ok()?;
    let millis = (id >> 22).checked_add(DISCORD_EPOCH_MS)?;
    DateTime::<Utc>::from_timestamp_millis(i64::try_from(millis).ok()?)
}

fn membership_eligibility_at(
    discord_id: &str,
    now: DateTime<Utc>,
    joined_at: Option<DateTime<Utc>>,
    present: bool,
    synced_at: Option<DateTime<Utc>>,
    latest: Option<(String, DateTime<Utc>)>,
) -> Result<DiscordEligibility, Response> {
    // The directory is refreshed daily. Never trust an indefinitely stale census.
    if synced_at.is_none_or(|at| at < now - Duration::hours(26) || at > now) {
        return Err(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "community_check_unavailable",
            "Die Discord-Mitgliedschaft kann gerade nicht zuverlässig geprüft werden.",
        ));
    }
    let mut joined_at = joined_at;
    let mut present = present;
    // Live join/leave events take precedence over the most recent full census.
    if let Some((kind, at)) = latest.filter(|(_, at)| synced_at.is_none_or(|synced| *at > synced)) {
        if at > now {
            return Err(unavailable());
        }
        present = kind == "join";
        if present {
            joined_at = Some(at);
        }
    }
    Ok(DiscordEligibility {
        account_age_ok: discord_account_created_at(discord_id)
            .is_some_and(|created| created <= now - Duration::days(30)),
        member_age_ok: present && joined_at.is_some_and(|joined| joined <= now - Duration::days(7)),
        present,
        joined_at,
    })
}

async fn discord_eligibility(
    pool: &PgPool,
    discord_id: &str,
) -> Result<DiscordEligibility, Response> {
    let id = discord_id.parse::<i64>().map_err(|_| unavailable())?;
    let row = sqlx::query("SELECT d.joined_at, d.present, d.synced_at, d.is_bot,
        e.event_type, e.occurred_at
        FROM (SELECT 1) AS anchor
        LEFT JOIN activity.guild_member_directory d ON d.guild_id=$1 AND d.user_id=$2
        LEFT JOIN LATERAL (SELECT event_type, occurred_at FROM activity.member_events
            WHERE guild_id=$1 AND user_id=$2 AND event_type IN ('join','leave') AND occurred_at IS NOT NULL
            ORDER BY occurred_at DESC, id DESC LIMIT 1) e ON TRUE")
        .bind(COMMUNITY_GUILD_ID).bind(id).fetch_one(pool).await.map_err(|_| unavailable())?;
    let kind: Option<String> = row.try_get("event_type").map_err(|_| unavailable())?;
    let occurred: Option<DateTime<Utc>> = row.try_get("occurred_at").map_err(|_| unavailable())?;
    let present: Option<bool> = row.try_get("present").map_err(|_| unavailable())?;
    let bot: Option<bool> = row.try_get("is_bot").map_err(|_| unavailable())?;
    if bot == Some(true) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "bot_account",
            "Bot-Konten können nicht abstimmen.",
        ));
    }
    membership_eligibility_at(
        discord_id,
        Utc::now(),
        row.try_get("joined_at").map_err(|_| unavailable())?,
        present.unwrap_or(false),
        row.try_get("synced_at").map_err(|_| unavailable())?,
        kind.zip(occurred),
    )
}

mod contest;
pub use contest::{
    admin_submissions_handler, archive_handler, current_handler, finalize_due_months, hide_handler,
    session_handler, submit_handler, vote_handler,
};

fn broker_token() -> Option<String> {
    crate::handlers::discord_link::broker_token_from(uplink_config::platform_value)
}

async fn broker_post(path: &str, token: &str, payload: &Value) -> Option<Value> {
    crate::handlers::discord_link::broker_post(path, token, payload).await
}

pub async fn discord_login_handler(headers: HeaderMap) -> Response {
    let Some(token) = broker_token() else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "discord_login_unavailable",
            "Der Discord-Login ist gerade nicht verfügbar.",
        );
    };
    let payload = json!({
        "scope": "identify",
        "redirect_after": "https://deutsche-deadlock-community.de/clips/auth/discord/callback",
        "requesting_service": "twitch-clip-contest",
        "metadata": {
            "clip_contest": true,
            "guild_id": COMMUNITY_GUILD_ID.to_string(),
        }
    });
    let Some(data) = broker_post(BROKER_INITIATE_PATH, &token, &payload).await else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "discord_login_unavailable",
            "Der Discord-Login ist gerade nicht verfügbar.",
        );
    };
    let authorize_url = data
        .get("authorize_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let state_id = data
        .get("state_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if authorize_url.is_empty() || state_id.is_empty() {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "discord_login_unavailable",
            "Der Discord-Login ist gerade nicht verfügbar.",
        );
    }
    let Ok(authorize) = url::Url::parse(authorize_url) else {
        return unavailable();
    };
    if authorize.scheme() != "https"
        || !matches!(authorize.host_str(), Some("discord.com" | "discordapp.com"))
    {
        return unavailable();
    }
    let mut response = Redirect::to(authorize_url).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(value) = HeaderValue::from_str(&cookie(
        OAUTH_COOKIE,
        state_id,
        "/clips/auth/discord/callback",
        OAUTH_TTL_SECS,
        secure_request(&headers),
    )) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

pub async fn discord_callback_handler(
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    Query(query): Query<DiscordCallbackQuery>,
) -> Response {
    let secure = secure_request(&headers);
    let fail = |target: &'static str| {
        let mut response = Redirect::to(target).into_response();
        if let Ok(value) = HeaderValue::from_str(&clear_cookie(
            OAUTH_COOKIE,
            "/clips/auth/discord/callback",
            secure,
        )) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
        response
    };
    if query.error.as_deref().is_some_and(|v| !v.trim().is_empty()) {
        return fail("/clips?login=abgebrochen");
    }
    let Some(state_id) = query
        .state_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return fail("/clips?login=fehler");
    };
    let cookies = cookie_values(&headers, OAUTH_COOKIE);
    if cookies.len() != 1 || state_id.len() > 200 {
        return fail("/clips?login=fehler");
    }
    let oauth_cookie = cookies[0];
    if !tb_crypto::constant_time_eq(oauth_cookie.as_bytes(), state_id.as_bytes()) {
        return fail("/clips?login=fehler");
    }
    let Some(token) = broker_token() else {
        return fail("/clips?login=fehler");
    };
    let Some(data) = broker_post(
        BROKER_CONSUME_PATH,
        &token,
        &json!({ "state_id": state_id }),
    )
    .await
    else {
        return fail("/clips?login=fehler");
    };
    if data
        .pointer("/service_metadata/clip_contest")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return fail("/clips?login=fehler");
    }
    let discord_id = data
        .get("discord_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let discord_name = data
        .get("discord_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if discord_id.is_empty()
        || !discord_id.chars().all(|c| c.is_ascii_digit())
        || discord_name.is_empty()
    {
        return fail("/clips?login=fehler");
    }
    let Some(Extension(state)) = state else {
        return fail("/clips?login=fehler");
    };
    let session = match state
        .create_clip_contest_session("discord", discord_id, discord_name)
        .await
    {
        Ok(session) => session,
        Err(_) => return fail("/clips?login=fehler"),
    };
    let raw = session.session_id;
    let mut response = Redirect::to("/clips?login=discord").into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    for value in [
        cookie(SESSION_COOKIE, &raw, "/clips", SESSION_TTL_SECS, secure),
        clear_cookie(OAUTH_COOKIE, "/clips/auth/discord/callback", secure),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

pub async fn logout_handler(
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
) -> Response {
    if !valid_write_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage wurde abgelehnt.",
        );
    }
    if let Some(raw) = cookie_values(&headers, SESSION_COOKIE).into_iter().next() {
        let Some(state) = state else {
            return unavailable();
        };
        let result = sqlx::query("DELETE FROM dashboard_sessions WHERE session_type = 'clip_contest' AND session_id = $1")
            .bind(session_lookup_key(raw))
            .execute(state.pool())
            .await;
        if result.is_err() {
            return unavailable();
        }
    }
    let mut response = no_store_json(json!({ "ok": true }));
    if let Ok(value) = HeaderValue::from_str(&cookie(
        SESSION_COOKIE,
        "signed-out",
        "/clips",
        SESSION_TTL_SECS,
        secure_request(&headers),
    )) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phasen_sind_berliner_kalendertage() {
        let day21 = DateTime::parse_from_rfc3339("2026-09-21T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let day22 = DateTime::parse_from_rfc3339("2026-09-22T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(contest_clock(day21).phase, Phase::Submission);
        assert_eq!(contest_clock(day22).phase, Phase::Voting);
    }

    #[test]
    fn twitch_clip_urls_werden_strikt_aufgeloest() {
        assert_eq!(
            clip_id_from_url("https://clips.twitch.tv/FancyClip-123").as_deref(),
            Some("FancyClip-123")
        );
        assert_eq!(
            clip_id_from_url("https://www.twitch.tv/foo/clip/FancyClip123?x=1").as_deref(),
            Some("FancyClip123")
        );
        assert!(clip_id_from_url("http://clips.twitch.tv/FancyClip123").is_none());
        assert!(clip_id_from_url("https://evil.example/FancyClip123").is_none());
    }

    #[test]
    fn discord_snowflake_liefert_account_zeitpunkt() {
        let created = discord_account_created_at("175928847299117063").unwrap();
        assert!((2016..=2017).contains(&created.year()));
    }

    #[test]
    fn dezember_wechselt_ins_naechste_jahr() {
        assert_eq!(
            next_month(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }
}
