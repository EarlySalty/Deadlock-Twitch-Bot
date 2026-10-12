use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;

use super::ScheduleError;
use crate::production_jobs::{self, JobCompletion, ProductionJob};

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Recovery {
    pub resumed: u64,
    pub held: u64,
}

pub async fn recover_after_lease(pool: &PgPool) -> Result<Recovery, ScheduleError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(2026101110)")
        .execute(&mut *tx)
        .await?;
    let held = sqlx::query(
        "UPDATE social_production_jobs SET status = 'review', finished_at = NOW(),
        last_error = 'Die Produktion wurde unterbrochen und das Versuchslimit ist erreicht.',
        review_reasons = '[\"attempts_exhausted\"]'::jsonb
        WHERE status = 'running' AND attempts >= max_attempts",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let resumed = sqlx::query(
        "UPDATE social_production_jobs SET status = 'queued',
        last_error = 'Die Produktion wurde unterbrochen und wird fortgesetzt.',
        metadata = metadata || '{\"recovered\":true}'::jsonb
        WHERE status = 'running' AND attempts < max_attempts",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(Recovery { resumed, held })
}

pub async fn retry_or_review(
    pool: &PgPool,
    job: &ProductionJob,
    reason: &str,
    retryable: bool,
) -> Result<(), ScheduleError> {
    if !retryable || job.attempts >= job.max_attempts {
        let reasons = if job.attempts >= job.max_attempts {
            vec!["attempts_exhausted".into()]
        } else {
            vec![reason.into()]
        };
        production_jobs::finish(
            pool,
            &job.twitch_user_id,
            job.id,
            JobCompletion::Review,
            &reasons,
            Some("Die Produktion konnte nicht abgeschlossen werden. Bitte prüfe den Auftrag."),
        )
        .await?;
        return Ok(());
    }
    let retry_after = Utc::now() + Duration::minutes(i64::from(job.attempts.clamp(1, 10)));
    let changed = sqlx::query(
        "UPDATE social_production_jobs SET status = 'queued',
        last_error = 'Die Produktion wird nach einer kurzen Wartezeit erneut versucht.',
        metadata = metadata || $4::text::jsonb
        WHERE id = $1 AND twitch_user_id = $2 AND status = 'running' AND attempts = $3",
    )
    .bind(job.id)
    .bind(&job.twitch_user_id)
    .bind(job.attempts)
    .bind(serde_json::to_string(
        &json!({"retry_after":retry_after,"last_failure":reason}),
    )?)
    .execute(pool)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(ScheduleError::QueueChanged);
    }
    Ok(())
}
