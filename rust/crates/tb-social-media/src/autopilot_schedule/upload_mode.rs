use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::io::AsyncReadExt;

use super::{AutopilotWorker, RunnerError};
use crate::{
    ai_editor::{self, MediaInfo},
    autopilot_settings::{
        self, CutoverState, PipelineMode, PipelinePolicy, PolicyScope, ReviewPolicy,
    },
    clip_templates,
    produced_video::{self, NewProducedVideo, ProducedVideo, RenderedVideo},
    production_jobs::{self, JobCompletion, JobSource, ProductionJob, ProductionMode},
    production_lease::ProductionLease,
    video_processor::bounded_ffmpeg_command,
};

const YOUTUBE_MAX_MS: u64 = 180_000;
const INSTAGRAM_MIN_MS: u64 = 3_000;
const INSTAGRAM_MAX_MS: u64 = 900_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Conversion {
    Exact,
    Remux,
    Transcode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadCheckpoint {
    source_path: PathBuf,
    source_sha256: String,
    output_path: PathBuf,
    source_media: MediaInfo,
    conversion: Conversion,
}

#[derive(Deserialize)]
struct Probe {
    format: ProbeFormat,
    streams: Vec<ProbeStream>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    format_name: String,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: String,
    codec_name: Option<String>,
    pix_fmt: Option<String>,
}

struct Artifact<'a> {
    checkpoint: &'a UploadCheckpoint,
    media: MediaInfo,
    sha256: String,
}

pub(super) enum StaticRoute {
    Legacy,
    Upload(Box<PipelinePolicy>),
    Unavailable(&'static str),
}

impl AutopilotWorker {
    pub(super) async fn static_route(
        &self,
        job: &ProductionJob,
    ) -> Result<StaticRoute, RunnerError> {
        if job.source == JobSource::Manual || job.metadata.get("static").is_some() {
            return Ok(StaticRoute::Legacy);
        }
        let prepared = match self.existing_static(job).await? {
            Some(video) if video.metadata.get("upload").is_none() => {
                return Ok(StaticRoute::Legacy)
            }
            Some(_) => true,
            None => false,
        };
        if !prepared && self.cutover_state(job).await? != CutoverState::Unified {
            return Ok(StaticRoute::Legacy);
        }
        let policy = self.frozen_policy(job, prepared).await?;
        Ok(match policy.pipeline_mode {
            PipelineMode::Upload | PipelineMode::UploadText => {
                StaticRoute::Upload(Box::new(policy))
            }
            PipelineMode::AiEdit => StaticRoute::Unavailable("ai_edit_mode_mismatch"),
        })
    }

    async fn cutover_state(&self, job: &ProductionJob) -> Result<CutoverState, RunnerError> {
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM social_pipeline_cutover WHERE twitch_user_id = $1",
        )
        .bind(&job.twitch_user_id)
        .fetch_optional(&self.pool)
        .await?;
        match state {
            Some(state) => Ok(serde_json::from_value(Value::String(state))?),
            None => Ok(CutoverState::Legacy),
        }
    }

    async fn frozen_policy(
        &self,
        job: &ProductionJob,
        prepared: bool,
    ) -> Result<PipelinePolicy, RunnerError> {
        if let (true, Some(policy)) = (prepared, &job.policy_snapshot) {
            return Ok(policy.clone());
        }
        let policy = autopilot_settings::load_policy(&self.pool, &job.twitch_user_id)
            .await
            .map_err(super::ScheduleError::from)?;
        let stored: Option<String> = sqlx::query_scalar(
            "UPDATE social_production_jobs SET policy_snapshot =
            CASE WHEN $4 THEN COALESCE(policy_snapshot, $3::text::jsonb) ELSE $3::text::jsonb END
            WHERE id = $1 AND twitch_user_id = $2 RETURNING policy_snapshot::text",
        )
        .bind(job.id)
        .bind(&job.twitch_user_id)
        .bind(serde_json::to_string(&policy)?)
        .bind(prepared)
        .fetch_optional(&self.pool)
        .await?;
        Ok(serde_json::from_str(&stored.ok_or(RunnerError::Source)?)?)
    }

    pub(super) async fn hold_unavailable_mode(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
        reason: &str,
    ) -> Result<(), RunnerError> {
        lease.ensure_held().await?;
        production_jobs::finish(
            &self.pool,
            &job.twitch_user_id,
            job.id,
            JobCompletion::Review,
            &[reason.into()],
            Some(unavailable_message(reason)),
        )
        .await?;
        Ok(())
    }

    pub(super) async fn produce_upload(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
        policy: &PipelinePolicy,
    ) -> Result<(), RunnerError> {
        let video = match self.existing_static(job).await? {
            Some(video) => video,
            None => self.prepare_upload(lease, job, policy).await?,
        };
        let video = self.inherit_static_slot(lease, job, video).await?;
        if matches!(
            video.status.as_str(),
            "review" | "approved" | "scheduled" | "published"
        ) {
            if video.status != "published" {
                let checkpoint = self.upload_checkpoint(job, &video)?;
                let recorded = video.metadata["upload_artifact"]["sha256"]
                    .as_str()
                    .ok_or(RunnerError::Source)?;
                self.verified_artifact(&checkpoint, &checkpoint.output_path, Some(recorded))
                    .await?;
            }
            lease.ensure_held().await?;
            self.freeze_video_policy(job, &video, policy).await?;
            production_jobs::finish(
                &self.pool,
                &job.twitch_user_id,
                job.id,
                JobCompletion::Completed,
                &review_reasons(&video.review_reasons),
                None,
            )
            .await?;
            return Ok(());
        }
        if video.status != "rendering" {
            return Err(RunnerError::Source);
        }
        let checkpoint = self.upload_checkpoint(job, &video)?;
        let output_sha256 = self.build_artifact(lease, job, &video, &checkpoint).await?;
        let media = self
            .verified_artifact(&checkpoint, &checkpoint.output_path, Some(&output_sha256))
            .await?;
        if policy.pipeline_mode == PipelineMode::UploadText
            && !self.resolve_text(lease, job, policy, &video).await?
        {
            return Ok(());
        }
        let artifact = Artifact {
            checkpoint: &checkpoint,
            media,
            sha256: output_sha256,
        };
        self.complete_upload(lease, job, policy, &video, artifact)
            .await
    }

    async fn prepare_upload(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
        policy: &PipelinePolicy,
    ) -> Result<ProducedVideo, RunnerError> {
        if job.twitch_user_id.is_empty()
            || !job.twitch_user_id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(RunnerError::Source);
        }
        let input = self.source_file(job).await?;
        let source_media =
            ai_editor::inspect_media(&self.config.source_root, &input, &self.config.media_limits)
                .await
                .map_err(|_| RunnerError::Source)?;
        let conversion = self.conversion_for(&input).await?;
        let source_sha256 = file_sha256(&input).await?;
        let version_no: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(version_no), 0) + 1 FROM social_produced_videos WHERE clip_id = $1")
            .bind(job.clip_id).fetch_one(&self.pool).await?;
        let directory = self.production_directory(job).await?;
        let checkpoint = UploadCheckpoint {
            source_path: input,
            source_sha256,
            output_path: directory.join(format!("upload-{version_no}.mp4")),
            source_media,
            conversion,
        };
        let mut metadata = self.upload_publication(job, policy).await?;
        metadata["upload"] = serde_json::to_value(&checkpoint)?;
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

    async fn production_directory(&self, job: &ProductionJob) -> Result<PathBuf, RunnerError> {
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
        Ok(directory)
    }

    async fn upload_publication(
        &self,
        job: &ProductionJob,
        policy: &PipelinePolicy,
    ) -> Result<Value, RunnerError> {
        let defaults_empty = policy.text_defaults.shared.description.is_empty()
            && policy.text_defaults.shared.hashtags.is_empty()
            && policy
                .text_defaults
                .per_platform
                .values()
                .all(|text| text.description.is_empty() && text.hashtags.is_empty());
        let (mut title, mut description, mut hashtags) = self.clip_text(job).await?;
        if description.is_empty() && defaults_empty {
            self.apply_default_template(job).await?;
            (title, description, hashtags) = self.clip_text(job).await?;
        }
        let mut platforms = serde_json::Map::new();
        for platform in self.publication_platforms(job).await? {
            let defaults = match policy.metadata_scope {
                PolicyScope::PerPlatform => policy
                    .text_defaults
                    .per_platform
                    .get(&platform)
                    .unwrap_or(&policy.text_defaults.shared),
                PolicyScope::Shared => &policy.text_defaults.shared,
            };
            let description = if description.is_empty() {
                defaults.description.clone()
            } else {
                description.clone()
            };
            let hashtags = if hashtags.is_empty() {
                defaults.hashtags.clone()
            } else {
                hashtags.clone()
            };
            platforms.insert(
                platform,
                json!({"title":title,"description":description,"hashtags":hashtags}),
            );
        }
        Ok(json!({
            "platforms": platforms,
            "production_mode": "upload",
            "pipeline_mode": policy.pipeline_mode,
            "review_policy": policy.review_policy,
        }))
    }

    async fn clip_text(
        &self,
        job: &ProductionJob,
    ) -> Result<(String, String, Vec<String>), RunnerError> {
        let row = sqlx::query(
            "SELECT COALESCE(NULLIF(custom_title, ''), clip_title, '') AS title,
            COALESCE(custom_description, '') AS description, COALESCE(hashtags, '[]') AS hashtags
            FROM twitch_clips_social_media WHERE id = $1 AND twitch_user_id = $2",
        )
        .bind(job.clip_id)
        .bind(&job.twitch_user_id)
        .fetch_one(&self.pool)
        .await?;
        let hashtags: Vec<String> =
            serde_json::from_str(row.try_get::<String, _>("hashtags")?.as_str())?;
        Ok((row.try_get("title")?, row.try_get("description")?, hashtags))
    }

    async fn apply_default_template(&self, job: &ProductionJob) -> Result<(), RunnerError> {
        let Some(template) =
            clip_templates::get_streamer_templates(&self.pool, &job.twitch_user_id)
                .await
                .into_iter()
                .find(|template| template.is_default)
        else {
            return Ok(());
        };
        if clip_templates::apply_template_to_clip(&self.pool, job.clip_id, template.id, false).await
        {
            Ok(())
        } else {
            Err(RunnerError::Source)
        }
    }

    fn upload_checkpoint(
        &self,
        job: &ProductionJob,
        video: &ProducedVideo,
    ) -> Result<UploadCheckpoint, RunnerError> {
        let checkpoint: UploadCheckpoint =
            serde_json::from_value(video.metadata["upload"].clone())?;
        let expected = self
            .config
            .output_root
            .canonicalize()?
            .join(&job.twitch_user_id)
            .join(format!("production-{}", job.id))
            .join(format!("upload-{}.mp4", video.version_no));
        if checkpoint.output_path != expected
            || video.job_id != Some(job.id)
            || video.clip_id != job.clip_id
            || video.twitch_user_id != job.twitch_user_id
            || video.mode != ProductionMode::Static
            || video.version_no <= 0
            || video
                .file_path
                .as_deref()
                .is_some_and(|path| Path::new(path) != expected)
        {
            return Err(RunnerError::Source);
        }
        Ok(checkpoint)
    }

    async fn build_artifact(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
        video: &ProducedVideo,
        checkpoint: &UploadCheckpoint,
    ) -> Result<String, RunnerError> {
        let directory = checkpoint
            .output_path
            .parent()
            .ok_or(RunnerError::Source)?
            .canonicalize()?;
        if !directory.starts_with(self.config.output_root.canonicalize()?) {
            return Err(RunnerError::Source);
        }
        let recorded = job.metadata["upload"]["output_sha256"]
            .as_str()
            .map(str::to_owned);
        let mut metadata = job.metadata.clone();
        metadata["upload"] = json!({"produced_video_id":video.id,"output_sha256":recorded});
        production_jobs::update_progress(
            &self.pool,
            &job.twitch_user_id,
            job.id,
            "rendering",
            &metadata,
        )
        .await?;
        if checkpoint.output_path.exists() {
            let current = file_sha256(&checkpoint.output_path).await?;
            return match recorded {
                Some(recorded) if recorded != current => Err(RunnerError::Source),
                _ => Ok(current),
            };
        }
        let staged = checkpoint.output_path.with_extension("staged.mp4");
        let reusable = self
            .verified_artifact(checkpoint, &staged, recorded.as_deref())
            .await
            .is_ok();
        if !reusable {
            let source = ai_editor::authorized_path(
                &self.config.source_root,
                &checkpoint.source_path,
                self.config.media_limits.max_source_bytes,
            )
            .map_err(|_| RunnerError::Source)?;
            if file_sha256(&source).await? != checkpoint.source_sha256 {
                return Err(RunnerError::Source);
            }
            lease.ensure_held().await?;
            self.write_artifact(checkpoint.conversion, &source, &staged)
                .await?;
            self.verified_artifact(checkpoint, &staged, None).await?;
        }
        let output_sha256 = file_sha256(&staged).await?;
        metadata["upload"]["output_sha256"] = json!(output_sha256);
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
        Ok(output_sha256)
    }

    async fn complete_upload(
        &self,
        lease: &mut ProductionLease,
        job: &ProductionJob,
        policy: &PipelinePolicy,
        video: &ProducedVideo,
        artifact: Artifact<'_>,
    ) -> Result<(), RunnerError> {
        let Artifact {
            checkpoint,
            media,
            sha256: output_sha256,
        } = artifact;
        let platforms: Vec<String> = video.metadata["platforms"]
            .as_object()
            .map(|platforms| platforms.keys().cloned().collect())
            .unwrap_or_default();
        let tasks = preparation_tasks(&media, &platforms);
        let needs_review = !tasks.is_empty() || policy.review_policy == ReviewPolicy::Required;
        let metadata = json!({
            "upload_artifact": {
                "sha256": output_sha256,
                "conversion": checkpoint.conversion,
                "media": media,
            },
            "preparation_tasks": tasks,
            "veto_deadline": (policy.review_policy == ReviewPolicy::VetoUntilDeadline)
                .then_some(video.scheduled_at)
                .flatten(),
        });
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
                needs_review,
                review_reasons: tasks.clone(),
                metadata,
            },
        )
        .await?;
        self.freeze_video_policy(job, video, policy).await?;
        production_jobs::finish(
            &self.pool,
            &job.twitch_user_id,
            job.id,
            JobCompletion::Completed,
            &tasks,
            None,
        )
        .await?;
        Ok(())
    }

    async fn freeze_video_policy(
        &self,
        job: &ProductionJob,
        video: &ProducedVideo,
        policy: &PipelinePolicy,
    ) -> Result<(), RunnerError> {
        sqlx::query(
            "UPDATE social_produced_videos SET policy_snapshot = COALESCE(policy_snapshot, $3::text::jsonb)
            WHERE id = $1 AND twitch_user_id = $2",
        )
        .bind(video.id)
        .bind(&job.twitch_user_id)
        .bind(serde_json::to_string(policy)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn verified_artifact(
        &self,
        checkpoint: &UploadCheckpoint,
        path: &Path,
        expected_sha256: Option<&str>,
    ) -> Result<MediaInfo, RunnerError> {
        let mut limits = self.config.media_limits.clone();
        limits.max_source_bytes = limits.max_output_bytes;
        let media = ai_editor::inspect_media(&self.config.output_root, path, &limits)
            .await
            .map_err(|_| RunnerError::Source)?;
        let source = &checkpoint.source_media;
        if media.width != source.width
            || media.height != source.height
            || media.has_audio != source.has_audio
            || media.duration_ms.abs_diff(source.duration_ms)
                > self.config.media_limits.max_duration_drift_ms
        {
            return Err(RunnerError::Source);
        }
        let digest = if expected_sha256.is_some() || checkpoint.conversion == Conversion::Exact {
            Some(file_sha256(path).await?)
        } else {
            None
        };
        if expected_sha256.is_some_and(|expected| digest.as_deref() != Some(expected))
            || (checkpoint.conversion == Conversion::Exact
                && digest.as_deref() != Some(checkpoint.source_sha256.as_str()))
        {
            return Err(RunnerError::Source);
        }
        Ok(media)
    }

    async fn conversion_for(&self, path: &Path) -> Result<Conversion, RunnerError> {
        let mut command = tokio::process::Command::new("ffprobe");
        command
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=format_name:stream=codec_type,codec_name,pix_fmt",
                "-of",
                "json",
            ])
            .arg(path)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        let output = tokio::time::timeout(
            Duration::from_millis(self.config.media_limits.process_deadline_ms),
            command.output(),
        )
        .await
        .map_err(|_| RunnerError::Source)??;
        if !output.status.success()
            || output.stdout.len() > self.config.media_limits.max_probe_bytes
        {
            return Err(RunnerError::Source);
        }
        let probe: Probe = serde_json::from_slice(&output.stdout)?;
        Ok(conversion(path, &probe))
    }

    async fn write_artifact(
        &self,
        conversion: Conversion,
        source: &Path,
        staged: &Path,
    ) -> Result<(), RunnerError> {
        if tokio::fs::try_exists(staged).await? {
            tokio::fs::remove_file(staged).await?;
        }
        if conversion == Conversion::Exact {
            tokio::fs::copy(source, staged).await?;
            return Ok(());
        }
        let limits = &self.config.media_limits;
        let mut command = bounded_ffmpeg_command("ffmpeg", limits.max_output_bytes);
        command
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
            .arg(source)
            .args(["-map", "0:v:0", "-map", "0:a?", "-map_metadata", "0"]);
        if conversion == Conversion::Remux {
            command.args(["-c", "copy"]);
        } else {
            command
                .args(["-c:v", "libx264", "-preset", "medium", "-crf", "18"])
                .args(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k"])
                .arg("-threads")
                .arg(limits.encoder_threads.to_string());
        }
        command
            .args(["-movflags", "+faststart", "-f", "mp4"])
            .arg(staged)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let status = tokio::time::timeout(
            Duration::from_millis(limits.process_deadline_ms),
            command.status(),
        )
        .await
        .map_err(|_| RunnerError::Source)??;
        if status.success() {
            Ok(())
        } else {
            Err(RunnerError::Source)
        }
    }
}

fn conversion(path: &Path, probe: &Probe) -> Conversion {
    let videos: Vec<&ProbeStream> = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type == "video")
        .collect();
    let codecs_ready = videos.len() == 1
        && videos.iter().all(|stream| {
            stream.codec_name.as_deref() == Some("h264")
                && stream.pix_fmt.as_deref() == Some("yuv420p")
        })
        && probe
            .streams
            .iter()
            .filter(|stream| stream.codec_type == "audio")
            .all(|stream| stream.codec_name.as_deref() == Some("aac"));
    let only_media = probe
        .streams
        .iter()
        .all(|stream| matches!(stream.codec_type.as_str(), "video" | "audio"));
    let mp4 = probe
        .format
        .format_name
        .split(',')
        .any(|name| name == "mp4")
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"));
    match (codecs_ready, mp4 && only_media) {
        (true, true) => Conversion::Exact,
        (true, false) => Conversion::Remux,
        (false, _) => Conversion::Transcode,
    }
}

fn preparation_tasks(media: &MediaInfo, platforms: &[String]) -> Vec<String> {
    let mut tasks = Vec::new();
    for platform in platforms {
        match platform.as_str() {
            "youtube" => {
                if media.width > media.height {
                    tasks.push("youtube_needs_vertical_layout".to_owned());
                }
                if media.duration_ms > YOUTUBE_MAX_MS {
                    tasks.push("youtube_too_long".to_owned());
                }
            }
            "instagram" if !(INSTAGRAM_MIN_MS..=INSTAGRAM_MAX_MS).contains(&media.duration_ms) => {
                tasks.push("instagram_duration_out_of_range".to_owned());
            }
            _ => {}
        }
    }
    tasks
}

fn unavailable_message(reason: &str) -> &'static str {
    match reason {
        "ai_edit_mode_mismatch" => "Für diesen Kanal ist KI-Videoschnitt eingestellt, die KI-Bearbeitung ist aber aus. Bitte prüfe die Einstellungen und starte den Auftrag dann erneut.",
        _ => "Dieser Produktionsmodus ist noch nicht verfügbar. Bitte wähle einen anderen Modus und starte den Auftrag dann erneut.",
    }
}

fn review_reasons(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|reasons| {
            reasons
                .iter()
                .filter_map(|reason| reason.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn file_sha256(path: &Path) -> Result<String, RunnerError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
