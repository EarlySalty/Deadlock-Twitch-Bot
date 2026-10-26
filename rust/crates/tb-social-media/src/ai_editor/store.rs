use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, Connection, PgPool, Row};
use thiserror::Error;

pub use crate::production_lease::{acquire_production_lease, ProductionLease};

#[derive(Debug, Error)]
pub enum AiStoreError {
    #[error("Die Datenbank ist nicht verfügbar.")]
    Database(#[from] sqlx::Error),
    #[error("Ungültige Eingabe für den KI-Editor.")]
    InvalidInput,
    #[error("Der KI-Auftrag wurde nicht gefunden.")]
    NotFound,
    #[error("Der Clip ist nicht verfügbar.")]
    SourceUnavailable,
    #[error("Der KI-Auftrag hat sich bereits geändert.")]
    Conflict,
    #[error("Die erlaubten Versuche für diesen Schritt sind aufgebraucht.")]
    AttemptsExhausted,
    #[error(transparent)]
    Lease(#[from] crate::production_lease::ProductionLeaseError),
    #[error("Gespeicherte Auftragsdaten sind ungültig.")]
    InvalidStoredData,
    #[error("Für diesen Clip läuft auf dieser Plattform schon eine Veröffentlichung.")]
    PipelineBusy,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", content = "result", rename_all = "snake_case")]
pub enum StageCheckpoint {
    Preparing {
        source_path: String,
        original_duration_ms: Option<i64>,
        snapshot: serde_json::Map<String, serde_json::Value>,
    },
    Transcribing {
        transcript: String,
        language: Option<String>,
        segments: Vec<TranscriptSegment>,
    },
    Analyzing {
        highlights: Vec<TimelineEntry>,
        uncertainty: Vec<String>,
        analysis_notes: Vec<String>,
    },
    Planning {
        decisions: Vec<TimelineEntry>,
        review_reasons: Vec<String>,
        edit_plan: serde_json::Map<String, serde_json::Value>,
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

    pub(super) fn validate(&self) -> Result<String, AiStoreError> {
        let valid = match self {
            Self::Preparing {
                source_path,
                original_duration_ms,
                snapshot,
            } => {
                valid_path(source_path)
                    && valid_duration(*original_duration_ms)
                    && valid_payload(snapshot, 32_768)
            }
            Self::Transcribing {
                transcript,
                language,
                segments,
            } => {
                transcript.len() <= 48_000
                    && language
                        .as_ref()
                        .is_none_or(|value| !value.trim().is_empty() && value.len() <= 32)
                    && valid_segments(segments)
                    && if segments.is_empty() {
                        transcript.is_empty()
                    } else {
                        !transcript.trim().is_empty()
                    }
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
                edit_plan,
            } => {
                valid_timeline(decisions)
                    && valid_labels(review_reasons)
                    && valid_payload(edit_plan, 32_768)
            }
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

fn valid_payload(payload: &serde_json::Map<String, serde_json::Value>, max_bytes: usize) -> bool {
    !payload.is_empty()
        && serde_json::to_vec(payload).is_ok_and(|encoded| encoded.len() <= max_bytes)
}

fn valid_segments(segments: &[TranscriptSegment]) -> bool {
    segments.len() <= 512
        && segments.iter().enumerate().all(|(index, segment)| {
            segment.start_ms >= 0
                && segment.end_ms > segment.start_ms
                && !segment.text.trim().is_empty()
                && segment.text.len() <= 512
                && (index == 0 || segments[index - 1].end_ms <= segment.start_ms)
        })
}

#[derive(Debug, Clone, Default)]
pub struct CumulativeProviderTotals {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub estimated_provider_cost_usd: Option<f64>,
    pub measured_provider_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct JobMetrics {
    pub processing_ms: Option<i64>,
    pub rendering_ms: Option<i64>,
    pub cumulative_provider_totals: Option<CumulativeProviderTotals>,
}

impl JobMetrics {
    fn validate(&self) -> Result<(), AiStoreError> {
        let totals = self.cumulative_provider_totals.as_ref();
        let names_valid = totals.is_none_or(|totals| {
            [&totals.provider, &totals.model].iter().all(|name| {
                name.as_ref()
                    .is_none_or(|name| !name.trim().is_empty() && name.len() <= 128)
            })
        });
        let counts_valid = [
            self.processing_ms,
            self.rendering_ms,
            totals.and_then(|value| value.input_tokens),
            totals.and_then(|value| value.output_tokens),
        ]
        .iter()
        .all(|count| count.is_none_or(|count| count >= 0));
        let costs_valid = totals.is_none_or(|totals| {
            [
                totals.estimated_provider_cost_usd,
                totals.measured_provider_cost_usd,
            ]
            .iter()
            .all(|cost| cost.is_none_or(|cost| cost.is_finite() && cost >= 0.0))
        });
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
    pub job_id: i64,
    pub variant_index: i32,
    pub original_url: Option<String>,
    pub decisions: Vec<TimelineEntry>,
    pub approved_platforms: Vec<String>,
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub layout_mode: Option<String>,
    #[serde(default)]
    pub streamer_layout_configured: bool,
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
    pub provider_latency_ms: Option<i64>,
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
    pub root_job_id: i64,
    pub parent_version_id: Option<i64>,
    pub selected_version_id: Option<i64>,
    pub revision_proposal: Option<super::revision::RevisionProposal>,
    #[serde(default)]
    pub publishing_in_autopilot: bool,
}

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: i64,
    pub clip_db_id: i64,
    pub source_twitch_clip_id: String,
    pub status: String,
    pub stage: AiStage,
    pub attempts_for_stage: i32,
    pub checkpoints: BTreeMap<AiStage, StageCheckpoint>,
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

fn notice_key(notice: &str) -> String {
    notice
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn distinct_notices(notices: Vec<String>, shown: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> =
        shown.iter().map(|notice| notice_key(notice)).collect();
    notices
        .into_iter()
        .filter(|notice| seen.insert(notice_key(notice)))
        .collect()
}

macro_rules! public_job_query {
    ($tail:literal) => {
        concat!(
            "SELECT j.id, j.clip_db_id, j.status, j.stage, j.created_at, \
             COALESCE(j.root_job_id,j.id) AS root_job_id, j.parent_version_id, j.revision_proposal::text AS revision_proposal_text, \
             (SELECT r.selected_version_id FROM social_ai_jobs r WHERE r.id=COALESCE(j.root_job_id,j.id) AND r.twitch_user_id=j.twitch_user_id) AS selected_version_id, \
             j.updated_at, j.completed_at, j.provider, j.model, j.original_duration_ms, \
             j.output_duration_ms, j.processing_ms, j.rendering_ms, j.input_tokens, j.output_tokens, \
             j.estimated_provider_cost_usd, j.measured_provider_cost_usd, j.failure_code, j.failure_message, \
             j.highlights::text AS highlights_text, j.decisions::text AS decisions_text, \
             j.uncertainty::text AS uncertainty_text, j.review_reasons::text AS review_reasons_text, \
             j.attempts::text AS attempts_text, \
             EXISTS (SELECT 1 FROM social_ai_jobs sibling WHERE sibling.twitch_user_id = j.twitch_user_id \
             AND sibling.clip_db_id = j.clip_db_id AND sibling.id <> j.id \
             AND sibling.status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating','awaiting_confirmation')) AS active_sibling, \
             EXISTS (SELECT 1 FROM twitch_clips_social_media c WHERE c.id = j.source_clip_id \
             AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id \
             AND c.twitch_user_id = j.twitch_user_id::text AND c.category_key = 'deadlock' \
             AND c.discarded_at IS NULL AND c.source_kind = 'twitch') AS source_eligible",
            $tail
        )
    };
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
    let source_eligible: bool = row.try_get("source_eligible")?;
    let can_retry = status == "failed"
        && source_eligible
        && !active_sibling
        && attempts.get(&stage).copied().unwrap_or(0) < limit.0;
    let uncertainty = distinct_notices(parse_json(row.try_get("uncertainty_text")?)?, &[]);
    let review_reasons = distinct_notices(
        parse_json(row.try_get("review_reasons_text")?)?,
        &uncertainty,
    );
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
        provider_latency_ms: None,
        rendering_ms: row.try_get("rendering_ms")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        estimated_provider_cost_usd,
        measured_provider_cost_usd,
        cost_basis: cost_basis.to_string(),
        highlights: parse_json(row.try_get("highlights_text")?)?,
        decisions: parse_json(row.try_get("decisions_text")?)?,
        uncertainty,
        review_reasons,
        original_url: None,
        versions: Vec::new(),
        failure_code: row.try_get("failure_code")?,
        failure_message: row.try_get("failure_message")?,
        can_retry,
        root_job_id: row.try_get("root_job_id")?,
        parent_version_id: row.try_get("parent_version_id")?,
        selected_version_id: row.try_get("selected_version_id")?,
        publishing_in_autopilot: false,
        revision_proposal: row
            .try_get::<Option<String>, _>("revision_proposal_text")?
            .map(|text| parse_json(&text))
            .transpose()?,
    })
}

async fn load_versions(
    pool: &PgPool,
    owner: i64,
    jobs: &mut [AiJobView],
) -> Result<(), AiStoreError> {
    if jobs.is_empty() {
        return Ok(());
    }
    let ids: Vec<i64> = jobs.iter().map(|job| job.id).collect();
    let rows = sqlx::query(
        "SELECT j.id AS job_id, p.id, p.version_no, p.created_at, p.status, \
         p.duration_ms, p.parent_version_id, p.variant_index, p.maker_job_id, p.decisions_text, p.planning_text, \
         (SELECT CASE e.event_kind WHEN 'version_accepted' THEN 'accepted' ELSE 'rejected' END \
         FROM social_ai_revision_events e WHERE e.twitch_user_id = j.twitch_user_id AND e.produced_video_id = p.id \
         AND e.event_kind IN ('version_accepted','version_rejected') ORDER BY e.id DESC LIMIT 1) AS rating, \
         p.layout_mode, \
         EXISTS (SELECT 1 FROM social_media_streamer_layout l WHERE l.twitch_user_id = j.twitch_user_id::text) AS streamer_layout_configured \
         FROM social_ai_jobs j \
         JOIN LATERAL (SELECT p.id, p.version_no, p.created_at, \
         CASE WHEN p.status IN ('rendering','review','failed') AND p.expires_at <= NOW() THEN 'expired' ELSE p.status END AS status, \
         p.duration_ms, p.parent_version_id, p.variant_index, maker.id AS maker_job_id, maker.decisions::text AS decisions_text, (p.metadata->'planning')::text AS planning_text, \
         COALESCE(maker.checkpoints->'preparing'->'result'->'snapshot'->'layout'->>'mode','blur_pad') AS layout_mode FROM social_ai_jobs maker \
         JOIN social_produced_videos p ON p.metadata->>'manual_ai_job_id'=maker.id::text \
         WHERE COALESCE(maker.root_job_id,maker.id)=COALESCE(j.root_job_id,j.id) AND maker.twitch_user_id=j.twitch_user_id AND maker.clip_db_id=j.clip_db_id \
         AND p.twitch_user_id = j.twitch_user_id::text AND p.clip_id = j.clip_db_id \
         AND p.mode = 'ai' \
         ORDER BY p.version_no, p.variant_index, p.id LIMIT 50) p ON TRUE \
         WHERE j.twitch_user_id = $1 AND j.id = ANY($2::bigint[]) \
         ORDER BY j.id DESC, p.version_no, p.variant_index, p.id",
    )
    .bind(owner)
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut versions: HashMap<i64, Vec<AiVideoVersion>> = HashMap::new();
    for row in &rows {
        let mut decisions = parse_json(row.try_get("decisions_text")?)?;
        if let Some(text) = row.try_get::<Option<String>, _>("planning_text")? {
            let planning: super::stage_contract::Planning = parse_json(&text)?;
            decisions = planning
                .plan
                .timeline()
                .into_iter()
                .map(|interval| TimelineEntry {
                    start_ms: interval.start_ms as i64,
                    end_ms: interval.end_ms as i64,
                    description: interval.reason,
                })
                .collect();
        }
        versions
            .entry(row.try_get("job_id")?)
            .or_default()
            .push(AiVideoVersion {
                id: row.try_get("id")?,
                version_no: row.try_get("version_no")?,
                created_at: row.try_get("created_at")?,
                status: row.try_get("status")?,
                duration_ms: row.try_get("duration_ms")?,
                file_url: None,
                parent_version_id: row.try_get("parent_version_id")?,
                job_id: row.try_get("maker_job_id")?,
                variant_index: row.try_get("variant_index")?,
                original_url: None,
                decisions,
                approved_platforms: Vec::new(),
                rating: row.try_get("rating")?,
                layout_mode: row.try_get("layout_mode")?,
                streamer_layout_configured: row.try_get("streamer_layout_configured")?,
            });
    }
    for job in jobs {
        job.versions = versions.remove(&job.id).unwrap_or_default();
    }
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
    let row = sqlx::query(public_job_query!(
        " FROM social_ai_jobs j WHERE j.id = $1 AND j.twitch_user_id = $2"
    ))
    .bind(job_id)
    .bind(owner)
    .fetch_optional(pool)
    .await?
    .ok_or(AiStoreError::NotFound)?;
    let view = view_from_row(&row, limit)?;
    let mut jobs = [view];
    load_versions(pool, owner, &mut jobs).await?;
    let [view] = jobs;
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
    let rows = sqlx::query(public_job_query!(
        " FROM social_ai_jobs j WHERE j.twitch_user_id = $1 \
         AND ($2::bigint IS NULL OR j.id < $2) ORDER BY j.id DESC LIMIT $3"
    ))
    .bind(owner)
    .bind(before_id)
    .bind(requested_limit.min(50))
    .fetch_all(pool)
    .await?;
    let mut jobs = rows
        .iter()
        .map(|row| view_from_row(row, limit))
        .collect::<Result<Vec<_>, _>>()?;
    load_versions(pool, owner, &mut jobs).await?;
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
        "INSERT INTO social_ai_jobs (twitch_user_id, clip_db_id, source_clip_id, source_twitch_clip_id) \
         SELECT $1, c.id, c.id, c.clip_id FROM twitch_clips_social_media c \
         WHERE c.id = $2 AND c.twitch_user_id = $3 \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch' \
         ON CONFLICT (twitch_user_id, clip_db_id) \
         WHERE status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating','awaiting_confirmation') \
         DO UPDATE SET updated_at = social_ai_jobs.updated_at \
         WHERE social_ai_jobs.source_twitch_clip_id = EXCLUDED.source_twitch_clip_id RETURNING id",
    )
    .bind(owner)
    .bind(clip_db_id)
    .bind(owner.to_string())
    .fetch_optional(pool)
    .await?;
    let id = id.ok_or(AiStoreError::NotFound)?;
    get_job(pool, twitch_user_id, id, limit).await
}

async fn fail_missing_source(
    lease: &mut ProductionLease,
    owner: i64,
    job_id: i64,
) -> Result<bool, AiStoreError> {
    let changed = sqlx::query(
        "UPDATE social_ai_jobs j SET status = 'failed', failure_code = 'source_unavailable', \
         failure_message = 'Der Clip ist nicht verfügbar.', updated_at = NOW() \
         WHERE j.id = $1 AND j.twitch_user_id = $2 \
         AND j.status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating','awaiting_confirmation') \
         AND NOT EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = j.source_clip_id AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id \
         AND c.twitch_user_id = j.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch')",
    )
    .bind(job_id)
    .bind(owner)
    .execute(&mut *lease.connection)
    .await?;
    Ok(changed.rows_affected() == 1)
}

pub async fn load_job_for_runner(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
) -> Result<JobSnapshot, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let row = sqlx::query(
        "SELECT j.id, j.clip_db_id, j.source_twitch_clip_id, j.status, j.stage, j.attempts::text AS attempts_text, \
         j.checkpoints::text AS checkpoints_text, \
         EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = j.source_clip_id AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id \
         AND c.twitch_user_id = j.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch') AS source_eligible \
         FROM social_ai_jobs j WHERE j.id = $1 AND j.twitch_user_id = $2",
    )
    .bind(job_id)
    .bind(owner)
    .fetch_optional(&mut *lease.connection)
    .await?
    .ok_or(AiStoreError::NotFound)?;
    if !row.try_get::<bool, _>("source_eligible")? {
        fail_missing_source(lease, owner, job_id).await?;
        return Err(AiStoreError::SourceUnavailable);
    }
    let stage_name: String = row.try_get("stage")?;
    let stage = stage_from_str(&stage_name)?;
    let attempts: BTreeMap<String, i32> = parse_json(row.try_get("attempts_text")?)?;
    let checkpoints: BTreeMap<AiStage, StageCheckpoint> =
        parse_json(row.try_get("checkpoints_text")?)?;
    if checkpoints
        .iter()
        .any(|(key, checkpoint)| *key != checkpoint.stage() || checkpoint.validate().is_err())
    {
        return Err(AiStoreError::InvalidStoredData);
    }
    let snapshot = JobSnapshot {
        id: row.try_get("id")?,
        clip_db_id: row.try_get("clip_db_id")?,
        source_twitch_clip_id: row.try_get("source_twitch_clip_id")?,
        status: row.try_get("status")?,
        stage,
        attempts_for_stage: attempts.get(&stage_name).copied().unwrap_or(0),
        checkpoints,
    };
    if matches!(
        snapshot.status.as_str(),
        "preparing" | "transcribing" | "analyzing" | "planning" | "rendering" | "validating"
    ) {
        lease.mark_claimed_work();
    }
    Ok(snapshot)
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
        "UPDATE social_ai_jobs j SET status = stage, \
         attempts = jsonb_set(attempts, ARRAY[stage], \
         to_jsonb(COALESCE((attempts ->> stage)::integer, 0) + 1)), \
         started_at = COALESCE(started_at, NOW()), updated_at = NOW(), \
         failure_code = NULL, failure_message = NULL \
         WHERE j.id = $1 AND j.twitch_user_id = $2 AND j.status = 'queued' \
         AND COALESCE((attempts ->> stage)::integer, 0) < $3 \
         AND EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = j.source_clip_id AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id \
         AND c.twitch_user_id = j.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch') \
         RETURNING j.id",
    )
    .bind(job_id)
    .bind(owner)
    .bind(limit.0)
    .fetch_optional(&mut *lease.connection)
    .await?;
    if claimed.is_some() {
        lease.mark_claimed_work();
        Ok(())
    } else if fail_missing_source(lease, owner, job_id).await? {
        Err(AiStoreError::SourceUnavailable)
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
    let totals = metrics.and_then(|value| value.cumulative_provider_totals.as_ref());
    let saved: Option<i64> = sqlx::query_scalar(
        "UPDATE social_ai_jobs SET \
         checkpoints = jsonb_set(checkpoints, ARRAY[$3::text], $4::jsonb), \
         stage = $5, status = CASE WHEN $3 = 'validating' THEN 'validating' \
             WHEN $3='planning' AND revision_feedback IS NOT NULL AND NOT revision_confirmed \
             AND COALESCE(($4::jsonb->'result'->'edit_plan'->'revision_proposal'->>'requires_confirmation')::boolean,FALSE) THEN 'awaiting_confirmation' ELSE 'queued' END, \
         revision_proposal = CASE WHEN $3='planning' AND revision_feedback IS NOT NULL THEN $4::jsonb->'result'->'edit_plan'->'revision_proposal' ELSE revision_proposal END, \
         highlights = COALESCE($6::jsonb, highlights), \
         decisions = COALESCE($7::jsonb, decisions), \
         uncertainty = COALESCE($8::jsonb, uncertainty), \
         review_reasons = CASE WHEN $3 = 'validating' THEN \
             (SELECT COALESCE(jsonb_agg(value ORDER BY ord), '[]'::jsonb) FROM \
             (SELECT DISTINCT ON (value) value, ord \
             FROM jsonb_array_elements(review_reasons || $9::jsonb) WITH ORDINALITY AS r(value, ord) \
             ORDER BY value, ord) distinct_reasons) \
             ELSE COALESCE($9::jsonb, review_reasons) END, \
         original_duration_ms = COALESCE($10::bigint, original_duration_ms), \
         output_duration_ms = COALESCE($11::bigint, output_duration_ms), \
         rendering_ms = COALESCE($20::bigint, $12::bigint, rendering_ms), \
         provider = CASE WHEN $21::boolean THEN $13::text ELSE provider END, \
         model = CASE WHEN $21::boolean THEN $14::text ELSE model END, \
         processing_ms = COALESCE($15::bigint, processing_ms), \
         input_tokens = CASE WHEN $21::boolean THEN $16::bigint ELSE input_tokens END, \
         output_tokens = CASE WHEN $21::boolean THEN $17::bigint ELSE output_tokens END, \
         estimated_provider_cost_usd = CASE WHEN $21::boolean THEN $18::double precision ELSE estimated_provider_cost_usd END, \
         measured_provider_cost_usd = CASE WHEN $21::boolean THEN $19::double precision ELSE measured_provider_cost_usd END, \
         updated_at = NOW() \
         WHERE id = $1 AND twitch_user_id = $2 AND stage = $3 AND status = $3 \
         AND NOT checkpoints ? $3 \
         AND EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = social_ai_jobs.source_clip_id AND c.id = social_ai_jobs.clip_db_id \
         AND c.clip_id = social_ai_jobs.source_twitch_clip_id \
         AND c.twitch_user_id = social_ai_jobs.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch') RETURNING id",
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
    .bind(totals.and_then(|value| value.provider.as_deref()))
    .bind(totals.and_then(|value| value.model.as_deref()))
    .bind(metrics.and_then(|value| value.processing_ms))
    .bind(totals.and_then(|value| value.input_tokens))
    .bind(totals.and_then(|value| value.output_tokens))
    .bind(totals.and_then(|value| value.estimated_provider_cost_usd))
    .bind(totals.and_then(|value| value.measured_provider_cost_usd))
    .bind(metrics.and_then(|value| value.rendering_ms))
    .bind(totals.is_some())
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
    ProviderAccess,
    BudgetExceeded,
    ConsentMissing,
    RevisionUnchanged,
    RequestTooLarge,
}

impl JobFailure {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::SourceUnavailable => "source_unavailable",
            Self::TranscriptionFailed => "transcription_failed",
            Self::AnalysisFailed => "analysis_failed",
            Self::PlanningFailed => "planning_failed",
            Self::RenderingFailed => "rendering_failed",
            Self::ValidationFailed => "validation_failed",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderAccess => "provider_access",
            Self::BudgetExceeded => "budget_exceeded",
            Self::ConsentMissing => "consent_missing",
            Self::RevisionUnchanged => "revision_unchanged",
            Self::RequestTooLarge => "request_too_large",
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
            Self::ProviderAccess => {
                "Der KI-Zugang ist auf dem Server nicht vollständig eingerichtet."
            }
            Self::BudgetExceeded => "Das Kostenlimit ist erreicht.",
            Self::ConsentMissing => "Für die externe KI-Verarbeitung fehlt die Einwilligung.",
            Self::RevisionUnchanged => {
                "Die gewünschte Änderung ließ sich nicht umsetzen. Es wurde keine neue Fassung erstellt."
            }
            Self::RequestTooLarge => {
                "Der Clip ist zu lang, um ihn per KI zu bearbeiten. Es wurde keine neue Fassung erstellt."
            }
        }
    }
}

pub async fn fail_job(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
    stage: AiStage,
    failure: JobFailure,
    metrics: Option<&JobMetrics>,
) -> Result<(), AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    if let Some(metrics) = metrics {
        metrics.validate()?;
    }
    let totals = metrics.and_then(|value| value.cumulative_provider_totals.as_ref());
    let changed = sqlx::query(
        "UPDATE social_ai_jobs SET status = 'failed', failure_code = $4, \
         failure_message = $5, \
         provider = CASE WHEN $6::boolean THEN $7::text ELSE provider END, \
         model = CASE WHEN $6::boolean THEN $8::text ELSE model END, \
         processing_ms = COALESCE($9::bigint, processing_ms), \
         rendering_ms = COALESCE($10::bigint, rendering_ms), \
         input_tokens = CASE WHEN $6::boolean THEN $11::bigint ELSE input_tokens END, \
         output_tokens = CASE WHEN $6::boolean THEN $12::bigint ELSE output_tokens END, \
         estimated_provider_cost_usd = CASE WHEN $6::boolean THEN $13::double precision ELSE estimated_provider_cost_usd END, \
         measured_provider_cost_usd = CASE WHEN $6::boolean THEN $14::double precision ELSE measured_provider_cost_usd END, \
         updated_at = NOW() \
         WHERE id = $1 AND twitch_user_id = $2 AND stage = $3 AND status = $3",
    )
    .bind(job_id)
    .bind(owner)
    .bind(stage.as_str())
    .bind(failure.code())
    .bind(failure.message())
    .bind(totals.is_some())
    .bind(totals.and_then(|value| value.provider.as_deref()))
    .bind(totals.and_then(|value| value.model.as_deref()))
    .bind(metrics.and_then(|value| value.processing_ms))
    .bind(metrics.and_then(|value| value.rendering_ms))
    .bind(totals.and_then(|value| value.input_tokens))
    .bind(totals.and_then(|value| value.output_tokens))
    .bind(totals.and_then(|value| value.estimated_provider_cost_usd))
    .bind(totals.and_then(|value| value.measured_provider_cost_usd))
    .execute(&mut *lease.connection)
    .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AiStoreError::Conflict)
    }
}

pub(super) async fn save_revision_answer(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: i64,
    proposal: &super::revision::RevisionProposal,
) -> Result<(), AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let proposal = serde_json::to_string(proposal).map_err(|_| AiStoreError::InvalidInput)?;
    let changed = sqlx::query("UPDATE social_ai_jobs SET revision_proposal=$3::jsonb, updated_at=NOW() WHERE id=$1 AND twitch_user_id=$2 AND status='planning' AND revision_feedback IS NOT NULL")
        .bind(job_id)
        .bind(owner)
        .bind(proposal)
        .execute(pool)
        .await?;
    if changed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AiStoreError::Conflict)
    }
}

pub(super) async fn render_retry_available(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: i64,
    limit: AttemptLimit,
) -> Result<bool, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    sqlx::query_scalar("SELECT COALESCE((attempts ->> 'rendering')::integer, 0) < $3 FROM social_ai_jobs WHERE id=$1 AND twitch_user_id=$2")
        .bind(job_id).bind(owner).bind(limit.0).fetch_optional(pool).await?
        .ok_or(AiStoreError::NotFound)
}

pub(super) async fn rewind_media_stages(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
    limit: AttemptLimit,
) -> Result<bool, AiStoreError> {
    lease.ensure_held().await?;
    let owner = numeric_owner(twitch_user_id)?;
    let status: Option<String> = sqlx::query_scalar(
        "UPDATE social_ai_jobs SET stage='rendering', \
         status=CASE WHEN COALESCE((attempts ->> 'rendering')::integer, 0) < $3 THEN 'queued' ELSE 'failed' END, \
         checkpoints=checkpoints - 'rendering' - 'validating', \
         output_duration_ms=NULL, rendering_ms=NULL, \
         failure_code=CASE WHEN COALESCE((attempts ->> 'rendering')::integer, 0) < $3 THEN NULL ELSE 'rendering_failed' END, \
         failure_message=CASE WHEN COALESCE((attempts ->> 'rendering')::integer, 0) < $3 THEN NULL ELSE 'Die erlaubten Versuche für diesen Schritt sind aufgebraucht.' END, \
         updated_at=NOW() WHERE id=$1 AND twitch_user_id=$2 \
         AND stage IN ('rendering','validating') AND status IN ('queued','rendering','validating') \
         RETURNING status",
    )
    .bind(job_id)
    .bind(owner)
    .bind(limit.0)
    .fetch_optional(&mut *lease.connection)
    .await?;
    status
        .map(|status| status == "queued")
        .ok_or(AiStoreError::Conflict)
}

pub async fn retry_job(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: i64,
    limit: AttemptLimit,
) -> Result<AiJobView, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let mut tx = pool.begin().await?;
    crate::autopilot_settings::lock_owner(&mut tx, twitch_user_id).await?;
    let parent: Option<i64> = sqlx::query_scalar("SELECT parent_version_id FROM social_ai_jobs WHERE id=$1 AND twitch_user_id=$2 AND status='failed'")
        .bind(job_id).bind(owner).fetch_optional(&mut *tx).await?.flatten();
    if let Some(parent_id) = parent {
        let video = crate::produced_video::get(pool, twitch_user_id, parent_id)
            .await
            .map_err(|error| match error {
                crate::produced_video::ProducedVideoError::Database(error) => {
                    AiStoreError::Database(error)
                }
                _ => AiStoreError::Conflict,
            })?
            .ok_or(AiStoreError::NotFound)?;
        super::autopilot_revision::hold_parent(&mut tx, twitch_user_id, &video, job_id)
            .await
            .map_err(|error| match error {
                super::AiEditorError::Storage => AiStoreError::InvalidStoredData,
                _ => AiStoreError::Conflict,
            })?;
    }
    let changed = sqlx::query(
        "UPDATE social_ai_jobs SET status = 'queued', failure_code = NULL, \
         failure_message = NULL, updated_at = NOW(), \
         revision_proposal = CASE WHEN stage = 'planning' THEN NULL ELSE revision_proposal END \
         WHERE id = $1 AND twitch_user_id = $2 AND status = 'failed' \
         AND COALESCE((attempts ->> stage)::integer, 0) < $3 \
         AND EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = social_ai_jobs.source_clip_id AND c.id = social_ai_jobs.clip_db_id \
         AND c.clip_id = social_ai_jobs.source_twitch_clip_id \
         AND c.twitch_user_id = social_ai_jobs.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch') \
         AND NOT EXISTS (SELECT 1 FROM social_ai_jobs sibling \
         WHERE sibling.twitch_user_id = social_ai_jobs.twitch_user_id \
         AND sibling.clip_db_id = social_ai_jobs.clip_db_id AND sibling.id <> social_ai_jobs.id \
         AND sibling.status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating','awaiting_confirmation'))",
    )
    .bind(job_id)
    .bind(owner)
    .bind(limit.0)
    .execute(&mut *tx)
    .await;
    match changed {
        Ok(result) if result.rows_affected() == 1 => {
            tx.commit().await?;
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
    let mut tx = lease.connection.begin().await?;
    crate::autopilot_settings::lock_owner(&mut tx, twitch_user_id).await?;
    let changed = sqlx::query(
        "UPDATE social_ai_jobs j SET status = 'review', produced_video_id = $3, \
         completed_at = NOW(), updated_at = NOW() \
         WHERE j.id = $1 AND j.twitch_user_id = $2 AND j.status = 'validating' \
         AND j.stage = 'validating' AND j.checkpoints ? 'validating' \
         AND EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = j.source_clip_id AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id \
         AND c.twitch_user_id = j.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch') \
         AND EXISTS (SELECT 1 FROM social_produced_videos p \
         WHERE p.id = $3 AND p.twitch_user_id = $4 AND p.clip_id = j.clip_db_id \
         AND p.mode = 'ai' AND p.status = 'review' \
         AND p.metadata ->> 'manual_ai_job_id' = j.id::text)",
    )
    .bind(job_id)
    .bind(owner)
    .bind(produced_video_id)
    .bind(owner.to_string())
    .execute(&mut *tx)
    .await?;
    if changed.rows_affected() == 1 {
        super::autopilot_revision::replace_parent_on(
            &mut tx,
            twitch_user_id,
            job_id,
            produced_video_id,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    } else {
        Err(AiStoreError::Conflict)
    }
}

pub(super) async fn checkpoints(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: i64,
) -> Result<BTreeMap<AiStage, StageCheckpoint>, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let raw: String = sqlx::query_scalar(
        "SELECT checkpoints::text FROM social_ai_jobs WHERE id=$1 AND twitch_user_id=$2",
    )
    .bind(job_id)
    .bind(owner)
    .fetch_optional(pool)
    .await?
    .ok_or(AiStoreError::NotFound)?;
    parse_json(&raw)
}

pub async fn original_file_path(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: i64,
) -> Result<String, AiStoreError> {
    let owner = numeric_owner(twitch_user_id)?;
    let path: Option<String> = sqlx::query_scalar("SELECT c.local_file_path FROM social_ai_jobs j JOIN twitch_clips_social_media c ON c.id = j.source_clip_id AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id AND c.twitch_user_id = j.twitch_user_id::text JOIN social_media_category k ON k.category_key = c.category_key WHERE j.id = $1 AND j.twitch_user_id = $2 AND c.source_kind = 'twitch' AND c.category_key = 'deadlock' AND c.game_id = k.twitch_game_id AND c.discarded_at IS NULL")
        .bind(job_id).bind(owner).fetch_optional(pool).await?.flatten();
    path.ok_or(AiStoreError::NotFound)
}

pub(super) async fn fail_invalid_checkpoint(
    lease: &mut ProductionLease,
    twitch_user_id: &str,
    job_id: i64,
) -> Result<(), AiStoreError> {
    lease.ensure_held().await?;
    let owner = numeric_owner(twitch_user_id)?;
    let changed = sqlx::query("UPDATE social_ai_jobs SET status='failed', failure_code='invalid_checkpoint', failure_message='Die gespeicherten Auftragsdaten konnten nicht sicher geprüft werden.', updated_at=NOW() WHERE id=$1 AND twitch_user_id=$2 AND status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating','awaiting_confirmation')")
        .bind(job_id).bind(owner).execute(&mut *lease.connection).await?.rows_affected();
    if changed != 1 {
        return Err(AiStoreError::Conflict);
    }
    Ok(())
}

pub async fn next_queued_job(
    lease: &mut ProductionLease,
) -> Result<Option<(String, i64)>, AiStoreError> {
    sqlx::query_as("SELECT twitch_user_id::text, id FROM social_ai_jobs WHERE status = 'queued' ORDER BY id LIMIT 1")
        .fetch_optional(&mut *lease.connection).await.map_err(Into::into)
}

pub async fn recover_all_stale_jobs(
    lease: &mut ProductionLease,
    limit: i64,
) -> Result<u64, AiStoreError> {
    if lease.has_claimed_work() || !(1..=100).contains(&limit) {
        return Err(AiStoreError::Conflict);
    }
    let owners: Vec<String> = sqlx::query_scalar("SELECT twitch_user_id::text FROM social_ai_jobs WHERE status IN ('preparing','transcribing','analyzing','planning','rendering','validating') GROUP BY twitch_user_id ORDER BY MIN(updated_at), twitch_user_id LIMIT $1")
        .bind(limit).fetch_all(&mut *lease.connection).await?;
    let mut recovered = 0;
    for owner in owners {
        recovered += recover_stale_jobs(lease, &owner, limit).await?;
    }
    Ok(recovered)
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
    if lease.has_claimed_work() {
        return Err(AiStoreError::Conflict);
    }
    let changed = sqlx::query(
        "WITH stale AS (SELECT j.id, EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = j.source_clip_id AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id \
         AND c.twitch_user_id = j.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch') AS source_eligible \
         FROM social_ai_jobs j WHERE j.twitch_user_id = $1 \
         AND (j.status IN ('preparing','transcribing','analyzing','planning','rendering','validating') \
         OR (j.status = 'queued' AND NOT EXISTS (SELECT 1 FROM twitch_clips_social_media c \
         WHERE c.id = j.source_clip_id AND c.id = j.clip_db_id AND c.clip_id = j.source_twitch_clip_id \
         AND c.twitch_user_id = j.twitch_user_id::text \
         AND c.category_key = 'deadlock' AND c.discarded_at IS NULL \
         AND c.source_kind = 'twitch'))) \
         ORDER BY j.updated_at, j.id LIMIT $2 FOR UPDATE OF j SKIP LOCKED) \
         UPDATE social_ai_jobs j SET status = 'failed', \
         failure_code = CASE WHEN stale.source_eligible THEN 'interrupted' ELSE 'source_unavailable' END, \
         failure_message = CASE WHEN stale.source_eligible THEN 'Der Verarbeitungsschritt wurde unterbrochen.' \
         ELSE 'Der Clip ist nicht verfügbar.' END, updated_at = NOW() \
         FROM stale WHERE j.id = stale.id AND j.twitch_user_id = $1",
    )
    .bind(owner)
    .bind(requested_limit.min(100))
    .execute(&mut *lease.connection)
    .await?;
    Ok(changed.rows_affected())
}
