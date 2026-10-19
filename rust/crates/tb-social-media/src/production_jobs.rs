use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgConnection, PgPool, Row};

use crate::autopilot_settings::{self, json_column, SettingsError};

#[path = "autopilot_schedule/mod.rs"]
pub mod autopilot_schedule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProductionMode {
    Static,
    Ai,
}

impl ProductionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Ai => "ai",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, JobError> {
        match raw {
            "static" => Ok(Self::Static),
            "ai" => Ok(Self::Ai),
            _ => Err(JobError::Invalid("Unbekannter Produktionsmodus".into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobSource {
    Autopilot,
    Manual,
    Test,
}

impl JobSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Autopilot => "autopilot",
            Self::Manual => "manual",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("Ungültiger Produktionsauftrag: {0}")]
    Invalid(String),
    #[error("Der Clip oder Auftrag gehört nicht zu deinem Kanal")]
    Ownership,
    #[error("Der Produktionsauftrag befindet sich nicht im erwarteten Zustand")]
    State,
    #[error("Der Produktionsauftrag konnte nicht gespeichert oder geladen werden")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error("Die Auftragsdaten sind ungültig")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct NewProductionJob {
    pub twitch_user_id: String,
    pub clip_id: i64,
    pub source: JobSource,
    pub requested_mode: Option<ProductionMode>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub max_attempts: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductionJob {
    pub id: i64,
    pub twitch_user_id: String,
    pub clip_id: i64,
    pub source: JobSource,
    pub requested_mode: Option<ProductionMode>,
    pub mode: Option<ProductionMode>,
    pub queue_order: i64,
    pub status: String,
    pub stage: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub review_reasons: Value,
    pub metadata: Value,
    pub policy_snapshot: Option<autopilot_settings::PipelinePolicy>,
    pub last_error: Option<String>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

const SELECT: &str =
    "SELECT id, twitch_user_id, clip_id, source, requested_mode, mode, queue_order,
    status, stage, attempts, max_attempts, review_reasons::text, metadata::text, last_error,
    scheduled_at, created_at, started_at, finished_at, NULLIF(to_jsonb(social_production_jobs)->'policy_snapshot', 'null'::jsonb)::text AS policy_snapshot FROM social_production_jobs";

fn from_row(row: sqlx::postgres::PgRow) -> Result<ProductionJob, JobError> {
    let source: String = row.try_get("source")?;
    let requested: Option<String> = row.try_get("requested_mode")?;
    let mode: Option<String> = row.try_get("mode")?;
    Ok(ProductionJob {
        id: row.try_get("id")?,
        twitch_user_id: row.try_get("twitch_user_id")?,
        clip_id: row.try_get("clip_id")?,
        source: match source.as_str() {
            "autopilot" => JobSource::Autopilot,
            "manual" => JobSource::Manual,
            "test" => JobSource::Test,
            _ => return Err(JobError::Invalid("Unbekannte Auftragsquelle".into())),
        },
        requested_mode: requested
            .as_deref()
            .map(ProductionMode::parse)
            .transpose()?,
        mode: mode.as_deref().map(ProductionMode::parse).transpose()?,
        queue_order: row.try_get("queue_order")?,
        status: row.try_get("status")?,
        stage: row.try_get("stage")?,
        attempts: row.try_get("attempts")?,
        max_attempts: row.try_get("max_attempts")?,
        review_reasons: json_column(&row, "review_reasons")?,
        metadata: json_column(&row, "metadata")?,
        policy_snapshot: row
            .try_get::<Option<String>, _>("policy_snapshot")?
            .map(|raw| serde_json::from_str(&raw))
            .transpose()?,
        last_error: row.try_get("last_error")?,
        scheduled_at: row.try_get("scheduled_at")?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

pub async fn get(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
) -> Result<Option<ProductionJob>, JobError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE id = $1 AND twitch_user_id = $2"
    )))
    .bind(id)
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await?;
    row.map(from_row).transpose()
}

pub async fn list(
    pool: &PgPool,
    twitch_user_id: &str,
    limit: i64,
) -> Result<Vec<ProductionJob>, JobError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE twitch_user_id = $1 ORDER BY queue_order, id LIMIT $2"
    )))
    .bind(twitch_user_id)
    .bind(limit.clamp(0, 500))
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(from_row)
    .collect()
}

pub async fn insert(pool: &PgPool, job: &NewProductionJob) -> Result<ProductionJob, JobError> {
    if job.max_attempts <= 0
        || (job.source == JobSource::Manual && job.requested_mode.is_none())
        || (job.source != JobSource::Manual && job.requested_mode.is_some())
    {
        return Err(JobError::Invalid(
            "Ungültiger Modus oder Versuchslimit".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, &job.twitch_user_id).await?;
    let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_clips_social_media WHERE id = $1 AND twitch_user_id = $2)")
        .bind(job.clip_id).bind(&job.twitch_user_id).fetch_one(&mut *tx).await?;
    if !owned {
        return Err(JobError::Ownership);
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO social_production_jobs (twitch_user_id, clip_id, source, requested_mode, queue_order, scheduled_at, max_attempts)
         VALUES ($1, $2, $3, $4, COALESCE((SELECT MAX(queue_order) + 1 FROM social_production_jobs), 1), $5, $6) RETURNING id",
    ).bind(&job.twitch_user_id).bind(job.clip_id).bind(job.source.as_str())
        .bind(job.requested_mode.map(ProductionMode::as_str)).bind(job.scheduled_at).bind(job.max_attempts)
        .fetch_one(&mut *tx).await?;
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("{SELECT} WHERE id = $1")))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let job = from_row(row)?;
    tx.commit().await?;
    Ok(job)
}

async fn eligible_head_on(
    connection: &mut PgConnection,
) -> Result<Option<(i64, String)>, JobError> {
    let backlog = autopilot_schedule::review_backlog_sql("j.twitch_user_id");
    let limit = autopilot_schedule::REVIEW_LIMIT;
    let scheduled = autopilot_schedule::has_schedule_sql("s");
    let review = crate::autopilot_publish::EFFECTIVE_REVIEW;
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT j.id, j.twitch_user_id FROM social_production_jobs j WHERE j.status = 'queued'
         AND j.attempts < j.max_attempts
         AND COALESCE((j.metadata->>'retry_after')::timestamptz, '-infinity'::timestamptz) <= clock_timestamp()
         AND (j.started_at IS NOT NULL OR j.source IN ('manual', 'test') OR EXISTS (
             SELECT 1 FROM social_media_autopilot_settings s WHERE s.twitch_user_id = j.twitch_user_id
             AND s.enabled AND NOT s.paused AND {scheduled}
             AND (j.scheduled_at IS NULL OR (j.scheduled_at > clock_timestamp()
                 AND j.scheduled_at <= clock_timestamp() + make_interval(hours => s.lead_time_hours)
                     + CASE WHEN s.lead_time_hours = 0 THEN INTERVAL '1 minute' ELSE INTERVAL '0' END))
             AND ({review} <> 'required' OR {backlog} < {limit})))
         ORDER BY j.queue_order, j.id LIMIT 1"
    )))
    .fetch_optional(connection)
    .await?)
}

pub async fn claim_next(pool: &PgPool) -> Result<Option<ProductionJob>, JobError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(2026101110)")
        .execute(&mut *tx)
        .await?;
    let running: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_production_jobs WHERE status = 'running')",
    )
    .fetch_one(&mut *tx)
    .await?;
    if running {
        tx.commit().await?;
        return Ok(None);
    }
    let Some((_, locked_owner)) = eligible_head_on(&mut tx).await? else {
        tx.commit().await?;
        return Ok(None);
    };
    autopilot_settings::lock_owner(&mut tx, &locked_owner).await?;
    let Some((candidate_id, owner)) = eligible_head_on(&mut tx).await? else {
        tx.commit().await?;
        return Ok(None);
    };
    if owner != locked_owner {
        tx.commit().await?;
        return Ok(None);
    }
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE twitch_user_id = $1 AND id = $2 AND status = 'queued' AND attempts < max_attempts
         AND COALESCE((metadata->>'retry_after')::timestamptz, '-infinity'::timestamptz) <= clock_timestamp()
         FOR UPDATE"
    ))).bind(&owner).bind(candidate_id).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    let mut job = from_row(row)?;
    let settings = autopilot_settings::load_on(&mut tx, &owner).await?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    if job.started_at.is_none() && job.source == JobSource::Autopilot {
        let reviews: i64 = autopilot_schedule::review_count_on(&mut tx, &owner).await?;
        let scheduled = autopilot_schedule::has_schedule_on(&mut tx, &owner, &settings).await?;
        if !settings.enabled
            || settings.paused
            || !scheduled
            || (reviews >= autopilot_schedule::REVIEW_LIMIT
                && crate::autopilot_publish::review_mode_on(&mut tx, &owner).await?
                    == crate::autopilot_publish::ReviewMode::Review)
            || job.scheduled_at.is_some_and(|slot| {
                slot <= now || slot > autopilot_schedule::production_horizon(&settings, now)
            })
        {
            tx.commit().await?;
            return Ok(None);
        }
    }
    let (mode, policy) = if job.started_at.is_some() {
        (job.mode.ok_or(JobError::State)?, None)
    } else {
        match job.source {
            JobSource::Manual => (job.requested_mode.ok_or(JobError::State)?, None),
            JobSource::Autopilot | JobSource::Test => {
                automatic_mode(&mut tx, &owner, &settings).await?
            }
        }
    };
    if let Some(policy) = policy {
        sqlx::query(
            "UPDATE social_production_jobs SET policy_snapshot = $2::text::jsonb WHERE id = $1",
        )
        .bind(job.id)
        .bind(serde_json::to_string(&policy)?)
        .execute(&mut *tx)
        .await?;
        job.policy_snapshot = Some(policy);
    }
    let first_start = job.started_at.is_none();
    sqlx::query(
        "UPDATE social_production_jobs SET mode = $2, status = 'running',
        stage = CASE WHEN started_at IS NULL THEN 'starting' ELSE stage END,
        started_at = COALESCE(started_at, $3), attempts = attempts + 1,
        metadata = metadata - 'retry_after', finished_at = NULL WHERE id = $1",
    )
    .bind(job.id)
    .bind(mode.as_str())
    .bind(now)
    .execute(&mut *tx)
    .await?;
    job.mode = Some(mode);
    job.status = "running".into();
    if first_start {
        job.stage = "starting".into();
        job.started_at = Some(now);
    }
    if let Some(metadata) = job.metadata.as_object_mut() {
        metadata.remove("retry_after");
    }
    job.finished_at = None;
    job.attempts += 1;
    tx.commit().await?;
    Ok(Some(job))
}

async fn automatic_mode(
    tx: &mut PgConnection,
    owner: &str,
    settings: &autopilot_settings::AutopilotSettings,
) -> Result<(ProductionMode, Option<autopilot_settings::PipelinePolicy>), JobError> {
    let unified: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_pipeline_cutover WHERE twitch_user_id = $1 AND state = 'unified')",
    )
    .bind(owner)
    .fetch_one(&mut *tx)
    .await?;
    if !unified {
        return Ok(if settings.ai_editing_enabled {
            (ProductionMode::Ai, None)
        } else {
            (ProductionMode::Static, None)
        });
    }
    let policy = autopilot_settings::load_policy_on(tx, owner).await?;
    Ok(
        if policy.pipeline_mode == autopilot_settings::PipelineMode::AiEdit {
            (ProductionMode::Ai, Some(policy))
        } else {
            (ProductionMode::Static, None)
        },
    )
}

pub async fn update_progress(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
    stage: &str,
    metadata: &Value,
) -> Result<(), JobError> {
    if stage.trim().is_empty() || !metadata.is_object() {
        return Err(JobError::Invalid("Ungültiger Verarbeitungsschritt".into()));
    }
    let changed = sqlx::query("UPDATE social_production_jobs SET stage = $3, metadata = $4::text::jsonb WHERE id = $1 AND twitch_user_id = $2 AND status = 'running'")
        .bind(id).bind(twitch_user_id).bind(stage).bind(serde_json::to_string(metadata)?).execute(pool).await?.rows_affected();
    if changed != 1 {
        return Err(JobError::State);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum JobCompletion {
    Completed,
    Review,
    Failed,
}

pub async fn finish(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
    completion: JobCompletion,
    reasons: &[String],
    error: Option<&str>,
) -> Result<(), JobError> {
    let status = match completion {
        JobCompletion::Completed => "completed",
        JobCompletion::Review => "review",
        JobCompletion::Failed => "failed",
    };
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, twitch_user_id).await?;
    let changed = sqlx::query("UPDATE social_production_jobs SET status = $3, stage = $3, review_reasons = $4::text::jsonb, last_error = $5, finished_at = NOW() WHERE id = $1 AND twitch_user_id = $2 AND status = 'running'")
        .bind(id).bind(twitch_user_id).bind(status).bind(serde_json::to_string(reasons)?).bind(error)
        .execute(&mut *tx).await?.rows_affected();
    if changed != 1 {
        return Err(JobError::State);
    }
    if matches!(completion, JobCompletion::Review | JobCompletion::Failed) {
        release_unfinished_slots_on(&mut tx, twitch_user_id, id).await?;
    }
    if matches!(completion, JobCompletion::Failed) {
        release_pool_selection_on(&mut tx, twitch_user_id, id).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn release_unfinished_slots_on(
    connection: &mut PgConnection,
    twitch_user_id: &str,
    id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE social_produced_videos SET scheduled_at = NULL WHERE job_id = $1
        AND twitch_user_id = $2 AND status IN ('rendering', 'failed', 'rejected', 'expired')",
    )
    .bind(id)
    .bind(twitch_user_id)
    .execute(connection)
    .await?;
    Ok(())
}

async fn release_pool_selection_on(
    connection: &mut PgConnection,
    twitch_user_id: &str,
    id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE social_clip_pool p SET selected_at = NULL
        FROM social_production_jobs j
        WHERE j.id = $2 AND j.twitch_user_id = $1
        AND p.twitch_user_id = j.twitch_user_id AND p.clip_db_id = j.clip_id
        AND NOT social_clip_video_holds(p.clip_db_id)",
    )
    .bind(twitch_user_id)
    .bind(id)
    .execute(connection)
    .await?;
    Ok(())
}

pub const UNSUITABLE_REASON: &str = "unsuitable_clip";
const MAX_EXPLANATION_CHARS: usize = 1000;

pub async fn flag_unsuitable(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
    explanation: &str,
) -> Result<(), JobError> {
    let explanation = explanation.trim();
    if explanation.is_empty() || explanation.chars().count() > MAX_EXPLANATION_CHARS {
        return Err(JobError::Invalid("Ungültige Begründung".into()));
    }
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, twitch_user_id).await?;
    let changed = sqlx::query(
        "UPDATE social_production_jobs SET status = 'review', stage = 'review',
        review_reasons = jsonb_build_array($3::text), last_error = NULL, finished_at = NOW(),
        metadata = metadata || jsonb_build_object('review_explanation', $4::text)
        WHERE id = $1 AND twitch_user_id = $2 AND status = 'running'",
    )
    .bind(id)
    .bind(twitch_user_id)
    .bind(UNSUITABLE_REASON)
    .bind(explanation)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(JobError::State);
    }
    release_unfinished_slots_on(&mut tx, twitch_user_id, id).await?;
    tx.commit().await?;
    Ok(())
}
