use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tb_config::{stt::SttConfig, BotConfigSnapshot};
use tb_llm::social_editor::{EditorConfig, SocialAiSection};

use super::{brain::BrainRetrieveConfig, stt::SttJobConfig, PlanLimits, ResourceLimits};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub source_root: PathBuf,
    pub work_root: PathBuf,
    pub output_root: PathBuf,
    pub stage_attempts: i32,
    pub worker_poll_ms: u64,
    pub job_deadline_ms: u64,
    pub recovery_limit: i64,
    pub list_limit: i64,
    pub max_snapshot_bytes: usize,
    pub max_skill_bytes: usize,
    pub resources: ResourceLimits,
    pub plan: PlanLimits,
    pub original_preview: OriginalPreviewConfig,
    pub copy: CopyLimits,
    pub stt: SttRuntimeConfig,
    pub brain: Option<BrainRuntimeConfig>,
    #[serde(default)]
    pub revisions: RevisionLimits,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RevisionLimits {
    pub max_feedback_bytes: usize,
    pub max_revisions: i64,
    pub max_auto_changed_percent: u8,
    pub max_variants: usize,
}

impl Default for RevisionLimits {
    fn default() -> Self {
        Self {
            max_feedback_bytes: 2000,
            max_revisions: 10,
            max_auto_changed_percent: 10,
            max_variants: 3,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginalPreviewConfig {
    pub max_width: u32,
    pub max_height: u32,
    pub max_output_bytes: u64,
    pub encoder_threads: u16,
    pub nice: u8,
    pub crf: u8,
    pub preset: String,
    pub process_deadline_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyLimits {
    pub max_title_chars: usize,
    pub max_description_chars: usize,
    pub max_hashtags: usize,
    pub max_hashtag_chars: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SttRuntimeConfig {
    pub binary: PathBuf,
    pub settings: SttConfig,
    pub nice: u8,
    pub startup_deadline_ms: u64,
    pub shutdown_deadline_ms: u64,
    pub poll_interval_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrainRuntimeConfig {
    pub endpoint: String,
    pub expected_release: String,
    pub deadline_ms: u64,
    pub max_response_bytes: usize,
    pub max_evidence_count: usize,
    pub max_evidence_bytes: usize,
    pub allowed_game_terms: Vec<String>,
}

impl BrainRuntimeConfig {
    pub fn retrieval(&self) -> BrainRetrieveConfig {
        BrainRetrieveConfig {
            endpoint: self.endpoint.clone(),
            expected_release: self.expected_release.clone(),
            deadline: Duration::from_millis(self.deadline_ms),
            max_response_bytes: self.max_response_bytes,
            max_evidence_count: self.max_evidence_count,
            max_evidence_bytes: self.max_evidence_bytes,
            allowed_game_terms: self.allowed_game_terms.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MediaRuntime {
    pub package_root: PathBuf,
    pub source_root: PathBuf,
    pub work_root: PathBuf,
    pub output_root: PathBuf,
    pub resources: ResourceLimits,
}

#[derive(Deserialize)]
struct MediaConfiguration {
    source_root: PathBuf,
    work_root: PathBuf,
    output_root: PathBuf,
    resources: ResourceLimits,
}

#[derive(Clone)]
pub struct EditorRuntime {
    pub configuration: EditorConfig,
    pub limits: RuntimeConfig,
    pub package_root: PathBuf,
    media: MediaRuntime,
}

fn valid_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.to_string_lossy().len() <= 4096
        && !path.to_string_lossy().chars().any(char::is_control)
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::RootDir))
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn packaged_root() -> Result<PathBuf, super::AiEditorError> {
    let executable = std::env::current_exe().map_err(|_| super::AiEditorError::Configuration)?;
    executable
        .ancestors()
        .nth(4)
        .map(Path::to_path_buf)
        .ok_or(super::AiEditorError::Configuration)
}

fn active_section() -> Result<&'static toml::Table, super::AiEditorError> {
    tb_config::runtime::settings()
        .ok()
        .and_then(|settings| settings.social_ai.as_ref())
        .ok_or(super::AiEditorError::Configuration)
}

fn typed_section<T: serde::de::DeserializeOwned>(
    section: &toml::Table,
) -> Result<T, super::AiEditorError> {
    toml::Value::Table(section.clone())
        .try_into()
        .map_err(|_| super::AiEditorError::Configuration)
}

fn validate_media(
    source_root: &Path,
    work_root: &Path,
    output_root: &Path,
    resources: &ResourceLimits,
) -> Result<(), super::AiEditorError> {
    if ![source_root, work_root, output_root]
        .into_iter()
        .all(valid_path)
        || source_root != output_root
        || !work_root.starts_with(output_root)
        || work_root == output_root
        || !(1..=512 * 1024 * 1024).contains(&resources.max_source_bytes)
        || !(1..=512 * 1024 * 1024).contains(&resources.max_output_bytes)
        || !(1024..=65_536).contains(&resources.max_probe_bytes)
        || !(1..=120_000).contains(&resources.max_media_duration_ms)
        || !(1920..=7680).contains(&resources.max_media_width)
        || !(1920..=4320).contains(&resources.max_media_height)
        || resources.max_frames == 0
        || !(2..=1920).contains(&resources.frame_width)
        || !resources.frame_width.is_multiple_of(2)
        || resources.max_frame_bytes == 0
        || !(1024..=262_144).contains(&resources.max_ass_bytes)
        || !(1000..=600_000).contains(&resources.process_deadline_ms)
        || !(1000..=600_000).contains(&resources.total_frames_deadline_ms)
        || !(1..=1000).contains(&resources.max_duration_drift_ms)
        || !(1..=3).contains(&resources.encoder_threads)
    {
        return Err(super::AiEditorError::Configuration);
    }
    Ok(())
}

impl MediaRuntime {
    pub fn load_packaged() -> Result<Self, super::AiEditorError> {
        Self::load(&packaged_root()?, active_section()?)
    }

    pub fn load(package_root: &Path, section: &toml::Table) -> Result<Self, super::AiEditorError> {
        #[derive(Deserialize)]
        struct Document {
            runtime: MediaConfiguration,
        }
        let document: Document = typed_section(section)?;
        Self::from_configuration(package_root, document.runtime)
    }

    fn from_configuration(
        package_root: &Path,
        configuration: MediaConfiguration,
    ) -> Result<Self, super::AiEditorError> {
        let root = package_root
            .canonicalize()
            .map_err(|_| super::AiEditorError::Configuration)?;
        validate_media(
            &configuration.source_root,
            &configuration.work_root,
            &configuration.output_root,
            &configuration.resources,
        )?;
        Ok(Self {
            source_root: resolve(&root, &configuration.source_root),
            work_root: resolve(&root, &configuration.work_root),
            output_root: resolve(&root, &configuration.output_root),
            resources: configuration.resources,
            package_root: root,
        })
    }
}

impl EditorRuntime {
    pub fn load_packaged() -> Result<Self, super::AiEditorError> {
        Self::load(&packaged_root()?, active_section()?)
    }

    pub fn validate_packaged(section: &toml::Table) -> Result<(), super::AiEditorError> {
        Self::load(&packaged_root()?, section).map(|_| ())
    }

    pub fn load(package_root: &Path, section: &toml::Table) -> Result<Self, super::AiEditorError> {
        let configuration = typed_section::<SocialAiSection>(section)?
            .into_editor_config()
            .map_err(|_| super::AiEditorError::Configuration)?;
        Self::from_configuration(package_root, configuration)
    }

    pub fn from_configuration(
        package_root: &Path,
        mut configuration: EditorConfig,
    ) -> Result<Self, super::AiEditorError> {
        configuration
            .validate()
            .map_err(|_| super::AiEditorError::Configuration)?;
        let mut limits: RuntimeConfig = serde_json::from_value(configuration.runtime.clone())
            .map_err(|_| super::AiEditorError::Configuration)?;
        let media = MediaRuntime::from_configuration(
            package_root,
            MediaConfiguration {
                source_root: limits.source_root.clone(),
                work_root: limits.work_root.clone(),
                output_root: limits.output_root.clone(),
                resources: limits.resources.clone(),
            },
        )?;
        limits.validate(&configuration)?;
        limits.source_root = media.source_root.clone();
        limits.work_root = media.work_root.clone();
        limits.output_root = media.output_root.clone();
        limits.stt.binary = resolve(&media.package_root, &limits.stt.binary);
        configuration.runtime =
            serde_json::to_value(&limits).map_err(|_| super::AiEditorError::Configuration)?;
        Ok(Self {
            configuration,
            limits,
            package_root: media.package_root.clone(),
            media,
        })
    }

    pub fn restore_configuration(
        &self,
        mut configuration: EditorConfig,
    ) -> Result<Self, super::AiEditorError> {
        configuration
            .restore_trusted_pricing(&self.configuration)
            .map_err(|_| super::AiEditorError::Configuration)?;
        Self::from_configuration(&self.package_root, configuration)
    }

    pub fn media(&self) -> &MediaRuntime {
        &self.media
    }
}

impl RuntimeConfig {
    pub fn validate(&self, configuration: &EditorConfig) -> Result<(), super::AiEditorError> {
        let resources = &self.resources;
        let plan = &self.plan;
        validate_media(
            &self.source_root,
            &self.work_root,
            &self.output_root,
            resources,
        )?;
        if !valid_path(&self.stt.binary)
            || !(1..=4000).contains(&self.revisions.max_feedback_bytes)
            || !(1..=20).contains(&self.revisions.max_revisions)
            || !(1..=20).contains(&self.revisions.max_auto_changed_percent)
            || !(1..=3).contains(&self.revisions.max_variants)
            || !(1..=3).contains(&self.stage_attempts)
            || !(100..=60_000).contains(&self.worker_poll_ms)
            || !(60_000..=3_600_000).contains(&self.job_deadline_ms)
            || !(1..=100).contains(&self.recovery_limit)
            || !(1..=50).contains(&self.list_limit)
            || !(1024..=32_768).contains(&self.max_snapshot_bytes)
            || !(128..=4096).contains(&self.max_skill_bytes)
            || resources.max_frames > configuration.provider.max_frames
            || resources.max_frame_bytes > configuration.provider.max_frame_bytes as u64
            || resources.process_deadline_ms > self.job_deadline_ms
            || resources.total_frames_deadline_ms > self.job_deadline_ms
            || plan.max_source_duration_ms > resources.max_media_duration_ms
            || plan.max_output_duration_ms > resources.max_media_duration_ms
            || plan.max_source_duration_drift_ms > resources.max_duration_drift_ms
            || plan.max_intervals > 64
            || plan.max_highlights > 64
            || plan.max_review_reasons > 32
            || plan.max_text_bytes > 512
            || plan.max_subtitle_segments > 512
            || plan.max_subtitle_text_bytes > 512
            || plan.max_plan_bytes > self.max_snapshot_bytes
            || !(320..=1920).contains(&self.original_preview.max_width)
            || !(180..=1080).contains(&self.original_preview.max_height)
            || !self.original_preview.max_width.is_multiple_of(2)
            || !self.original_preview.max_height.is_multiple_of(2)
            || !(1..=resources.max_output_bytes).contains(&self.original_preview.max_output_bytes)
            || !(1..=3).contains(&self.original_preview.encoder_threads)
            || !(1..=19).contains(&self.original_preview.nice)
            || !(18..=35).contains(&self.original_preview.crf)
            || !matches!(
                self.original_preview.preset.as_str(),
                "ultrafast" | "superfast" | "veryfast" | "faster" | "fast"
            )
            || !(1000..=600_000).contains(&self.original_preview.process_deadline_ms)
            || self.original_preview.process_deadline_ms > self.job_deadline_ms
            || !(1..=100).contains(&self.copy.max_title_chars)
            || !(1..=2200).contains(&self.copy.max_description_chars)
            || !(1..=30).contains(&self.copy.max_hashtags)
            || !(2..=64).contains(&self.copy.max_hashtag_chars)
            || self
                .brain
                .as_ref()
                .is_some_and(|brain| !brain.retrieval().validate())
        {
            return Err(super::AiEditorError::Configuration);
        }
        super::validation::check_plan_limits(plan)
            .map_err(|_| super::AiEditorError::Configuration)?;
        self.stt
            .settings
            .validate()
            .map_err(|_| super::AiEditorError::Configuration)?;
        if self
            .stt
            .settings
            .model_directory
            .as_ref()
            .is_none_or(|p| !p.is_absolute() || !valid_path(p) || p.starts_with("/home"))
            || self.stt.settings.remote_endpoint.is_some()
            || self.stt.settings.language.is_some()
            || self.stt.settings.port == 8791
            || self.stt.settings.threads > 4
            || !(1..=19).contains(&self.stt.nice)
            || !(100..=600_000).contains(&self.stt.startup_deadline_ms)
            || !(100..=60_000).contains(&self.stt.shutdown_deadline_ms)
            || !(10..=10_000).contains(&self.stt.poll_interval_ms)
            || self.stt.poll_interval_ms > self.stt.startup_deadline_ms
        {
            return Err(super::AiEditorError::Configuration);
        }
        Ok(())
    }
}

impl SttRuntimeConfig {
    pub fn seed(&self) -> Result<BotConfigSnapshot, super::AiEditorError> {
        #[derive(Serialize)]
        struct Document<'a> {
            schema_version: u32,
            twitch: Twitch,
            stt: &'a SttConfig,
        }
        #[derive(Serialize)]
        struct Twitch {
            bot_user_id: &'static str,
            notify_channel_id: &'static str,
            eventsub_callback_url: &'static str,
        }
        let document = Document {
            schema_version: 1,
            twitch: Twitch {
                bot_user_id: "1",
                notify_channel_id: "2",
                eventsub_callback_url: "https://example.invalid/callback",
            },
            stt: &self.settings,
        };
        let text = toml::to_string(&document).map_err(|_| super::AiEditorError::Configuration)?;
        BotConfigSnapshot::parse(&text, Path::new("/social-ai/stt-config.toml"))
            .map_err(|_| super::AiEditorError::Configuration)
    }

    pub fn job_config(&self) -> Result<SttJobConfig, super::AiEditorError> {
        Ok(SttJobConfig {
            binary: self.binary.clone(),
            model_directory: self
                .settings
                .model_directory
                .clone()
                .ok_or(super::AiEditorError::Configuration)?,
            legacy_cache_home: None,
            port: self.settings.port,
            threads: self.settings.threads,
            nice: self.nice,
            startup_deadline: Duration::from_millis(self.startup_deadline_ms),
            request_deadline: Duration::from_secs(self.settings.timeout_seconds),
            extraction_deadline: Duration::from_secs(self.settings.extraction_timeout_seconds),
            shutdown_deadline: Duration::from_millis(self.shutdown_deadline_ms),
            poll_interval: Duration::from_millis(self.poll_interval_ms),
            max_upload_bytes: self.settings.max_upload_bytes,
        })
    }
}
