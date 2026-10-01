//! Fachlich geteilte Betriebswerte für Bot, Dashboard und Medienworker.
use crate::file::FileError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct KnowledgePaths {
    pub directory: PathBuf,
}
impl Default for KnowledgePaths {
    fn default() -> Self {
        Self {
            directory: "rust/knowledge".into(),
        }
    }
}
impl KnowledgePaths {
    pub(crate) fn validate(&self) -> Result<(), FileError> {
        let path = self
            .directory
            .to_str()
            .ok_or_else(|| FileError::invalid("knowledge.directory"))?;
        if path.trim().is_empty() || path.contains("://") || path.chars().any(char::is_control) {
            return Err(FileError::invalid("knowledge.directory"));
        }
        Ok(())
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaPublicOptions {
    pub youtube_audit_passed: bool,
    pub social_media_public_origin: String,
    pub llm_price_input_per_1k: f64,
    pub llm_price_output_per_1k: f64,
}

/// Existing archive operating parameters, shared by both streamer channels.
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct VodArchiveOptions {
    pub download_dir: PathBuf,
    pub max_downloads_per_run: usize,
    pub max_uploads_per_run: usize,
    pub min_free_gb: u64,
    pub keep_local_days: i64,
    pub rate_limit: Option<String>,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub download_timeout_seconds: u64,
    pub interval_hours: u64,
    pub playlist_id: Option<String>,
    pub category_id: String,
    pub title_template: String,
}

/// Existing engagement operating switches and bounds; defaults preserve source behavior.
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngagementOptions {
    pub streamlink_binary: String,
    pub ffmpeg_binary: String,
    pub persona_mode: String,
    pub stream_transcripts_enabled: bool,
    pub min_pause_seconds: f64,
    pub burst_limit: usize,
    pub burst_window_seconds: f64,
    pub load_limit_percent: f32,
    pub load_release_percent: f32,
    pub load_window_seconds: u64,
    pub load_max_hold_seconds: u64,
    pub transcript_capture_seconds: i64,
    pub transcript_interval_seconds: f64,
    pub transcript_quality: String,
    pub transcript_prompt_max_chars: i64,
    pub transcript_context_minutes: i64,
    pub transcript_context_limit: i64,
    pub transcript_retention_minutes: i64,
    pub transcript_keep_per_channel: i64,
    pub learn_enabled: bool,
    pub learn_login: String,
    pub learn_hot_minutes: i64,
    pub learn_capture_seconds: i64,
    pub learn_max_channels: i64,
    pub learn_idle_channels: i64,
    pub learn_retention_hours: i64,
    pub learn_window_pre_seconds: i64,
    pub learn_window_post_seconds: i64,
    pub learn_chat_lines: i64,
    pub learn_chat_minutes: i64,
}
impl Default for EngagementOptions {
    fn default() -> Self {
        Self {
            streamlink_binary: "streamlink".into(),
            ffmpeg_binary: "ffmpeg".into(),
            persona_mode: "veteran".into(),
            stream_transcripts_enabled: false,
            min_pause_seconds: 5.0,
            burst_limit: 3,
            burst_window_seconds: 60.0,
            load_limit_percent: 90.0,
            load_release_percent: 80.0,
            load_window_seconds: 240,
            load_max_hold_seconds: 1800,
            transcript_capture_seconds: 45,
            transcript_interval_seconds: 75.0,
            transcript_quality: "audio_only".into(),
            transcript_prompt_max_chars: 1200,
            transcript_context_minutes: 15,
            transcript_context_limit: 8,
            transcript_retention_minutes: 60,
            transcript_keep_per_channel: 40,
            learn_enabled: false,
            learn_login: "earlysalty".into(),
            learn_hot_minutes: 45,
            learn_capture_seconds: 30,
            learn_max_channels: 3,
            learn_idle_channels: 1,
            learn_retention_hours: 168,
            learn_window_pre_seconds: 45,
            learn_window_post_seconds: 10,
            learn_chat_lines: 8,
            learn_chat_minutes: 4,
        }
    }
}
impl Default for VodArchiveOptions {
    fn default() -> Self {
        Self {
            download_dir: "data/vod-archive".into(),
            max_downloads_per_run: 6,
            max_uploads_per_run: 3,
            min_free_gb: 80,
            keep_local_days: 0,
            rate_limit: None,
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            download_timeout_seconds: 21_600,
            interval_hours: 12,
            playlist_id: None,
            category_id: "20".into(),
            title_template: "{title} [{date}]{part}".into(),
        }
    }
}
impl Default for MediaPublicOptions {
    fn default() -> Self {
        Self {
            youtube_audit_passed: false,
            social_media_public_origin: "https://admin.deutsche-deadlock-community.de".into(),
            llm_price_input_per_1k: 0.0009,
            llm_price_output_per_1k: 0.0009,
        }
    }
}
impl MediaPublicOptions {
    pub(crate) fn validate(&self) -> Result<(), FileError> {
        if [self.llm_price_input_per_1k, self.llm_price_output_per_1k]
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(FileError::invalid("media.llm_prices"));
        }
        crate::global::public_url(
            &self.social_media_public_origin,
            "media.social_media_public_origin",
            false,
        )
    }
}
