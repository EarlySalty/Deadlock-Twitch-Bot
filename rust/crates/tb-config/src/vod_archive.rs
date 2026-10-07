use crate::{file::FileError, global::range};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct VodArchiveOptions {
    pub download_dir: PathBuf,
    pub max_downloads_per_run: usize,
    pub max_uploads_per_run: usize,
    pub min_free_gb: u64,
    pub rate_limit: Option<String>,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub rclone: PathBuf,
    pub download_timeout_seconds: u64,
    pub interval_hours: u64,
    pub category_id: String,
    pub title_template: String,
    pub playlist_id: Option<String>,
    pub youtube_poll_seconds: u64,
    pub youtube_check_hours: u64,
    pub youtube_processing_minutes: u64,
    pub youtube_error_minutes: u64,
    pub youtube_quota_hours: u64,
    pub youtube_requests_per_run: usize,
}

impl Default for VodArchiveOptions {
    fn default() -> Self {
        Self {
            download_dir: "data/vod-archive".into(),
            max_downloads_per_run: 6,
            max_uploads_per_run: 3,
            min_free_gb: 80,
            rate_limit: None,
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            rclone: "rclone".into(),
            download_timeout_seconds: 21_600,
            interval_hours: 12,
            category_id: "20".into(),
            title_template: "{title} [{date}]{part}".into(),
            playlist_id: None,
            youtube_poll_seconds: 60,
            youtube_check_hours: 24,
            youtube_processing_minutes: 10,
            youtube_error_minutes: 60,
            youtube_quota_hours: 24,
            youtube_requests_per_run: 20,
        }
    }
}

impl VodArchiveOptions {
    pub fn validate(&self) -> Result<(), FileError> {
        range(
            self.max_downloads_per_run as u64,
            1,
            100,
            "bot.vod_archive.max_downloads_per_run",
        )?;
        range(
            self.max_uploads_per_run as u64,
            1,
            3,
            "bot.vod_archive.max_uploads_per_run",
        )?;
        range(
            self.interval_hours,
            12,
            168,
            "bot.vod_archive.interval_hours",
        )?;
        range(
            self.download_timeout_seconds,
            60,
            86_400,
            "bot.vod_archive.download_timeout_seconds",
        )?;
        for (value, min, max, name) in [
            (self.youtube_poll_seconds, 30, 3600, "youtube_poll_seconds"),
            (self.youtube_check_hours, 1, 168, "youtube_check_hours"),
            (
                self.youtube_processing_minutes,
                5,
                1440,
                "youtube_processing_minutes",
            ),
            (
                self.youtube_error_minutes,
                10,
                1440,
                "youtube_error_minutes",
            ),
            (self.youtube_quota_hours, 1, 168, "youtube_quota_hours"),
            (
                self.youtube_requests_per_run as u64,
                3,
                100,
                "youtube_requests_per_run",
            ),
        ] {
            range(value, min, max, name)?;
        }
        for path in [
            &self.download_dir,
            &self.ffmpeg,
            &self.ffprobe,
            &self.rclone,
        ] {
            if path.as_os_str().is_empty()
                || path
                    .to_str()
                    .is_none_or(|value| value.chars().any(char::is_control))
            {
                return Err(FileError::invalid("bot.vod_archive.paths"));
            }
        }
        if self.category_id.is_empty() || !self.category_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(FileError::invalid("bot.vod_archive.category_id"));
        }
        if self.title_template.len() > 500 || self.title_template.chars().any(char::is_control) {
            return Err(FileError::invalid("bot.vod_archive.title_template"));
        }
        Ok(())
    }
}
