use serde::Serialize;
use sqlx::{PgConnection, PgPool, Row};

use super::{planner::select_on, ScheduleError};
use crate::autopilot_settings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestRun {
    pub job_id: i64,
    pub clip_id: i64,
    pub clip_title: String,
    pub status: String,
    pub stage: String,
    pub mode: Option<String>,
    pub last_error: Option<String>,
    pub video_id: Option<i64>,
    pub video_status: Option<String>,
}

pub(super) async fn active_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Option<TestRun>, ScheduleError> {
    let row = sqlx::query(
        "SELECT j.id, j.clip_id, j.status, j.stage, j.mode, j.last_error,
            v.id AS video_id, v.status AS video_status,
            COALESCE((SELECT c.clip_title FROM twitch_clips_social_media c WHERE c.id = j.clip_id), '') AS clip_title
        FROM social_production_jobs j
        LEFT JOIN LATERAL (SELECT id, status FROM social_produced_videos
            WHERE job_id = j.id ORDER BY id DESC LIMIT 1) v ON TRUE
        WHERE j.twitch_user_id = $1 AND j.source = 'test'
        AND (j.status IN ('queued', 'running', 'review', 'failed')
            OR (j.status = 'completed' AND v.status IN ('rendering', 'review')))
        ORDER BY j.id DESC LIMIT 1",
    )
    .bind(owner)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(TestRun {
        job_id: row.try_get("id")?,
        clip_id: row.try_get("clip_id")?,
        clip_title: row.try_get("clip_title")?,
        status: row.try_get("status")?,
        stage: row.try_get("stage")?,
        mode: row.try_get("mode")?,
        last_error: row.try_get("last_error")?,
        video_id: row.try_get("video_id")?,
        video_status: row.try_get("video_status")?,
    }))
}

pub async fn start_test_run(pool: &PgPool, owner: &str) -> Result<TestRun, ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    if active_on(&mut tx, owner).await?.is_some() {
        return Err(ScheduleError::TestRunActive);
    }
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let now: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let window = crate::clip_pool::bounds(&settings, now)
        .map_err(|error| ScheduleError::Invalid(error.to_string()))?;
    let Some(clip) = select_on(
        &mut tx,
        owner,
        (window.start, window.end),
        settings.clip_sort,
    )
    .await?
    else {
        return Err(ScheduleError::EmptyPool);
    };
    sqlx::query(
        "INSERT INTO social_production_jobs (twitch_user_id, clip_id, source, queue_order)
        VALUES ($1, $2, 'test', COALESCE((SELECT MAX(queue_order) + 1 FROM social_production_jobs), 1))",
    )
    .bind(owner)
    .bind(clip)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE social_clip_pool SET selected_at = COALESCE(selected_at, $3),
            first_selected_at = COALESCE(first_selected_at, $3)
        WHERE twitch_user_id = $1 AND clip_db_id = $2",
    )
    .bind(owner)
    .bind(clip)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let run = active_on(&mut tx, owner)
        .await?
        .ok_or(ScheduleError::NotFound)?;
    tx.commit().await?;
    Ok(run)
}

pub async fn discard_test_run(
    pool: &PgPool,
    owner: &str,
    job_id: i64,
) -> Result<(), ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let row = sqlx::query(
        "SELECT status, clip_id FROM social_production_jobs
        WHERE id = $1 AND twitch_user_id = $2 AND source = 'test' FOR UPDATE",
    )
    .bind(job_id)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ScheduleError::NotFound)?;
    let status: String = row.try_get("status")?;
    let clip: i64 = row.try_get("clip_id")?;
    let videos: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM social_produced_videos WHERE job_id = $1
        AND status IN ('rendering', 'review') AND publication_executed_at IS NULL FOR UPDATE",
    )
    .bind(job_id)
    .fetch_all(&mut *tx)
    .await?;
    match status.as_str() {
        "running" => return Err(ScheduleError::Started),
        "cancelled" => return Err(ScheduleError::NotInReview),
        "completed" if videos.is_empty() => return Err(ScheduleError::NotInReview),
        _ => {}
    }
    sqlx::query(
        "UPDATE social_production_jobs SET status = 'cancelled', stage = 'test_discarded',
            finished_at = NOW(), scheduled_at = NULL WHERE id = $1",
    )
    .bind(job_id)
    .execute(&mut *tx)
    .await?;
    for video in videos {
        crate::autopilot_publish::release_slot(&mut tx, video).await?;
        sqlx::query("DELETE FROM social_media_clip_approval WHERE produced_video_id = $1")
            .bind(video)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE social_produced_videos SET status = 'rejected', expires_at = clock_timestamp(),
                needs_review = TRUE WHERE id = $1",
        )
        .bind(video)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE social_clip_pool SET selected_at = NULL,
            processed_at = CASE WHEN social_clip_video_holds(clip_db_id) THEN processed_at END
        WHERE twitch_user_id = $1 AND clip_db_id = $2",
    )
    .bind(owner)
    .bind(clip)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
