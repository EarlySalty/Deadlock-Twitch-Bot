use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};

use super::{bounds, repository::manage_on, ImportWindow, PoolError};
use crate::autopilot_settings;

#[derive(Debug, Clone, Serialize)]
pub struct ImportJob {
    pub id: i64,
    pub twitch_user_id: String,
    pub clip_date_filter: crate::autopilot_settings::ClipDateFilter,
    pub timezone: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub status: String,
    pub raw_fetched: i64,
    pub eligible_fetched: i64,
    pub persisted_count: i64,
    pub completed_windows: i64,
    pub pending_windows: usize,
    pub active_window: Option<ImportWindow>,
    pub incomplete_windows: Vec<ImportWindow>,
    pub attempts: i32,
    pub total_attempts: i64,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub(crate) fn from_row(row: &sqlx::postgres::PgRow) -> Result<ImportJob, PoolError> {
    let pending: Vec<ImportWindow> = serde_json::from_value(row.try_get("pending_windows")?)?;
    let active: Option<ImportWindow> = row
        .try_get::<Option<serde_json::Value>, _>("active_window")?
        .map(serde_json::from_value)
        .transpose()?;
    Ok(ImportJob {
        id: row.try_get("id")?,
        twitch_user_id: row.try_get("twitch_user_id")?,
        clip_date_filter: serde_json::from_value(row.try_get("clip_date_filter")?)?,
        timezone: row.try_get("timezone")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
        status: row.try_get("status")?,
        raw_fetched: row.try_get("raw_fetched")?,
        eligible_fetched: row.try_get("eligible_fetched")?,
        persisted_count: row.try_get("persisted_count")?,
        completed_windows: row.try_get("completed_windows")?,
        pending_windows: pending.len(),
        active_window: active,
        incomplete_windows: serde_json::from_value(row.try_get("incomplete_windows")?)?,
        attempts: row.try_get("attempts")?,
        total_attempts: row.try_get("total_attempts")?,
        next_retry_at: row.try_get("next_retry_at")?,
        last_error: row.try_get("last_error")?,
        created_at: row.try_get("created_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}

pub async fn get_import(pool: &PgPool, owner: &str, id: i64) -> Result<ImportJob, PoolError> {
    let row =
        sqlx::query("SELECT * FROM social_clip_import_jobs WHERE twitch_user_id = $1 AND id = $2")
            .bind(owner)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or(PoolError::NotFound)?;
    from_row(&row)
}

pub async fn latest_import(pool: &PgPool, owner: &str) -> Result<Option<ImportJob>, PoolError> {
    sqlx::query(
        "SELECT * FROM social_clip_import_jobs WHERE twitch_user_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(owner)
    .fetch_optional(pool)
    .await?
    .as_ref()
    .map(from_row)
    .transpose()
}

pub async fn start_import(pool: &PgPool, owner: &str, login: &str) -> Result<ImportJob, PoolError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let now = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let window = bounds(&settings, now)?;
    if let Some(row) = sqlx::query("SELECT * FROM social_clip_import_jobs WHERE twitch_user_id = $1 AND status IN ('queued', 'running', 'retry')")
        .bind(owner).fetch_optional(&mut *tx).await? {
        return from_row(&row);
    }
    manage_on(&mut tx, owner).await?;
    let pending = if window.start < window.end {
        vec![window.clone()]
    } else {
        Vec::new()
    };
    let row = sqlx::query("INSERT INTO social_clip_import_jobs (twitch_user_id, streamer_login, clip_date_filter, timezone,
        started_at, ended_at, pending_windows, status, completed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, CASE WHEN $5 = $6 THEN 'completed' ELSE 'queued' END,
            CASE WHEN $5 = $6 THEN NOW() END) RETURNING *")
        .bind(owner).bind(login).bind(serde_json::to_value(&settings.clip_date_filter)?).bind(&settings.timezone)
        .bind(window.start).bind(window.end).bind(serde_json::to_value(pending)?).fetch_one(&mut *tx).await?;
    let job = from_row(&row)?;
    tx.commit().await?;
    Ok(job)
}

pub async fn resume_import(pool: &PgPool, owner: &str, id: i64) -> Result<ImportJob, PoolError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let row = sqlx::query("SELECT *, lease_until > clock_timestamp() AS leased FROM social_clip_import_jobs WHERE twitch_user_id = $1 AND id = $2 FOR UPDATE")
        .bind(owner).bind(id).fetch_optional(&mut *tx).await?.ok_or(PoolError::NotFound)?;
    let job = from_row(&row)?;
    if row.try_get::<Option<bool>, _>("leased")?.unwrap_or(false) {
        return Err(PoolError::Busy);
    }
    if matches!(
        job.status.as_str(),
        "completed" | "queued" | "running" | "retry"
    ) {
        return Ok(job);
    }
    let active_other: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_clip_import_jobs WHERE twitch_user_id = $1 AND id <> $2 AND status IN ('queued', 'running', 'retry'))")
        .bind(owner).bind(id).fetch_one(&mut *tx).await?;
    if active_other {
        return Err(PoolError::Busy);
    }
    let pending = job
        .incomplete_windows
        .iter()
        .map(|window| ImportWindow::new(window.start, window.end))
        .collect::<Vec<_>>();
    let restart_cursor = job
        .active_window
        .as_ref()
        .is_some_and(|window| window.cursor_restarts >= 3);
    let active = job.active_window.map(|window| {
        if restart_cursor {
            ImportWindow::new(window.start, window.end)
        } else {
            window
        }
    });
    let row = sqlx::query("UPDATE social_clip_import_jobs SET status = 'queued',
        pending_windows = pending_windows || $2, incomplete_windows = '[]', active_window = $3,
        cursor = CASE WHEN $4 THEN NULL ELSE cursor END,
        attempts = 0, lease_until = NULL, next_retry_at = NULL, last_error = NULL, completed_at = NULL,
        lease_generation = lease_generation + 1 WHERE id = $1 RETURNING *")
        .bind(id).bind(serde_json::to_value(pending)?).bind(active.map(serde_json::to_value).transpose()?)
        .bind(restart_cursor).fetch_one(&mut *tx).await?;
    let job = from_row(&row)?;
    tx.commit().await?;
    Ok(job)
}
