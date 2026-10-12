mod planner;
mod queue;
mod recovery;
mod settings;

pub use planner::{plan_owner, PlanReport};

pub use queue::{cancel, mark_next, reorder};
pub use recovery::{recover_after_lease, retry_or_review, Recovery};
pub use settings::{save_schedule, SchedulePatch, ScheduleSettings};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgConnection, PgPool};

use super::{from_row, JobError, JobSource, ProductionJob, ProductionMode, SELECT};
use crate::autopilot_settings::{self, AutopilotSettings, SettingsError};

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("Der Zeitplan ist ungültig: {0}")]
    Invalid(String),
    #[error("Der Produktionsauftrag wurde für deinen Kanal nicht gefunden")]
    NotFound,
    #[error("Die Warteschlange wurde inzwischen geändert. Bitte aktualisiere sie")]
    QueueChanged,
    #[error("Der Auftrag wurde bereits gestartet und kann nicht mehr geändert werden")]
    Started,
    #[error("Der Zeitplan konnte nicht gespeichert oder geladen werden")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Job(#[from] JobError),
    #[error("Die Zeitplandaten sind ungültig")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionHold {
    Disabled,
    Paused,
    ReviewBacklog,
    EmptyPool,
    NoSchedule,
    OutsideLead,
}

#[derive(Debug, Serialize)]
pub struct QueueJob {
    pub id: i64,
    pub clip_id: i64,
    pub clip_title: String,
    pub source: JobSource,
    pub mode: Option<ProductionMode>,
    pub requested_mode: Option<ProductionMode>,
    pub status: String,
    pub stage: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub review_reasons: Vec<String>,
}

impl QueueJob {
    fn from_job(job: ProductionJob, clip_title: String) -> Result<Self, ScheduleError> {
        Ok(Self {
            id: job.id,
            clip_id: job.clip_id,
            clip_title,
            source: job.source,
            mode: job.mode,
            requested_mode: job.requested_mode,
            status: job.status,
            stage: job.stage,
            attempts: job.attempts,
            max_attempts: job.max_attempts,
            scheduled_at: job.scheduled_at,
            started_at: job.started_at,
            last_error: job.last_error,
            review_reasons: serde_json::from_value(job.review_reasons)?,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ScheduleQueueSnapshot {
    pub settings: ScheduleSettings,
    pub persisted: bool,
    pub next_slot: Option<DateTime<Utc>>,
    pub jobs: Vec<QueueJob>,
    pub review_count: i64,
    pub production_hold: Option<ProductionHold>,
}

pub(crate) async fn review_count_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_produced_videos WHERE twitch_user_id = $1
        AND status = 'review' AND (expires_at IS NULL OR expires_at > clock_timestamp())",
    )
    .bind(owner)
    .fetch_one(connection)
    .await
}

pub(crate) async fn taken_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Vec<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar("SELECT scheduled_at FROM social_production_jobs WHERE twitch_user_id = $1
        AND scheduled_at IS NOT NULL AND status IN ('queued', 'running', 'completed')
        UNION SELECT scheduled_at FROM social_produced_videos WHERE twitch_user_id = $1 AND scheduled_at IS NOT NULL
        UNION SELECT q.scheduled_at FROM twitch_clips_upload_queue q
        JOIN twitch_clips_social_media c ON c.id = q.clip_id
        WHERE c.twitch_user_id = $1 AND q.scheduled_at IS NOT NULL
        AND COALESCE(q.status, '') NOT IN ('failed', 'cancelled')")
        .bind(owner).fetch_all(connection).await
}

pub(crate) fn production_horizon(
    settings: &AutopilotSettings,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    now + chrono::Duration::hours(i64::from(settings.lead_time_hours))
        .max(chrono::Duration::minutes(1))
}

pub(crate) fn hold(
    settings: &AutopilotSettings,
    reviews: i64,
    next: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<ProductionHold> {
    if !settings.enabled {
        return Some(ProductionHold::Disabled);
    }
    if settings.paused {
        return Some(ProductionHold::Paused);
    }
    if reviews >= 2 {
        return Some(ProductionHold::ReviewBacklog);
    }
    if settings.schedule_slots.is_empty() {
        return Some(ProductionHold::NoSchedule);
    }
    if next.is_none_or(|slot| slot > production_horizon(settings, now)) {
        return Some(ProductionHold::OutsideLead);
    }
    None
}

pub async fn snapshot(pool: &PgPool, owner: &str) -> Result<ScheduleQueueSnapshot, ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let persisted = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_media_autopilot_settings WHERE twitch_user_id = $1)",
    )
    .bind(owner)
    .fetch_one(&mut *tx)
    .await?;
    let now = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let next_slot =
        autopilot_settings::next_shared_slot(now, &taken_on(&mut tx, owner).await?, &settings)?;
    let review_count = review_count_on(&mut tx, owner).await?;
    let jobs = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE twitch_user_id = $1
        AND status NOT IN ('completed', 'cancelled')
        ORDER BY CASE WHEN status = 'running' THEN 0 WHEN status = 'queued' THEN 1 ELSE 2 END,
            queue_order, id LIMIT 500"
    )))
    .bind(owner)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(from_row)
    .collect::<Result<Vec<_>, _>>()?;
    let clip_ids: Vec<i64> = jobs.iter().map(|job| job.clip_id).collect();
    let titles: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id::bigint, COALESCE(clip_title, '') FROM twitch_clips_social_media
        WHERE twitch_user_id = $1 AND id = ANY($2)",
    )
    .bind(owner)
    .bind(&clip_ids)
    .fetch_all(&mut *tx)
    .await?;
    let titles: std::collections::HashMap<i64, String> = titles.into_iter().collect();
    let queued = jobs
        .into_iter()
        .map(|job| {
            let title = titles.get(&job.clip_id).cloned().unwrap_or_default();
            QueueJob::from_job(job, title)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut production_hold = hold(&settings, review_count, next_slot, now);
    if production_hold.is_none() {
        let empty: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_autopilot_schedule_state
            WHERE twitch_user_id = $1 AND production_hold = 'empty_pool' AND planned_at > $2 - INTERVAL '2 minutes')")
            .bind(owner).bind(now).fetch_one(&mut *tx).await?;
        if empty {
            production_hold = Some(ProductionHold::EmptyPool);
        }
    }
    tx.commit().await?;
    Ok(ScheduleQueueSnapshot {
        settings: ScheduleSettings::from(&settings),
        persisted,
        next_slot,
        jobs: queued,
        review_count,
        production_hold,
    })
}

#[cfg(test)]
mod tests;
