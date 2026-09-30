//! Öffentlicher monatlicher Clip-Wettbewerb.
//!
//! Identität:
//! - Discord läuft ausschließlich über den bestehenden zentralen dl-web-Broker.
//! - Twitch verwendet die bereits vorhandene Dashboard-Partner-Session.
//! Stimmen und Limits liegen vollständig serverseitig.

use crate::{
    auth::{
        level::DashboardAuthLevel,
        oauth_login::TwitchIdentity as OAuthTwitchIdentity,
        security::{rate_limit_middleware, RateLimitLayerConfig, RateLimiter},
        session::{build_session_cookie, clear_session_cookie, SameSite},
    },
    handlers::{discord_link, website},
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, LocalResult, NaiveDate, TimeZone, Utc,
};
use chrono_tz::Europe::Berlin;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions, PgPool, Row,
};
use std::{str::FromStr, sync::Arc, time::Duration};
use tb_social_media::clip::{helix::HelixClipSource, model::ClipRecord};
use tb_transport_twitch::HelixClient;
use tokio::sync::OnceCell;
use url::Url;

const DISCORD_SESSION_COOKIE: &str = "ddc_clip_session";
const TWITCH_SESSION_COOKIE: &str = "ddc_clip_twitch_session";
const CONTEST_SESSION_TTL_SECS: u64 = 14 * 24 * 60 * 60;
const BROKER_BASE_URL: &str = "http://127.0.0.1:8766";
const BROKER_INITIATE_PATH: &str = "/internal/v1/discord/initiate";
const BROKER_CONSUME_PATH: &str = "/internal/v1/discord/consume-result";
const BROKER_TOKEN_HEADER: &str = "X-Internal-Token";
const DISCORD_CALLBACK_URL: &str =
    "https://deutsche-deadlock-community.de/clips/auth/discord/complete";
const DEADLOCK_GAME_ID_FALLBACK: &str = "2132205352";
const SUBMISSION_LIMIT: i64 = 3;
const VOTE_LIMIT: i64 = 5;
const MIN_ACCOUNT_AGE_DAYS: i64 = 30;
const MIN_MEMBER_AGE_DAYS: i64 = 7;
const CLIP_MAX_AGE_DAYS: i64 = 60;

static COMMUNITY_POOL: OnceCell<PgPool> = OnceCell::const_new();

#[derive(Clone)]
pub struct ClipContestState {
    pool: PgPool,
    helix: Option<Arc<HelixClient>>,
}

#[derive(Debug, Clone)]
struct DiscordIdentity {
    id: String,
    name: String,
}

#[derive(Debug, Clone)]
struct TwitchIdentity {
    id: String,
    login: String,
    name: String,
}

#[derive(Debug, Clone)]
struct SubmitIdentity {
    kind: &'static str,
    id: String,
    display_name: String,
    discord_id: Option<String>,
    twitch_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContestPhase {
    Submission,
    Voting,
}

impl ContestPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Submission => "submission",
            Self::Voting => "voting",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Submission => "Einreichungsphase",
            Self::Voting => "Abstimmungsphase",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SubmitBody {
    clip_url: String,
}

#[derive(Debug, Deserialize)]
pub struct DiscordCompleteQuery {
    state_id: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VisibilityBody {
    hidden: bool,
}

fn api_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
        ],
        Json(json!({"error": code, "message": message})),
    )
        .into_response()
}

fn json_no_store(value: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

fn current_month(now: DateTime<chrono_tz::Tz>) -> NaiveDate {
    NaiveDate::from_ymd_opt(now.year(), now.month(), 1).expect("gültiger Kalendermonat")
}

fn next_month(month: NaiveDate) -> NaiveDate {
    if month.month() == 12 {
        NaiveDate::from_ymd_opt(month.year() + 1, 1, 1).expect("gültiger Folgemonat")
    } else {
        NaiveDate::from_ymd_opt(month.year(), month.month() + 1, 1).expect("gültiger Folgemonat")
    }
}

fn phase(now: DateTime<chrono_tz::Tz>) -> ContestPhase {
    if now.day() <= 21 {
        ContestPhase::Submission
    } else {
        ContestPhase::Voting
    }
}

fn month_key(month: NaiveDate) -> String {
    month.format("%Y-%m").to_string()
}

fn local_start(date: NaiveDate) -> DateTime<chrono_tz::Tz> {
    match Berlin.with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(first, _) => first,
        LocalResult::None => Utc::now().with_timezone(&Berlin),
    }
}

fn submission_end(month: NaiveDate) -> DateTime<chrono_tz::Tz> {
    match Berlin.with_ymd_and_hms(month.year(), month.month(), 21, 23, 59, 59) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(first, _) => first,
        LocalResult::None => local_start(month),
    }
}

fn voting_end(month: NaiveDate) -> DateTime<chrono_tz::Tz> {
    local_start(next_month(month))
}

fn session_lookup_key(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    crate::auth::level::cookie_values(headers, name)
        .into_iter()
        .next()
        .map(str::to_string)
}

fn request_is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("https"))
}

fn same_origin(headers: &HeaderMap) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    let source = headers
        .get(header::ORIGIN)
        .or_else(|| headers.get(header::REFERER))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Url::parse(value).ok());

    source
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|origin_host| !host.is_empty() && origin_host == host)
}

fn broker_token() -> Option<String> {
    discord_link::broker_token_from(crate::uplink_config::platform_value)
}

async fn broker_post(path: &str, token: &str, payload: &Value) -> Option<Value> {
    let url = format!("{}{}", BROKER_BASE_URL.trim_end_matches('/'), path);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?;
    let response = client
        .post(url)
        .header(BROKER_TOKEN_HEADER, token)
        .json(payload)
        .send()
        .await
        .map_err(|error| tracing::warn!(%error, path, "Clip-Wettbewerb: Discord-Broker nicht erreichbar"))
        .ok()?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), path, "Clip-Wettbewerb: Discord-Broker hat abgelehnt");
        return None;
    }
    response.json::<Value>().await.ok()
}

async fn contest_discord_identity(
    pool: &PgPool,
    headers: &HeaderMap,
) -> Result<Option<DiscordIdentity>, sqlx::Error> {
    let Some(raw) = cookie_value(headers, DISCORD_SESSION_COOKIE) else {
        return Ok(None);
    };
    let lookup = session_lookup_key(&raw);
    let row = sqlx::query(
        r#"
        SELECT discord_id, discord_name
          FROM twitch_clip_contest_discord_sessions
         WHERE session_lookup_key = $1
           AND expires_at > now()
         LIMIT 1
        "#,
    )
    .bind(lookup)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| DiscordIdentity {
        id: row.try_get::<String, _>("discord_id").unwrap_or_default(),
        name: row.try_get::<String, _>("discord_name").unwrap_or_default(),
    }))
}

async fn contest_twitch_identity(
    pool: &PgPool,
    headers: &HeaderMap,
) -> Result<Option<TwitchIdentity>, sqlx::Error> {
    let Some(raw) = cookie_value(headers, TWITCH_SESSION_COOKIE) else {
        return Ok(None);
    };
    let lookup = session_lookup_key(&raw);
    let row = sqlx::query(
        r#"
        SELECT twitch_user_id, twitch_login, display_name
          FROM twitch_clip_contest_twitch_sessions
         WHERE session_lookup_key = $1
           AND expires_at > now()
         LIMIT 1
        "#,
    )
    .bind(lookup)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| TwitchIdentity {
        id: row
            .try_get::<String, _>("twitch_user_id")
            .unwrap_or_default(),
        login: row.try_get::<String, _>("twitch_login").unwrap_or_default(),
        name: row.try_get::<String, _>("display_name").unwrap_or_default(),
    }))
}

pub(crate) async fn complete_twitch_login(
    pool: &PgPool,
    identity: &OAuthTwitchIdentity,
    cookie_secure: bool,
) -> Response {
    let raw = tb_crypto::random_urlsafe_token(32);
    let lookup = session_lookup_key(&raw);
    let display = if identity.display_name.trim().is_empty() {
        identity.twitch_login.trim()
    } else {
        identity.display_name.trim()
    };
    let expires = Utc::now() + ChronoDuration::seconds(CONTEST_SESSION_TTL_SECS as i64);

    if let Err(error) = sqlx::query(
        r#"
        INSERT INTO twitch_clip_contest_twitch_sessions
            (session_lookup_key, twitch_user_id, twitch_login, display_name, expires_at)
        VALUES ($1,$2,$3,$4,$5)
        "#,
    )
    .bind(lookup)
    .bind(identity.twitch_user_id.trim())
    .bind(identity.twitch_login.trim().to_ascii_lowercase())
    .bind(display)
    .bind(expires)
    .execute(pool)
    .await
    {
        tracing::error!(%error, "Clip-Wettbewerb: Twitch-Session konnte nicht gespeichert werden");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_login_unavailable",
            "Der Twitch-Login für den Clip-Wettbewerb ist gerade nicht verfügbar.",
        );
    }

    let cookie = build_session_cookie(
        TWITCH_SESSION_COOKIE,
        &raw,
        cookie_secure,
        SameSite::Lax,
        CONTEST_SESSION_TTL_SECS,
    );
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, "/clips"),
            (header::SET_COOKIE, cookie.as_str()),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
        .into_response()
}

async fn linked_discord_for_twitch(
    pool: &PgPool,
    twitch_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT discord_user_id::text
          FROM twitch_streamer_identities
         WHERE twitch_user_id = $1
           AND discord_user_id IS NOT NULL
         ORDER BY updated_at DESC
         LIMIT 1
        "#,
    )
    .bind(twitch_id)
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
}

async fn linked_twitch_for_discord(
    pool: &PgPool,
    discord_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT twitch_user_id
          FROM twitch_streamer_identities
         WHERE discord_user_id::text = $1
           AND COALESCE(twitch_user_id, '') <> ''
         ORDER BY updated_at DESC
         LIMIT 1
        "#,
    )
    .bind(discord_id)
    .fetch_optional(pool)
    .await
}

async fn submit_identity(
    pool: &PgPool,
    headers: &HeaderMap,
    auth: &DashboardAuthLevel,
) -> Result<Option<SubmitIdentity>, sqlx::Error> {
    if let Some(discord) = contest_discord_identity(pool, headers).await? {
        let twitch_id = linked_twitch_for_discord(pool, &discord.id).await?;
        return Ok(Some(SubmitIdentity {
            kind: "discord",
            id: discord.id.clone(),
            display_name: discord.name,
            discord_id: Some(discord.id),
            twitch_id,
        }));
    }

    if let Some(twitch) = contest_twitch_identity(pool, headers).await? {
        if !twitch.id.trim().is_empty() {
            return Ok(Some(SubmitIdentity {
                kind: "twitch",
                id: twitch.id.clone(),
                display_name: if twitch.name.trim().is_empty() {
                    twitch.login.clone()
                } else {
                    twitch.name
                },
                discord_id: linked_discord_for_twitch(pool, &twitch.id).await?,
                twitch_id: Some(twitch.id),
            }));
        }
    }

    if let DashboardAuthLevel::Partner {
        twitch_user_id,
        twitch_login,
        display_name,
    } = auth
    {
        let twitch_id = twitch_user_id.trim();
        if !twitch_id.is_empty() {
            return Ok(Some(SubmitIdentity {
                kind: "twitch",
                id: twitch_id.to_string(),
                display_name: if display_name.trim().is_empty() {
                    twitch_login.clone()
                } else {
                    display_name.clone()
                },
                discord_id: linked_discord_for_twitch(pool, twitch_id).await?,
                twitch_id: Some(twitch_id.to_string()),
            }));
        }
    }
    Ok(None)
}

async fn community_pool() -> Result<PgPool, Response> {
    COMMUNITY_POOL
        .get_or_try_init(|| async {
            let dsn = std::env::var("DEADLOCK_COMMUNITY_READONLY_DSN")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    std::env::var("DEADLOCK_CENTRAL_DSN")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                })
                .ok_or_else(|| {
                    api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "community_data_unavailable",
                        "Die Wahlberechtigung kann gerade nicht geprüft werden.",
                    )
                })?;
            let options = PgConnectOptions::from_str(&dsn)
                .map_err(|_| {
                    api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "community_data_unavailable",
                        "Die Wahlberechtigung kann gerade nicht geprüft werden.",
                    )
                })?
                .application_name("twitch-clip-contest")
                .options([
                    ("default_transaction_read_only", "on"),
                    ("statement_timeout", "5000"),
                ])
                .disable_statement_logging();

            PgPoolOptions::new()
                .max_connections(2)
                .acquire_timeout(Duration::from_secs(5))
                .idle_timeout(Duration::from_secs(60))
                .connect_with(options)
                .await
                .map_err(|_| {
                    api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "community_data_unavailable",
                        "Die Wahlberechtigung kann gerade nicht geprüft werden.",
                    )
                })
        })
        .await
        .cloned()
}

fn snowflake_created_at(discord_id: u64) -> Option<DateTime<Utc>> {
    let millis = (discord_id >> 22).checked_add(1_420_070_400_000)?;
    DateTime::<Utc>::from_timestamp_millis(i64::try_from(millis).ok()?)
}

async fn vote_eligibility(discord_id: &str) -> Result<Value, Response> {
    let user_id = discord_id.parse::<i64>().map_err(|_| {
        api_error(
            StatusCode::UNAUTHORIZED,
            "discord_session_invalid",
            "Die Discord-Sitzung ist ungültig.",
        )
    })?;
    let guild_id = i64::try_from(
        tb_config::discord::DiscordOperations::default()
            .streamer_link
            .guild_id,
    )
    .map_err(|_| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "community_data_unavailable",
            "Die Wahlberechtigung kann gerade nicht geprüft werden.",
        )
    })?;
    let pool = community_pool().await?;
    let row = sqlx::query(
        r#"
        SELECT joined_at, account_created_at, present, is_bot
          FROM activity.guild_member_directory
         WHERE guild_id = $1 AND user_id = $2
         LIMIT 1
        "#,
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "Clip-Wettbewerb: Community-Mitgliedschaft nicht lesbar");
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "community_data_unavailable",
            "Die Wahlberechtigung kann gerade nicht geprüft werden.",
        )
    })?;

    let Some(row) = row else {
        return Ok(json!({
            "eligible": false,
            "account_old_enough": false,
            "member_old_enough": false,
            "present": false,
            "reason": "Du musst Mitglied der Community sein."
        }));
    };

    let joined_at = row
        .try_get::<Option<DateTime<Utc>>, _>("joined_at")
        .unwrap_or(None);
    let account_created_at = row
        .try_get::<Option<DateTime<Utc>>, _>("account_created_at")
        .unwrap_or(None)
        .or_else(|| {
            discord_id
                .parse::<u64>()
                .ok()
                .and_then(snowflake_created_at)
        });
    let present = row.try_get::<bool, _>("present").unwrap_or(false);
    let is_bot = row.try_get::<bool, _>("is_bot").unwrap_or(true);
    let now = Utc::now();
    let account_old_enough = account_created_at.is_some_and(|created| {
        now.signed_duration_since(created) >= ChronoDuration::days(MIN_ACCOUNT_AGE_DAYS)
    });
    let member_old_enough = joined_at.is_some_and(|joined| {
        now.signed_duration_since(joined) >= ChronoDuration::days(MIN_MEMBER_AGE_DAYS)
    });
    let eligible = present && !is_bot && account_old_enough && member_old_enough;
    let reason = if !present || is_bot {
        "Du musst Mitglied der Community sein."
    } else if !account_old_enough {
        "Dein Discord-Account muss mindestens 30 Tage alt sein."
    } else if !member_old_enough {
        "Du musst seit mindestens 7 Tagen in der Community sein."
    } else {
        ""
    };

    Ok(json!({
        "eligible": eligible,
        "account_old_enough": account_old_enough,
        "member_old_enough": member_old_enough,
        "present": present,
        "reason": reason
    }))
}

fn parse_clip_id(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    let segments: Vec<_> = url
        .path_segments()?
        .filter(|part| !part.trim().is_empty())
        .collect();

    let candidate = if host == "clips.twitch.tv" {
        segments.first().copied()
    } else if host == "twitch.tv" && segments.len() >= 3 && segments[1] == "clip" {
        segments.get(2).copied()
    } else {
        None
    }?;

    if candidate.len() > 128
        || candidate.is_empty()
        || !candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(candidate.to_string())
}

async fn deadlock_game_id(pool: &PgPool) -> String {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT twitch_game_id FROM social_media_category WHERE category_key = 'deadlock' LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
    .filter(|value| !value.trim().is_empty())
    .unwrap_or_else(|| DEADLOCK_GAME_ID_FALLBACK.to_string())
}

async fn active_partner_login(
    pool: &PgPool,
    broadcaster_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT twitch_login
          FROM twitch_streamers_partner_state
         WHERE twitch_user_id = $1
           AND is_partner_active = 1
         LIMIT 1
        "#,
    )
    .bind(broadcaster_id)
    .fetch_optional(pool)
    .await
}

fn clip_created_at(clip: &ClipRecord) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&clip.created_at)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn clip_is_recent(clip: &ClipRecord, now: DateTime<Utc>) -> bool {
    clip_created_at(clip).is_some_and(|created| {
        created <= now + ChronoDuration::minutes(5)
            && now.signed_duration_since(created) <= ChronoDuration::days(CLIP_MAX_AGE_DAYS)
    })
}

async fn lock_identity(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(key)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn finalize_month(pool: &PgPool, month: NaiveDate) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock_identity(
        &mut tx,
        &format!("clip-contest-finalize:{}", month_key(month)),
    )
    .await?;

    let already: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clip_contest_results WHERE contest_month = $1",
    )
    .bind(month)
    .fetch_one(&mut *tx)
    .await?;
    if already > 0 {
        tx.commit().await?;
        return Ok(());
    }

    let rows = sqlx::query(
        r#"
        SELECT s.id, s.clip_id, s.clip_url, s.clip_title, s.clip_thumbnail_url,
               s.streamer_login, s.broadcaster_id,
               COUNT(v.id)::BIGINT AS vote_count
          FROM twitch_clip_contest_submissions s
          LEFT JOIN twitch_clip_contest_votes v ON v.submission_id = s.id
         WHERE s.contest_month = $1
           AND s.hidden_at IS NULL
         GROUP BY s.id
         ORDER BY COUNT(v.id) DESC, s.submitted_at ASC, s.id ASC
         LIMIT 3
        "#,
    )
    .bind(month)
    .fetch_all(&mut *tx)
    .await?;

    for (index, row) in rows.iter().enumerate() {
        let rank = i16::try_from(index + 1).unwrap_or(3);
        let submission_id: i64 = row.try_get("id")?;
        let clip_id: String = row.try_get("clip_id")?;
        let clip_url: String = row.try_get("clip_url")?;
        let clip_title: String = row.try_get("clip_title")?;
        let thumbnail: Option<String> = row.try_get("clip_thumbnail_url")?;
        let streamer_login: String = row.try_get("streamer_login")?;
        let broadcaster_id: String = row.try_get("broadcaster_id")?;
        let vote_count: i64 = row.try_get("vote_count")?;

        let inserted = sqlx::query(
            r#"
            INSERT INTO twitch_clip_contest_results
                (contest_month, rank, submission_id, clip_id, clip_url, clip_title,
                 clip_thumbnail_url, streamer_login, broadcaster_id, vote_count)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            ON CONFLICT (contest_month, rank) DO NOTHING
            "#,
        )
        .bind(month)
        .bind(rank)
        .bind(submission_id)
        .bind(&clip_id)
        .bind(&clip_url)
        .bind(&clip_title)
        .bind(&thumbnail)
        .bind(&streamer_login)
        .bind(&broadcaster_id)
        .bind(i32::try_from(vote_count).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;

        if inserted.rows_affected() == 1 {
            let source_id = format!("clip-contest-top3:{}:{rank}:{clip_id}", month_key(month));
            sqlx::query(
                r#"
                INSERT INTO twitch_clip_contest_effort_outbox
                    (event_type, streamer_login, source_id, payload)
                VALUES ('clip_top3', $1, $2, $3)
                ON CONFLICT (event_type, source_id) DO NOTHING
                "#,
            )
            .bind(&streamer_login)
            .bind(source_id)
            .bind(json!({
                "contest_month": month_key(month),
                "rank": rank,
                "clip_id": clip_id,
                "submission_id": submission_id,
                "vote_count": vote_count
            }))
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await
}

async fn finalize_past_months(
    pool: &PgPool,
    now: DateTime<chrono_tz::Tz>,
) -> Result<(), sqlx::Error> {
    let current = current_month(now);
    let months = sqlx::query_scalar::<_, NaiveDate>(
        r#"
        SELECT DISTINCT contest_month
          FROM twitch_clip_contest_submissions
         WHERE contest_month < $1
         ORDER BY contest_month
        "#,
    )
    .bind(current)
    .fetch_all(pool)
    .await?;

    for month in months {
        finalize_month(pool, month).await?;
    }
    Ok(())
}

pub fn spawn_monthly_finalizer(pool: PgPool) {
    tokio::spawn(async move {
        loop {
            let now = Utc::now().with_timezone(&Berlin);
            let current = current_month(now);
            let next = next_month(current);
            let target = match Berlin.with_ymd_and_hms(next.year(), next.month(), 1, 0, 5, 0) {
                LocalResult::Single(value) => value,
                LocalResult::Ambiguous(first, _) => first,
                LocalResult::None => local_start(next),
            };
            let wait = target
                .signed_duration_since(now)
                .to_std()
                .unwrap_or_else(|_| Duration::from_secs(60));
            tokio::time::sleep(wait).await;
            if let Err(error) = finalize_past_months(&pool, Utc::now().with_timezone(&Berlin)).await
            {
                tracing::error!(%error, "Clip-Wettbewerb: Monatsabschluss fehlgeschlagen");
            }
        }
    });
}

fn row_to_clip(row: &sqlx::postgres::PgRow, voted_by_me: bool) -> Result<Value, sqlx::Error> {
    let hidden_at: Option<DateTime<Utc>> = row.try_get("hidden_at")?;
    Ok(json!({
        "id": row.try_get::<i64, _>("id")?,
        "clip_id": row.try_get::<String, _>("clip_id")?,
        "url": row.try_get::<String, _>("clip_url")?,
        "title": row.try_get::<String, _>("clip_title")?,
        "thumbnail_url": row.try_get::<Option<String>, _>("clip_thumbnail_url")?,
        "created_at": row.try_get::<DateTime<Utc>, _>("clip_created_at")?,
        "streamer": row.try_get::<String, _>("streamer_login")?,
        "votes": row.try_get::<i64, _>("vote_count")?,
        "hidden": hidden_at.is_some(),
        "voted_by_me": voted_by_me
    }))
}

async fn current_clips(
    pool: &PgPool,
    month: NaiveDate,
    voter: Option<&DiscordIdentity>,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT s.id, s.clip_id, s.clip_url, s.clip_title, s.clip_thumbnail_url,
               s.clip_created_at, s.streamer_login, s.hidden_at,
               COUNT(v.id)::BIGINT AS vote_count,
               BOOL_OR(v.voter_discord_id = $2) AS voted_by_me
          FROM twitch_clip_contest_submissions s
          LEFT JOIN twitch_clip_contest_votes v ON v.submission_id = s.id
         WHERE s.contest_month = $1
           AND s.hidden_at IS NULL
         GROUP BY s.id
         ORDER BY COUNT(v.id) DESC, s.submitted_at ASC, s.id ASC
        "#,
    )
    .bind(month)
    .bind(voter.map(|value| value.id.as_str()).unwrap_or(""))
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            let voted = row
                .try_get::<Option<bool>, _>("voted_by_me")?
                .unwrap_or(false);
            row_to_clip(row, voted)
        })
        .collect()
}

async fn hall_of_fame(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT r.contest_month, r.rank, r.submission_id, r.clip_id, r.clip_url,
               r.clip_title, r.clip_thumbnail_url, r.streamer_login, r.vote_count,
               r.finalized_at, s.hidden_at
          FROM twitch_clip_contest_results r
          JOIN twitch_clip_contest_submissions s ON s.id = r.submission_id
         ORDER BY r.contest_month DESC, r.rank ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut archive: Vec<Value> = Vec::new();
    let mut active_month: Option<NaiveDate> = None;
    let mut winners: Vec<Value> = Vec::new();

    for row in rows {
        let month: NaiveDate = row.try_get("contest_month")?;
        if active_month.is_some_and(|value| value != month) {
            let completed = active_month.expect("Monat gesetzt");
            archive.push(json!({
                "month": month_key(completed),
                "winners": std::mem::take(&mut winners)
            }));
        }
        active_month = Some(month);
        winners.push(json!({
            "rank": row.try_get::<i16, _>("rank")?,
            "submission_id": row.try_get::<i64, _>("submission_id")?,
            "clip_id": row.try_get::<String, _>("clip_id")?,
            "url": row.try_get::<String, _>("clip_url")?,
            "title": row.try_get::<String, _>("clip_title")?,
            "thumbnail_url": row.try_get::<Option<String>, _>("clip_thumbnail_url")?,
            "streamer": row.try_get::<String, _>("streamer_login")?,
            "votes": row.try_get::<i32, _>("vote_count")?,
            "finalized_at": row.try_get::<DateTime<Utc>, _>("finalized_at")?,
            "hidden": row.try_get::<Option<DateTime<Utc>>, _>("hidden_at")?.is_some()
        }));
    }
    if let Some(month) = active_month {
        archive.push(json!({
            "month": month_key(month),
            "winners": winners
        }));
    }
    Ok(archive)
}

pub async fn page_handler() -> Response {
    website::serve_website_asset(website::website_dist_root(), "clips/index.html").await
}

pub async fn discord_start_handler() -> Response {
    let Some(token) = broker_token() else {
        return Redirect::to("/clips?login=unavailable").into_response();
    };
    let guild_id = tb_config::discord::DiscordOperations::default()
        .streamer_link
        .guild_id;
    let payload = json!({
        "scope": "identify",
        "redirect_after": DISCORD_CALLBACK_URL,
        "requesting_service": "clip-contest",
        "metadata": {
            "next_path": "/clips",
            "guild_id": guild_id
        }
    });
    let Some(result) = broker_post(BROKER_INITIATE_PATH, &token, &payload).await else {
        return Redirect::to("/clips?login=unavailable").into_response();
    };
    let authorize_url = result
        .get("authorize_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if authorize_url.is_empty() {
        Redirect::to("/clips?login=unavailable").into_response()
    } else {
        Redirect::to(authorize_url).into_response()
    }
}

pub async fn discord_complete_handler(
    State(state): State<ClipContestState>,
    headers: HeaderMap,
    Query(query): Query<DiscordCompleteQuery>,
) -> Response {
    if query
        .error
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Redirect::to("/clips?login=cancelled").into_response();
    }
    let Some(state_id) = query
        .state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Redirect::to("/clips?login=invalid").into_response();
    };
    let Some(token) = broker_token() else {
        return Redirect::to("/clips?login=unavailable").into_response();
    };
    let Some(result) =
        broker_post(BROKER_CONSUME_PATH, &token, &json!({"state_id": state_id})).await
    else {
        return Redirect::to("/clips?login=invalid").into_response();
    };

    let metadata = result.get("service_metadata").and_then(Value::as_object);
    let expected_guild = tb_config::discord::DiscordOperations::default()
        .streamer_link
        .guild_id
        .to_string();
    let metadata_ok = metadata.is_some_and(|metadata| {
        metadata
            .get("next_path")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "/clips")
            && metadata
                .get("guild_id")
                .map(|value| {
                    value
                        .as_u64()
                        .map(|id| id.to_string())
                        .or_else(|| value.as_str().map(str::to_string))
                })
                .flatten()
                .is_some_and(|value| value == expected_guild)
    });
    if !metadata_ok {
        return Redirect::to("/clips?login=invalid").into_response();
    }

    let discord_id = result
        .get("discord_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if discord_id.is_empty() || !discord_id.chars().all(|c| c.is_ascii_digit()) {
        return Redirect::to("/clips?login=invalid").into_response();
    }
    let discord_name = result
        .get("discord_name")
        .and_then(Value::as_str)
        .unwrap_or("Discord")
        .trim()
        .chars()
        .take(100)
        .collect::<String>();

    let raw = tb_crypto::random_urlsafe_token(32);
    let lookup = session_lookup_key(&raw);
    let expires = Utc::now() + ChronoDuration::seconds(CONTEST_SESSION_TTL_SECS as i64);
    let stored = sqlx::query(
        r#"
        INSERT INTO twitch_clip_contest_discord_sessions
            (session_lookup_key, discord_id, discord_name, expires_at)
        VALUES ($1,$2,$3,$4)
        "#,
    )
    .bind(lookup)
    .bind(&discord_id)
    .bind(&discord_name)
    .bind(expires)
    .execute(&state.pool)
    .await;
    if let Err(error) = stored {
        tracing::error!(%error, "Clip-Wettbewerb: Discord-Session konnte nicht gespeichert werden");
        return Redirect::to("/clips?login=unavailable").into_response();
    }

    let cookie = build_session_cookie(
        DISCORD_SESSION_COOKIE,
        &raw,
        request_is_https(&headers),
        SameSite::Lax,
        CONTEST_SESSION_TTL_SECS,
    );
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, "/clips"),
            (header::SET_COOKIE, cookie.as_str()),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
        .into_response()
}

pub async fn logout_handler(State(state): State<ClipContestState>, headers: HeaderMap) -> Response {
    if let Some(raw) = cookie_value(&headers, DISCORD_SESSION_COOKIE) {
        let lookup = session_lookup_key(&raw);
        let _ = sqlx::query(
            "DELETE FROM twitch_clip_contest_discord_sessions WHERE session_lookup_key = $1",
        )
        .bind(lookup)
        .execute(&state.pool)
        .await;
    }
    if let Some(raw) = cookie_value(&headers, TWITCH_SESSION_COOKIE) {
        let lookup = session_lookup_key(&raw);
        let _ = sqlx::query(
            "DELETE FROM twitch_clip_contest_twitch_sessions WHERE session_lookup_key = $1",
        )
        .bind(lookup)
        .execute(&state.pool)
        .await;
    }

    let secure = request_is_https(&headers);
    let discord_cookie = clear_session_cookie(DISCORD_SESSION_COOKIE, secure, SameSite::Lax);
    let twitch_cookie = clear_session_cookie(TWITCH_SESSION_COOKIE, secure, SameSite::Lax);
    let mut response = (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"ok": true})),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&discord_cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&twitch_cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

pub async fn state_handler(
    State(state): State<ClipContestState>,
    auth: DashboardAuthLevel,
    headers: HeaderMap,
) -> Response {
    let now = Utc::now().with_timezone(&Berlin);
    let month = current_month(now);
    if let Err(error) = finalize_past_months(&state.pool, now).await {
        tracing::warn!(%error, "Clip-Wettbewerb: Hall-of-Fame-Abschluss beim Seitenaufruf fehlgeschlagen");
    }

    let discord = match contest_discord_identity(&state.pool, &headers).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Session-Lookup fehlgeschlagen");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
            );
        }
    };

    let twitch = match contest_twitch_identity(&state.pool, &headers).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Twitch-Session-Lookup fehlgeschlagen");
            None
        }
    };

    let submitter = match submit_identity(&state.pool, &headers, &auth).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Einreicher-Identität nicht lesbar");
            None
        }
    };

    let clips = match current_clips(&state.pool, month, discord.as_ref()).await {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(%error, "Clip-Wettbewerb: Clips nicht lesbar");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
            );
        }
    };
    let archive = hall_of_fame(&state.pool).await.unwrap_or_default();

    let submission_count = if let Some(identity) = submitter.as_ref() {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
              FROM twitch_clip_contest_submissions
             WHERE contest_month = $1
               AND (
                    ($2 <> '' AND submitter_discord_id = $2)
                    OR ($3 <> '' AND submitter_twitch_id = $3)
               )
            "#,
        )
        .bind(month)
        .bind(identity.discord_id.as_deref().unwrap_or(""))
        .bind(identity.twitch_id.as_deref().unwrap_or(""))
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0)
    } else {
        0
    };

    let votes_used = if let Some(discord) = discord.as_ref() {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_clip_contest_votes WHERE contest_month = $1 AND voter_discord_id = $2",
        )
        .bind(month)
        .bind(&discord.id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0)
    } else {
        0
    };

    let eligibility = if let Some(discord) = discord.as_ref() {
        match vote_eligibility(&discord.id).await {
            Ok(value) => value,
            Err(_) => json!({
                "eligible": false,
                "account_old_enough": false,
                "member_old_enough": false,
                "present": false,
                "reason": "Die Wahlberechtigung kann gerade nicht geprüft werden."
            }),
        }
    } else {
        json!({
            "eligible": false,
            "account_old_enough": false,
            "member_old_enough": false,
            "present": false,
            "reason": "Zum Abstimmen musst du dich mit Discord anmelden."
        })
    };

    let current_top3 = clips.iter().take(3).cloned().collect::<Vec<_>>();
    json_no_store(json!({
        "now": now.to_rfc3339(),
        "month": month_key(month),
        "phase": phase(now).as_str(),
        "phase_label": phase(now).label(),
        "submission_ends_at": submission_end(month).to_rfc3339(),
        "voting_ends_at": voting_end(month).to_rfc3339(),
        "limits": {
            "submissions_per_month": SUBMISSION_LIMIT,
            "votes_per_month": VOTE_LIMIT
        },
        "auth": {
            "discord": discord.as_ref().map(|value| json!({"id": value.id, "name": value.name})),
            "twitch": if let Some(twitch) = twitch.as_ref() {
                Some(json!({
                    "login": twitch.login,
                    "name": if twitch.name.is_empty() { &twitch.login } else { &twitch.name }
                }))
            } else {
                match &auth {
                    DashboardAuthLevel::Partner { twitch_login, display_name, .. } => Some(json!({
                        "login": twitch_login,
                        "name": if display_name.is_empty() { twitch_login } else { display_name }
                    })),
                    _ => None
                }
            },
            "is_admin": auth.is_privileged(),
            "can_submit": submitter.is_some() && submission_count < SUBMISSION_LIMIT,
            "submission_count": submission_count,
            "votes_used": votes_used,
            "vote_eligibility": eligibility
        },
        "top3": current_top3,
        "clips": clips,
        "archive": archive
    }))
}

pub async fn submit_handler(
    State(state): State<ClipContestState>,
    auth: DashboardAuthLevel,
    headers: HeaderMap,
    Json(body): Json<SubmitBody>,
) -> Response {
    let now_local = Utc::now().with_timezone(&Berlin);
    if phase(now_local) != ContestPhase::Submission {
        return api_error(
            StatusCode::CONFLICT,
            "submissions_closed",
            "Einreichungen sind nur vom 1. bis zum 21. möglich.",
        );
    }
    let Some(clip_id) = parse_clip_id(&body.clip_url) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_clip_url",
            "Bitte gib eine gültige Twitch-Clip-URL an.",
        );
    };
    let identity = match submit_identity(&state.pool, &headers, &auth).await {
        Ok(Some(value)) => value,
        Ok(None) => {
            return api_error(
                StatusCode::UNAUTHORIZED,
                "login_required",
                "Zum Einreichen musst du dich mit Discord oder Twitch anmelden.",
            )
        }
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Einreicher-Identität nicht lesbar");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
            );
        }
    };

    let Some(helix) = state.helix.as_ref() else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "clip_validation_unavailable",
            "Twitch-Clips können gerade nicht geprüft werden.",
        );
    };
    let source = HelixClipSource::new(helix.clone());
    let clip = match source.fetch_clip_by_id(&clip_id).await {
        Ok(Some(value)) => value,
        Ok(None) => {
            return api_error(
                StatusCode::NOT_FOUND,
                "clip_not_found",
                "Der Twitch-Clip wurde nicht gefunden.",
            )
        }
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Helix-Clipprüfung fehlgeschlagen");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "clip_validation_unavailable",
                "Twitch-Clips können gerade nicht geprüft werden.",
            );
        }
    };

    if !clip_is_recent(&clip, Utc::now()) {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "clip_too_old",
            "Der Clip darf höchstens 60 Tage alt sein.",
        );
    }

    let expected_game = deadlock_game_id(&state.pool).await;
    if clip.game_id.as_deref() != Some(expected_game.as_str()) {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_deadlock",
            "Der Clip muss aus einem Deadlock-Stream stammen.",
        );
    }

    let streamer_login = match active_partner_login(&state.pool, &clip.twitch_user_id).await {
        Ok(Some(value)) => value,
        Ok(None) => {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "not_network_partner",
                "Der Clip muss aus einem aktiven Partnerkanal der Community stammen.",
            )
        }
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Partnerprüfung fehlgeschlagen");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
            );
        }
    };

    let Some(created_at) = clip_created_at(&clip) else {
        return api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "clip_timestamp_invalid",
            "Die Twitch-Clipdaten sind unvollständig.",
        );
    };
    let month = current_month(now_local);
    let person_key = identity
        .discord_id
        .as_deref()
        .or(identity.twitch_id.as_deref())
        .unwrap_or(identity.id.as_str());

    let mut tx = match state.pool.begin().await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Transaktion konnte nicht gestartet werden");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
            );
        }
    };
    if let Err(error) = lock_identity(
        &mut tx,
        &format!("clip-contest-submit:{}:{person_key}", month_key(month)),
    )
    .await
    {
        tracing::warn!(%error, "Clip-Wettbewerb: Einreichungs-Lock fehlgeschlagen");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
        );
    }

    let count: i64 = match sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
          FROM twitch_clip_contest_submissions
         WHERE contest_month = $1
           AND (
                ($2 <> '' AND submitter_discord_id = $2)
                OR ($3 <> '' AND submitter_twitch_id = $3)
           )
        "#,
    )
    .bind(month)
    .bind(identity.discord_id.as_deref().unwrap_or(""))
    .bind(identity.twitch_id.as_deref().unwrap_or(""))
    .fetch_one(&mut *tx)
    .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Einreichungslimit nicht lesbar");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
            );
        }
    };
    if count >= SUBMISSION_LIMIT {
        return api_error(
            StatusCode::CONFLICT,
            "submission_limit",
            "Du kannst höchstens 3 Clips pro Monat einreichen.",
        );
    }

    let inserted = sqlx::query(
        r#"
        INSERT INTO twitch_clip_contest_submissions
            (contest_month, clip_id, clip_url, clip_title, clip_thumbnail_url,
             clip_created_at, game_id, broadcaster_id, streamer_login,
             submitter_kind, submitter_id, submitter_display_name,
             submitter_discord_id, submitter_twitch_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
        ON CONFLICT (contest_month, clip_id) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(month)
    .bind(&clip.clip_id)
    .bind(&clip.clip_url)
    .bind(&clip.clip_title)
    .bind(&clip.thumbnail_url)
    .bind(created_at)
    .bind(clip.game_id.as_deref().unwrap_or(""))
    .bind(&clip.twitch_user_id)
    .bind(&streamer_login)
    .bind(identity.kind)
    .bind(&identity.id)
    .bind(&identity.display_name)
    .bind(identity.discord_id.as_deref())
    .bind(identity.twitch_id.as_deref())
    .fetch_optional(&mut *tx)
    .await;

    let submission_id: i64 = match inserted {
        Ok(Some(row)) => match row.try_get("id") {
            Ok(value) => value,
            Err(_) => {
                return api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "contest_unavailable",
                    "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
                )
            }
        },
        Ok(None) => {
            return api_error(
                StatusCode::CONFLICT,
                "clip_already_submitted",
                "Dieser Clip wurde für diesen Monat bereits eingereicht.",
            )
        }
        Err(error) => {
            tracing::error!(%error, "Clip-Wettbewerb: Einreichung fehlgeschlagen");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip konnte gerade nicht gespeichert werden.",
            );
        }
    };

    let source_id = format!("clip-contest:{}:{}", month_key(month), clip.clip_id);
    if let Err(error) = sqlx::query(
        r#"
        INSERT INTO twitch_clip_contest_effort_outbox
            (event_type, streamer_login, source_id, payload)
        VALUES ('clip_submitted', $1, $2, $3)
        ON CONFLICT (event_type, source_id) DO NOTHING
        "#,
    )
    .bind(&streamer_login)
    .bind(source_id)
    .bind(json!({
        "contest_month": month_key(month),
        "submission_id": submission_id,
        "clip_id": clip.clip_id
    }))
    .execute(&mut *tx)
    .await
    {
        tracing::error!(%error, "Clip-Wettbewerb: Effort-Outbox konnte nicht geschrieben werden");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Der Clip konnte gerade nicht gespeichert werden.",
        );
    }

    if let Err(error) = tx.commit().await {
        tracing::error!(%error, "Clip-Wettbewerb: Einreichung konnte nicht abgeschlossen werden");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Der Clip konnte gerade nicht gespeichert werden.",
        );
    }

    json_no_store(json!({"ok": true, "submission_id": submission_id}))
}

pub async fn vote_handler(
    State(state): State<ClipContestState>,
    headers: HeaderMap,
    Path(submission_id): Path<i64>,
) -> Response {
    let now = Utc::now().with_timezone(&Berlin);
    if phase(now) != ContestPhase::Voting {
        return api_error(
            StatusCode::CONFLICT,
            "voting_closed",
            "Abstimmen ist nur vom 22. bis zum Monatsende möglich.",
        );
    }
    let discord = match contest_discord_identity(&state.pool, &headers).await {
        Ok(Some(value)) => value,
        Ok(None) => {
            return api_error(
                StatusCode::UNAUTHORIZED,
                "discord_login_required",
                "Zum Abstimmen musst du dich mit Discord anmelden.",
            )
        }
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Discord-Session nicht lesbar");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb ist gerade nicht erreichbar.",
            );
        }
    };
    let eligibility = match vote_eligibility(&discord.id).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !eligibility
        .get("eligible")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return (
            StatusCode::FORBIDDEN,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({
                "error": "not_eligible",
                "message": eligibility.get("reason").and_then(Value::as_str).unwrap_or("Du bist noch nicht wahlberechtigt."),
                "eligibility": eligibility
            })),
        )
            .into_response();
    }

    let month = current_month(now);
    let mut tx = match state.pool.begin().await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Vote-Transaktion konnte nicht gestartet werden");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Stimme konnte gerade nicht gespeichert werden.",
            );
        }
    };
    if let Err(error) = lock_identity(
        &mut tx,
        &format!("clip-contest-vote:{}:{}", month_key(month), discord.id),
    )
    .await
    {
        tracing::warn!(%error, "Clip-Wettbewerb: Vote-Lock fehlgeschlagen");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Die Stimme konnte gerade nicht gespeichert werden.",
        );
    }

    let submission = match sqlx::query(
        r#"
        SELECT contest_month, submitter_discord_id, submitter_twitch_id, hidden_at
          FROM twitch_clip_contest_submissions
         WHERE id = $1
         LIMIT 1
        "#,
    )
    .bind(submission_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(Some(value)) => value,
        Ok(None) => {
            return api_error(
                StatusCode::NOT_FOUND,
                "clip_not_found",
                "Der Clip wurde nicht gefunden.",
            )
        }
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Clip für Abstimmung nicht lesbar");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Stimme konnte gerade nicht gespeichert werden.",
            );
        }
    };

    let contest_month: NaiveDate = submission.try_get("contest_month").unwrap_or(month);
    let hidden_at: Option<DateTime<Utc>> = submission.try_get("hidden_at").unwrap_or(None);
    if contest_month != month || hidden_at.is_some() {
        return api_error(
            StatusCode::CONFLICT,
            "clip_not_votable",
            "Für diesen Clip kann nicht abgestimmt werden.",
        );
    }
    let submitter_discord: Option<String> =
        submission.try_get("submitter_discord_id").unwrap_or(None);
    let submitter_twitch: Option<String> =
        submission.try_get("submitter_twitch_id").unwrap_or(None);
    let twitch_discord = if let Some(twitch_id) = submitter_twitch.as_deref() {
        sqlx::query_scalar::<_, Option<String>>(
            r#"
            SELECT discord_user_id::text
              FROM twitch_streamer_identities
             WHERE twitch_user_id = $1
               AND discord_user_id IS NOT NULL
             ORDER BY updated_at DESC
             LIMIT 1
            "#,
        )
        .bind(twitch_id)
        .fetch_optional(&mut *tx)
        .await
        .ok()
        .flatten()
        .flatten()
    } else {
        None
    };
    if submitter_discord.as_deref() == Some(discord.id.as_str())
        || twitch_discord.as_deref() == Some(discord.id.as_str())
    {
        return api_error(
            StatusCode::FORBIDDEN,
            "self_vote",
            "Für den eigenen Clip kannst du nicht abstimmen.",
        );
    }

    let used: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clip_contest_votes WHERE contest_month = $1 AND voter_discord_id = $2",
    )
    .bind(month)
    .bind(&discord.id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Stimmenlimit nicht lesbar");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Stimme konnte gerade nicht gespeichert werden.",
            );
        }
    };
    if used >= VOTE_LIMIT {
        return api_error(
            StatusCode::CONFLICT,
            "vote_limit",
            "Du kannst höchstens 5 Stimmen pro Monat vergeben.",
        );
    }

    let result = sqlx::query(
        r#"
        INSERT INTO twitch_clip_contest_votes
            (contest_month, submission_id, voter_discord_id, voter_display_name)
        VALUES ($1,$2,$3,$4)
        ON CONFLICT (submission_id, voter_discord_id) DO NOTHING
        "#,
    )
    .bind(month)
    .bind(submission_id)
    .bind(&discord.id)
    .bind(&discord.name)
    .execute(&mut *tx)
    .await;
    match result {
        Ok(done) if done.rows_affected() == 1 => {}
        Ok(_) => {
            return api_error(
                StatusCode::CONFLICT,
                "already_voted",
                "Du hast für diesen Clip bereits abgestimmt.",
            )
        }
        Err(error) => {
            tracing::error!(%error, "Clip-Wettbewerb: Stimme konnte nicht gespeichert werden");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Stimme konnte gerade nicht gespeichert werden.",
            );
        }
    }
    if let Err(error) = tx.commit().await {
        tracing::error!(%error, "Clip-Wettbewerb: Vote-Commit fehlgeschlagen");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Die Stimme konnte gerade nicht gespeichert werden.",
        );
    }
    json_no_store(json!({"ok": true, "votes_used": used + 1}))
}

fn admin_actor(auth: &DashboardAuthLevel) -> String {
    match auth {
        DashboardAuthLevel::Admin { actor: Some(actor) } => actor.twitch_login.clone(),
        DashboardAuthLevel::Admin { actor: None } => "admin".to_string(),
        _ => "unknown".to_string(),
    }
}

pub async fn visibility_handler(
    State(state): State<ClipContestState>,
    auth: DashboardAuthLevel,
    headers: HeaderMap,
    Path(submission_id): Path<i64>,
    Json(body): Json<VisibilityBody>,
) -> Response {
    if !auth.is_privileged() {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "admin_required",
            "Für diese Aktion ist Admin-Zugriff erforderlich.",
        );
    }
    if !same_origin(&headers) {
        return api_error(
            StatusCode::FORBIDDEN,
            "same_origin_required",
            "Diese Admin-Aktion ist nur direkt von der Community-Seite erlaubt.",
        );
    }
    let actor = admin_actor(&auth);
    let mut tx = match state.pool.begin().await {
        Ok(value) => value,
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Moderation ist gerade nicht erreichbar.",
            )
        }
    };

    let changed = if body.hidden {
        sqlx::query(
            r#"
            UPDATE twitch_clip_contest_submissions
               SET hidden_at = COALESCE(hidden_at, now()), hidden_by = $2
             WHERE id = $1
            "#,
        )
        .bind(submission_id)
        .bind(&actor)
        .execute(&mut *tx)
        .await
    } else {
        sqlx::query(
            r#"
            UPDATE twitch_clip_contest_submissions
               SET hidden_at = NULL, hidden_by = NULL
             WHERE id = $1
            "#,
        )
        .bind(submission_id)
        .execute(&mut *tx)
        .await
    };
    let changed = match changed {
        Ok(value) if value.rows_affected() == 1 => true,
        Ok(_) => false,
        Err(error) => {
            tracing::error!(%error, "Clip-Wettbewerb: Moderation fehlgeschlagen");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Moderation ist gerade nicht erreichbar.",
            );
        }
    };
    if !changed {
        return api_error(
            StatusCode::NOT_FOUND,
            "clip_not_found",
            "Der Clip wurde nicht gefunden.",
        );
    }

    if let Err(error) = sqlx::query(
        r#"
        INSERT INTO twitch_clip_contest_moderation_audit
            (submission_id, action, actor)
        VALUES ($1,$2,$3)
        "#,
    )
    .bind(submission_id)
    .bind(if body.hidden { "hide" } else { "unhide" })
    .bind(&actor)
    .execute(&mut *tx)
    .await
    {
        tracing::error!(%error, "Clip-Wettbewerb: Moderations-Audit fehlgeschlagen");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Die Moderation ist gerade nicht erreichbar.",
        );
    }
    if let Err(error) = tx.commit().await {
        tracing::error!(%error, "Clip-Wettbewerb: Moderations-Commit fehlgeschlagen");
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Die Moderation ist gerade nicht erreichbar.",
        );
    }
    json_no_store(json!({"ok": true, "hidden": body.hidden}))
}

pub fn build_router(pool: PgPool, rate_limiter: RateLimiter, helix: Option<HelixClient>) -> Router {
    let state = ClipContestState {
        pool,
        helix: helix.map(Arc::new),
    };
    let submit_rl = RateLimitLayerConfig::new(rate_limiter.clone(), "clip_contest_submit", 10, 60);
    let vote_rl = RateLimitLayerConfig::new(rate_limiter, "clip_contest_vote", 20, 60);

    Router::new()
        .route("/clips", get(page_handler))
        .route("/clips/", get(page_handler))
        .route("/clips/auth/discord", get(discord_start_handler))
        .route(
            "/clips/auth/discord/complete",
            get(discord_complete_handler),
        )
        .route("/clips/api/state", get(state_handler))
        .route(
            "/clips/api/submit",
            post(submit_handler).layer(middleware::from_fn_with_state(
                submit_rl,
                rate_limit_middleware,
            )),
        )
        .route(
            "/clips/api/vote/{submission_id}",
            post(vote_handler).layer(middleware::from_fn_with_state(
                vote_rl,
                rate_limit_middleware,
            )),
        )
        .route("/clips/api/logout", post(logout_handler))
        .route(
            "/clips/api/admin/{submission_id}/visibility",
            post(visibility_handler),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn berlin_at(year: i32, month: u32, day: u32, hour: u32) -> DateTime<chrono_tz::Tz> {
        Berlin
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn phases_wechseln_am_22() {
        assert_eq!(phase(berlin_at(2026, 9, 21, 23)), ContestPhase::Submission);
        assert_eq!(phase(berlin_at(2026, 9, 22, 0)), ContestPhase::Voting);
    }

    #[test]
    fn monatswechsel_funktioniert_ueber_jahresgrenze() {
        let december = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        assert_eq!(
            next_month(december),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }

    #[test]
    fn twitch_clip_urls_werden_strikt_erkannt() {
        assert_eq!(
            parse_clip_id("https://clips.twitch.tv/FancyClip_42"),
            Some("FancyClip_42".into())
        );
        assert_eq!(
            parse_clip_id("https://www.twitch.tv/streamer/clip/Fancy-Clip"),
            Some("Fancy-Clip".into())
        );
        assert_eq!(parse_clip_id("http://clips.twitch.tv/FancyClip"), None);
        assert_eq!(parse_clip_id("https://evil.example/FancyClip"), None);
    }

    #[test]
    fn discord_snowflake_liefert_accountzeit() {
        let created = snowflake_created_at(175928847299117063).unwrap();
        assert!(created.year() >= 2016);
        assert!(created.year() <= 2017);
    }
}
