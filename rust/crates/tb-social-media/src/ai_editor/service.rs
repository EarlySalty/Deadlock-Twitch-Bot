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

const LEARNING_INTERVAL: Duration = Duration::from_secs(900);
const MAX_POLL_BACKOFF: Duration = Duration::from_secs(60);

enum WorkerError {
    Poll(AiEditorError),
    Job { id: i64, error: AiEditorError },
}

impl WorkerError {
    fn into_inner(self) -> AiEditorError {
        match self {
            Self::Poll(error) | Self::Job { error, .. } => error,
        }
    }
}

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
    let production: Option<i64> = sqlx::query_scalar("SELECT p.job_id FROM social_produced_videos p WHERE p.twitch_user_id=$1 AND p.metadata->>'manual_ai_job_id'=$2::text AND p.job_id IS NOT NULL ORDER BY p.id LIMIT 1")
        .bind(owner).bind(job.id).fetch_optional(pool).await.map_err(|_| AiEditorError::Storage)?.flatten();
    let identity = production
        .map(Identity::Production)
        .unwrap_or(Identity::Manual(job.id));
    let totals = social_editor::job_usage(pool, owner, &identity.namespace()).await?;
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
    job.original_url = None;
    for version in &mut job.versions {
        let video = super::revision::owned_version(pool, owner, job.id, version.id).await?;
        job.publishing_in_autopilot |= super::autopilot_revision::production_id(&video).is_some();
        if super::revision::usable(&video) {
            version.file_url = Some(format!(
                "/twitch/social-media/api/ai/jobs/{}/versions/{}/file?twitch_user_id={owner}",
                job.id, version.id
            ));
        }
        if super::revision::usable(&video)
            && video
                .expires_at
                .is_some_and(|expiry| expiry > chrono::Utc::now())
            && video.metadata.get("original_preview").is_some()
        {
            version.original_url = Some(format!(
                "/twitch/social-media/api/ai/jobs/{}/versions/{}/original?twitch_user_id={owner}",
                job.id, version.id
            ));
        }
        version.approved_platforms = crate::produced_video::approvals(pool, owner, version.id)
            .await?
            .into_iter()
            .filter(|approval| {
                approval.state == "approved" && approval.approver_user_id.as_deref() == Some(owner)
            })
            .map(|approval| approval.platform)
            .collect();
    }
    job.original_url = job
        .versions
        .iter()
        .find(|version| Some(version.id) == job.selected_version_id && version.file_url.is_some())
        .or_else(|| {
            job.versions
                .iter()
                .rev()
                .find(|version| version.file_url.is_some())
        })
        .and_then(|version| version.original_url.clone());
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
    if let Some(version_id) = job.selected_version_id.or_else(|| {
        job.versions
            .iter()
            .rev()
            .find(|version| version.file_url.is_some())
            .map(|version| version.id)
    }) {
        return manual_version_original_file(pool, runtime, owner, job_id, version_id).await;
    }
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

pub async fn manual_version_original_file(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    job_id: i64,
    version_id: i64,
) -> Result<std::path::PathBuf, AiEditorError> {
    let video = super::revision::owned_version(pool, owner, job_id, version_id).await?;
    let checkpoints = store::checkpoints(pool, owner, super::revision::maker_id(&video)?).await?;
    pipeline::version_original(pool, runtime, owner, &video, &checkpoints).await
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
    let downloader = crate::clip_prep_worker::YtDlpDownloader::new("yt-dlp");
    super::source::load_source(pool, runtime, owner, clip_id, Some(&downloader)).await?;
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
        AiEditorError::Source
        | AiEditorError::SourceDownload
        | AiEditorError::SourceMissing
        | AiEditorError::SourceUnreadable
        | AiEditorError::NotFound => JobFailure::SourceUnavailable,
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
        let mut last_learning: Option<Instant> = None;
        let poll = Duration::from_millis(self.runtime.limits.worker_poll_ms);
        let mut delay = poll;
        loop {
            delay = match self.poll().await {
                Ok(_) => poll,
                Err(WorkerError::Job { id, error }) => {
                    tracing::warn!(job_id = id, error = %error, "Manual AI production job failed");
                    poll
                }
                Err(WorkerError::Poll(error)) => {
                    let backoff = delay.saturating_mul(2).min(MAX_POLL_BACKOFF).max(poll);
                    tracing::warn!(error = %error, retry_in_ms = backoff.as_millis(), "Manual AI queue could not be polled");
                    backoff
                }
            };
            if last_learning.is_none_or(|at| at.elapsed() >= LEARNING_INTERVAL) {
                last_learning = Some(Instant::now());
                self.learn().await;
            }
            tokio::time::sleep(delay).await;
        }
    }

    pub async fn learn(&self) {
        match super::learning::run_pass(&self.pool, self.runtime.limits.max_skill_bytes).await {
            Ok(report) if report.profiles_updated > 0 || !report.skills_changed.is_empty() => {
                tracing::info!(
                    profiles = report.profiles_updated,
                    skills = report.skills_changed.len(),
                    proposals = report.proposals_recorded,
                    "AI feedback learning updated"
                );
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(error = %error, "AI feedback learning failed"),
        }
    }

    pub async fn process_next(&self) -> Result<bool, AiEditorError> {
        self.poll().await.map_err(WorkerError::into_inner)
    }

    async fn poll(&self) -> Result<bool, WorkerError> {
        let Some(mut lease) = store::acquire_production_lease(&self.pool)
            .await
            .map_err(|_| WorkerError::Poll(AiEditorError::Storage))?
        else {
            return Ok(false);
        };
        let result = async {
            lease
                .ensure_held()
                .await
                .map_err(|_| WorkerError::Poll(AiEditorError::Conflict))?;
            pipeline::cleanup_expired_artifacts(&self.pool, &self.runtime, &mut lease)
                .await
                .map_err(WorkerError::Poll)?;
            store::recover_all_stale_jobs(&mut lease, self.runtime.limits.recovery_limit)
                .await
                .map_err(|error| WorkerError::Poll(error.into()))?;
            super::autopilot_revision::restore_failed_parents(&mut lease)
                .await
                .map_err(|error| WorkerError::Poll(error.into()))?;
            let Some((owner, id)) = store::next_queued_job(&mut lease)
                .await
                .map_err(|error| WorkerError::Poll(error.into()))?
            else {
                return Ok(false);
            };
            let processed = self.process_job(&mut lease, &owner, id).await;
            super::autopilot_revision::restore_failed_parents(&mut lease)
                .await
                .map_err(|error| WorkerError::Poll(error.into()))?;
            processed.map_err(|error| WorkerError::Job { id, error })?;
            Ok(true)
        }
        .await;
        let released = lease
            .release()
            .await
            .map_err(|_| WorkerError::Poll(AiEditorError::Storage));
        let processed = result?;
        released.map(|()| processed)
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
            if job.status == "awaiting_confirmation" || job.status == "rejected" {
                return Ok(());
            }
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
            let downloader = crate::clip_prep_worker::YtDlpDownloader::new("yt-dlp");
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
                            downloader: Some(&downloader),
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
