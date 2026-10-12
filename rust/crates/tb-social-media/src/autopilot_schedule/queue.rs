use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};
use std::collections::HashSet;

use super::ScheduleError;
use crate::autopilot_settings;

async fn unstarted_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Vec<(i64, i64, String, Option<DateTime<Utc>>)>, ScheduleError> {
    Ok(sqlx::query_as(
        "SELECT id, queue_order, source, scheduled_at FROM social_production_jobs
        WHERE twitch_user_id = $1 AND status = 'queued' AND started_at IS NULL
        ORDER BY queue_order, id FOR UPDATE",
    )
    .bind(owner)
    .fetch_all(connection)
    .await?)
}

async fn reorder_on(
    connection: &mut PgConnection,
    owner: &str,
    ids: &[i64],
) -> Result<(), ScheduleError> {
    let jobs = unstarted_on(connection, owner).await?;
    let actual: HashSet<i64> = jobs.iter().map(|job| job.0).collect();
    let requested: HashSet<i64> = ids.iter().copied().collect();
    if requested.len() != ids.len() || requested != actual {
        return Err(ScheduleError::QueueChanged);
    }
    let mut slots: Vec<DateTime<Utc>> = jobs
        .iter()
        .filter(|job| job.2 == "autopilot")
        .filter_map(|job| job.3)
        .collect();
    slots.sort_unstable();
    let mut slots = slots.into_iter();
    sqlx::query("UPDATE social_production_jobs SET scheduled_at = NULL
        WHERE twitch_user_id = $1 AND status = 'queued' AND started_at IS NULL AND source = 'autopilot'")
        .bind(owner).execute(&mut *connection).await?;
    for (index, id) in ids.iter().enumerate() {
        let job = jobs
            .iter()
            .find(|job| job.0 == *id)
            .ok_or(ScheduleError::QueueChanged)?;
        let slot = if job.2 == "autopilot" {
            slots.next()
        } else {
            job.3
        };
        sqlx::query(
            "UPDATE social_production_jobs SET queue_order = $3, scheduled_at = $4
            WHERE id = $1 AND twitch_user_id = $2 AND status = 'queued' AND started_at IS NULL",
        )
        .bind(id)
        .bind(owner)
        .bind(jobs[index].1)
        .bind(slot)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub async fn reorder(pool: &PgPool, owner: &str, ids: &[i64]) -> Result<(), ScheduleError> {
    if ids.len() > 500 || ids.iter().any(|id| *id <= 0) {
        return Err(ScheduleError::QueueChanged);
    }
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    reorder_on(&mut tx, owner, ids).await?;
    tx.commit().await?;
    Ok(())
}

async fn require_unstarted(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
) -> Result<(), ScheduleError> {
    let state: Option<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT status, started_at FROM social_production_jobs
        WHERE id = $1 AND twitch_user_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(connection)
    .await?;
    match state {
        None => Err(ScheduleError::NotFound),
        Some((status, None)) if status == "queued" => Ok(()),
        Some(_) => Err(ScheduleError::Started),
    }
}

pub async fn mark_next(pool: &PgPool, owner: &str, id: i64) -> Result<(), ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    require_unstarted(&mut tx, owner, id).await?;
    let mut ids = vec![id];
    ids.extend(
        unstarted_on(&mut tx, owner)
            .await?
            .into_iter()
            .map(|job| job.0)
            .filter(|value| *value != id),
    );
    reorder_on(&mut tx, owner, &ids).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn cancel(pool: &PgPool, owner: &str, id: i64) -> Result<(), ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    require_unstarted(&mut tx, owner, id).await?;
    sqlx::query("UPDATE social_production_jobs SET status = 'cancelled', stage = 'cancelled',
        finished_at = NOW(), scheduled_at = NULL, metadata = metadata || '{\"selection_cancelled\":true}'::jsonb
        WHERE id = $1 AND twitch_user_id = $2")
        .bind(id).bind(owner).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
