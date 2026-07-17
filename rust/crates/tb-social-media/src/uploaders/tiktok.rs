//! TikTok-Uploader (Port von `uploaders/tiktok.py`).
//!
//! Nutzt die TikTok Content Posting API (Business-Account nötig): Upload in drei
//! Schritten (init → Chunks → publish), dazu Status- und Analytics-Abfrage.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    as_count, expect_ok_json, truncate_chars, validate_local_file, AnalyticsSnapshot,
    PlatformUploader, UploadError,
};
use crate::video_processor::format_hashtags;

const MAX_FILE_MB: f64 = 287.6;
const CAPTION_MAX: usize = 2200;
const TITLE_MAX: usize = 150;
const CHUNK_SIZE: usize = 10 * 1024 * 1024;
const DEFAULT_API_BASE: &str = "https://open.tiktokapis.com/v2";
const DEFAULT_PRIVACY: &str = "PUBLIC_TO_EVERYONE";

/// TikTok Content-Posting-Uploader.
pub struct TikTokUploader {
    access_token: String,
    api_base: String,
    http: reqwest::Client,
}

impl TikTokUploader {
    /// `client_key`/`client_secret` werden für den Token-Tausch nicht mehr
    /// benötigt (Token kommt fertig vom credential_manager) — der Uploader hält
    /// nur das Access-Token.
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            api_base: DEFAULT_API_BASE.to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Überschreibt die API-Basis-URL (für Tests).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    async fn init_upload(&self) -> Result<String, UploadError> {
        let resp = self
            .http
            .post(format!("{}/post/publish/video/init/", self.api_base))
            .bearer_auth(&self.access_token)
            .json(&json!({ "source_info": { "source": "FILE_UPLOAD" } }))
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = expect_ok_json(resp, "TikTok init upload").await?;
        data["data"]["upload_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("No upload_id in response".to_string()))
    }

    async fn upload_chunks(&self, video_path: &str, upload_id: &str) -> Result<(), UploadError> {
        let bytes = tokio::fs::read(video_path).await?;
        for (chunk_index, chunk) in bytes.chunks(CHUNK_SIZE).enumerate() {
            let part = reqwest::multipart::Part::bytes(chunk.to_vec())
                .file_name(format!("chunk_{chunk_index}.mp4"))
                .mime_str("video/mp4")
                .map_err(|e| UploadError::Request(e.to_string()))?;
            let form = reqwest::multipart::Form::new().part("video", part);
            let resp = self
                .http
                .post(format!("{}/post/publish/video/chunk/", self.api_base))
                .bearer_auth(&self.access_token)
                .query(&[
                    ("upload_id", upload_id),
                    ("chunk_index", &chunk_index.to_string()),
                ])
                .multipart(form)
                .send()
                .await
                .map_err(|e| UploadError::Request(e.to_string()))?;
            if resp.status().as_u16() != 200 {
                let body = resp.text().await.unwrap_or_default();
                return Err(UploadError::Api(format!(
                    "TikTok chunk upload failed: {body}"
                )));
            }
        }
        Ok(())
    }

    async fn publish_post(
        &self,
        upload_id: &str,
        title: &str,
        caption: &str,
        privacy_level: &str,
    ) -> Result<String, UploadError> {
        let resp = self
            .http
            .post(format!("{}/post/publish/", self.api_base))
            .bearer_auth(&self.access_token)
            .json(&json!({
                "post_info": {
                    "title": title,
                    "description": caption,
                    "privacy_level": privacy_level,
                    "disable_comment": false,
                    "disable_duet": false,
                    "disable_stitch": false,
                },
                "source_info": { "source": "FILE_UPLOAD", "upload_id": upload_id },
            }))
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = expect_ok_json(resp, "TikTok publish").await?;
        data["data"]["publish_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("No publish_id in response".to_string()))
    }
}

#[async_trait]
impl PlatformUploader for TikTokUploader {
    fn platform_name(&self) -> &str {
        "tiktok"
    }

    fn validate_video(&self, video_path: &str) -> Result<(), UploadError> {
        validate_local_file(video_path, MAX_FILE_MB)
    }

    async fn upload_video(
        &self,
        video_path: &str,
        title: &str,
        description: &str,
        hashtags: &[String],
    ) -> Result<String, UploadError> {
        if self.access_token.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        self.validate_video(video_path)?;
        let upload_id = self.init_upload().await?;
        self.upload_chunks(video_path, &upload_id).await?;
        let caption = truncate_chars(
            &format!("{description}\n\n{}", format_hashtags(hashtags)),
            CAPTION_MAX,
        );
        let title = truncate_chars(title, TITLE_MAX);
        self.publish_post(&upload_id, &title, &caption, DEFAULT_PRIVACY)
            .await
    }

    async fn get_video_status(&self, video_id: &str) -> Value {
        let result = async {
            let resp = self
                .http
                .get(format!("{}/post/publish/status/fetch/", self.api_base))
                .bearer_auth(&self.access_token)
                .query(&[("publish_id", video_id)])
                .send()
                .await
                .map_err(|e| UploadError::Request(e.to_string()))?;
            let data = expect_ok_json(resp, "TikTok status fetch").await?;
            Ok::<Value, UploadError>(data.get("data").cloned().unwrap_or_else(|| json!({})))
        }
        .await;
        result.unwrap_or_else(|_| json!({}))
    }

    async fn fetch_video_analytics(
        &self,
        video_id: &str,
        bucket: &str,
    ) -> Result<AnalyticsSnapshot, UploadError> {
        if self.access_token.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        let resp = self
            .http
            .post(format!("{}/video/query/", self.api_base))
            .bearer_auth(&self.access_token)
            .json(&json!({
                "filters": { "video_ids": [video_id] },
                "fields": ["id", "view_count", "like_count", "comment_count", "share_count"],
            }))
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let payload = expect_ok_json(resp, "TikTok analytics").await?;
        let item = payload["data"]["videos"]
            .get(0)
            .cloned()
            .unwrap_or_else(|| json!({}));
        Ok(AnalyticsSnapshot::build(
            bucket,
            "tiktok_open_api_v2",
            as_count(item.get("view_count")),
            as_count(item.get("like_count")),
            as_count(item.get("comment_count")),
            as_count(item.get("share_count")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn temp_video() -> String {
        let dir = std::env::temp_dir();
        let p = dir.join("tb_tiktok_test_clip.mp4");
        tokio::fs::write(&p, b"fake-video-bytes").await.unwrap();
        p.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn upload_video_drei_schritte() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/video/init/"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "upload_id": "u-1" } })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/post/publish/video/chunk/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/post/publish/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "data": { "publish_id": "pub-42" } })),
            )
            .mount(&server)
            .await;

        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let video = temp_video().await;
        let id = uploader
            .upload_video(
                &video,
                "Mein Titel",
                "Beschreibung",
                &["deadlock".into(), "haze".into()],
            )
            .await
            .unwrap();
        assert_eq!(id, "pub-42");
    }

    #[tokio::test]
    async fn upload_ohne_token_und_fehlerpfad() {
        // Kein Token → NotAuthenticated (vor jedem HTTP).
        let video = temp_video().await;
        let no_tok = TikTokUploader::new("");
        assert!(matches!(
            no_tok.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::NotAuthenticated)
        ));

        // init liefert non-200 → Api-Fehler.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/video/init/"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        assert!(matches!(
            uploader.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::Api(_))
        ));

        // Fehlende Datei → Validation.
        let uploader2 = TikTokUploader::new("tok").with_api_base(server.uri());
        assert!(matches!(
            uploader2
                .upload_video("/nope/missing.mp4", "t", "d", &[])
                .await,
            Err(UploadError::Validation(_))
        ));
    }

    #[tokio::test]
    async fn analytics_und_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/video/query/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "videos": [{ "id": "v1", "view_count": 100, "like_count": 10, "comment_count": 5, "share_count": 5 }] }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/post/publish/status/fetch/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "data": { "status": "PROCESSING_DOWNLOAD" } })),
            )
            .mount(&server)
            .await;

        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let stats = uploader.fetch_video_analytics("v1", "d1").await.unwrap();
        assert_eq!(stats.views, 100);
        assert_eq!(stats.provider, "tiktok_open_api_v2");
        assert_eq!(stats.engagement_rate, Some(20.0)); // (10+5+5)/100*100

        let status = uploader.get_video_status("pub-42").await;
        assert_eq!(status["status"], "PROCESSING_DOWNLOAD");
    }
}
