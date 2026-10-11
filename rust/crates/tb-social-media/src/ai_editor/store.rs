use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{pool::PoolConnection, postgres::PgRow, PgPool, Postgres, Row};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiStoreError {
    #[error("Die Datenbank ist nicht verfügbar.")]
    Database(#[from] sqlx::Error),
    #[error("Ungültige Eingabe für den KI-Editor.")]
    InvalidInput,
    #[error("Der KI-Auftrag wurde nicht gefunden.")]
    NotFound,
    #[error("Der KI-Auftrag hat sich bereits geändert.")]
    Conflict,
    #[error("Die erlaubten Versuche für diesen Schritt sind aufgebraucht.")]
    AttemptsExhausted,
    #[error("Gespeicherte Auftragsdaten sind ungültig.")]
    InvalidStoredData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiStage {
    Preparing,
    Transcribing,
    Analyzing,
    Planning,
    Rendering,
    Validating,
}

impl AiStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Transcribing => "transcribing",
            Self::Analyzing => "analyzing",
            Self::Planning => "planning",
            Self::Rendering => "rendering",
            Self::Validating => "validating",
        }
    }

    pub fn next(self) -> Option<Self> {
        match self {
            Self::Preparing => Some(Self::Transcribing),
            Self::Transcribing => Some(Self::Analyzing),
            Self::Analyzing => Some(Self::Planning),
            Self::Planning => Some(Self::Rendering),
            Self::Rendering => Some(Self::Validating),
            Self::Validating => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AttemptLimit(i32);

impl AttemptLimit {
    pub fn from_server_config(value: i32) -> Result<Self, AiStoreError> {
        if !(1..=10).contains(&value) {
            return Err(AiStoreError::InvalidInput);
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TimelineEntry {
    pub start_ms: i64,
    pub end_ms: i64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", content = "result", rename_all = "snake_case")]
pub enum StageCheckpoint {
    Preparing {
        source_path: String,
        original_duration_ms: Option<i64>,
    },
    Transcribing {
        transcript: String,
        language: String,
    },
    Analyzing {
        highlights: Vec<TimelineEntry>,
        uncertainty: Vec<String>,
        analysis_notes: Vec<String>,
    },
    Planning {
        decisions: Vec<TimelineEntry>,
        review_reasons: Vec<String>,
    },
    Rendering {
        output_path: String,
        output_duration_ms: Option<i64>,
        rendering_ms: i64,
    },
    Validating {
        review_reasons: Vec<String>,
    },
}

impl StageCheckpoint {
    pub fn stage(&self) -> AiStage {
        match self {
            Self::Preparing { .. } => AiStage::Preparing,
            Self::Transcribing { .. } => AiStage::Transcribing,
            Self::Analyzing { .. } => AiStage::Analyzing,
            Self::Planning { .. } => AiStage::Planning,
            Self::Rendering { .. } => AiStage::Rendering,
            Self::Validating { .. } => AiStage::Validating,
        }
    }

    fn validate(&self) -> Result<String, AiStoreError> {
        let valid = match self {
            Self::Preparing {
                source_path,
                original_duration_ms,
            } => valid_path(source_path) && valid_duration(*original_duration_ms),
            Self::Transcribing {
                transcript,
                language,
            } => {
                !transcript.trim().is_empty()
                    && transcript.len() <= 48_000
                    && !language.trim().is_empty()
                    && language.len() <= 32
            }
            Self::Analyzing {
                highlights,
                uncertainty,
                analysis_notes,
            } => {
                valid_timeline(highlights)
                    && valid_labels(uncertainty)
                    && valid_labels(analysis_notes)
            }
            Self::Planning {
                decisions,
                review_reasons,
            } => valid_timeline(decisions) && valid_labels(review_reasons),
            Self::Rendering {
                output_path,
                output_duration_ms,
                rendering_ms,
            } => {
                valid_path(output_path) && valid_duration(*output_duration_ms) && *rendering_ms >= 0
            }
            Self::Validating { review_reasons } => valid_labels(review_reasons),
        };
        if !valid {
            return Err(AiStoreError::InvalidInput);
        }
        let json = serde_json::to_string(self).map_err(|_| AiStoreError::InvalidInput)?;
        if json.len() > 65_536 {
            return Err(AiStoreError::InvalidInput);
        }
        Ok(json)
    }
}

fn valid_path(path: &str) -> bool {
    !path.trim().is_empty() && path.len() <= 4096 && !path.contains('\0')
}

fn valid_duration(value: Option<i64>) -> bool {
    value.is_none_or(|value| value >= 0)
}

fn valid_labels(values: &[String]) -> bool {
    values.len() <= 64
        && values
            .iter()
            .all(|value| !value.trim().is_empty() && value.len() <= 512)
}

fn valid_timeline(values: &[TimelineEntry]) -> bool {
    values.len() <= 64
        && values.iter().all(|entry| {
            entry.start_ms >= 0
                && entry.end_ms > entry.start_ms
                && !entry.description.trim().is_empty()
                && entry.description.len() <= 512
        })
}

#[derive(Debug, Clone, Default)]
pub struct JobMetrics {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub processing_ms: Option<i64>,
    pub rendering_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub estimated_provider_cost_usd: Option<f64>,
    pub measured_provider_cost_usd: Option<f64>,
}

impl JobMetrics {
    fn validate(&self) -> Result<(), AiStoreError> {
        let names_valid = [&self.provider, &self.model].iter().all(|name| {
            name.as_ref()
                .is_none_or(|name| !name.trim().is_empty() && name.len() <= 128)
        });
        let counts_valid = [
            self.processing_ms,
            self.rendering_ms,
            self.input_tokens,
            self.output_tokens,
        ]
        .iter()
        .all(|count| count.is_none_or(|count| count >= 0));
        let costs_valid = [
            self.estimated_provider_cost_usd,
            self.measured_provider_cost_usd,
        ]
        .iter()
        .all(|cost| cost.is_none_or(|cost| cost.is_finite() && cost >= 0.0));
        if names_valid && counts_valid && costs_valid {
            Ok(())
        } else {
            Err(AiStoreError::InvalidInput)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiVideoVersion {
    pub id: i64,
    pub version_no: i32,
    pub created_at: DateTime<Utc>,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub file_url: Option<String>,
    pub parent_version_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiJobView {
    pub id: i64,
    pub clip_db_id: i64,
    pub status: String,
    pub stage: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub original_duration_ms: Option<i64>,
    pub output_duration_ms: Option<i64>,
    pub processing_ms: Option<i64>,
    pub rendering_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub estimated_provider_cost_usd: Option<f64>,
    pub measured_provider_cost_usd: Option<f64>,
    pub cost_basis: String,
    pub highlights: Vec<TimelineEntry>,
    pub decisions: Vec<TimelineEntry>,
    pub uncertainty: Vec<String>,
    pub review_reasons: Vec<String>,
    pub original_url: Option<String>,
    pub versions: Vec<AiVideoVersion>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub can_retry: bool,
}

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: i64,
    pub clip_db_id: i64,
    pub status: String,
    pub stage: AiStage,
    pub attempts_for_stage: i32,
    pub checkpoints: BTreeMap<AiStage, StageCheckpoint>,
}

pub struct ProductionLease {
    connection: PoolConnection<Postgres>,
}

impl ProductionLease {
    pub async fn release(mut self) -> Result<(), AiStoreError> {
        let unlocked: bool = sqlx::query_scalar(
            "SELECT pg_advisory_unlock(hashtextextended('social_media_video_production', 0))",
        )
        .fetch_one(&mut *self.connection)
        .await?;
        self.connection.close().await?;
        if unlocked {
            Ok(())
        } else {
            Err(AiStoreError::Conflict)
        }
    }
}

pub async fn acquire_production_lease(
    pool: &PgPool,
) -> Result<Option<ProductionLease>, AiStoreError> {
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    let acquired: bool = sqlx::query_scalar(
        "SELECT pg_try_advisory_lock(hashtextextended('social_media_video_production', 0))",
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(acquired.then_some(ProductionLease { connection }))
}

fn numeric_owner(twitch_user_id: &str) -> Result<i64, AiStoreError> {
    if twitch_user_id.is_empty()
        || !twitch_user_id.bytes().all(|byte| byte.is_ascii_digit())
        || twitch_user_id.starts_with('0')
    {
        return Err(AiStoreError::InvalidInput);
    }
    twitch_user_id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or(AiStoreError::InvalidInput)
}

pub async fn get_user_consent(pool: &PgPool, twitch_user_id: &str) -> Result<bool, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let value: Option<bool> = sqlx::query_scalar(
        "SELECT external_ai_consent FROM social_ai_user_consent WHERE twitch_user_id = $1",
    )
    .bind(owner.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(value.unwrap_or(false))
}

pub async fn upsert_user_consent(
    pool: &PgPool,
    twitch_user_id: &str,
    external_ai_consent: bool,
    updated_by: &str,
) -> Result<bool, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    if updated_by.trim().is_empty() || updated_by.len() > 128 {
        return Err(AiStoreError::InvalidInput);
    }
    sqlx::query(
        "INSERT INTO social_ai_user_consent (twitch_user_id, external_ai_consent, updated_at, updated_by) \
         VALUES ($1, $2, NOW(), $3) ON CONFLICT (twitch_user_id) DO UPDATE \
         SET external_ai_consent = EXCLUDED.external_ai_consent, updated_at = NOW(), updated_by = EXCLUDED.updated_by",
    )
    .bind(owner.to_string())
    .bind(external_ai_consent)
    .bind(updated_by)
    .execute(pool)
    .await?;
    Ok(external_ai_consent)
}

fn parse_json<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AiStoreError> {
    serde_json::from_str(value).map_err(|_| AiStoreError::InvalidStoredData)
}

fn view_from_row(row: &PgRow, limit: AttemptLimit) -> Result<AiJobView, AiStoreError> {
    let attempts: BTreeMap<String, i32> = parse_json(row.try_get("attempts_text")?)?;
    let status: String = row.try_get("status")?;
    let stage: String = row.try_get("stage")?;
    let measured_provider_cost_usd: Option<f64> = row.try_get("measured_provider_cost_usd")?;
    let estimated_provider_cost_usd: Option<f64> = row.try_get("estimated_provider_cost_usd")?;
    let cost_basis = if measured_provider_cost_usd.is_some() {
        "provider_billed"
    } else if estimated_provider_cost_usd.is_some() {
        "usage_priced_estimate"
    } else {
        "unknown"
    };
    let active_sibling: bool = row.try_get("active_sibling")?;
    let can_retry = status == "failed"
        && !active_sibling
        && attempts.get(&stage).copied().unwrap_or(0) < limit.0;
    Ok(AiJobView {
        id: row.try_get("id")?,
        clip_db_id: row.try_get("clip_db_id")?,
        status,
        stage,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        completed_at: row.try_get("completed_at")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        original_duration_ms: row.try_get("original_duration_ms")?,
        output_duration_ms: row.try_get("output_duration_ms")?,
        processing_ms: row.try_get("processing_ms")?,
        rendering_ms: row.try_get("rendering_ms")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        estimated_provider_cost_usd,
        measured_provider_cost_usd,
        cost_basis: cost_basis.to_string(),
        highlights: parse_json(row.try_get("highlights_text")?)?,
        decisions: parse_json(row.try_get("decisions_text")?)?,
        uncertainty: parse_json(row.try_get("uncertainty_text")?)?,
        review_reasons: parse_json(row.try_get("review_reasons_text")?)?,
        original_url: None,
        versions: Vec::new(),
        failure_code: row.try_get("failure_code")?,
        failure_message: row.try_get("failure_message")?,
        can_retry,
    })
}

async fn load_versions(pool: &PgPool, owner: i64, job: &mut AiJobView) -> Result<(), AiStoreError> {
    let rows = sqlx::query(
        "SELECT p.id, p.version_no, p.created_at, p.status, p.duration_ms, p.parent_version_id \
         FROM social_produced_videos p JOIN social_ai_jobs j ON j.twitch_user_id = $1 \
         AND j.id = $2 AND j.clip_db_id = p.clip_id \
         WHERE p.twitch_user_id = $3 AND p.mode = 'ai' \
         AND p.metadata ->> 'manual_ai_job_id' = j.id::text \
         ORDER BY p.version_no, p.id LIMIT 50",
    )
    .bind(owner)
    .bind(job.id)
    .bind(owner.to_string())
    .fetch_all(pool)
    .await?;
    job.versions = rows
        .iter()
        .map(|row| {
            Ok(AiVideoVersion {
                id: row.try_get("id")?,
                version_no: row.try_get("version_no")?,
                created_at: row.try_get("created_at")?,
                status: row.try_get("status")?,
                duration_ms: row.try_get("duration_ms")?,
                file_url: None,
                parent_version_id: row.try_get("parent_version_id")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()?;
    Ok(())
}

pub async fn get_job(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: i64,
    limit: AttemptLimit,
) -> Result<AiJobView, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    if job_id <= 0 {
        return Err(AiStoreError::InvalidInput);
    }
    let row = sqlx::query(
        "SELECT j.*, j.highlights::text AS highlights_text, j.decisions::text AS decisions_text, \
         j.uncertainty::text AS uncertainty_text, j.review_reasons::text AS review_reasons_text, \
         j.attempts::text AS attempts_text, \
         EXISTS (SELECT 1 FROM social_ai_jobs sibling WHERE sibling.twitch_user_id = j.twitch_user_id \
         AND sibling.clip_db_id = j.clip_db_id AND sibling.id <> j.id \
         AND sibling.status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating')) AS active_sibling \
         FROM social_ai_jobs j WHERE j.id = $1 AND j.twitch_user_id = $2",
    )
    .bind(job_id)
    .bind(owner)
    .fetch_optional(pool)
    .await?
    .ok_or(AiStoreError::NotFound)?;
    let mut view = view_from_row(&row, limit)?;
    load_versions(pool, owner, &mut view).await?;
    Ok(view)
}

pub async fn list_jobs(
    pool: &PgPool,
    twitch_user_id: &str,
    requested_limit: i64,
    before_id: Option<i64>,
    limit: AttemptLimit,
) -> Result<Vec<AiJobView>, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    if requested_limit <= 0 || before_id.is_some_and(|id| id <= 0) {
        return Err(AiStoreError::InvalidInput);
    }
    let rows = sqlx::query(
        "SELECT j.*, j.highlights::text AS highlights_text, j.decisions::text AS decisions_text, \
         j.uncertainty::text AS uncertainty_text, j.review_reasons::text AS review_reasons_text, \
         j.attempts::text AS attempts_text, \
         EXISTS (SELECT 1 FROM social_ai_jobs sibling WHERE sibling.twitch_user_id = j.twitch_user_id \
         AND sibling.clip_db_id = j.clip_db_id AND sibling.id <> j.id \
         AND sibling.status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating')) AS active_sibling \
         FROM social_ai_jobs j WHERE j.twitch_user_id = $1 \
         AND ($2::bigint IS NULL OR j.id < $2) ORDER BY j.id DESC LIMIT $3",
    )
    .bind(owner)
    .bind(before_id)
    .bind(requested_limit.min(50))
    .fetch_all(pool)
    .await?;
    let mut jobs = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut view = view_from_row(row, limit)?;
        load_versions(pool, owner, &mut view).await?;
        jobs.push(view);
    }
    Ok(jobs)
}

pub async fn create_job(
    pool: &PgPool,
    twitch_user_id: &str,
    clip_db_id: i64,
    limit: AttemptLimit,
) -> Result<AiJobView, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    if clip_db_id <= 0 {
        return Err(AiStoreError::InvalidInput);
    }
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO social_ai_jobs (twitch_user_id, clip_db_id) \
         SELECT $1, c.id FROM twitch_clips_social_media c \
         WHERE c.id = $2 AND c.twitch_user_id = $3 \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         ON CONFLICT (twitch_user_id, clip_db_id) \
         WHERE status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating') \
         DO UPDATE SET updated_at = social_ai_jobs.updated_at RETURNING id",
    )
    .bind(owner)
    .bind(clip_db_id)
    .bind(owner.to_string())
    .fetch_optional(pool)
    .await?;
    let id = id.ok_or(AiStoreError::NotFound)?;
    get_job(pool, twitch_user_id, id, limit).await
}

pub async fn load_job_for_runner(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
) -> Result<JobSnapshot, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let row = sqlx::query(
        "SELECT id, clip_db_id, status, stage, attempts::text AS attempts_text, \
         checkpoints::text AS checkpoints_text FROM social_ai_jobs \
         WHERE id = $1 AND twitch_user_id = $2",
    )
    .bind(job_id)
    .bind(owner)
    .fetch_optional(&mut *lease.connection)
    .await?
    .ok_or(AiStoreError::NotFound)?;
    let stage_name: String = row.try_get("stage")?;
    let stage = stage_from_str(&stage_name)?;
    let attempts: BTreeMap<String, i32> = parse_json(row.try_get("attempts_text")?)?;
    let checkpoints: BTreeMap<AiStage, StageCheckpoint> =
        parse_json(row.try_get("checkpoints_text")?)?;
    if checkpoints
        .iter()
        .any(|(key, checkpoint)| *key != checkpoint.stage())
    {
        return Err(AiStoreError::InvalidStoredData);
    }
    Ok(JobSnapshot {
        id: row.try_get("id")?,
        clip_db_id: row.try_get("clip_db_id")?,
        status: row.try_get("status")?,
        stage,
        attempts_for_stage: attempts.get(&stage_name).copied().unwrap_or(0),
        checkpoints,
    })
}

fn stage_from_str(value: &str) -> Result<AiStage, AiStoreError> {
    match value {
        "preparing" => Ok(AiStage::Preparing),
        "transcribing" => Ok(AiStage::Transcribing),
        "analyzing" => Ok(AiStage::Analyzing),
        "planning" => Ok(AiStage::Planning),
        "rendering" => Ok(AiStage::Rendering),
        "validating" => Ok(AiStage::Validating),
        _ => Err(AiStoreError::InvalidStoredData),
    }
}

pub async fn claim_job(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
    limit: AttemptLimit,
) -> Result<(), AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let claimed: Option<i64> = sqlx::query_scalar(
        "UPDATE social_ai_jobs SET status = stage, \
         attempts = jsonb_set(attempts, ARRAY[stage], \
         to_jsonb(COALESCE((attempts ->> stage)::integer, 0) + 1)), \
         started_at = COALESCE(started_at, NOW()), updated_at = NOW(), \
         failure_code = NULL, failure_message = NULL \
         WHERE id = $1 AND twitch_user_id = $2 AND status = 'queued' \
         AND COALESCE((attempts ->> stage)::integer, 0) < $3 \
         RETURNING id",
    )
    .bind(job_id)
    .bind(owner)
    .bind(limit.0)
    .fetch_optional(&mut *lease.connection)
    .await?;
    if claimed.is_some() {
        Ok(())
    } else {
        Err(AiStoreError::Conflict)
    }
}

pub async fn save_stage(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
    checkpoint: &StageCheckpoint,
    metrics: Option<&JobMetrics>,
) -> Result<(), AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let json = checkpoint.validate()?;
    if let Some(metrics) = metrics {
        metrics.validate()?;
    }
    let stage = checkpoint.stage();
    let next = stage.next().unwrap_or(stage);
    let highlights = match checkpoint {
        StageCheckpoint::Analyzing { highlights, .. } => Some(highlights),
        _ => None,
    };
    let decisions = match checkpoint {
        StageCheckpoint::Planning { decisions, .. } => Some(decisions),
        _ => None,
    };
    let uncertainty = match checkpoint {
        StageCheckpoint::Analyzing { uncertainty, .. } => Some(uncertainty),
        _ => None,
    };
    let review_reasons = match checkpoint {
        StageCheckpoint::Planning { review_reasons, .. }
        | StageCheckpoint::Validating { review_reasons } => Some(review_reasons),
        _ => None,
    };
    let original_duration_ms = match checkpoint {
        StageCheckpoint::Preparing {
            original_duration_ms,
            ..
        } => *original_duration_ms,
        _ => None,
    };
    let (output_duration_ms, stage_rendering_ms) = match checkpoint {
        StageCheckpoint::Rendering {
            output_duration_ms,
            rendering_ms,
            ..
        } => (*output_duration_ms, Some(*rendering_ms)),
        _ => (None, None),
    };
    let highlights = highlights
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| AiStoreError::InvalidInput)?;
    let decisions = decisions
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| AiStoreError::InvalidInput)?;
    let uncertainty = uncertainty
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| AiStoreError::InvalidInput)?;
    let review_reasons = review_reasons
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| AiStoreError::InvalidInput)?;
    let saved: Option<i64> = sqlx::query_scalar(
        "UPDATE social_ai_jobs SET \
         checkpoints = jsonb_set(checkpoints, ARRAY[$3::text], $4::jsonb), \
         stage = $5, status = CASE WHEN $3 = 'validating' THEN 'validating' ELSE 'queued' END, \
         highlights = COALESCE($6::jsonb, highlights), \
         decisions = COALESCE($7::jsonb, decisions), \
         uncertainty = COALESCE($8::jsonb, uncertainty), \
         review_reasons = CASE WHEN $3 = 'validating' THEN review_reasons || $9::jsonb \
             ELSE COALESCE($9::jsonb, review_reasons) END, \
         original_duration_ms = COALESCE($10::bigint, original_duration_ms), \
         output_duration_ms = COALESCE($11::bigint, output_duration_ms), \
         rendering_ms = COALESCE($21::bigint, $12::bigint, rendering_ms), \
         provider = CASE WHEN $13 THEN $14 ELSE provider END, \
         model = CASE WHEN $13 THEN $15 ELSE model END, \
         processing_ms = CASE WHEN $13 THEN $16::bigint ELSE processing_ms END, \
         input_tokens = CASE WHEN $13 THEN $17::bigint ELSE input_tokens END, \
         output_tokens = CASE WHEN $13 THEN $18::bigint ELSE output_tokens END, \
         estimated_provider_cost_usd = CASE WHEN $13 THEN $19::double precision ELSE estimated_provider_cost_usd END, \
         measured_provider_cost_usd = CASE WHEN $13 THEN $20::double precision ELSE measured_provider_cost_usd END, \
         updated_at = NOW() \
         WHERE id = $1 AND twitch_user_id = $2 AND stage = $3 AND status = $3 \
         AND NOT checkpoints ? $3 RETURNING id",
    )
    .bind(job_id)
    .bind(owner)
    .bind(stage.as_str())
    .bind(json)
    .bind(next.as_str())
    .bind(highlights)
    .bind(decisions)
    .bind(uncertainty)
    .bind(review_reasons)
    .bind(original_duration_ms)
    .bind(output_duration_ms)
    .bind(stage_rendering_ms)
    .bind(metrics.is_some())
    .bind(metrics.and_then(|value| value.provider.as_deref()))
    .bind(metrics.and_then(|value| value.model.as_deref()))
    .bind(metrics.and_then(|value| value.processing_ms))
    .bind(metrics.and_then(|value| value.input_tokens))
    .bind(metrics.and_then(|value| value.output_tokens))
    .bind(metrics.and_then(|value| value.estimated_provider_cost_usd))
    .bind(metrics.and_then(|value| value.measured_provider_cost_usd))
    .bind(metrics.and_then(|value| value.rendering_ms))
    .fetch_optional(&mut *lease.connection)
    .await?;
    if saved.is_some() {
        Ok(())
    } else {
        Err(AiStoreError::Conflict)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum JobFailure {
    SourceUnavailable,
    TranscriptionFailed,
    AnalysisFailed,
    PlanningFailed,
    RenderingFailed,
    ValidationFailed,
    ProviderUnavailable,
    BudgetExceeded,
    ConsentMissing,
}

impl JobFailure {
    fn code(self) -> &'static str {
        match self {
            Self::SourceUnavailable => "source_unavailable",
            Self::TranscriptionFailed => "transcription_failed",
            Self::AnalysisFailed => "analysis_failed",
            Self::PlanningFailed => "planning_failed",
            Self::RenderingFailed => "rendering_failed",
            Self::ValidationFailed => "validation_failed",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::BudgetExceeded => "budget_exceeded",
            Self::ConsentMissing => "consent_missing",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::SourceUnavailable => "Der Clip ist nicht verfügbar.",
            Self::TranscriptionFailed => "Die Untertitel konnten nicht erstellt werden.",
            Self::AnalysisFailed => "Die Clip-Analyse ist fehlgeschlagen.",
            Self::PlanningFailed => "Der Schnitt konnte nicht geplant werden.",
            Self::RenderingFailed => "Das Video konnte nicht erstellt werden.",
            Self::ValidationFailed => "Das Video konnte nicht geprüft werden.",
            Self::ProviderUnavailable => "Der KI-Dienst ist derzeit nicht verfügbar.",
            Self::BudgetExceeded => "Das Kostenlimit ist erreicht.",
            Self::ConsentMissing => "Für die externe KI-Verarbeitung fehlt die Einwilligung.",
        }
    }
}

pub async fn fail_job(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
    stage: AiStage,
    failure: JobFailure,
) -> Result<(), AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let changed = sqlx::query(
        "UPDATE social_ai_jobs SET status = 'failed', failure_code = $4, \
         failure_message = $5, updated_at = NOW() \
         WHERE id = $1 AND twitch_user_id = $2 AND stage = $3 AND status = $3",
    )
    .bind(job_id)
    .bind(owner)
    .bind(stage.as_str())
    .bind(failure.code())
    .bind(failure.message())
    .execute(&mut *lease.connection)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AiStoreError::Conflict)
    }
}

pub async fn retry_job(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: i64,
    limit: AttemptLimit,
) -> Result<AiJobView, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let changed = sqlx::query(
        "UPDATE social_ai_jobs SET status = 'queued', failure_code = NULL, \
         failure_message = NULL, updated_at = NOW() \
         WHERE id = $1 AND twitch_user_id = $2 AND status = 'failed' \
         AND COALESCE((attempts ->> stage)::integer, 0) < $3 \
         AND NOT EXISTS (SELECT 1 FROM social_ai_jobs sibling \
         WHERE sibling.twitch_user_id = social_ai_jobs.twitch_user_id \
         AND sibling.clip_db_id = social_ai_jobs.clip_db_id AND sibling.id <> social_ai_jobs.id \
         AND sibling.status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating'))",
    )
    .bind(job_id)
    .bind(owner)
    .bind(limit.0)
    .execute(pool)
    .await;
    match changed {
        Ok(result) if result.rows_affected() == 1 => {
            get_job(pool, twitch_user_id, job_id, limit).await
        }
        Ok(_) => {
            let view = get_job(pool, twitch_user_id, job_id, limit).await?;
            if view.status == "failed" {
                let attempts: i32 = sqlx::query_scalar(
                    "SELECT COALESCE((attempts ->> stage)::integer, 0) FROM social_ai_jobs \
                     WHERE id = $1 AND twitch_user_id = $2",
                )
                .bind(job_id)
                .bind(owner)
                .fetch_one(pool)
                .await?;
                if attempts >= limit.0 {
                    return Err(AiStoreError::AttemptsExhausted);
                }
            }
            Err(AiStoreError::Conflict)
        }
        Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("23505") => {
            Err(AiStoreError::Conflict)
        }
        Err(error) => Err(error.into()),
    }
}

pub async fn finish_job(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
    produced_video_id: i64,
) -> Result<(), AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    if produced_video_id <= 0 {
        return Err(AiStoreError::InvalidInput);
    }
    let changed = sqlx::query(
        "UPDATE social_ai_jobs j SET status = 'review', produced_video_id = $3, \
         completed_at = NOW(), updated_at = NOW() \
         WHERE j.id = $1 AND j.twitch_user_id = $2 AND j.status = 'validating' \
         AND j.stage = 'validating' AND j.checkpoints ? 'validating' \
         AND EXISTS (SELECT 1 FROM social_produced_videos p \
         WHERE p.id = $3 AND p.twitch_user_id = $4 AND p.clip_id = j.clip_db_id \
         AND p.mode = 'ai' AND p.status = 'review' \
         AND p.metadata ->> 'manual_ai_job_id' = j.id::text)",
    )
    .bind(job_id)
    .bind(owner)
    .bind(produced_video_id)
    .bind(owner.to_string())
    .execute(&mut *lease.connection)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AiStoreError::Conflict)
    }
}

pub async fn recover_stale_jobs(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    requested_limit: i64,
) -> Result<u64, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    if requested_limit <= 0 {
        return Err(AiStoreError::InvalidInput);
    }
    let changed = sqlx::query(
        "WITH stale AS (SELECT id FROM social_ai_jobs \
         WHERE twitch_user_id = $1 AND status IN ('preparing','transcribing','analyzing','planning','rendering','validating') \
         ORDER BY updated_at, id LIMIT $2 FOR UPDATE SKIP LOCKED) \
         UPDATE social_ai_jobs j SET status = 'failed', failure_code = 'interrupted', \
         failure_message = 'Der Verarbeitungsschritt wurde unterbrochen.', updated_at = NOW() \
         FROM stale WHERE j.id = stale.id AND j.twitch_user_id = $1",
    )
    .bind(owner)
    .bind(requested_limit.min(100))
    .execute(&mut *lease.connection)
    .await?;
    Ok(changed.rows_affected())
}
