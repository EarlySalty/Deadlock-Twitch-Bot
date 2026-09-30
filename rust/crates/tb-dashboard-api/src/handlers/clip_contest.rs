//! Öffentlicher monatlicher Clip-Wettbewerb unter /clips.
//!
//! Discord-Login wird ausschließlich an den bestehenden lokalen Discord-OAuth-
//! Broker delegiert. Twitch-Login nutzt die vorhandene Dashboard-Session.
//! Votes und Limits liegen serverseitig in PostgreSQL.

use std::{collections::BTreeMap, str::FromStr, time::Duration as StdDuration};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions, PgPool, Postgres, Row, Transaction,
};
use tb_social_media::{clip::model::ClipRecord, ClipRepository};
use tb_transport_twitch::{HelixClient, HelixConfig};
use tokio::sync::OnceCell;

use crate::{
    auth::{
        csrf::is_allowed_origin,
        level::{cookie_values, DashboardAuthLevel},
    },
    uplink_config,
};

const SESSION_COOKIE: &str = "ddc_clip_contest_session";
const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
const MAX_SUBMISSIONS_PER_MONTH: i64 = 3;
const MAX_VOTES_PER_MONTH: i64 = 5;
const MAX_CLIP_AGE_DAYS: i64 = 60;
const COMMUNITY_GUILD_ID: i64 = 1_289_721_245_281_292_288;
const DISCORD_BROKER_BASE: &str = "http://127.0.0.1:8766";
const BROKER_INITIATE_PATH: &str = "/internal/v1/discord/initiate";
const BROKER_CONSUME_PATH: &str = "/internal/v1/discord/consume-result";
const BROKER_TOKEN_HEADER: &str = "X-Internal-Token";
const DEADLOCK_GAME_ID_FALLBACK: &str = "2132205352";

static CENTRAL_POOL: OnceCell<PgPool> = OnceCell::const_new();

#[derive(Debug, Clone)]
struct DiscordIdentity {
    user_id: String,
    display_name: String,
}

#[derive(Debug, Clone)]
struct SubmitIdentity {
    provider: &'static str,
    user_id: String,
    person_key: String,
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
    #[serde(default = "default_true")]
    hidden: bool,
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
    let (phase, end_day) = if local.day() <= 21 {
        (Phase::Submission, 22)
    } else {
        (
            Phase::Voting,
            days_in_month(local.year(), local.month()) + 1,
        )
    };

    let phase_ends_local = if end_day <= days_in_month(local.year(), local.month()) {
        Berlin
            .with_ymd_and_hms(local.year(), local.month(), end_day, 0, 0, 0)
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
        phase_ends_at: phase_ends_local.with_timezone(&Utc),
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };
    (next - Duration::days(1)).day()
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

fn session_hash(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

fn secure_request(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
        || headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| !v.starts_with("127.0.0.1") && !v.starts_with("localhost"))
}

fn session_cookie(raw: &str, secure: bool) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE}={raw}; Path=/clips; Max-Age={SESSION_TTL_SECS}; HttpOnly; SameSite=Lax"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn clear_session_cookie(secure: bool) -> String {
    let mut cookie = format!("{SESSION_COOKIE}=; Path=/clips; Max-Age=0; HttpOnly; SameSite=Lax");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

async fn discord_session(pool: &PgPool, headers: &HeaderMap) -> Option<DiscordIdentity> {
    let raw = cookie_values(headers, SESSION_COOKIE).into_iter().next()?;
    if raw.len() < 20 || raw.len() > 200 {
        return None;
    }
    let hash = session_hash(raw);
    let row = sqlx::query(
        "SELECT discord_user_id, discord_name
           FROM twitch_clip_contest_sessions
          WHERE session_hash = $1 AND expires_at > now()
          LIMIT 1",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    Some(DiscordIdentity {
        user_id: row.try_get("discord_user_id").ok()?,
        display_name: row.try_get("discord_name").ok()?,
    })
}

async fn linked_discord_person_key(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Option<String> {
    let discord_id: Option<String> = sqlx::query_scalar(
        "SELECT NULLIF(TRIM(discord_user_id), '')
           FROM twitch_streamer_identities
          WHERE twitch_user_id = $1 OR LOWER(twitch_login) = LOWER($2)
          ORDER BY CASE WHEN twitch_user_id = $1 THEN 0 ELSE 1 END
          LIMIT 1",
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    discord_id.map(|id| format!("discord:{id}"))
}

async fn submit_identity(
    pool: &PgPool,
    headers: &HeaderMap,
    auth: &DashboardAuthLevel,
) -> Option<SubmitIdentity> {
    if let Some(discord) = discord_session(pool, headers).await {
        return Some(SubmitIdentity {
            provider: "discord",
            person_key: format!("discord:{}", discord.user_id),
            user_id: discord.user_id,
            display_name: discord.display_name,
        });
    }
    let (user_id, login, display_name) = match auth {
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
            display_name,
        } => (
            twitch_user_id.clone(),
            twitch_login.clone(),
            display_name.clone(),
        ),
        DashboardAuthLevel::Admin { actor: Some(actor) } => (
            actor.twitch_user_id.clone(),
            actor.twitch_login.clone(),
            actor.twitch_login.clone(),
        ),
        _ => return None,
    };
    let person_key = linked_discord_person_key(pool, &user_id, &login)
        .await
        .unwrap_or_else(|| format!("twitch:{user_id}"));
    Some(SubmitIdentity {
        provider: "twitch",
        user_id,
        person_key,
        display_name: if display_name.trim().is_empty() {
            login
        } else {
            display_name
        },
    })
}

fn clip_id_from_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    let candidate = if host == "clips.twitch.tv" {
        segments.first().copied()
    } else if matches!(host.as_str(), "twitch.tv" | "www.twitch.tv" | "m.twitch.tv") {
        segments
            .iter()
            .position(|s| s.eq_ignore_ascii_case("clip"))
            .and_then(|i| segments.get(i + 1).copied())
    } else {
        None
    }?;
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

fn build_helix_client() -> Option<HelixClient> {
    let client_id = std::env::var("TWITCH_CLIENT_ID").ok()?.trim().to_string();
    let client_secret = std::env::var("TWITCH_CLIENT_SECRET")
        .ok()?
        .trim()
        .to_string();
    if client_id.is_empty() || client_secret.is_empty() {
        return None;
    }
    HelixClient::new(HelixConfig::new(client_id, client_secret)).ok()
}

async fn fetch_helix_clip(clip_id: &str) -> Result<HelixClip, Response> {
    let Some(client) = build_helix_client() else {
        return Err(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "twitch_unavailable",
            "Die Twitch-Prüfung ist gerade nicht verfügbar.",
        ));
    };
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
        .and_then(|v| v.first())
    else {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "clip_not_found",
            "Diesen Twitch-Clip gibt es nicht oder er ist nicht mehr verfügbar.",
        ));
    };

    let required = |name: &str| {
        clip.get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let created_raw = required("created_at").ok_or_else(|| {
        json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "clip_invalid",
            "Twitch hat für diesen Clip keinen gültigen Zeitstempel geliefert.",
        )
    })?;
    let created_at = DateTime::parse_from_rfc3339(&created_raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| {
            json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "clip_invalid",
                "Twitch hat für diesen Clip keinen gültigen Zeitstempel geliefert.",
            )
        })?;

    Ok(HelixClip {
        clip_id: required("id").unwrap_or_else(|| clip_id.to_string()),
        url: required("url").unwrap_or_else(|| format!("https://clips.twitch.tv/{clip_id}")),
        title: required("title").unwrap_or_else(|| "Deadlock-Clip".to_string()),
        thumbnail_url: required("thumbnail_url"),
        broadcaster_id: required("broadcaster_id").ok_or_else(|| {
            json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "clip_invalid",
                "Der Kanal des Clips konnte nicht geprüft werden.",
            )
        })?,
        broadcaster_name: required("broadcaster_name").unwrap_or_default(),
        game_id: required("game_id").unwrap_or_default(),
        created_at,
        duration_seconds: clip.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
        view_count: clip.get("view_count").and_then(Value::as_i64).unwrap_or(0),
        vod_id: required("video_id"),
        vod_offset_s: clip
            .get("vod_offset")
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok()),
    })
}

async fn deadlock_game_id(pool: &PgPool) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT twitch_game_id
           FROM social_media_category
          WHERE category_key = 'deadlock'
            AND NULLIF(TRIM(twitch_game_id), '') IS NOT NULL
          LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| DEADLOCK_GAME_ID_FALLBACK.to_string())
}

async fn active_partner_login(pool: &PgPool, twitch_user_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT twitch_login
           FROM twitch_streamers_partner_state
          WHERE twitch_user_id = $1
            AND is_partner_active = 1
          LIMIT 1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

async fn central_pool() -> Result<PgPool, Response> {
    CENTRAL_POOL
        .get_or_try_init(|| async {
            let dsn = std::env::var("DEADLOCK_CENTRAL_READONLY_DSN")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .or_else(|| {
                    std::env::var("DEADLOCK_CENTRAL_DSN")
                        .ok()
                        .filter(|v| !v.trim().is_empty())
                })
                .ok_or_else(|| {
                    json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "community_check_unavailable",
                        "Die Discord-Mitgliedschaft kann gerade nicht geprüft werden.",
                    )
                })?;
            let options = PgConnectOptions::from_str(&dsn)
                .map_err(|_| {
                    json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "community_check_unavailable",
                        "Die Discord-Mitgliedschaft kann gerade nicht geprüft werden.",
                    )
                })?
                .application_name("twitch-clip-contest")
                .options([
                    ("default_transaction_read_only", "on"),
                    ("statement_timeout", "5000"),
                ])
                .disable_statement_logging();
            PgPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(StdDuration::from_secs(5))
                .idle_timeout(StdDuration::from_secs(60))
                .connect_with(options)
                .await
                .map_err(|_| {
                    json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "community_check_unavailable",
                        "Die Discord-Mitgliedschaft kann gerade nicht geprüft werden.",
                    )
                })
        })
        .await
        .cloned()
}

fn discord_account_created_at(discord_id: &str) -> Option<DateTime<Utc>> {
    const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
    let id = discord_id.parse::<u64>().ok()?;
    let millis = (id >> 22).checked_add(DISCORD_EPOCH_MS)?;
    DateTime::<Utc>::from_timestamp_millis(i64::try_from(millis).ok()?)
}

async fn discord_eligibility(discord_id: &str) -> Result<DiscordEligibility, Response> {
    let id = discord_id.parse::<i64>().map_err(|_| {
        json_error(
            StatusCode::UNAUTHORIZED,
            "discord_session_invalid",
            "Die Discord-Sitzung ist ungültig. Bitte erneut anmelden.",
        )
    })?;
    let pool = central_pool().await?;
    let row = sqlx::query(
        "SELECT joined_at, account_created_at, present
           FROM activity.guild_member_directory
          WHERE guild_id = $1 AND user_id = $2
          LIMIT 1",
    )
    .bind(COMMUNITY_GUILD_ID)
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| {
        json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "community_check_unavailable",
            "Die Discord-Mitgliedschaft kann gerade nicht geprüft werden.",
        )
    })?;

    let now = Utc::now();
    let snowflake_created = discord_account_created_at(discord_id);
    let (joined_at, stored_created, present) = match row {
        Some(row) => (
            row.try_get::<Option<DateTime<Utc>>, _>("joined_at")
                .unwrap_or(None),
            row.try_get::<Option<DateTime<Utc>>, _>("account_created_at")
                .unwrap_or(None),
            row.try_get::<bool, _>("present").unwrap_or(false),
        ),
        None => (None, None, false),
    };
    let account_created_at = stored_created.or(snowflake_created);
    Ok(DiscordEligibility {
        account_age_ok: account_created_at
            .is_some_and(|created| created <= now - Duration::days(30)),
        member_age_ok: present && joined_at.is_some_and(|joined| joined <= now - Duration::days(7)),
        present,
        joined_at,
    })
}

async fn ensure_due_months_finalized(pool: &PgPool, current_month: NaiveDate) {
    let months = match sqlx::query(
        "SELECT DISTINCT s.contest_month
           FROM twitch_clip_contest_submissions s
          WHERE s.contest_month < $1
            AND NOT EXISTS (
                SELECT 1 FROM twitch_clip_contest_hall_of_fame h
                 WHERE h.contest_month = s.contest_month
            )
          ORDER BY s.contest_month",
    )
    .bind(current_month)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: fällige Monate konnten nicht geladen werden");
            return;
        }
    };

    for row in months {
        if let Ok(month) = row.try_get::<NaiveDate, _>("contest_month") {
            if let Err(error) = finalize_month(pool, month).await {
                tracing::warn!(%error, %month, "Clip-Wettbewerb: Monatsabschluss fehlgeschlagen");
            }
        }
    }
}

async fn finalize_month(pool: &PgPool, month: NaiveDate) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let lock_key = format!("twitch_clip_contest_finalize:{month}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await?;
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM twitch_clip_contest_hall_of_fame WHERE contest_month = $1",
    )
    .bind(month)
    .fetch_one(&mut *tx)
    .await?;
    if existing > 0 {
        tx.commit().await?;
        return Ok(());
    }

    let winners = sqlx::query(
        "SELECT s.id, s.broadcaster_twitch_id, s.broadcaster_login, COUNT(v.id)::bigint AS votes
           FROM twitch_clip_contest_submissions s
           LEFT JOIN twitch_clip_contest_votes v ON v.submission_id = s.id
          WHERE s.contest_month = $1
            AND s.hidden_at IS NULL
          GROUP BY s.id, s.broadcaster_twitch_id, s.broadcaster_login, s.submitted_at
          ORDER BY COUNT(v.id) DESC, s.submitted_at ASC, s.id ASC
          LIMIT 3",
    )
    .bind(month)
    .fetch_all(&mut *tx)
    .await?;

    for (idx, row) in winners.into_iter().enumerate() {
        let rank = i16::try_from(idx + 1).unwrap_or(3);
        let submission_id: i64 = row.try_get("id")?;
        let partner_twitch_user_id: String = row.try_get("broadcaster_twitch_id")?;
        let partner_login: String = row.try_get("broadcaster_login")?;
        let votes: i64 = row.try_get("votes")?;
        let points_hint = match rank {
            1 => 20,
            2 => 15,
            _ => 10,
        };
        sqlx::query(
            "INSERT INTO twitch_clip_contest_hall_of_fame
                (contest_month, rank, submission_id, vote_count)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (contest_month, rank) DO NOTHING",
        )
        .bind(month)
        .bind(rank)
        .bind(submission_id)
        .bind(i32::try_from(votes).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;

        emit_effort_event(
            &mut tx,
            "clip_top3",
            &partner_twitch_user_id,
            &partner_login,
            points_hint,
            &format!("clip-contest:top3:{month}:{rank}:{submission_id}"),
            json!({
                "contest_month": month.to_string(),
                "rank": rank,
                "submission_id": submission_id,
                "votes": votes,
            }),
        )
        .await?;
    }
    tx.commit().await
}

async fn emit_effort_event(
    tx: &mut Transaction<'_, Postgres>,
    event_type: &str,
    partner_twitch_user_id: &str,
    partner_login: &str,
    points_hint: i32,
    source_id: &str,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_clip_contest_effort_outbox
            (event_type, partner_twitch_user_id, partner_login, points_hint, source_id, occurred_at, metadata)
         VALUES ($1, $2, LOWER($3), $4, $5, now(), $6)
         ON CONFLICT (source_id) DO NOTHING",
    )
    .bind(event_type)
    .bind(partner_twitch_user_id)
    .bind(partner_login)
    .bind(points_hint)
    .bind(source_id)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn submission_json(row: &sqlx::postgres::PgRow, my_discord_id: Option<&str>) -> Value {
    let submitter_key: String = row.try_get("submitter_person_key").unwrap_or_default();
    let my_own = my_discord_id
        .map(|id| submitter_key == format!("discord:{id}"))
        .unwrap_or(false);
    json!({
        "id": row.try_get::<i64, _>("id").unwrap_or_default(),
        "clip_id": row.try_get::<String, _>("twitch_clip_id").unwrap_or_default(),
        "clip_url": row.try_get::<String, _>("clip_url").unwrap_or_default(),
        "title": row.try_get::<String, _>("clip_title").unwrap_or_default(),
        "thumbnail_url": row.try_get::<Option<String>, _>("clip_thumbnail_url").unwrap_or(None),
        "channel": row.try_get::<String, _>("broadcaster_login").unwrap_or_default(),
        "channel_name": row.try_get::<Option<String>, _>("broadcaster_name").unwrap_or(None),
        "votes": row.try_get::<i64, _>("votes").unwrap_or_default(),
        "submitted_at": row.try_get::<DateTime<Utc>, _>("submitted_at").ok().map(|v| v.to_rfc3339()),
        "my_vote": row.try_get::<bool, _>("my_vote").unwrap_or(false),
        "my_own": my_own,
    })
}

async fn current_submission_rows(
    pool: &PgPool,
    month: NaiveDate,
    my_discord_id: Option<&str>,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT s.id, s.twitch_clip_id, s.clip_url, s.clip_title,
                s.clip_thumbnail_url, s.broadcaster_login, s.broadcaster_name,
                s.submitter_person_key, s.submitted_at,
                COUNT(v.id)::bigint AS votes,
                COALESCE(BOOL_OR(v.voter_discord_id = $2), FALSE) AS my_vote
           FROM twitch_clip_contest_submissions s
           LEFT JOIN twitch_clip_contest_votes v ON v.submission_id = s.id
          WHERE s.contest_month = $1
            AND s.hidden_at IS NULL
          GROUP BY s.id, s.twitch_clip_id, s.clip_url, s.clip_title,
                   s.clip_thumbnail_url, s.broadcaster_login, s.broadcaster_name,
                   s.submitter_person_key, s.submitted_at
          ORDER BY COUNT(v.id) DESC, s.submitted_at ASC, s.id ASC",
    )
    .bind(month)
    .bind(my_discord_id.unwrap_or(""))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| submission_json(row, my_discord_id))
        .collect())
}

pub async fn current_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Response {
    let clock = contest_clock(Utc::now());
    ensure_due_months_finalized(&pool, clock.month).await;
    let discord = discord_session(&pool, &headers).await;
    let rows = match current_submission_rows(
        &pool,
        clock.month,
        discord.as_ref().map(|d| d.user_id.as_str()),
    )
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: aktuelle Clips konnten nicht geladen werden");
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip-Wettbewerb kann gerade nicht geladen werden.",
            );
        }
    };
    let top3: Vec<Value> = rows.iter().take(3).cloned().collect();
    let identity = submit_identity(&pool, &headers, &auth).await;
    no_store_json(json!({
        "month": clock.month.to_string(),
        "month_label": month_label(clock.month),
        "phase": clock.phase.as_str(),
        "phase_ends_at": clock.phase_ends_at.to_rfc3339(),
        "submissions": rows,
        "top3": top3,
        "authenticated": identity.is_some(),
        "discord_authenticated": discord.is_some(),
        "limits": {
            "submissions_per_month": MAX_SUBMISSIONS_PER_MONTH,
            "votes_per_month": MAX_VOTES_PER_MONTH,
        }
    }))
}

pub async fn archive_handler(State(pool): State<PgPool>) -> Response {
    let clock = contest_clock(Utc::now());
    ensure_due_months_finalized(&pool, clock.month).await;
    let rows = match sqlx::query(
        "SELECT h.contest_month, h.rank, h.vote_count,
                s.twitch_clip_id, s.clip_url, s.clip_title,
                s.clip_thumbnail_url, s.broadcaster_login, s.broadcaster_name
           FROM twitch_clip_contest_hall_of_fame h
           JOIN twitch_clip_contest_submissions s ON s.id = h.submission_id
          WHERE s.hidden_at IS NULL
          ORDER BY h.contest_month DESC, h.rank ASC",
    )
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Archiv konnte nicht geladen werden");
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "archive_unavailable",
                "Das Clip-Archiv kann gerade nicht geladen werden.",
            );
        }
    };

    let mut grouped: BTreeMap<NaiveDate, Vec<Value>> = BTreeMap::new();
    for row in rows {
        let Ok(month) = row.try_get::<NaiveDate, _>("contest_month") else {
            continue;
        };
        grouped.entry(month).or_default().push(json!({
            "rank": row.try_get::<i16, _>("rank").unwrap_or_default(),
            "votes": row.try_get::<i32, _>("vote_count").unwrap_or_default(),
            "clip_id": row.try_get::<String, _>("twitch_clip_id").unwrap_or_default(),
            "clip_url": row.try_get::<String, _>("clip_url").unwrap_or_default(),
            "title": row.try_get::<String, _>("clip_title").unwrap_or_default(),
            "thumbnail_url": row.try_get::<Option<String>, _>("clip_thumbnail_url").unwrap_or(None),
            "channel": row.try_get::<String, _>("broadcaster_login").unwrap_or_default(),
            "channel_name": row.try_get::<Option<String>, _>("broadcaster_name").unwrap_or(None),
        }));
    }
    let months: Vec<Value> = grouped
        .into_iter()
        .rev()
        .map(|(month, winners)| {
            json!({
                "month": month.to_string(),
                "month_label": month_label(month),
                "winners": winners,
            })
        })
        .collect();
    no_store_json(json!({ "months": months }))
}

pub async fn session_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Response {
    let clock = contest_clock(Utc::now());
    let discord = discord_session(&pool, &headers).await;
    let identity = submit_identity(&pool, &headers, &auth).await;
    let mut eligibility = None;
    let mut votes_used = 0i64;
    if let Some(ref discord) = discord {
        if let Ok(value) = discord_eligibility(&discord.user_id).await {
            eligibility = Some(value);
        }
        votes_used = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint
               FROM twitch_clip_contest_votes
              WHERE voter_discord_id = $1 AND contest_month = $2",
        )
        .bind(&discord.user_id)
        .bind(clock.month)
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
    }
    let submissions_used = if let Some(ref identity) = identity {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint
               FROM twitch_clip_contest_submissions
              WHERE submitter_person_key = $1 AND contest_month = $2",
        )
        .bind(&identity.person_key)
        .bind(clock.month)
        .fetch_one(&pool)
        .await
        .unwrap_or(0)
    } else {
        0
    };
    no_store_json(json!({
        "authenticated": identity.is_some(),
        "provider": identity.as_ref().map(|i| i.provider),
        "display_name": identity.as_ref().map(|i| i.display_name.as_str()),
        "discord_authenticated": discord.is_some(),
        "is_admin": auth.is_privileged(),
        "can_submit": identity.is_some() && clock.phase == Phase::Submission && submissions_used < MAX_SUBMISSIONS_PER_MONTH,
        "submissions_used": submissions_used,
        "submissions_limit": MAX_SUBMISSIONS_PER_MONTH,
        "can_vote": discord.is_some()
            && clock.phase == Phase::Voting
            && eligibility.as_ref().is_some_and(|e| e.account_age_ok && e.member_age_ok && e.present)
            && votes_used < MAX_VOTES_PER_MONTH,
        "votes_used": votes_used,
        "votes_limit": MAX_VOTES_PER_MONTH,
        "discord_eligibility": eligibility.map(|e| json!({
            "account_age_ok": e.account_age_ok,
            "member_age_ok": e.member_age_ok,
            "present": e.present,
            "joined_at": e.joined_at.map(|v| v.to_rfc3339()),
        })),
    }))
}

pub async fn submit_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(body): Json<SubmitBody>,
) -> Response {
    if !is_allowed_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage muss von der Clip-Seite kommen.",
        );
    }
    let clock = contest_clock(Utc::now());
    if clock.phase != Phase::Submission {
        return json_error(
            StatusCode::CONFLICT,
            "submission_closed",
            "Einreichungen sind nur vom 1. bis einschließlich 21. möglich.",
        );
    }
    let Some(identity) = submit_identity(&pool, &headers, &auth).await else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "login_required",
            "Zum Einreichen musst du mit Discord oder Twitch angemeldet sein.",
        );
    };
    let Some(clip_id) = clip_id_from_url(&body.clip_url) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_clip_url",
            "Bitte gib eine gültige Twitch-Clip-Adresse ein.",
        );
    };

    let used = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint
           FROM twitch_clip_contest_submissions
          WHERE contest_month = $1 AND submitter_person_key = $2",
    )
    .bind(clock.month)
    .bind(&identity.person_key)
    .fetch_one(&pool)
    .await
    .unwrap_or(MAX_SUBMISSIONS_PER_MONTH);
    if used >= MAX_SUBMISSIONS_PER_MONTH {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "submission_limit",
            "Du hast für diesen Monat bereits drei Clips eingereicht.",
        );
    }

    let clip = match fetch_helix_clip(&clip_id).await {
        Ok(clip) => clip,
        Err(response) => return response,
    };
    if clip.created_at < Utc::now() - Duration::days(MAX_CLIP_AGE_DAYS) {
        return json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "clip_too_old",
            "Der Clip darf beim Einreichen höchstens 60 Tage alt sein.",
        );
    }
    let deadlock_game_id = deadlock_game_id(&pool).await;
    if clip.game_id != deadlock_game_id {
        return json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "wrong_game",
            "Der Clip muss aus einem Deadlock-Stream stammen.",
        );
    }
    let Some(channel_login) = active_partner_login(&pool, &clip.broadcaster_id).await else {
        return json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "channel_not_active_partner",
            "Der Clip muss aus einem aktuell aktiven Partnerkanal der Community stammen.",
        );
    };

    let repository = ClipRepository::new(pool.clone());
    if let Err(error) = repository
        .ensure_monitored_streamer(&channel_login, &clip.broadcaster_id)
        .await
    {
        tracing::warn!(%error, "Clip-Wettbewerb: Streamer konnte nicht sichergestellt werden");
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Der Clip konnte gerade nicht gespeichert werden.",
        );
    }
    let record = ClipRecord {
        clip_id: clip.clip_id.clone(),
        clip_url: clip.url.clone(),
        clip_title: clip.title.clone(),
        thumbnail_url: clip.thumbnail_url.clone(),
        streamer_login: channel_login.clone(),
        twitch_user_id: clip.broadcaster_id.clone(),
        broadcaster_name: Some(clip.broadcaster_name.clone()),
        created_at: clip.created_at.to_rfc3339(),
        duration_seconds: clip.duration_seconds,
        view_count: clip.view_count,
        game_name: Some("Deadlock".to_string()),
        game_id: Some(clip.game_id.clone()),
        vod_id: clip.vod_id.clone(),
        vod_offset_s: clip.vod_offset_s,
    };
    let clip_db_id = match repository.register_clip(&record).await {
        Ok((id, _)) => id,
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: vorhandene Clip-Daten konnten nicht wiederverwendet werden");
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip konnte gerade nicht gespeichert werden.",
            );
        }
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip konnte gerade nicht gespeichert werden.",
            )
        }
    };
    let lock_key = format!(
        "clip_contest_submit:{}:{}",
        identity.person_key, clock.month
    );
    if sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Der Clip konnte gerade nicht gespeichert werden.",
        );
    }
    let used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
           FROM twitch_clip_contest_submissions
          WHERE contest_month = $1 AND submitter_person_key = $2",
    )
    .bind(clock.month)
    .bind(&identity.person_key)
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(MAX_SUBMISSIONS_PER_MONTH);
    if used >= MAX_SUBMISSIONS_PER_MONTH {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "submission_limit",
            "Du hast für diesen Monat bereits drei Clips eingereicht.",
        );
    }

    let inserted = sqlx::query(
        "INSERT INTO twitch_clip_contest_submissions
            (contest_month, clip_db_id, twitch_clip_id, clip_url, clip_title,
             clip_thumbnail_url, broadcaster_twitch_id, broadcaster_login,
             broadcaster_name, game_id, clip_created_at, submitter_provider,
             submitter_user_id, submitter_person_key, submitter_display_name)
         VALUES
            ($1,$2,$3,$4,$5,$6,$7,LOWER($8),$9,$10,$11,$12,$13,$14,$15)
         ON CONFLICT (contest_month, twitch_clip_id) DO NOTHING
         RETURNING id",
    )
    .bind(clock.month)
    .bind(clip_db_id)
    .bind(&clip.clip_id)
    .bind(&clip.url)
    .bind(&clip.title)
    .bind(clip.thumbnail_url.as_deref())
    .bind(&clip.broadcaster_id)
    .bind(&channel_login)
    .bind(&clip.broadcaster_name)
    .bind(&clip.game_id)
    .bind(clip.created_at)
    .bind(identity.provider)
    .bind(&identity.user_id)
    .bind(&identity.person_key)
    .bind(&identity.display_name)
    .fetch_optional(&mut *tx)
    .await;

    let submission_id: i64 = match inserted {
        Ok(Some(row)) => match row.try_get("id") {
            Ok(id) => id,
            Err(_) => {
                return json_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "contest_unavailable",
                    "Der Clip konnte gerade nicht gespeichert werden.",
                )
            }
        },
        Ok(None) => {
            return json_error(
                StatusCode::CONFLICT,
                "clip_already_submitted",
                "Dieser Clip ist für den laufenden Monat bereits eingereicht.",
            )
        }
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Einreichung fehlgeschlagen");
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Der Clip konnte gerade nicht gespeichert werden.",
            );
        }
    };
    if let Err(error) = emit_effort_event(
        &mut tx,
        "clip_submitted",
        &clip.broadcaster_id,
        &channel_login,
        2,
        &format!("clip-contest:submission:{submission_id}"),
        json!({
            "contest_month": clock.month.to_string(),
            "submission_id": submission_id,
            "twitch_clip_id": clip.clip_id,
        }),
    )
    .await
    {
        tracing::warn!(%error, "Clip-Wettbewerb: Punkte-Übergabe konnte nicht vorgemerkt werden");
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Der Clip konnte gerade nicht vollständig gespeichert werden.",
        );
    }
    if tx.commit().await.is_err() {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Der Clip konnte gerade nicht gespeichert werden.",
        );
    }
    no_store_json(json!({
        "ok": true,
        "submission_id": submission_id,
        "message": "Clip eingereicht.",
    }))
}

pub async fn vote_handler(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(body): Json<VoteBody>,
) -> Response {
    if !is_allowed_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage muss von der Clip-Seite kommen.",
        );
    }
    let clock = contest_clock(Utc::now());
    if clock.phase != Phase::Voting {
        return json_error(
            StatusCode::CONFLICT,
            "voting_closed",
            "Abgestimmt wird vom 22. bis zum Monatsende.",
        );
    }
    let Some(discord) = discord_session(&pool, &headers).await else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "discord_required",
            "Zum Abstimmen musst du mit Discord angemeldet sein.",
        );
    };
    let eligibility = match discord_eligibility(&discord.user_id).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !eligibility.account_age_ok {
        return json_error(
            StatusCode::FORBIDDEN,
            "account_too_new",
            "Dein Discord-Konto muss mindestens 30 Tage alt sein.",
        );
    }
    if !eligibility.present || !eligibility.member_age_ok {
        return json_error(
            StatusCode::FORBIDDEN,
            "member_too_new",
            "Du musst seit mindestens 7 Tagen Mitglied der Community sein.",
        );
    }
    if body.submission_id <= 0 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_submission",
            "Der Clip ist ungültig.",
        );
    }

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Stimme konnte gerade nicht gespeichert werden.",
            )
        }
    };
    let lock_key = format!("clip_contest_vote:{}:{}", discord.user_id, clock.month);
    if sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Die Stimme konnte gerade nicht gespeichert werden.",
        );
    }

    let submission = sqlx::query(
        "SELECT id, submitter_person_key
           FROM twitch_clip_contest_submissions
          WHERE id = $1 AND contest_month = $2 AND hidden_at IS NULL
          LIMIT 1",
    )
    .bind(body.submission_id)
    .bind(clock.month)
    .fetch_optional(&mut *tx)
    .await;
    let row = match submission {
        Ok(Some(row)) => row,
        Ok(None) => {
            return json_error(
                StatusCode::NOT_FOUND,
                "submission_not_found",
                "Dieser Clip steht in diesem Monat nicht zur Abstimmung.",
            )
        }
        Err(_) => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Stimme konnte gerade nicht gespeichert werden.",
            )
        }
    };
    let person_key: String = row.try_get("submitter_person_key").unwrap_or_default();
    if person_key == format!("discord:{}", discord.user_id) {
        return json_error(
            StatusCode::FORBIDDEN,
            "own_clip",
            "Für deinen eigenen Clip kannst du nicht abstimmen.",
        );
    }

    let votes_used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
           FROM twitch_clip_contest_votes
          WHERE voter_discord_id = $1 AND contest_month = $2",
    )
    .bind(&discord.user_id)
    .bind(clock.month)
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(MAX_VOTES_PER_MONTH);
    if votes_used >= MAX_VOTES_PER_MONTH {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "vote_limit",
            "Du hast deine fünf Stimmen für diesen Monat bereits vergeben.",
        );
    }

    let result = sqlx::query(
        "INSERT INTO twitch_clip_contest_votes
            (submission_id, voter_discord_id, contest_month)
         VALUES ($1, $2, $3)
         ON CONFLICT (submission_id, voter_discord_id) DO NOTHING",
    )
    .bind(body.submission_id)
    .bind(&discord.user_id)
    .bind(clock.month)
    .execute(&mut *tx)
    .await;
    let result = match result {
        Ok(result) => result,
        Err(_) => {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Stimme konnte gerade nicht gespeichert werden.",
            )
        }
    };
    if result.rows_affected() == 0 {
        return json_error(
            StatusCode::CONFLICT,
            "already_voted",
            "Für diesen Clip hast du bereits abgestimmt.",
        );
    }
    if tx.commit().await.is_err() {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "contest_unavailable",
            "Die Stimme konnte gerade nicht gespeichert werden.",
        );
    }
    no_store_json(json!({
        "ok": true,
        "votes_used": votes_used + 1,
        "votes_left": MAX_VOTES_PER_MONTH - votes_used - 1,
        "message": "Stimme gespeichert.",
    }))
}

fn broker_token() -> Option<String> {
    crate::handlers::discord_link::broker_token_from(uplink_config::platform_value)
}

async fn broker_post(path: &str, token: &str, payload: &Value) -> Option<Value> {
    let client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let url = format!("{}{}", DISCORD_BROKER_BASE.trim_end_matches('/'), path);
    let response = client
        .post(url)
        .header(BROKER_TOKEN_HEADER, token)
        .json(payload)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().await.ok()
}

pub async fn discord_login_handler() -> Response {
    let Some(token) = broker_token() else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "discord_login_unavailable",
            "Der Discord-Login ist gerade nicht verfügbar.",
        );
    };
    let public_origin = std::env::var("CLIP_CONTEST_PUBLIC_ORIGIN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://deutsche-deadlock-community.de".to_string());
    let callback = format!(
        "{}/clips/auth/discord/callback",
        public_origin.trim_end_matches('/')
    );
    let payload = json!({
        "scope": "identify",
        "redirect_after": callback,
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
    let Some(authorize_url) = data
        .get("authorize_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "discord_login_unavailable",
            "Der Discord-Login ist gerade nicht verfügbar.",
        );
    };
    Redirect::to(authorize_url).into_response()
}

#[derive(Debug, Deserialize, Default)]
pub struct DiscordCallbackQuery {
    #[serde(default)]
    state_id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub async fn discord_callback_handler(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DiscordCallbackQuery>,
) -> Response {
    if query.error.as_deref().is_some_and(|v| !v.trim().is_empty()) {
        return Redirect::to("/clips?login=abgebrochen").into_response();
    }
    let Some(state_id) = query
        .state_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return Redirect::to("/clips?login=fehler").into_response();
    };
    let Some(token) = broker_token() else {
        return Redirect::to("/clips?login=fehler").into_response();
    };
    let Some(data) = broker_post(
        BROKER_CONSUME_PATH,
        &token,
        &json!({ "state_id": state_id }),
    )
    .await
    else {
        return Redirect::to("/clips?login=fehler").into_response();
    };
    if data
        .pointer("/service_metadata/clip_contest")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Redirect::to("/clips?login=fehler").into_response();
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
        return Redirect::to("/clips?login=fehler").into_response();
    }

    let raw = tb_crypto::random_urlsafe_token(32);
    let hash = session_hash(&raw);
    if sqlx::query(
        "INSERT INTO twitch_clip_contest_sessions
            (session_hash, discord_user_id, discord_name, expires_at)
         VALUES ($1, $2, $3, now() + interval '30 days')",
    )
    .bind(hash)
    .bind(discord_id)
    .bind(discord_name)
    .execute(&pool)
    .await
    .is_err()
    {
        return Redirect::to("/clips?login=fehler").into_response();
    }
    let cookie = session_cookie(&raw, secure_request(&headers));
    let mut response = Redirect::to("/clips?login=discord").into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

pub async fn logout_handler(State(pool): State<PgPool>, headers: HeaderMap) -> Response {
    if !is_allowed_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage muss von der Clip-Seite kommen.",
        );
    }
    if let Some(raw) = cookie_values(&headers, SESSION_COOKIE).into_iter().next() {
        let _ = sqlx::query("DELETE FROM twitch_clip_contest_sessions WHERE session_hash = $1")
            .bind(session_hash(raw))
            .execute(&pool)
            .await;
    }
    let mut response = no_store_json(json!({ "ok": true }));
    if let Ok(value) = HeaderValue::from_str(&clear_session_cookie(secure_request(&headers))) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

pub async fn hide_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(submission_id): Path<i64>,
    Json(body): Json<HideBody>,
) -> Response {
    if !auth.is_privileged() {
        return json_error(
            StatusCode::FORBIDDEN,
            "admin_required",
            "Diese Aktion ist nur für Admins verfügbar.",
        );
    }
    if !is_allowed_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage muss von derselben Seite kommen.",
        );
    }
    let actor = match auth {
        DashboardAuthLevel::Admin { actor: Some(actor) } => actor.twitch_login,
        _ => "admin".to_string(),
    };
    let result = if body.hidden {
        sqlx::query(
            "UPDATE twitch_clip_contest_submissions
                SET hidden_at = COALESCE(hidden_at, now()), hidden_by = $2
              WHERE id = $1",
        )
        .bind(submission_id)
        .bind(actor)
        .execute(&pool)
        .await
    } else {
        sqlx::query(
            "UPDATE twitch_clip_contest_submissions
                SET hidden_at = NULL, hidden_by = NULL
              WHERE id = $1",
        )
        .bind(submission_id)
        .execute(&pool)
        .await
    };
    match result {
        Ok(result) if result.rows_affected() > 0 => no_store_json(json!({
            "ok": true,
            "hidden": body.hidden,
            "votes_preserved": true,
        })),
        Ok(_) => json_error(
            StatusCode::NOT_FOUND,
            "submission_not_found",
            "Der Clip wurde nicht gefunden.",
        ),
        Err(error) => {
            tracing::warn!(%error, "Clip-Wettbewerb: Moderation fehlgeschlagen");
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "contest_unavailable",
                "Die Moderation konnte gerade nicht gespeichert werden.",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phasen_sind_berlin_kalendertage() {
        let d20 = DateTime::parse_from_rfc3339("2026-09-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let d22 = DateTime::parse_from_rfc3339("2026-09-22T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(contest_clock(d20).phase, Phase::Submission);
        assert_eq!(contest_clock(d22).phase, Phase::Voting);
        assert_eq!(
            contest_clock(d22).month,
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
        );
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
        assert!(created.year() >= 2016);
        assert!(created.year() <= 2017);
    }

    #[test]
    fn monatswechsel_funktioniert_ueber_dezember() {
        assert_eq!(
            next_month(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }
}
