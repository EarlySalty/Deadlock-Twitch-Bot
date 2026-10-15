use sqlx::PgConnection;

use super::{hold_slots_on, Allocation, ScheduleError};

pub(crate) async fn queued_platforms_on(
    connection: &mut PgConnection,
    video: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT platform FROM twitch_clips_upload_queue
        WHERE produced_video_id = $1 AND platform IS NOT NULL AND scheduled_at IS NOT NULL
        AND COALESCE(status, '') NOT IN ('failed', 'cancelled', 'autopilot_discarded', 'autopilot_failed')
        ORDER BY platform",
    )
    .bind(video)
    .fetch_all(connection)
    .await
}

async fn move_uploads_on(
    connection: &mut PgConnection,
    video: i64,
    allocation: &Allocation,
) -> Result<(), sqlx::Error> {
    if allocation.slots.is_empty() {
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET scheduled_at = $2
            WHERE produced_video_id = $1 AND scheduled_at IS NOT NULL AND scheduled_at <> $2",
        )
        .bind(video)
        .bind(allocation.reference)
        .execute(&mut *connection)
        .await?;
        return Ok(());
    }
    for slot in &allocation.slots {
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET scheduled_at = $3
            WHERE produced_video_id = $1 AND platform = $2 AND scheduled_at IS NOT NULL
            AND scheduled_at <> $3",
        )
        .bind(video)
        .bind(&slot.platform)
        .bind(slot.scheduled_at)
        .execute(&mut *connection)
        .await?;
    }
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET scheduled_at = $3
        WHERE produced_video_id = $1 AND scheduled_at IS NOT NULL AND scheduled_at <> $3
        AND NOT (platform = ANY($2))",
    )
    .bind(video)
    .bind(
        allocation
            .slots
            .iter()
            .map(|slot| slot.platform.clone())
            .collect::<Vec<_>>(),
    )
    .bind(allocation.reference)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn place_video_on(
    connection: &mut PgConnection,
    owner: &str,
    video: i64,
    allocation: &Allocation,
) -> Result<(), ScheduleError> {
    let job: Option<i64> = sqlx::query_scalar(
        "UPDATE social_produced_videos SET scheduled_at = $3
        WHERE id = $1 AND twitch_user_id = $2 RETURNING job_id",
    )
    .bind(video)
    .bind(owner)
    .bind(allocation.reference)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(ScheduleError::NotFound)?;
    if let Some(job) = job {
        let active: bool = sqlx::query_scalar(
            "UPDATE social_production_jobs
            SET scheduled_at = CASE WHEN status = 'completed' THEN $3 ELSE scheduled_at END
            WHERE id = $1 AND twitch_user_id = $2
            RETURNING status IN ('queued', 'running', 'completed')",
        )
        .bind(job)
        .bind(owner)
        .bind(allocation.reference)
        .fetch_optional(&mut *connection)
        .await?
        .unwrap_or(false);
        let slots = if active {
            allocation.slots.as_slice()
        } else {
            &[]
        };
        hold_slots_on(connection, owner, job, Some(video), slots).await?;
    }
    move_uploads_on(connection, video, allocation).await?;
    Ok(())
}
