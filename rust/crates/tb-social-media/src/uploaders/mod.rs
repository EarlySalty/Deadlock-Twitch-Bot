//! Plattform-Uploader (Port von `bot/social_media/uploaders/`).
//!
//! Gemeinsame Schnittstelle (`PlatformUploader`) + die plattformspezifischen
//! Implementierungen (TikTok/YouTube/Instagram). Jeder Uploader bekommt ein
//! bereits frisches Access-Token (der Token-Refresh läuft im `refresh_worker`,
//! nicht inline) und spricht die jeweilige Content-API per Roh-HTTP an.

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;

pub mod instagram;
pub mod tiktok;
pub mod youtube;

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("not authenticated")]
    NotAuthenticated,
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("request failed: {0}")]
    Request(String),
    #[error("api error: {0}")]
    Api(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Best-effort-Statistiken eines veröffentlichten Clips (mirror
/// `fetch_video_analytics`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyticsSnapshot {
    pub bucket: String,
    pub provider: String,
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub shares: i64,
    pub watch_time_seconds: Option<i64>,
    pub ctr_percent: Option<f64>,
    pub engagement_rate: Option<f64>,
}

impl AnalyticsSnapshot {
    /// Baut den Snapshot inkl. `engagement_rate = (likes+comments+shares)/views*100`
    /// (nur falls `views > 0`).
    pub fn build(bucket: &str, provider: &str, views: i64, likes: i64, comments: i64, shares: i64) -> Self {
        let engagement_rate = if views > 0 {
            Some((likes + comments + shares) as f64 / views as f64 * 100.0)
        } else {
            None
        };
        Self {
            bucket: bucket.to_string(),
            provider: provider.to_string(),
            views,
            likes,
            comments,
            shares,
            watch_time_seconds: None,
            ctr_percent: None,
            engagement_rate,
        }
    }
}

/// Gemeinsame Uploader-Schnittstelle.
#[async_trait]
pub trait PlatformUploader: Send + Sync {
    /// Plattformname (tiktok/youtube/instagram).
    fn platform_name(&self) -> &str;

    /// Prüft, ob das Video die Plattform-Anforderungen erfüllt.
    fn validate_video(&self, video_path: &str) -> Result<(), UploadError>;

    /// Lädt das Video hoch und liefert die externe Video-ID.
    async fn upload_video(
        &self,
        video_path: &str,
        title: &str,
        description: &str,
        hashtags: &[String],
    ) -> Result<String, UploadError>;

    /// Verarbeitungs-/Veröffentlichungsstatus (best-effort, `{}` bei Fehler).
    async fn get_video_status(&self, video_id: &str) -> Value;

    /// Best-effort-Statistiken eines veröffentlichten Clips.
    async fn fetch_video_analytics(&self, video_id: &str, bucket: &str) -> Result<AnalyticsSnapshot, UploadError>;
}

/// Datei existiert + Größe ≤ `max_mb` MB (für lokale Pfade).
pub fn validate_local_file(video_path: &str, max_mb: f64) -> Result<(), UploadError> {
    let path = Path::new(video_path);
    let meta = std::fs::metadata(path).map_err(|_| UploadError::Validation(format!("Video file not found: {video_path}")))?;
    let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
    if size_mb > max_mb {
        return Err(UploadError::Validation(format!("Video too large: {size_mb:.1}MB (max {max_mb}MB)")));
    }
    Ok(())
}

/// Schneidet einen String auf `max` Zeichen (code-point-basiert, mirror Pythons
/// `s[:max]`).
pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Liest einen Zähler aus JSON (Zahl oder String) mit Default 0.
pub(crate) fn as_count(value: Option<&Value>) -> i64 {
    value
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(0)
}

/// Erwartet HTTP 200 + JSON-Body (mirror Pythons `if resp.status != 200`).
pub(crate) async fn expect_ok_json(resp: reqwest::Response, ctx: &str) -> Result<Value, UploadError> {
    if resp.status().as_u16() == 200 {
        resp.json::<Value>().await.map_err(|e| UploadError::Request(e.to_string()))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(UploadError::Api(format!("{ctx} failed: {body}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engagement_rate_berechnung() {
        let s = AnalyticsSnapshot::build("d1", "prov", 200, 10, 5, 5);
        assert_eq!(s.engagement_rate, Some(10.0)); // (10+5+5)/200*100
        let z = AnalyticsSnapshot::build("d1", "prov", 0, 1, 1, 1);
        assert_eq!(z.engagement_rate, None);
    }

    #[test]
    fn truncate_und_count() {
        assert_eq!(truncate_chars("hello", 3), "hel");
        assert_eq!(truncate_chars("hi", 5), "hi");
        assert_eq!(as_count(Some(&serde_json::json!("42"))), 42);
        assert_eq!(as_count(Some(&serde_json::json!(7))), 7);
        assert_eq!(as_count(None), 0);
    }
}
