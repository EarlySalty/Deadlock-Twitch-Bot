use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;

use super::ScheduleError;
use crate::{
    autopilot_settings,
    production_jobs::{self, JobCompletion, ProductionJob},
};

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Recovery {
    pub resumed: u64,
    pub failed: u64,
}

pub async fn recover_after_lease(pool: &PgPool) -> Result<Recovery, ScheduleError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(2026101110)")
        .execute(&mut *tx)
        .await?;
    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT twitch_user_id FROM social_production_jobs
        WHERE status = 'running' ORDER BY twitch_user_id",
    )
    .fetch_all(&mut *tx)
    .await?;
    for owner in owners {
        autopilot_settings::lock_owner(&mut tx, &owner).await?;
    }
    let failed = sqlx::query(
        "UPDATE social_production_jobs SET status = 'failed', stage = 'failed', finished_at = NOW(),
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
    Ok(Recovery { resumed, failed })
}

pub async fn retry_or_fail(
    pool: &PgPool,
    job: &ProductionJob,
    reason: &str,
    retryable: bool,
    detail: Option<&str>,
) -> Result<(), ScheduleError> {
    if job.attempts >= job.max_attempts {
        production_jobs::finish(
            pool,
            &job.twitch_user_id,
            job.id,
            JobCompletion::Failed,
            &["attempts_exhausted".into()],
            Some(&with_detail(
                detail,
                "Die Produktion ist nach mehreren Versuchen fehlgeschlagen.",
            )),
        )
        .await?;
        return Ok(());
    }
    if !retryable {
        production_jobs::finish(
            pool,
            &job.twitch_user_id,
            job.id,
            JobCompletion::Failed,
            &[reason.into()],
            Some(&with_detail(
                Some(detail.unwrap_or("Die Produktion konnte nicht abgeschlossen werden.")),
                "Der Autopilot macht mit dem nächsten Clip weiter, Testläufe kannst du erneut starten.",
            )),
        )
        .await?;
        return Ok(());
    }
    let retry_after = Utc::now() + Duration::minutes(i64::from(job.attempts.clamp(1, 10)));
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, &job.twitch_user_id).await?;
    let changed = sqlx::query(
        "UPDATE social_production_jobs SET status = 'queued',
        last_error = $5,
        metadata = metadata || $4::text::jsonb
        WHERE id = $1 AND twitch_user_id = $2 AND status = 'running' AND attempts = $3",
    )
    .bind(job.id)
    .bind(&job.twitch_user_id)
    .bind(job.attempts)
    .bind(serde_json::to_string(
        &json!({"retry_after":retry_after,"last_failure":reason}),
    )?)
    .bind(with_detail(
        detail,
        "Die Produktion wird nach einer kurzen Wartezeit erneut versucht.",
    ))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(ScheduleError::QueueChanged);
    }
    tx.commit().await?;
    Ok(())
}

fn with_detail(detail: Option<&str>, text: &str) -> String {
    match detail {
        Some(detail) => format!("{detail} {text}"),
        None => text.to_owned(),
    }
}
