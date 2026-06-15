//! YouTube-Shorts-Uploader (Port von `uploaders/youtube.py`).
//!
//! Das Python-Original nutzt die `google-api-python-client`-Bibliothek; hier
//! ist der **resumable Upload** der YouTube Data API v3 als Roh-HTTP umgesetzt:
//! 1. POST `…/videos?uploadType=resumable&part=snippet,status` (JSON-Body) →
//!    Upload-Session-URL im `Location`-Header.
//! 2. PUT der Video-Bytes an diese Session-URL → Antwort enthält die Video-ID.
//! Token-Refresh läuft im `refresh_worker` (kein inline 401-Retry wie im
//! google-Client) — der Uploader bekommt ein frisches Access-Token.

use async_trait::async_trait;
use reqwest::header::{CONTENT_TYPE, LOCATION};
use serde_json::{json, Value};

use super::{as_count, expect_ok_json, truncate_chars, validate_local_file, AnalyticsSnapshot, PlatformUploader, UploadError};
use crate::video_processor::format_hashtags;

const DESCRIPTION_MAX: usize = 5000;
const TITLE_MAX: usize = 100;
const TAGS_MAX: usize = 500;
const CATEGORY_GAMING: &str = "20";
const DEFAULT_PRIVACY: &str = "public";
const DEFAULT_UPLOAD_BASE: &str = "https://www.googleapis.com/upload/youtube/v3";
const DEFAULT_API_BASE: &str = "https://www.googleapis.com/youtube/v3";

/// YouTube-Shorts-Uploader.
pub struct YouTubeUploader {
    access_token: String,
    upload_base: String,
    api_base: String,
    http: reqwest::Client,
}

impl YouTubeUploader {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            upload_base: DEFAULT_UPLOAD_BASE.to_string(),
            api_base: DEFAULT_API_BASE.to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Überschreibt Upload- und API-Basis-URL (für Tests).
    pub fn with_bases(mut self, upload_base: impl Into<String>, api_base: impl Into<String>) -> Self {
        self.upload_base = upload_base.into();
        self.api_base = api_base.into();
        self
    }

    /// Startet die resumable Session und liefert die Upload-URL.
    async fn init_resumable(&self, body: &Value) -> Result<String, UploadError> {
        let resp = self
            .http
            .post(format!("{}/videos", self.upload_base))
            .query(&[("uploadType", "resumable"), ("part", "snippet,status")])
            .bearer_auth(&self.access_token)
            .header("X-Upload-Content-Type", "video/mp4")
            .json(body)
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        if resp.status().as_u16() != 200 {
            let err = resp.text().await.unwrap_or_default();
            return Err(UploadError::Api(format!("YouTube resumable init failed: {err}")));
        }
        resp.headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("YouTube resumable init: no Location header".to_string()))
    }

    /// Lädt die Bytes per PUT hoch und liefert die Video-ID.
    async fn put_bytes(&self, session_url: &str, video_path: &str) -> Result<String, UploadError> {
        let bytes = tokio::fs::read(video_path).await?;
        let resp = self
            .http
            .put(session_url)
            .header(CONTENT_TYPE, "video/mp4")
            .body(bytes)
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(UploadError::Api(format!("YouTube upload failed: {err}")));
        }
        let data: Value = resp.json().await.map_err(|e| UploadError::Request(e.to_string()))?;
        data["id"].as_str().map(str::to_string).ok_or_else(|| UploadError::Api("No video id in response".to_string()))
    }
}

#[async_trait]
impl PlatformUploader for YouTubeUploader {
    fn platform_name(&self) -> &str {
        "youtube"
    }

    fn validate_video(&self, video_path: &str) -> Result<(), UploadError> {
        // YouTube: praktisch unbegrenzt (256 GB) → nur Existenz prüfen.
        validate_local_file(video_path, f64::INFINITY)
    }

    async fn upload_video(&self, video_path: &str, title: &str, description: &str, hashtags: &[String]) -> Result<String, UploadError> {
        if self.access_token.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        self.validate_video(video_path)?;
        let full_description = truncate_chars(
            &format!("{description}\n\n{}\n\n#Shorts", format_hashtags(hashtags)),
            DESCRIPTION_MAX,
        );
        let tags: Vec<&String> = hashtags.iter().take(TAGS_MAX).collect();
        let body = json!({
            "snippet": {
                "title": truncate_chars(title, TITLE_MAX),
                "description": full_description,
                "tags": tags,
                "categoryId": CATEGORY_GAMING,
            },
            "status": { "privacyStatus": DEFAULT_PRIVACY, "selfDeclaredMadeForKids": false },
        });
        let session_url = self.init_resumable(&body).await?;
        self.put_bytes(&session_url, video_path).await
    }

    async fn get_video_status(&self, video_id: &str) -> Value {
        let result = async {
            let resp = self
                .http
                .get(format!("{}/videos", self.api_base))
                .query(&[("part", "status,processingDetails"), ("id", video_id)])
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|e| UploadError::Request(e.to_string()))?;
            let data = expect_ok_json(resp, "YouTube status fetch").await?;
            let item = data["items"].get(0).cloned().unwrap_or_else(|| json!({}));
            if item.is_null() || item.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                return Ok::<Value, UploadError>(json!({}));
            }
            Ok(json!({
                "status": item["status"]["uploadStatus"],
                "processing_status": item["processingDetails"]["processingStatus"],
            }))
        }
        .await;
        result.unwrap_or_else(|_| json!({}))
    }

    async fn fetch_video_analytics(&self, video_id: &str, bucket: &str) -> Result<AnalyticsSnapshot, UploadError> {
        if self.access_token.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        let resp = self
            .http
            .get(format!("{}/videos", self.api_base))
            .query(&[("part", "statistics"), ("id", video_id)])
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = expect_ok_json(resp, "YouTube analytics").await?;
        let stats = data["items"].get(0).map(|i| i["statistics"].clone()).unwrap_or_else(|| json!({}));
        Ok(AnalyticsSnapshot::build(
            bucket,
            "youtube_data_api_v3",
            as_count(stats.get("viewCount")),
            as_count(stats.get("likeCount")),
            as_count(stats.get("commentCount")),
            as_count(stats.get("shareCount")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn temp_video() -> String {
        let p = std::env::temp_dir().join("tb_youtube_test_clip.mp4");
        tokio::fs::write(&p, b"fake-video-bytes").await.unwrap();
        p.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn resumable_upload_zwei_schritte() {
        let server = MockServer::start().await;
        let session_url = format!("{}/session/abc", server.uri());
        // Schritt 1: init liefert Location-Header.
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(200).insert_header("Location", session_url.as_str()))
            .mount(&server)
            .await;
        // Schritt 2: PUT der Bytes → Video-ID.
        Mock::given(method("PUT"))
            .and(path("/session/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-123" })))
            .mount(&server)
            .await;

        let uploader = YouTubeUploader::new("tok").with_bases(server.uri(), server.uri());
        let video = temp_video().await;
        let id = uploader.upload_video(&video, "Titel", "Desc", &["deadlock".into()]).await.unwrap();
        assert_eq!(id, "yt-123");
    }

    #[tokio::test]
    async fn fehlerpfade() {
        let video = temp_video().await;
        // Kein Token.
        assert!(matches!(YouTubeUploader::new("").upload_video(&video, "t", "d", &[]).await, Err(UploadError::NotAuthenticated)));
        // Fehlende Datei.
        assert!(matches!(YouTubeUploader::new("tok").upload_video("/nope.mp4", "t", "d", &[]).await, Err(UploadError::Validation(_))));
        // init non-200.
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/videos")).respond_with(ResponseTemplate::new(401).set_body_string("unauth")).mount(&server).await;
        let up = YouTubeUploader::new("tok").with_bases(server.uri(), server.uri());
        assert!(matches!(up.upload_video(&video, "t", "d", &[]).await, Err(UploadError::Api(_))));
    }

    #[tokio::test]
    async fn analytics_aus_statistics() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{ "statistics": { "viewCount": "500", "likeCount": "40", "commentCount": "10" } }]
            })))
            .mount(&server)
            .await;
        let uploader = YouTubeUploader::new("tok").with_bases(server.uri(), server.uri());
        let stats = uploader.fetch_video_analytics("yt-1", "d1").await.unwrap();
        assert_eq!(stats.views, 500);
        assert_eq!(stats.likes, 40);
        assert_eq!(stats.shares, 0); // shareCount fehlt → 0
        assert_eq!(stats.provider, "youtube_data_api_v3");
        // engagement = (40+10+0)/500*100 = 10.
        assert_eq!(stats.engagement_rate, Some(10.0));
    }

    #[tokio::test]
    async fn status_mapping() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{ "status": { "uploadStatus": "uploaded" }, "processingDetails": { "processingStatus": "processing" } }]
            })))
            .mount(&server)
            .await;
        let uploader = YouTubeUploader::new("tok").with_bases(server.uri(), server.uri());
        let s = uploader.get_video_status("yt-1").await;
        assert_eq!(s["status"], "uploaded");
        assert_eq!(s["processing_status"], "processing");
    }
}
