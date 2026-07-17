//! Instagram-Reels-Uploader (Port von `uploaders/instagram.py`).
//!
//! Instagram Graph API v21. WICHTIG (wie im Python-Original): Instagram nimmt
//! **keinen direkten Datei-Upload**, sondern braucht eine öffentlich
//! erreichbare Video-URL. Ablauf: Media-Container (REELS) erstellen → Container
//! veröffentlichen. Das Hochladen auf einen temporären Host ist — exakt wie in
//! Python — nicht implementiert (`upload_to_temporary_host` liefert
//! `NotImplemented`).

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    as_count, expect_ok_json, truncate_chars, validate_local_file, AnalyticsSnapshot,
    PlatformUploader, UploadError,
};
use crate::video_processor::format_hashtags;

const MAX_FILE_MB: f64 = 1024.0;
const CAPTION_MAX: usize = 2200;
const DEFAULT_API_BASE: &str = "https://graph.facebook.com/v21.0";

/// Instagram-Reels-Uploader.
pub struct InstagramUploader {
    access_token: String,
    business_account_id: String,
    api_base: String,
    http: reqwest::Client,
}

fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

impl InstagramUploader {
    pub fn new(access_token: impl Into<String>, business_account_id: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            business_account_id: business_account_id.into(),
            api_base: DEFAULT_API_BASE.to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Überschreibt die API-Basis-URL (für Tests).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    /// Prüft das Access-Token via GET /me (Python `authenticate`). Nicht im
    /// Upload-Pfad, für Parität vorhanden.
    pub async fn verify_token(&self) -> bool {
        let result = async {
            let resp = self
                .http
                .get(format!("{}/me", self.api_base))
                .query(&[("access_token", self.access_token.as_str())])
                .send()
                .await
                .map_err(|e| UploadError::Request(e.to_string()))?;
            expect_ok_json(resp, "Instagram auth check").await
        }
        .await;
        result.is_ok()
    }

    /// Erstellt den Reel-Media-Container und liefert die Container-ID.
    pub async fn create_media_container(
        &self,
        video_url: &str,
        caption: &str,
        share_to_feed: bool,
    ) -> Result<String, UploadError> {
        let resp = self
            .http
            .post(format!(
                "{}/{}/media",
                self.api_base, self.business_account_id
            ))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("media_type", "REELS"),
                ("video_url", video_url),
                ("caption", caption),
                (
                    "share_to_feed",
                    if share_to_feed { "true" } else { "false" },
                ),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = expect_ok_json(resp, "Instagram create container").await?;
        data["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("No container ID in response".to_string()))
    }

    /// Veröffentlicht den Container und liefert die Media-ID.
    pub async fn publish_container(&self, container_id: &str) -> Result<String, UploadError> {
        let resp = self
            .http
            .post(format!(
                "{}/{}/media_publish",
                self.api_base, self.business_account_id
            ))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("creation_id", container_id),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = expect_ok_json(resp, "Instagram publish").await?;
        data["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("No media ID in response".to_string()))
    }

    /// Reel aus einer öffentlichen Video-URL veröffentlichen (Container → Publish).
    pub async fn upload_reel(
        &self,
        video_url: &str,
        caption: &str,
        share_to_feed: bool,
    ) -> Result<String, UploadError> {
        let container_id = self
            .create_media_container(video_url, caption, share_to_feed)
            .await?;
        self.publish_container(&container_id).await
    }

    /// Platzhalter — Video-Hosting ist nicht implementiert (1:1 wie Python).
    pub async fn upload_to_temporary_host(&self, _video_path: &str) -> Result<String, UploadError> {
        Err(UploadError::NotImplemented(
            "Video hosting not implemented. Upload video to public hosting and pass the URL."
                .to_string(),
        ))
    }
}

#[async_trait]
impl PlatformUploader for InstagramUploader {
    fn platform_name(&self) -> &str {
        "instagram"
    }

    fn validate_video(&self, video_path: &str) -> Result<(), UploadError> {
        // URL → Datei-Validierung überspringen.
        if is_url(video_path) {
            return Ok(());
        }
        validate_local_file(video_path, MAX_FILE_MB)
    }

    async fn upload_video(
        &self,
        video_path: &str,
        _title: &str,
        description: &str,
        hashtags: &[String],
    ) -> Result<String, UploadError> {
        // Instagram braucht eine öffentliche URL — der lokale Pfad reicht nicht.
        if !is_url(video_path) {
            return Err(UploadError::Validation(
                "Instagram requires a public video URL. Provide a video_url or upload to public hosting first.".to_string(),
            ));
        }
        self.validate_video(video_path)?;
        let caption = truncate_chars(
            &format!("{description}\n\n{}", format_hashtags(hashtags)),
            CAPTION_MAX,
        );
        self.upload_reel(video_path, &caption, true).await
    }

    async fn get_video_status(&self, media_id: &str) -> Value {
        let result = async {
            let resp = self
                .http
                .get(format!("{}/{}", self.api_base, media_id))
                .query(&[
                    ("access_token", self.access_token.as_str()),
                    ("fields", "status_code,media_type,timestamp"),
                ])
                .send()
                .await
                .map_err(|e| UploadError::Request(e.to_string()))?;
            expect_ok_json(resp, "Instagram status fetch").await
        }
        .await;
        result.unwrap_or_else(|_| json!({}))
    }

    async fn fetch_video_analytics(
        &self,
        media_id: &str,
        bucket: &str,
    ) -> Result<AnalyticsSnapshot, UploadError> {
        let media_resp = self
            .http
            .get(format!("{}/{}", self.api_base, media_id))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("fields", "like_count,comments_count,video_view_count"),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let media = expect_ok_json(media_resp, "Instagram analytics").await?;

        // Insights best-effort: bei non-200 leeres Objekt (mirror Python).
        let insights_resp = self
            .http
            .get(format!("{}/{}/insights", self.api_base, media_id))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("metric", "saved,shares,total_interactions"),
            ])
            .send()
            .await;
        let insights = match insights_resp {
            Ok(r) if r.status().as_u16() == 200 => {
                r.json::<Value>().await.unwrap_or_else(|_| json!({}))
            }
            _ => json!({}),
        };

        let shares = insights["data"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|i| i["name"] == json!("shares"))
                    .map(|i| as_count(i["values"].get(0).map(|v| &v["value"])))
            })
            .unwrap_or(0);

        Ok(AnalyticsSnapshot::build(
            bucket,
            "instagram_graph_api_v21",
            as_count(media.get("video_view_count")),
            as_count(media.get("like_count")),
            as_count(media.get("comments_count")),
            shares,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const BIZ: &str = "17841400000";

    #[tokio::test]
    async fn upload_reel_container_publish() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "container-1" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media_publish")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "media-99" })))
            .mount(&server)
            .await;

        let uploader = InstagramUploader::new("tok", BIZ).with_api_base(server.uri());
        let id = uploader
            .upload_video(
                "https://cdn.example/clip.mp4",
                "ignored",
                "Beschreibung",
                &["deadlock".into()],
            )
            .await
            .unwrap();
        assert_eq!(id, "media-99");
    }

    #[tokio::test]
    async fn lokaler_pfad_und_not_implemented() {
        let uploader = InstagramUploader::new("tok", BIZ);
        // Lokaler Pfad (keine URL) → Validation.
        assert!(matches!(
            uploader.upload_video("/tmp/clip.mp4", "t", "d", &[]).await,
            Err(UploadError::Validation(_))
        ));
        // Temporäres Hosting nicht implementiert (1:1 Python).
        assert!(matches!(
            uploader.upload_to_temporary_host("/tmp/clip.mp4").await,
            Err(UploadError::NotImplemented(_))
        ));
    }

    #[tokio::test]
    async fn analytics_media_plus_insights() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/media-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "video_view_count": "500", "like_count": "40", "comments_count": "10"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/media-1/insights"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "name": "shares", "values": [{ "value": 5 }] }]
            })))
            .mount(&server)
            .await;

        let uploader = InstagramUploader::new("tok", BIZ).with_api_base(server.uri());
        let stats = uploader
            .fetch_video_analytics("media-1", "d1")
            .await
            .unwrap();
        assert_eq!(stats.views, 500);
        assert_eq!(stats.likes, 40);
        assert_eq!(stats.comments, 10);
        assert_eq!(stats.shares, 5); // aus insights
        assert_eq!(stats.provider, "instagram_graph_api_v21");
        assert_eq!(stats.engagement_rate, Some(11.0)); // (40+10+5)/500*100
    }
}
