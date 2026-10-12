use std::time::{Duration, Instant};

use sqlx::PgPool;
use tb_llm::social_editor;

use super::{
    pipeline::{self, Identity, StageContext},
    store::{
        self, AiJobView, AiStage, AttemptLimit, CumulativeProviderTotals, JobFailure, JobMetrics,
        StageCheckpoint,
    },
    AiEditorError, EditorRuntime,
};

fn attempt_limit(runtime: &EditorRuntime) -> Result<AttemptLimit, AiEditorError> {
    AttemptLimit::from_server_config(runtime.limits.stage_attempts).map_err(Into::into)
}

async fn frozen_limit(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    id: i64,
) -> Result<AttemptLimit, AiEditorError> {
    let checkpoints = store::checkpoints(pool, owner, id).await?;
    attempt_limit(&pipeline::checkpoint_runtime(&checkpoints, runtime)?)
}

async fn project(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    mut job: AiJobView,
) -> Result<AiJobView, AiEditorError> {
    if job.failure_code.as_deref() == Some("invalid_checkpoint") {
        job.can_retry = false;
    }
    if job.can_retry {
        let checkpoints = store::checkpoints(pool, owner, job.id).await?;
        job.can_retry = match pipeline::checkpoint_runtime(&checkpoints, runtime) {
            Ok(frozen) => {
                let limit = attempt_limit(&frozen)?;
                store::get_job(pool, owner, job.id, limit).await?.can_retry
                    && (!pipeline::media_recovery_needed(
                        pool,
                        &frozen,
                        owner,
                        job.clip_db_id,
                        Identity::Manual(job.id),
                        &checkpoints,
                    )
                    .await?
                        || store::render_retry_available(pool, owner, job.id, limit).await?)
            }
            Err(_) => false,
        };
    }
    let totals =
        social_editor::job_usage(pool, owner, &Identity::Manual(job.id).namespace()).await?;
    job.provider = totals.provider;
    job.model = totals.model;
    job.input_tokens = totals.input_tokens;
    job.output_tokens = totals.output_tokens;
    job.estimated_provider_cost_usd = totals.estimated_provider_cost_usd;
    job.measured_provider_cost_usd = None;
    job.provider_latency_ms = totals.provider_latency_ms;
    job.cost_basis = if job.estimated_provider_cost_usd.is_some() {
        "usage_priced_estimate"
    } else {
        "unknown"
    }
    .into();
    job.original_url = job
        .versions
        .iter()
        .any(|version| {
            matches!(
                version.status.as_str(),
                "review" | "approved" | "scheduled" | "published"
            )
        })
        .then(|| {
            format!(
                "/twitch/social-media/api/ai/jobs/{}/original?twitch_user_id={owner}",
                job.id
            )
        });
    for version in &mut job.versions {
        if matches!(
            version.status.as_str(),
            "review" | "approved" | "scheduled" | "published"
        ) {
            version.file_url = Some(format!(
                "/twitch/social-media/api/ai/jobs/{}/versions/{}/file?twitch_user_id={owner}",
                job.id, version.id
            ));
        }
    }
    Ok(job)
}

pub async fn get_manual_job(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    job_id: i64,
) -> Result<AiJobView, AiEditorError> {
    let job = store::get_job(pool, owner, job_id, attempt_limit(runtime)?).await?;
    project(pool, runtime, owner, job).await
}

pub async fn manual_original_file(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    job_id: i64,
) -> Result<std::path::PathBuf, AiEditorError> {
    store::original_file_path(pool, owner, job_id).await?;
    let checkpoints = store::checkpoints(pool, owner, job_id).await?;
    let job = store::get_job(pool, owner, job_id, attempt_limit(runtime)?).await?;
    pipeline::original_source(
        pool,
        runtime,
        owner,
        job.clip_db_id,
        Identity::Manual(job_id),
        &checkpoints,
    )
    .await
}

pub async fn list_manual_jobs(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
) -> Result<Vec<AiJobView>, AiEditorError> {
    let jobs = store::list_jobs(
        pool,
        owner,
        runtime.limits.list_limit,
        None,
        attempt_limit(runtime)?,
    )
    .await?;
    let mut views = Vec::with_capacity(jobs.len());
    for job in jobs {
        views.push(project(pool, runtime, owner, job).await?);
    }
    Ok(views)
}

pub async fn create_manual_job(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    clip_id: i64,
) -> Result<AiJobView, AiEditorError> {
    social_editor::consent_allowed(pool, owner).await?;
    super::source::load_source(pool, runtime, owner, clip_id).await?;
    let job = store::create_job(pool, owner, clip_id, attempt_limit(runtime)?).await?;
    project(pool, runtime, owner, job).await
}

pub async fn retry_manual_job(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    job_id: i64,
) -> Result<AiJobView, AiEditorError> {
    social_editor::consent_allowed(pool, owner).await?;
    let limit = frozen_limit(pool, runtime, owner, job_id).await?;
    let job = get_manual_job(pool, runtime, owner, job_id).await?;
    if !job.can_retry {
        return Err(AiEditorError::Conflict);
    }
    let job = store::retry_job(pool, owner, job_id, limit).await?;
    project(pool, runtime, owner, job).await
}

fn failure(error: &AiEditorError, stage: AiStage) -> JobFailure {
    match error {
        AiEditorError::Source | AiEditorError::NotFound => JobFailure::SourceUnavailable,
        AiEditorError::Consent => JobFailure::ConsentMissing,
        AiEditorError::Budget => JobFailure::BudgetExceeded,
        AiEditorError::Provider => JobFailure::ProviderUnavailable,
        AiEditorError::Transcription => JobFailure::TranscriptionFailed,
        AiEditorError::Render => JobFailure::RenderingFailed,
        AiEditorError::Validation => JobFailure::ValidationFailed,
        _ => match stage {
            AiStage::Preparing => JobFailure::SourceUnavailable,
            AiStage::Transcribing => JobFailure::TranscriptionFailed,
            AiStage::Analyzing => JobFailure::AnalysisFailed,
            AiStage::Planning => JobFailure::PlanningFailed,
            AiStage::Rendering => JobFailure::RenderingFailed,
            AiStage::Validating => JobFailure::ValidationFailed,
        },
    }
}

pub struct ManualWorker {
    pool: PgPool,
    runtime: EditorRuntime,
    brain_token: Option<String>,
}

impl ManualWorker {
    pub fn new(pool: PgPool, runtime: EditorRuntime, brain_token: Option<String>) -> Self {
        Self {
            pool,
            runtime,
            brain_token,
        }
    }

    pub async fn run(self) {
        loop {
            if let Err(error) = self.process_next().await {
                tracing::warn!(error = %error, "Manual AI production could not finish");
            }
            tokio::time::sleep(Duration::from_millis(self.runtime.limits.worker_poll_ms)).await;
        }
    }

    pub async fn process_next(&self) -> Result<bool, AiEditorError> {
        let Some(mut lease) = store::acquire_production_lease(&self.pool)
            .await
            .map_err(|_| AiEditorError::Storage)?
        else {
            return Ok(false);
        };
        let result = async {
            lease
                .ensure_held()
                .await
                .map_err(|_| AiEditorError::Conflict)?;
            pipeline::cleanup_expired_artifacts(&self.pool, &self.runtime, &mut lease).await?;
            store::recover_all_stale_jobs(&mut lease, self.runtime.limits.recovery_limit).await?;
            let Some((owner, id)) = store::next_queued_job(&mut lease).await? else {
                return Ok(false);
            };
            self.process_job(&mut lease, &owner, id).await?;
            Ok(true)
        }
        .await;
        lease.release().await.map_err(|_| AiEditorError::Storage)?;
        result
    }

    async fn metrics(
        &self,
        owner: &str,
        id: i64,
        processing_ms: i64,
    ) -> Result<JobMetrics, AiEditorError> {
        let totals =
            social_editor::job_usage(&self.pool, owner, &Identity::Manual(id).namespace()).await?;
        Ok(JobMetrics {
            processing_ms: Some(processing_ms),
            rendering_ms: None,
            cumulative_provider_totals: Some(CumulativeProviderTotals {
                provider: totals.provider,
                model: totals.model,
                input_tokens: totals.input_tokens,
                output_tokens: totals.output_tokens,
                estimated_provider_cost_usd: totals.estimated_provider_cost_usd,
                measured_provider_cost_usd: None,
            }),
        })
    }

    async fn process_job(
        &self,
        lease: &mut store::ProductionLease,
        owner: &str,
        id: i64,
    ) -> Result<(), AiEditorError> {
        let started = Instant::now();
        let previous = store::get_job(&self.pool, owner, id, attempt_limit(&self.runtime)?)
            .await?
            .processing_ms
            .unwrap_or(0);
        loop {
            let job = match store::load_job_for_runner(lease, owner, id).await {
                Ok(job) => job,
                Err(store::AiStoreError::InvalidStoredData) => {
                    store::fail_invalid_checkpoint(lease, owner, id).await?;
                    return Err(AiEditorError::Storage);
                }
                Err(error) => return Err(error.into()),
            };
            let frozen_runtime = match pipeline::checkpoint_runtime(&job.checkpoints, &self.runtime)
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    store::fail_invalid_checkpoint(lease, owner, id).await?;
                    return Err(error);
                }
            };
            let limit = attempt_limit(&frozen_runtime)?;
            if matches!(job.stage, AiStage::Rendering | AiStage::Validating)
                && (job.stage != AiStage::Rendering
                    || job.checkpoints.contains_key(&AiStage::Rendering)
                    || job.checkpoints.contains_key(&AiStage::Validating))
                && pipeline::media_recovery_needed(
                    &self.pool,
                    &frozen_runtime,
                    owner,
                    job.clip_db_id,
                    Identity::Manual(id),
                    &job.checkpoints,
                )
                .await?
            {
                if !store::rewind_media_stages(lease, owner, id, limit).await? {
                    return Err(AiEditorError::Attempts);
                }
                continue;
            }
            if job.status == "queued" {
                store::claim_job(lease, owner, id, limit).await?;
            }
            if job.stage == AiStage::Validating
                && job.checkpoints.contains_key(&AiStage::Validating)
            {
                let outcome = pipeline::produced_outcome(
                    &self.pool,
                    owner,
                    job.clip_db_id,
                    Identity::Manual(id),
                )
                .await?;
                let produced_id = *outcome
                    .produced_video_ids
                    .first()
                    .ok_or(AiEditorError::Validation)?;
                lease
                    .ensure_held()
                    .await
                    .map_err(|_| AiEditorError::Conflict)?;
                store::finish_job(lease, owner, id, produced_id).await?;
                return Ok(());
            }
            let remaining = Duration::from_millis(frozen_runtime.limits.job_deadline_ms)
                .saturating_sub(started.elapsed());
            let result = if remaining.is_zero() {
                Err(AiEditorError::Deadline)
            } else {
                tokio::time::timeout(
                    remaining,
                    pipeline::execute_stage(
                        &mut StageContext {
                            pool: &self.pool,
                            current: &self.runtime,
                            lease,
                            owner,
                            clip_id: job.clip_db_id,
                            identity: Identity::Manual(id),
                            expected_twitch_clip_id: Some(&job.source_twitch_clip_id),
                            checkpoints: &job.checkpoints,
                            brain_token: self.brain_token.as_deref(),
                        },
                        job.stage,
                    ),
                )
                .await
                .map_err(|_| AiEditorError::Deadline)
                .and_then(|result| result)
            };
            let processing_ms = previous
                .checked_add(
                    i64::try_from(started.elapsed().as_millis())
                        .map_err(|_| AiEditorError::Storage)?,
                )
                .ok_or(AiEditorError::Storage)?;
            let metrics = self.metrics(owner, id, processing_ms).await?;
            match result {
                Ok(checkpoint) => {
                    lease
                        .ensure_held()
                        .await
                        .map_err(|_| AiEditorError::Conflict)?;
                    store::save_stage(lease, owner, id, &checkpoint, Some(&metrics)).await?;
                    if let StageCheckpoint::Validating { .. } = checkpoint {
                        let outcome = pipeline::produced_outcome(
                            &self.pool,
                            owner,
                            job.clip_db_id,
                            Identity::Manual(id),
                        )
                        .await?;
                        let produced_id = *outcome
                            .produced_video_ids
                            .first()
                            .ok_or(AiEditorError::Validation)?;
                        store::finish_job(lease, owner, id, produced_id).await?;
                        return Ok(());
                    }
                }
                Err(error) => {
                    lease
                        .ensure_held()
                        .await
                        .map_err(|_| AiEditorError::Conflict)?;
                    store::fail_job(
                        lease,
                        owner,
                        id,
                        job.stage,
                        failure(&error, job.stage),
                        Some(&metrics),
                    )
                    .await?;
                    return Err(error);
                }
            }
        }
    }
}
