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
    /// Tageskontingent der Plattform erschoepft. Kein Fehler im Code, sondern
    /// ein Grund, den Rest auf morgen zu verschieben — deshalb eine eigene
    /// Variante statt eines Api-Fehlers, den der Aufrufer nur am Text erkennt.
    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),
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
    pub fn build(
        bucket: &str,
        provider: &str,
        views: i64,
        likes: i64,
        comments: i64,
        shares: i64,
    ) -> Self {
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
    async fn fetch_video_analytics(
        &self,
        video_id: &str,
        bucket: &str,
    ) -> Result<AnalyticsSnapshot, UploadError>;
}

/// Datei existiert + Größe ≤ `max_mb` MB (für lokale Pfade).
pub fn validate_local_file(video_path: &str, max_mb: f64) -> Result<(), UploadError> {
    let path = Path::new(video_path);
    let meta = std::fs::metadata(path)
        .map_err(|_| UploadError::Validation(format!("Video file not found: {video_path}")))?;
    let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
    if size_mb > max_mb {
        return Err(UploadError::Validation(format!(
            "Video too large: {size_mb:.1}MB (max {max_mb}MB)"
        )));
    }
    Ok(())
}

/// Schneidet einen String auf `max` Zeichen (code-point-basiert, mirror Pythons
/// `s[:max]`).
pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Schneidet auf `max` UTF-16-Einheiten. TikTok zaehlt seine Caption-Grenze in
/// "UTF-16 runes", Emoji jenseits der BMP zaehlen dort also doppelt. Wer
/// code-point-basiert kuerzt, laeuft mit Emoji ueber die Grenze und bekommt den
/// Post abgelehnt. Geschnitten wird an einer Zeichengrenze, damit kein halbes
/// Surrogatpaar entsteht.
pub(crate) fn truncate_utf16(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let width = c.len_utf16();
        if used + width > max {
            break;
        }
        out.push(c);
        used += width;
    }
    out
}

/// Liest einen Zähler aus JSON (Zahl oder String) mit Default 0.
pub(crate) fn as_count(value: Option<&Value>) -> i64 {
    value
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(0)
}

/// Erwartet einen 2xx-Status und einen JSON-Body. 201 und 204 sind bei den
/// Content-APIs regulaere Erfolgsantworten, deshalb reicht die fruehere
/// Gleichheitspruefung auf 200 nicht.
pub(crate) async fn expect_ok_json(
    resp: reqwest::Response,
    ctx: &str,
) -> Result<Value, UploadError> {
    let status = resp.status();
    if status.is_success() {
        resp.json::<Value>()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))
    } else {
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 429 {
            return Err(UploadError::QuotaExceeded(format!("{ctx}: {body}")));
        }
        Err(UploadError::Api(format!("{ctx} failed: {status} {body}")))
    }
}

/// Die TikTok-Content-API liefert in jeder Antwort ein `error`-Objekt; der
/// Erfolgsfall ist `code == "ok"`. Ein 200 mit einem anderen Code ist ein
/// Fehler, der sonst als Erfolg durchlaeuft. `log_id` bleibt im Text, danach
/// fragt der TikTok-Support.
pub(crate) fn tiktok_error_guard(body: &Value, ctx: &str) -> Result<(), UploadError> {
    let error = match body.get("error") {
        Some(e) if e.is_object() => e,
        _ => return Ok(()),
    };
    let code = error.get("code").and_then(Value::as_str).unwrap_or("ok");
    if code == "ok" {
        return Ok(());
    }
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");
    let log_id = error.get("log_id").and_then(Value::as_str).unwrap_or("");
    let detail = format!("{ctx}: {code} {message} (log_id {log_id})");
    match code {
        "rate_limit_exceeded"
        | "spam_risk_too_many_posts"
        | "spam_risk_user_banned_from_posting"
        | "reached_active_user_cap"
        | "spam_risk_too_many_pending_share" => Err(UploadError::QuotaExceeded(detail)),
        "access_token_invalid" | "scope_not_authorized" | "scope_permission_missed" => {
            Err(UploadError::NotAuthenticated)
        }
        _ => Err(UploadError::Api(detail)),
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
    fn truncate_utf16_zaehlt_emoji_doppelt() {
        // "\u{1F600}" ist ein Surrogatpaar: zwei UTF-16-Einheiten, ein Zeichen.
        assert_eq!(truncate_utf16("a\u{1F600}b", 2), "a");
        assert_eq!(truncate_utf16("a\u{1F600}b", 3), "a\u{1F600}");
        assert_eq!(truncate_utf16("abc", 10), "abc");
    }

    #[test]
    fn tiktok_error_guard_trennt_ok_von_fehler() {
        assert!(tiktok_error_guard(&serde_json::json!({"error": {"code": "ok"}}), "x").is_ok());
        assert!(tiktok_error_guard(&serde_json::json!({"data": {}}), "x").is_ok());
        let quota = tiktok_error_guard(
            &serde_json::json!({"error": {"code": "spam_risk_too_many_posts", "message": "m", "log_id": "l"}}),
            "init",
        );
        assert!(matches!(quota, Err(UploadError::QuotaExceeded(_))));
        let auth = tiktok_error_guard(
            &serde_json::json!({"error": {"code": "scope_not_authorized", "message": "m", "log_id": "l"}}),
            "init",
        );
        assert!(matches!(auth, Err(UploadError::NotAuthenticated)));
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
