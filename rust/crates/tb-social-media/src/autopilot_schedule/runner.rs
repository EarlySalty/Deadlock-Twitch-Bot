use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::production_jobs::autopilot_schedule::{
    plan_owner, recover_after_lease, retry_or_review, ScheduleError,
};
use crate::{
    ai_editor::{self, EditorRuntime, MediaInfo, ProductionContext, ResourceLimits},
    autopilot_settings,
    clip_prep_worker::{download_atomic, register_local_file, ClipDownloader},
    clip_templates,
    produced_video::{self, NewProducedVideo, ProducedVideo, ProducedVideoError, RenderedVideo},
    production_jobs::{self, JobCompletion, JobSource, ProductionJob, ProductionMode},
    production_lease::{acquire_production_lease, ProductionLease, ProductionLeaseError},
    render::render_clip_vertical_with_lease,
    video_processor::{VideoProcessor, VideoProcessorError},
};

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    #[error(transparent)]
    Job(#[from] production_jobs::JobError),
    #[error(transparent)]
    Lease(#[from] ProductionLeaseError),
    #[error(transparent)]
    Video(#[from] ProducedVideoError),
    #[error("Die statische Produktion konnte nicht abgeschlossen werden")]
    Render(#[from] VideoProcessorError),
    #[error("Die Produktionsdaten konnten nicht geladen oder gespeichert werden")]
    Database(#[from] sqlx::Error),
    #[error("Die Produktionsdatei konnte nicht verwendet werden")]
    File(#[from] std::io::Error),
    #[error("Die gespeicherten Produktionsschritte sind ungültig")]
    Json(#[from] serde_json::Error),
    #[error("Der Clip konnte vorübergehend nicht heruntergeladen werden")]
    Download,
    #[error("Der ursprüngliche Clip oder die fertige Datei ist nicht verfügbar oder ungültig")]
    Source,
}

pub struct WorkerConfig {
    pub package_root: PathBuf,
    pub source_root: PathBuf,
    pub output_root: PathBuf,
    pub media_limits: ResourceLimits,
    pub max_duration_seconds: i64,
    pub poll_interval: Duration,
    pub editor: Option<EditorRuntime>,
    pub brain_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticCheckpoint {
    source_path: PathBuf,
    output_path: PathBuf,
    source_media: MediaInfo,
    max_duration_seconds: i64,
    rendering_ms: Option<i64>,
}

pub struct AutopilotWorker {
    pool: PgPool,
    config: WorkerConfig,
    downloader: Arc<dyn ClipDownloader>,
    video_processor: VideoProcessor,
}

impl AutopilotWorker {
    pub fn new(
        pool: PgPool,
        config: WorkerConfig,
        downloader: Arc<dyn ClipDownloader>,
        video_processor: VideoProcessor,
    ) -> Result<Self, RunnerError> {
        if config.max_duration_seconds <= 0
            || config.poll_interval.is_zero()
            || !config.package_root.is_absolute()
            || !config.source_root.is_absolute()
            || !config.output_root.is_absolute()
        {
            return Err(RunnerError::Source);
        }
        Ok(Self {
            pool,
            config,
            downloader,
            video_processor,
        })
    }

    pub async fn run(self) {
        loop {
            if let Err(error) = self.run_once().await {
                tracing::warn!(error = %error, "Auto-Pilot production iteration failed");
            }
            tokio::time::sleep(self.config.poll_interval).await;
        }
    }

    pub async fn run_once(&self) -> Result<bool, RunnerError> {
        self.plan_all().await?;
        let Some(mut lease) = acquire_production_lease(&self.pool).await? else {
            return Ok(false);
        };
        let result = async {
            lease.ensure_held().await?;
            recover_after_lease(&self.pool).await?;
            let Some(job) = production_jobs::claim_next(&self.pool).await? else {
                return Ok(false);
            };
            lease.mark_claimed_work();
            self.process_claimed(&mut lease, &job).await?;
            Ok(true)
        }
        .await;
        lease.release().await?;
        result
    }

    async fn plan_all(&self) -> Result<(), RunnerError> {
        let owners: Vec<String> = sqlx::query_scalar("SELECT twitch_user_id FROM social_media_autopilot_settings WHERE enabled ORDER BY twitch_user_id")
            .fetch_all(&self.pool).await?;
        for owner in owners {
            let report = plan_owner(&self.pool, &owner, chrono::Utc::now()).await?;
            tracing::debug!(
                created = report.created,
                retimed = report.retimed,
                "Auto-Pilot schedule planned"
            );
        }
        Ok(())
    }

    async fn process_claimed(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
    ) -> Result<(), RunnerError> {
        match job.mode.ok_or(RunnerError::Source)? {
            ProductionMode::Ai => self.produce_ai(lease, job).await,
            ProductionMode::Static => {
                if let Err(error) = self.produce_static(lease, job).await {
                    tracing::warn!(error = %error, job_id = job.id, "Static Auto-Pilot production failed");
                    lease.ensure_held().await?;
                    let retryable = matches!(
                        error,
                        RunnerError::Render(_)
                            | RunnerError::File(_)
                            | RunnerError::Database(_)
                            | RunnerError::Download
                            | RunnerError::Video(ProducedVideoError::Database(_))
                    );
                    retry_or_review(&self.pool, job, "static_production_failed", retryable).await?;
                    if !retryable || job.attempts >= job.max_attempts {
                        self.fail_static_video(job).await?;
                    }
                }
                Ok(())
            }
        }
    }

    async fn produce_ai(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
    ) -> Result<(), RunnerError> {
        let Some(runtime) = &self.config.editor else {
            production_jobs::finish(
                &self.pool,
                &job.twitch_user_id,
                job.id,
                JobCompletion::Review,
                &["ai_editor_unavailable".into()],
                Some("Der KI-Editor ist noch nicht verfügbar. Bitte prüfe den Auftrag."),
            )
            .await?;
            return Ok(());
        };
        let result = ai_editor::produce(
            ProductionContext {
                pool: &self.pool,
                runtime,
                lease,
                brain_token: self.config.brain_token.as_deref(),
            },
            job.clone(),
        )
        .await;
        match result {
            Ok(outcome) => {
                lease.ensure_held().await?;
                production_jobs::finish(
                    &self.pool,
                    &job.twitch_user_id,
                    job.id,
                    JobCompletion::Completed,
                    &outcome.review_reasons,
                    None,
                )
                .await?;
            }
            Err(error) => {
                tracing::warn!(error = %error, job_id = job.id, "AI Auto-Pilot production held for review");
                let current = production_jobs::get(&self.pool, &job.twitch_user_id, job.id)
                    .await?
                    .ok_or(RunnerError::Source)?;
                if current.status == "running" {
                    lease.ensure_held().await?;
                    let reason = if matches!(error, ai_editor::AiEditorError::Consent) {
                        "external_ai_consent_required"
                    } else {
                        "ai_production_failed"
                    };
                    production_jobs::finish(
                        &self.pool,
                        &job.twitch_user_id,
                        job.id,
                        JobCompletion::Review,
                        &[reason.into()],
                        Some(&error.to_string()),
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn existing_static(
        &self,
        job: &ProductionJob,
    ) -> Result<Option<ProducedVideo>, RunnerError> {
        let id: Option<i64> = sqlx::query_scalar("SELECT id FROM social_produced_videos
            WHERE job_id = $1 AND twitch_user_id = $2 AND clip_id = $3 AND mode = 'static' ORDER BY id LIMIT 1")
            .bind(job.id).bind(&job.twitch_user_id).bind(job.clip_id).fetch_optional(&self.pool).await?;
        match id {
            Some(id) => Ok(produced_video::get(&self.pool, &job.twitch_user_id, id).await?),
            None => Ok(None),
        }
    }

    async fn source_file(&self, job: &ProductionJob) -> Result<PathBuf, RunnerError> {
        let source: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT clip_url, local_file_path FROM twitch_clips_social_media
            WHERE id = $1 AND twitch_user_id = $2 AND discarded_at IS NULL
            AND ($3 OR (source_kind = 'twitch' AND game_id = '2132205352'))",
        )
        .bind(job.clip_id)
        .bind(&job.twitch_user_id)
        .bind(job.source == JobSource::Manual)
        .fetch_optional(&self.pool)
        .await?;
        let (url, path) = source.ok_or(RunnerError::Source)?;
        if let Some(path) = path {
            let path = PathBuf::from(path);
            let path = if path.is_absolute() {
                path
            } else {
                self.config.package_root.join(path)
            };
            if let Ok(path) = ai_editor::authorized_path(
                &self.config.source_root,
                &path,
                self.config.media_limits.max_source_bytes,
            ) {
                return Ok(path);
            }
        }
        tokio::fs::create_dir_all(&self.config.source_root).await?;
        let path = self.config.source_root.join(format!("{}.mp4", job.clip_id));
        let target = path.to_str().ok_or(RunnerError::Source)?;
        download_atomic(self.downloader.as_ref(), &url, target)
            .await
            .map_err(|_| RunnerError::Download)?;
        register_local_file(&self.pool, job.clip_id, target).await?;
        ai_editor::authorized_path(
            &self.config.source_root,
            &path,
            self.config.media_limits.max_source_bytes,
        )
        .map_err(|_| RunnerError::Source)
    }

    async fn publication(&self, job: &ProductionJob) -> Result<Value, RunnerError> {
        let has_custom: bool = sqlx::query_scalar("SELECT COALESCE(custom_description, '') <> '' FROM twitch_clips_social_media WHERE id = $1 AND twitch_user_id = $2")
            .bind(job.clip_id).bind(&job.twitch_user_id).fetch_one(&self.pool).await?;
        if !has_custom {
            if let Some(template) =
                clip_templates::get_streamer_templates(&self.pool, &job.twitch_user_id)
                    .await
                    .into_iter()
                    .find(|template| template.is_default)
            {
                if !clip_templates::apply_template_to_clip(
                    &self.pool,
                    job.clip_id,
                    template.id,
                    false,
                )
                .await
                {
                    return Err(RunnerError::Source);
                }
            }
        }
        let row = sqlx::query(
            "SELECT COALESCE(NULLIF(custom_title, ''), clip_title, '') AS title,
            COALESCE(custom_description, '') AS description, COALESCE(hashtags, '[]') AS hashtags
            FROM twitch_clips_social_media WHERE id = $1 AND twitch_user_id = $2",
        )
        .bind(job.clip_id)
        .bind(&job.twitch_user_id)
        .fetch_one(&self.pool)
        .await?;
        let title: String = row.try_get("title")?;
        let description: String = row.try_get("description")?;
        let hashtags: Vec<String> =
            serde_json::from_str(row.try_get::<String, _>("hashtags")?.as_str())?;
        let settings = autopilot_settings::load(&self.pool, &job.twitch_user_id)
            .await
            .map_err(ScheduleError::from)?;
        let mut platforms = serde_json::Map::new();
        for platform in settings.target_platforms {
            platforms.insert(
                platform,
                json!({"title":title,"description":description,"hashtags":hashtags}),
            );
        }
        Ok(json!({"platforms":platforms,"production_mode":"static"}))
    }

    async fn prepare_static(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
    ) -> Result<ProducedVideo, RunnerError> {
        if let Some(video) = self.existing_static(job).await? {
            return Ok(video);
        }
        if job.twitch_user_id.is_empty()
            || !job.twitch_user_id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(RunnerError::Source);
        }
        let input = self.source_file(job).await?;
        let media =
            ai_editor::inspect_media(&self.config.source_root, &input, &self.config.media_limits)
                .await
                .map_err(|_| RunnerError::Source)?;
        let version_no: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(version_no), 0) + 1 FROM social_produced_videos WHERE clip_id = $1")
            .bind(job.clip_id).fetch_one(&self.pool).await?;
        let directory = self
            .config
            .output_root
            .join(&job.twitch_user_id)
            .join(format!("production-{}", job.id));
        tokio::fs::create_dir_all(&directory).await?;
        let root = self.config.output_root.canonicalize()?;
        let directory = directory.canonicalize()?;
        if !directory.starts_with(&root) {
            return Err(RunnerError::Source);
        }
        let checkpoint = StaticCheckpoint {
            source_path: input,
            output_path: directory.join(format!("version-{version_no}.mp4")),
            source_media: media,
            max_duration_seconds: self.config.max_duration_seconds,
            rendering_ms: None,
        };
        let mut metadata = self.publication(job).await?;
        metadata["static"] = serde_json::to_value(&checkpoint)?;
        lease.ensure_held().await?;
        Ok(produced_video::insert_with_metadata(
            &self.pool,
            &NewProducedVideo {
                twitch_user_id: job.twitch_user_id.clone(),
                clip_id: job.clip_id,
                job_id: Some(job.id),
                mode: ProductionMode::Static,
                version_no,
                parent_version_id: None,
                variant_index: 0,
            },
            &metadata,
        )
        .await?)
    }

    async fn valid_output(
        &self,
        checkpoint: &StaticCheckpoint,
        path: &Path,
    ) -> Result<MediaInfo, RunnerError> {
        let mut limits = self.config.media_limits.clone();
        limits.max_source_bytes = limits.max_output_bytes;
        let media = ai_editor::inspect_media(&self.config.output_root, path, &limits)
            .await
            .map_err(|_| RunnerError::Source)?;
        let maximum = u64::try_from(checkpoint.max_duration_seconds)
            .map_err(|_| RunnerError::Source)?
            .checked_mul(1000)
            .ok_or(RunnerError::Source)?;
        let expected = checkpoint.source_media.duration_ms.min(maximum);
        if media.width != crate::layout::TARGET_WIDTH as u32
            || media.height != crate::layout::TARGET_HEIGHT as u32
            || (checkpoint.source_media.has_audio && !media.has_audio)
            || media.duration_ms.abs_diff(expected) > self.config.media_limits.max_duration_drift_ms
        {
            return Err(RunnerError::Source);
        }
        Ok(media)
    }

    async fn produce_static(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
    ) -> Result<(), RunnerError> {
        let video = self.prepare_static(lease, job).await?;
        if matches!(
            video.status.as_str(),
            "review" | "approved" | "scheduled" | "published"
        ) {
            if video.status != "published" {
                let checkpoint: StaticCheckpoint =
                    serde_json::from_value(video.metadata["static"].clone())?;
                self.valid_output(&checkpoint, &checkpoint.output_path)
                    .await?;
            }
            lease.ensure_held().await?;
            production_jobs::finish(
                &self.pool,
                &job.twitch_user_id,
                job.id,
                JobCompletion::Completed,
                &[],
                None,
            )
            .await?;
            return Ok(());
        }
        if video.status != "rendering" {
            return Err(RunnerError::Source);
        }
        let mut checkpoint: StaticCheckpoint =
            serde_json::from_value(video.metadata["static"].clone())?;
        checkpoint.rendering_ms = checkpoint
            .rendering_ms
            .or(job.metadata["static"]["rendering_ms"].as_i64());
        let source = ai_editor::authorized_path(
            &self.config.source_root,
            &checkpoint.source_path,
            self.config.media_limits.max_source_bytes,
        )
        .map_err(|_| RunnerError::Source)?;
        let root = self.config.output_root.canonicalize()?;
        let directory = checkpoint
            .output_path
            .parent()
            .ok_or(RunnerError::Source)?
            .canonicalize()?;
        if !directory.starts_with(&root) {
            return Err(RunnerError::Source);
        }
        let mut metadata = job.metadata.clone();
        metadata["static"] = json!({"produced_video_id":video.id});
        production_jobs::update_progress(
            &self.pool,
            &job.twitch_user_id,
            job.id,
            "rendering",
            &metadata,
        )
        .await?;
        if !checkpoint.output_path.exists() {
            let staged = checkpoint.output_path.with_extension("rendering.mp4");
            if self.valid_output(&checkpoint, &staged).await.is_err() {
                let started = Instant::now();
                render_clip_vertical_with_lease(
                    lease,
                    &self.video_processor,
                    &self.pool,
                    job.clip_id,
                    source.to_str().ok_or(RunnerError::Source)?,
                    staged.to_str().ok_or(RunnerError::Source)?,
                    checkpoint.max_duration_seconds,
                )
                .await?;
                checkpoint.rendering_ms = Some(
                    i64::try_from(started.elapsed().as_millis())
                        .map_err(|_| RunnerError::Source)?,
                );
            }
            self.valid_output(&checkpoint, &staged).await?;
            metadata["static"]["rendering_ms"] = json!(checkpoint.rendering_ms);
            production_jobs::update_progress(
                &self.pool,
                &job.twitch_user_id,
                job.id,
                "rendered",
                &metadata,
            )
            .await?;
            lease.ensure_held().await?;
            tokio::fs::hard_link(&staged, &checkpoint.output_path).await?;
            tokio::fs::remove_file(&staged).await?;
        }
        let media = self
            .valid_output(&checkpoint, &checkpoint.output_path)
            .await?;
        let mut metadata = video.metadata.clone();
        metadata["static"] = serde_json::to_value(&checkpoint)?;
        metadata["rendering_ms"] = json!(checkpoint.rendering_ms);
        lease.ensure_held().await?;
        produced_video::complete_render(
            &self.pool,
            &job.twitch_user_id,
            video.id,
            &RenderedVideo {
                file_path: checkpoint
                    .output_path
                    .to_str()
                    .ok_or(RunnerError::Source)?
                    .into(),
                duration_ms: i64::try_from(media.duration_ms).map_err(|_| RunnerError::Source)?,
                needs_review: true,
                review_reasons: vec![],
                metadata,
            },
        )
        .await?;
        production_jobs::finish(
            &self.pool,
            &job.twitch_user_id,
            job.id,
            JobCompletion::Completed,
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    async fn fail_static_video(&self, job: &ProductionJob) -> Result<(), RunnerError> {
        if let Some(video) = self.existing_static(job).await? {
            if video.status == "rendering" {
                produced_video::fail_render(
                    &self.pool,
                    &job.twitch_user_id,
                    video.id,
                    &["static_production_failed".into()],
                )
                .await?;
            }
        }
        Ok(())
    }
}
