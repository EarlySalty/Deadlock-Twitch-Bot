use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_PART_SECONDS: i64 = 11 * 3600 + 30 * 60;

#[derive(Debug, Clone)]
pub struct VodArchiveConfig {
    pub download_dir: PathBuf,
    pub max_downloads_per_run: usize,
    pub max_uploads_per_run: usize,
    pub min_free_gb: u64,
    pub keep_local_days: i64,
    pub rate_limit: Option<String>,
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub rclone: PathBuf,
    pub drive_remote_base: String,
    pub download_timeout: Duration,
    pub interval: Duration,
    pub playlist_id: Option<String>,
    pub category_id: String,
    pub title_template: String,
    pub youtube: tb_config::vod_archive::VodArchiveOptions,
}

impl Default for VodArchiveConfig {
    fn default() -> Self {
        Self::from_options(&tb_config::vod_archive::VodArchiveOptions::default())
    }
}

impl VodArchiveConfig {
    pub fn from_options(options: &tb_config::vod_archive::VodArchiveOptions) -> Self {
        Self {
            download_dir: options.download_dir.clone(),
            max_downloads_per_run: options.max_downloads_per_run,
            max_uploads_per_run: options.max_uploads_per_run,
            min_free_gb: options.min_free_gb,
            keep_local_days: 0,
            rate_limit: options.rate_limit.clone(),
            yt_dlp: "yt-dlp".into(),
            ffmpeg: options.ffmpeg.clone(),
            ffprobe: options.ffprobe.clone(),
            rclone: options.rclone.clone(),
            drive_remote_base: "gdrive:Deadlock/Twitch-VODs".into(),
            download_timeout: Duration::from_secs(options.download_timeout_seconds),
            interval: Duration::from_secs(options.interval_hours * 3600),
            playlist_id: options.playlist_id.clone(),
            category_id: options.category_id.clone(),
            title_template: options.title_template.clone(),
            youtube: options.clone(),
        }
    }

    pub fn verzeichnis_fuer(&self, streamer_login: &str) -> PathBuf {
        self.download_dir.join(sicherer_ordnername(streamer_login))
    }
}

fn sicherer_ordnername(streamer_login: &str) -> String {
    let sauber: String = streamer_login
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sauber.is_empty() {
        "unbekannt".to_string()
    } else {
        sauber
    }
}

pub fn wurzel_oder_elternteil(pfad: &Path) -> PathBuf {
    if pfad.exists() {
        pfad.to_path_buf()
    } else {
        pfad.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zentraler_config_stand_und_sichere_ordner() {
        let options = tb_config::vod_archive::VodArchiveOptions::default();
        options.validate().unwrap();
        let cfg = VodArchiveConfig::from_options(&options);
        assert_eq!(cfg.max_uploads_per_run, 3);
        assert_eq!(cfg.interval, Duration::from_secs(12 * 3600));
        assert_eq!(
            cfg.verzeichnis_fuer("../../etc"),
            PathBuf::from("data/vod-archive/______etc")
        );
    }
}
