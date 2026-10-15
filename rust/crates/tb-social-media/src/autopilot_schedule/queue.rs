use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use std::collections::HashSet;

use super::{Allocation, ScheduleError, reservations_on, reserve_on};
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
    let mut allocations = Vec::new();
    for job in jobs.iter().filter(|job| job.2 == "autopilot") {
        if let Some(reference) = job.3 {
            let slots = reservations_on(connection, job.0).await?;
            allocations.extend([Allocation { reference, slots }]);
        }
    }
    allocations.sort_by_key(|allocation| allocation.reference);
    let mut allocations = allocations.into_iter();
    sqlx::query("UPDATE social_production_jobs SET scheduled_at = NULL
        WHERE twitch_user_id = $1 AND status = 'queued' AND started_at IS NULL AND source = 'autopilot'")
        .bind(owner).execute(&mut *connection).await?;
    for (index, id) in ids.iter().enumerate() {
        let job = jobs
            .iter()
            .find(|job| job.0 == *id)
            .ok_or(ScheduleError::QueueChanged)?;
        sqlx::query(
            "UPDATE social_production_jobs SET queue_order = $3
            WHERE id = $1 AND twitch_user_id = $2 AND status = 'queued' AND started_at IS NULL",
        )
        .bind(id)
        .bind(owner)
        .bind(jobs[index].1)
        .execute(&mut *connection)
        .await?;
        if job.2 == "autopilot" {
            reserve_on(connection, owner, *id, allocations.next().as_ref()).await?;
        }
    }
    Ok(())
}

pub async fn reorder(pool: &PgPool, owner: &str, ids: &[i64]) -> Result<(), ScheduleError> {
    if ids.len() > 500 || ids.iter().any(|id| *id <= 0) {
        return Err(ScheduleError::Invalid(
            "Die Reihenfolge enthält ungültige Aufträge".into(),
        ));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Process,
    Skip,
}

const MAX_NOTE_CHARS: usize = 500;

async fn clear_stale_slots_on(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
) -> Result<(), ScheduleError> {
    sqlx::query(
        "UPDATE social_produced_videos SET scheduled_at = NULL WHERE job_id = $1
        AND twitch_user_id = $2 AND status IN ('rendering', 'failed', 'rejected', 'expired')",
    )
    .bind(id)
    .bind(owner)
    .execute(connection)
    .await?;
    Ok(())
}

async fn require_review(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
) -> Result<(serde_json::Value, serde_json::Value), ScheduleError> {
    let state: Option<(String, String, String)> = sqlx::query_as(
        "SELECT status, review_reasons::text, metadata::text FROM social_production_jobs
        WHERE id = $1 AND twitch_user_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(connection)
    .await?;
    match state {
        None => Err(ScheduleError::NotFound),
        Some((status, reasons, metadata)) if status == "review" => Ok((
            serde_json::from_str(&reasons)?,
            serde_json::from_str(&metadata)?,
        )),
        Some(_) => Err(ScheduleError::NotInReview),
    }
}

fn normalized_note(note: Option<&str>) -> Result<Option<String>, ScheduleError> {
    let note = note.map(str::trim).filter(|value| !value.is_empty());
    if note.is_some_and(|value| value.chars().count() > MAX_NOTE_CHARS) {
        return Err(ScheduleError::Invalid(
            "Die Begründung darf höchstens 500 Zeichen lang sein".into(),
        ));
    }
    Ok(note.map(str::to_owned))
}

async fn discard_review_outputs_on(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
) -> Result<(), ScheduleError> {
    let videos: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM social_produced_videos WHERE job_id = $1 AND twitch_user_id = $2
        AND status IN ('rendering', 'review', 'approved', 'scheduled')
        AND publication_executed_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_all(&mut *connection)
    .await?;
    for video in videos {
        crate::autopilot_publish::release_slot(connection, video).await?;
        sqlx::query(
            "UPDATE social_produced_videos SET expires_at = clock_timestamp(), needs_review = TRUE,
            status = CASE WHEN mode = 'ai' OR status = 'rendering' THEN 'rejected' ELSE 'review' END
            WHERE id = $1",
        )
        .bind(video)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

fn retry_metadata(mut metadata: serde_json::Value) -> serde_json::Value {
    if let Some(object) = metadata.as_object_mut() {
        object.remove("retry_after");
        object.remove("static");
    }
    if let Some(editor) = metadata.get_mut("ai_editor") {
        let completed: HashSet<String> = editor
            .get("checkpoints")
            .and_then(serde_json::Value::as_object)
            .map(|checkpoints| checkpoints.keys().cloned().collect())
            .unwrap_or_default();
        if let Some(attempts) = editor
            .get_mut("attempts")
            .and_then(serde_json::Value::as_object_mut)
        {
            attempts.retain(|stage, _| completed.contains(stage));
        }
    }
    metadata
}

async fn skip_on(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
    decision: &serde_json::Value,
) -> Result<(), ScheduleError> {
    sqlx::query(
        "UPDATE social_production_jobs SET status = 'cancelled', stage = 'skipped',
        finished_at = NOW(), scheduled_at = NULL,
        metadata = metadata || jsonb_build_object('review_decision', $3::text::jsonb)
        WHERE id = $1 AND twitch_user_id = $2",
    )
    .bind(id)
    .bind(owner)
    .bind(serde_json::to_string(decision)?)
    .execute(&mut *connection)
    .await?;
    discard_review_outputs_on(connection, owner, id).await?;
    clear_stale_slots_on(connection, owner, id).await
}

async fn process_on(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
    decision: &serde_json::Value,
    metadata: serde_json::Value,
) -> Result<(), ScheduleError> {
    let produced: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_produced_videos WHERE job_id = $1
        AND status IN ('review', 'approved', 'scheduled', 'published'))",
    )
    .bind(id)
    .fetch_one(&mut *connection)
    .await?;
    if produced {
        return Err(ScheduleError::Invalid(
            "Für diesen Auftrag gibt es bereits ein fertiges Video".into(),
        ));
    }
    clear_stale_slots_on(connection, owner, id).await?;
    sqlx::query(
        "UPDATE social_production_jobs SET status = 'queued', stage = 'queued', attempts = 0,
        mode = NULL, started_at = NULL, finished_at = NULL, last_error = NULL,
        review_reasons = '[]'::jsonb, scheduled_at = CASE WHEN source = 'autopilot' THEN NULL ELSE scheduled_at END,
        queue_order = (SELECT COALESCE(MAX(queue_order), 0) + 1 FROM social_production_jobs),
        metadata = $4::text::jsonb || jsonb_build_object('review_decision', $3::text::jsonb)
        WHERE id = $1 AND twitch_user_id = $2",
    )
    .bind(id)
    .bind(owner)
    .bind(serde_json::to_string(decision)?)
    .bind(serde_json::to_string(&retry_metadata(metadata))?)
    .execute(&mut *connection)
    .await?;
    let mut ids = vec![id];
    ids.extend(
        unstarted_on(connection, owner)
            .await?
            .into_iter()
            .map(|job| job.0)
            .filter(|value| *value != id),
    );
    reorder_on(connection, owner, &ids).await
}

pub async fn resolve_review(
    pool: &PgPool,
    owner: &str,
    id: i64,
    action: ReviewDecision,
    note: Option<&str>,
) -> Result<(), ScheduleError> {
    let note = normalized_note(note)?;
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let (reasons, metadata) = require_review(&mut tx, owner, id).await?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let decision = json!({"action": action, "note": note, "reasons": reasons, "decided_at": now});
    match action {
        ReviewDecision::Skip => skip_on(&mut tx, owner, id, &decision).await?,
        ReviewDecision::Process => process_on(&mut tx, owner, id, &decision, metadata).await?,
    }
    tx.commit().await?;
    Ok(())
}
