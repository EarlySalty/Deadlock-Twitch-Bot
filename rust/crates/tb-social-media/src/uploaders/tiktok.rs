use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;

use super::{
    expect_ok_json, tiktok_error_guard, truncate_utf16, validate_local_file, AnalyticsSnapshot,
    PlatformUploader, UploadCheckpoint, UploadError,
};
use crate::video_processor::format_hashtags;

const MAX_FILE_MB: f64 = 4096.0;
const CAPTION_MAX: usize = 2200;

const MIN_CHUNK_BYTES: u64 = 5 * 1024 * 1024;
const MAX_CHUNK_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_CHUNK_BYTES: u64 = 10 * 1024 * 1024;
const MAX_CHUNKS: u64 = 1000;

const DEFAULT_API_BASE: &str = "https://open.tiktokapis.com/v2";
const JSON_CONTENT_TYPE: &str = "application/json; charset=UTF-8";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TikTokPostOptions {
    pub caption: String,
    pub privacy_level: String,
    pub allow_comment: bool,
    pub allow_duet: bool,
    pub allow_stitch: bool,
    pub commercial_content: bool,
    pub brand_organic_toggle: bool,
    pub brand_content_toggle: bool,
    pub consent: bool,
    pub creator_username: String,
    pub credential_id: i32,
    pub platform_user_id: String,
    pub approved_video_sha256: String,
    #[serde(default)]
    pub video_path: String,
}

pub async fn video_digest(path: &str) -> Result<String, std::io::Error> {
    use sha2::{Digest, Sha256};
    let mut file = tokio::fs::File::open(path).await?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

impl TikTokPostOptions {
    pub fn validate(&self, info: &CreatorInfo, duration: f64) -> Result<(), UploadError> {
        let reason = if !self.consent {
            Some("Bitte bestätige die TikTok-Bedingungen für diesen Clip.")
        } else if self.creator_username.is_empty()
            || self.platform_user_id.is_empty()
            || self.credential_id <= 0
            || self.creator_username != info.creator_username
        {
            Some("Das TikTok-Konto hat sich geändert. Bitte plane den Clip erneut ein.")
        } else if self.caption.encode_utf16().count() > CAPTION_MAX {
            Some("Die TikTok-Beschreibung darf höchstens 2200 Zeichen enthalten.")
        } else if !info.privacy_level_options.contains(&self.privacy_level) {
            Some("Die gewählte Sichtbarkeit ist bei TikTok nicht mehr verfügbar. Bitte wähle sie erneut.")
        } else if !duration.is_finite() || duration <= 0.0 || info.max_video_post_duration_sec <= 0
        {
            Some("Die Videodauer konnte nicht geprüft werden. Bitte erstelle die Vorschau erneut.")
        } else if duration > info.max_video_post_duration_sec as f64 {
            Some("Der Clip ist für dieses TikTok-Konto zu lang. Bitte kürze ihn vor dem Einplanen.")
        } else if (self.allow_comment && info.comment_disabled)
            || (self.allow_duet && info.duet_disabled)
            || (self.allow_stitch && info.stitch_disabled)
        {
            Some("TikTok erlaubt eine gewählte Interaktion nicht mehr. Bitte prüfe die Freigabe erneut.")
        } else if self.commercial_content
            != (self.brand_organic_toggle || self.brand_content_toggle)
        {
            Some("Bitte wähle bei Werbung deine eigene Marke oder einen Markenpartner aus.")
        } else if self.brand_content_toggle && self.privacy_level == "SELF_ONLY" {
            Some("Inhalte für Markenpartner dürfen bei TikTok nicht auf „Nur ich“ stehen.")
        } else {
            None
        };
        match reason {
            Some(reason) => Err(UploadError::Validation(reason.into())),
            None => Ok(()),
        }
    }
}

const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_BASE_MS: u64 = 200;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreatorInfo {
    pub creator_username: String,
    pub creator_nickname: String,
    pub privacy_level_options: Vec<String>,
    pub comment_disabled: bool,
    pub duet_disabled: bool,
    pub stitch_disabled: bool,
    pub max_video_post_duration_sec: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChunkPlan {
    chunk_size: u64,
    total_chunk_count: u64,
}

fn source_info(video_size: u64, plan: ChunkPlan) -> Value {
    json!({
        "source": "FILE_UPLOAD",
        "video_size": video_size,
        "chunk_size": plan.chunk_size,
        "total_chunk_count": plan.total_chunk_count,
    })
}

fn chunk_plan(video_size: u64) -> (u64, u64) {
    if video_size < MIN_CHUNK_BYTES {
        return (video_size, 1);
    }
    let mut chunk_size = DEFAULT_CHUNK_BYTES;
    if video_size / chunk_size > MAX_CHUNKS {
        chunk_size = video_size.div_ceil(MAX_CHUNKS);
    }
    chunk_size = chunk_size.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);
    let total_chunk_count = video_size / chunk_size;
    if total_chunk_count <= 1 {
        return (video_size, 1);
    }
    (chunk_size, total_chunk_count)
}

pub fn build_caption(title: &str, description: &str, hashtags: &[String]) -> String {
    let tags = format_hashtags(hashtags);
    let parts: Vec<&str> = [title.trim(), description.trim(), tags.trim()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    truncate_utf16(&parts.join("\n\n"), CAPTION_MAX)
}

fn has_scope(credentials: &crate::credentials::SocialMediaCredentials, scope: &str) -> bool {
    credentials
        .scopes
        .as_deref()
        .unwrap_or("")
        .split([',', ' '])
        .any(|granted| granted == scope)
}

fn scoped_uploader(
    credentials: &crate::credentials::SocialMediaCredentials,
    scopes: &[&str],
) -> Result<TikTokUploader, UploadError> {
    if credentials.access_token.is_empty()
        || !scopes.iter().any(|scope| has_scope(credentials, scope))
    {
        return Err(UploadError::not_started(UploadError::NotAuthenticated));
    }
    Ok(TikTokUploader::new(credentials.access_token.clone()))
}

pub(crate) fn draft_uploader(
    credentials: &crate::credentials::SocialMediaCredentials,
) -> Result<TikTokUploader, UploadError> {
    scoped_uploader(credentials, &["video.upload"])
}

pub(crate) fn direct_uploader(
    credentials: &crate::credentials::SocialMediaCredentials,
    post: DirectPost,
) -> Result<TikTokUploader, UploadError> {
    Ok(scoped_uploader(credentials, &["video.publish"])?.with_direct_post(post))
}

pub(crate) fn status_reader(
    credentials: &crate::credentials::SocialMediaCredentials,
) -> Result<TikTokUploader, UploadError> {
    scoped_uploader(credentials, &["video.upload", "video.publish"])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectPost {
    pub caption: String,
    pub privacy_level: String,
    pub allow_comment: bool,
    pub allow_duet: bool,
    pub allow_stitch: bool,
}

impl DirectPost {
    fn check(&self, info: &CreatorInfo, duration: f64) -> Result<(), UploadError> {
        let reason = if !info.privacy_level_options.contains(&self.privacy_level) {
            Some("Die eingestellte TikTok-Sichtbarkeit ist für dieses Konto nicht verfügbar. Bitte wähle sie in den Einstellungen neu.")
        } else if !duration.is_finite() || duration <= 0.0 || info.max_video_post_duration_sec <= 0
        {
            Some("Die Videodauer konnte nicht geprüft werden.")
        } else if duration > info.max_video_post_duration_sec as f64 {
            Some("Der Clip ist für dieses TikTok-Konto zu lang.")
        } else {
            None
        };
        match reason {
            Some(reason) => Err(UploadError::Validation(reason.into())),
            None => Ok(()),
        }
    }

    fn post_info(&self, info: &CreatorInfo) -> Value {
        json!({
            "title": self.caption,
            "privacy_level": self.privacy_level,
            "disable_comment": !self.allow_comment || info.comment_disabled,
            "disable_duet": !self.allow_duet || info.duet_disabled,
            "disable_stitch": !self.allow_stitch || info.stitch_disabled,
            "brand_organic_toggle": false,
            "brand_content_toggle": false,
            "video_cover_timestamp_ms": 1000,
        })
    }
}

pub struct TikTokUploader {
    access_token: String,
    api_base: String,
    direct: Option<DirectPost>,
    video_processor: crate::video_processor::VideoProcessor,
    #[cfg(test)]
    known_duration: Option<f64>,
    http: reqwest::Client,
}

impl TikTokUploader {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            api_base: DEFAULT_API_BASE.to_string(),
            direct: None,
            video_processor: crate::video_processor::VideoProcessor::default(),
            #[cfg(test)]
            known_duration: None,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("TikTok HTTP client"),
        }
    }

    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    pub fn with_direct_post(mut self, post: DirectPost) -> Self {
        self.direct = Some(post);
        self
    }

    pub fn posts_directly(&self) -> bool {
        self.direct.is_some()
    }

    #[cfg(test)]
    fn with_known_duration(mut self, seconds: f64) -> Self {
        self.known_duration = Some(seconds);
        self
    }

    async fn video_duration(&self, video_path: &str) -> Result<f64, UploadError> {
        #[cfg(test)]
        if let Some(seconds) = self.known_duration {
            return Ok(seconds);
        }
        self.video_processor
            .get_video_info(video_path)
            .await
            .map(|info| info.duration)
            .map_err(|_| {
                UploadError::not_started(UploadError::Validation(
                    "Die Videodauer konnte nicht geprüft werden.".into(),
                ))
            })
    }

    async fn direct_body(
        &self,
        post: &DirectPost,
        video_path: &str,
        source: Value,
    ) -> Result<Value, UploadError> {
        let info = self
            .query_creator_info()
            .await
            .map_err(UploadError::not_started)?;
        let duration = self.video_duration(video_path).await?;
        post.check(&info, duration)
            .map_err(UploadError::not_started)?;
        Ok(json!({ "post_info": post.post_info(&info), "source_info": source }))
    }

    pub async fn query_creator_info(&self) -> Result<CreatorInfo, UploadError> {
        let url = format!("{}/post/publish/creator_info/query/", self.api_base);
        let payload = self
            .json_with_retry("TikTok creator info", || {
                self.http
                    .post(&url)
                    .bearer_auth(&self.access_token)
                    .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
            })
            .await?;
        serde_json::from_value(payload["data"].clone()).map_err(|_| {
            UploadError::Validation(
                "TikTok hat keine vollständigen Kontoeinstellungen geliefert. Bitte versuche es später erneut.".into(),
            )
        })
    }

    async fn upload_with_checkpoint(
        &self,
        video_path: &str,
        _title: &str,
        _description: &str,
        _hashtags: &[String],
        checkpoint: Option<&dyn UploadCheckpoint>,
    ) -> Result<String, UploadError> {
        if self.access_token.is_empty() {
            return Err(UploadError::not_started(UploadError::NotAuthenticated));
        }
        self.validate_video(video_path)
            .map_err(UploadError::not_started)?;
        let video_size = tokio::fs::metadata(video_path)
            .await
            .map_err(|error| UploadError::not_started(UploadError::Io(error)))?
            .len();
        if video_size == 0 {
            return Err(UploadError::not_started(UploadError::Validation(format!(
                "Video file is empty: {video_path}"
            ))));
        }

        let (chunk_size, total_chunk_count) = chunk_plan(video_size);
        let plan = ChunkPlan {
            chunk_size,
            total_chunk_count,
        };
        let source = source_info(video_size, plan);
        let (publish_id, upload_url) = match &self.direct {
            Some(post) => {
                let body = self.direct_body(post, video_path, source).await?;
                self.init_transfer("/post/publish/video/init/", "TikTok direct init", body)
                    .await?
            }
            None => {
                let body = json!({ "source_info": source });
                self.init_transfer("/post/publish/inbox/video/init/", "TikTok draft init", body)
                    .await?
            }
        };
        if let Some(checkpoint) = checkpoint {
            checkpoint.record_publish_id(&publish_id).await?;
        }
        self.upload_chunks(video_path, &upload_url, video_size, plan)
            .await?;
        Ok(publish_id)
    }

    async fn init_transfer(
        &self,
        endpoint: &str,
        context: &str,
        body: Value,
    ) -> Result<(String, String), UploadError> {
        let url = format!("{}{endpoint}", self.api_base);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
            .body(body.to_string())
            .send()
            .await
            .map_err(|error| UploadError::Request(error.without_url().to_string()))?;
        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .map_err(|error| UploadError::Request(error.without_url().to_string()))?;
        if let Err(error) = tiktok_error_guard(&payload, context) {
            let code = payload["error"]["code"].as_str().unwrap_or("");
            if !status.is_server_error()
                && matches!(
                    code,
                    "access_token_invalid"
                        | "scope_not_authorized"
                        | "scope_permission_missed"
                        | "rate_limit_exceeded"
                        | "spam_risk_too_many_posts"
                        | "spam_risk_user_banned_from_posting"
                        | "reached_active_user_cap"
                        | "spam_risk_too_many_pending_share"
                        | "invalid_param"
                        | "invalid_params"
                        | "unaudited_client_can_only_post_to_private_accounts"
                )
            {
                return Err(UploadError::not_started(error));
            }
            return Err(error);
        }
        if !status.is_success() {
            return Err(UploadError::Api(format!("{context}: {status}")));
        }
        let publish_id = payload["data"]["publish_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                UploadError::Api(format!("{context}: kein publish_id in der Antwort"))
            })?;
        let upload_url = payload["data"]["upload_url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                UploadError::Api(format!("{context}: keine upload_url in der Antwort"))
            })?;
        Ok((publish_id, upload_url))
    }

    async fn upload_chunks(
        &self,
        video_path: &str,
        upload_url: &str,
        video_size: u64,
        plan: ChunkPlan,
    ) -> Result<(), UploadError> {
        let mut file = tokio::fs::File::open(video_path).await?;
        let mut offset = 0u64;
        for index in 0..plan.total_chunk_count {
            let is_last = index + 1 == plan.total_chunk_count;
            let length = if is_last {
                video_size - offset
            } else {
                plan.chunk_size
            };
            let mut buffer = vec![0u8; length as usize];
            file.read_exact(&mut buffer).await?;
            let range = format!("bytes {}-{}/{}", offset, offset + length - 1, video_size);
            self.put_chunk(upload_url, buffer, &range, length).await?;
            offset += length;
        }
        Ok(())
    }

    async fn put_chunk(
        &self,
        upload_url: &str,
        body: Vec<u8>,
        range: &str,
        length: u64,
    ) -> Result<(), UploadError> {
        let mut last = UploadError::Request("kein Versuch gelaufen".to_string());
        for attempt in 1..=MAX_ATTEMPTS {
            let request = self
                .http
                .put(upload_url)
                .header(CONTENT_TYPE, "video/mp4")
                .header(reqwest::header::CONTENT_LENGTH, length.to_string())
                .header(reqwest::header::CONTENT_RANGE, range)
                .body(body.clone());
            match request.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(());
                    }
                    let text = resp.text().await.unwrap_or_default();
                    if status.as_u16() == 429 {
                        return Err(UploadError::QuotaExceeded(format!(
                            "TikTok chunk upload: {text}"
                        )));
                    }
                    if !status.is_server_error() {
                        return Err(UploadError::Api(format!(
                            "TikTok chunk upload failed: {status} {text}"
                        )));
                    }
                    last = UploadError::Api(format!("TikTok chunk upload failed: {status} {text}"));
                }
                Err(e) => last = UploadError::Request(e.without_url().to_string()),
            }
            backoff(attempt).await;
        }
        Err(last)
    }

    async fn json_with_retry<F>(&self, ctx: &str, build: F) -> Result<Value, UploadError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut last = UploadError::Request("kein Versuch gelaufen".to_string());
        for attempt in 1..=MAX_ATTEMPTS {
            match build().send().await {
                Ok(resp) => {
                    if resp.status().is_server_error() {
                        let status = resp.status();
                        let text = resp.text().await.unwrap_or_default();
                        last = UploadError::Api(format!("{ctx} failed: {status} {text}"));
                    } else {
                        let payload = expect_ok_json(resp, ctx).await?;
                        tiktok_error_guard(&payload, ctx)?;
                        return Ok(payload);
                    }
                }
                Err(e) => last = UploadError::Request(e.without_url().to_string()),
            }
            backoff(attempt).await;
        }
        Err(last)
    }
}

async fn backoff(attempt: u32) {
    if attempt >= MAX_ATTEMPTS {
        return;
    }
    tokio::time::sleep(Duration::from_millis(BACKOFF_BASE_MS << (attempt - 1))).await;
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
        self.upload_with_checkpoint(video_path, title, description, hashtags, None)
            .await
            .map_err(|error| match error {
                UploadError::NotStarted(inner) => *inner,
                other => other,
            })
    }

    async fn upload_video_checkpointed(
        &self,
        video_path: &str,
        title: &str,
        description: &str,
        hashtags: &[String],
        checkpoint: &dyn UploadCheckpoint,
    ) -> Result<String, UploadError> {
        self.upload_with_checkpoint(video_path, title, description, hashtags, Some(checkpoint))
            .await
    }

    async fn get_video_status(&self, video_id: &str) -> Value {
        let url = format!("{}/post/publish/status/fetch/", self.api_base);
        let body = json!({ "publish_id": video_id });
        let result = self
            .json_with_retry("TikTok status fetch", || {
                self.http
                    .post(&url)
                    .bearer_auth(&self.access_token)
                    .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
                    .body(body.to_string())
            })
            .await;
        match result {
            Ok(payload) => payload.get("data").cloned().unwrap_or_else(|| json!({})),
            Err(e) => {
                tracing::warn!(publish_id = %video_id, error = %e, "TikTok status fetch fehlgeschlagen");
                json!({})
            }
        }
    }

    async fn fetch_video_analytics(
        &self,
        _video_id: &str,
        _bucket: &str,
    ) -> Result<AnalyticsSnapshot, UploadError> {
        Err(UploadError::NotImplemented(
            "TikTok Display API nicht freigeschaltet: Scope video.list fehlt".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const MIB: u64 = 1024 * 1024;

    async fn temp_video(name: &str, bytes: usize) -> String {
        let p = std::env::temp_dir().join(name);
        tokio::fs::write(&p, vec![7u8; bytes]).await.unwrap();
        p.to_string_lossy().into_owned()
    }

    pub(super) fn post_options() -> TikTokPostOptions {
        TikTokPostOptions {
            caption: "Mein Titel\n\nBeschreibung\n\n#deadlock #haze".into(),
            privacy_level: "SELF_ONLY".into(),
            allow_comment: false,
            allow_duet: false,
            allow_stitch: false,
            commercial_content: false,
            brand_organic_toggle: false,
            brand_content_toggle: false,
            consent: true,
            creator_username: "ddc".into(),
            credential_id: 1,
            platform_user_id: "account-1".into(),
            approved_video_sha256: {
                use sha2::{Digest, Sha256};
                Sha256::digest(vec![7u8; 16])
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            },
            video_path: "fixture".into(),
        }
    }

    fn creator_info_body(options: Value) -> Value {
        json!({
            "data": {
                "creator_username": "ddc",
                "creator_nickname": "DDC",
                "privacy_level_options": options,
                "comment_disabled": true,
                "duet_disabled": false,
                "stitch_disabled": true,
                "max_video_post_duration_sec": 300,
            },
            "error": { "code": "ok", "message": "", "log_id": "l1" }
        })
    }

    async fn mount_creator_info(server: &MockServer, options: Value) {
        Mock::given(method("POST"))
            .and(path("/post/publish/creator_info/query/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(creator_info_body(options)))
            .mount(server)
            .await;
    }

    struct RecordingCheckpoint {
        ids: std::sync::Mutex<Vec<String>>,
        fail: bool,
    }

    #[async_trait]
    impl UploadCheckpoint for RecordingCheckpoint {
        async fn record_publish_id(&self, publish_id: &str) -> Result<(), UploadError> {
            if self.fail {
                return Err(UploadError::Api("Test: Datenbank nicht erreichbar".into()));
            }
            self.ids.lock().unwrap().push(publish_id.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn tiktok_local_validation_is_proven_not_started() {
        let checkpoint = RecordingCheckpoint {
            ids: Default::default(),
            fail: false,
        };
        let error = TikTokUploader::new("test-token")
            .upload_video_checkpointed(
                "/missing/tiktok-test-video.mp4",
                "Titel",
                "",
                &[],
                &checkpoint,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(error, UploadError::NotStarted(cause) if matches!(*cause, UploadError::Validation(_)))
        );
        assert!(checkpoint.ids.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn tiktok_explicit_init_rejection_allows_a_later_confirmed_attempt() {
        for (status, code) in [
            (401, "access_token_invalid"),
            (429, "rate_limit_exceeded"),
            (400, "invalid_params"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/post/publish/inbox/video/init/"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(json!({"error": {"code": code}})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let checkpoint = RecordingCheckpoint {
                ids: Default::default(),
                fail: false,
            };
            let video = temp_video(&format!("tb_tiktok_rejected_{status}.mp4"), 16).await;
            let uploader = TikTokUploader::new("test-token").with_api_base(server.uri());
            let error = uploader
                .upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint)
                .await
                .unwrap_err();
            assert!(matches!(error, UploadError::NotStarted(_)));
            assert!(checkpoint.ids.lock().unwrap().is_empty());
            server.verify().await;
            server.reset().await;
            Mock::given(method("POST")).and(path("/post/publish/inbox/video/init/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"publish_id": "later-operation", "upload_url": format!("{}/upload/", server.uri())}, "error": {"code": "ok"}})))
                .expect(1).mount(&server).await;
            Mock::given(method("PUT"))
                .respond_with(ResponseTemplate::new(201))
                .expect(1)
                .mount(&server)
                .await;
            assert_eq!(
                uploader
                    .upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint)
                    .await
                    .unwrap(),
                "later-operation"
            );
            assert_eq!(*checkpoint.ids.lock().unwrap(), vec!["later-operation"]);
            tokio::fs::remove_file(video).await.unwrap();
        }
    }

    #[tokio::test]
    async fn tiktok_unknown_init_error_does_not_claim_to_be_not_started() {
        for (status, code) in [
            (500, "access_token_invalid"),
            (403, "unknown_provider_error"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/post/publish/inbox/video/init/"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(json!({"error": {"code": code}})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let video = temp_video(&format!("tb_tiktok_unknown_{status}.mp4"), 16).await;
            let checkpoint = RecordingCheckpoint {
                ids: Default::default(),
                fail: false,
            };
            let error = TikTokUploader::new("test-token")
                .with_api_base(server.uri())
                .upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint)
                .await
                .unwrap_err();
            assert!(!matches!(error, UploadError::NotStarted(_)));
            assert!(checkpoint.ids.lock().unwrap().is_empty());
            tokio::fs::remove_file(video).await.unwrap();
        }
    }

    #[tokio::test]
    async fn tiktok_checkpoint_failure_prevents_video_transfer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/inbox/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "publish_id": "operation-1", "upload_url": format!("{}/upload/", server.uri()) },
                "error": { "code": "ok" }
            })))
            .expect(1).mount(&server).await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&server)
            .await;
        let checkpoint = RecordingCheckpoint {
            ids: Default::default(),
            fail: true,
        };
        let video = temp_video("tb_tiktok_checkpoint.mp4", 16).await;
        let result = TikTokUploader::new("test-token")
            .with_api_base(server.uri())
            .upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint)
            .await;
        assert!(result.is_err());
        server.verify().await;
        tokio::fs::remove_file(video).await.unwrap();
    }

    const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

    fn timeout_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap()
    }

    async fn fail_at_requests<F>(
        upload: F,
        mut requests: tokio::sync::mpsc::UnboundedReceiver<()>,
        attempts: u32,
    ) -> UploadError
    where
        F: std::future::Future<Output = Result<String, UploadError>>,
    {
        tokio::pin!(upload);
        for _ in 0..attempts {
            tokio::select! {
                request = requests.recv() => assert_eq!(request, Some(())),
                result = &mut upload => panic!("upload finished before the targeted request: {result:?}"),
            }
            tokio::time::pause();
            tokio::time::advance(REQUEST_TIMEOUT + Duration::from_secs(1)).await;
            tokio::time::resume();
        }
        upload.await.unwrap_err()
    }

    #[tokio::test]
    async fn tiktok_init_timeout_is_not_retried() {
        let server = MockServer::start().await;
        let (requests, observed) = tokio::sync::mpsc::unbounded_channel();
        Mock::given(method("POST"))
            .and(path("/post/publish/inbox/video/init/"))
            .respond_with(move |_: &wiremock::Request| {
                requests.send(()).unwrap();
                ResponseTemplate::new(200).set_delay(REQUEST_TIMEOUT * 2)
            })
            .expect(1)
            .mount(&server)
            .await;
        let checkpoint = RecordingCheckpoint {
            ids: Default::default(),
            fail: false,
        };
        let video = temp_video("tb_tiktok_init_timeout.mp4", 16).await;
        let mut uploader = TikTokUploader::new("test-token").with_api_base(server.uri());
        uploader.http = timeout_client();
        let error = fail_at_requests(
            uploader.upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint),
            observed,
            1,
        )
        .await;
        assert!(matches!(error, UploadError::Request(_)));
        assert!(checkpoint.ids.lock().unwrap().is_empty());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        server.verify().await;
        tokio::fs::remove_file(video).await.unwrap();
    }

    #[tokio::test]
    async fn tiktok_chunk_timeout_keeps_checkpoint_and_never_reinitializes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/inbox/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "publish_id": "operation-1", "upload_url": format!("{}/upload/?signature=test-only", server.uri()) },
                "error": { "code": "ok" }
            })))
            .expect(1).mount(&server).await;
        let (requests, observed) = tokio::sync::mpsc::unbounded_channel();
        Mock::given(method("PUT"))
            .respond_with(move |_: &wiremock::Request| {
                requests.send(()).unwrap();
                ResponseTemplate::new(201).set_delay(REQUEST_TIMEOUT * 2)
            })
            .expect(3)
            .mount(&server)
            .await;
        let checkpoint = RecordingCheckpoint {
            ids: Default::default(),
            fail: false,
        };
        let video = temp_video("tb_tiktok_chunk_timeout.mp4", 16).await;
        let mut uploader = TikTokUploader::new("test-token").with_api_base(server.uri());
        uploader.http = timeout_client();
        let error = fail_at_requests(
            uploader.upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint),
            observed,
            3,
        )
        .await;
        assert!(matches!(error, UploadError::Request(_)));
        assert_eq!(*checkpoint.ids.lock().unwrap(), vec!["operation-1"]);
        assert!(!error.to_string().contains("signature"));
        server.verify().await;
        tokio::fs::remove_file(video).await.unwrap();
    }

    #[test]
    fn chunk_plan_kleine_datei_geht_am_stueck() {
        assert_eq!(chunk_plan(3 * MIB), (3 * MIB, 1));
        assert_eq!(chunk_plan(5 * MIB), (5 * MIB, 1));
        assert_eq!(chunk_plan(10 * MIB), (10 * MIB, 1));
    }

    #[test]
    fn chunk_plan_mittlere_dateien() {
        assert_eq!(chunk_plan(64 * MIB), (10 * MIB, 6));
        assert_eq!(chunk_plan(70 * MIB), (10 * MIB, 7));
        assert_eq!(chunk_plan(300 * MIB), (10 * MIB, 30));
        assert_eq!(chunk_plan(3 * 1024 * MIB), (10 * MIB, 307));
    }

    #[test]
    fn chunk_plan_haelt_alle_grenzen_ein() {
        for size in [
            3 * MIB,
            5 * MIB,
            10 * MIB,
            64 * MIB,
            70 * MIB,
            300 * MIB,
            3 * 1024 * MIB,
            60 * 1024 * MIB,
        ] {
            let (chunk_size, total) = chunk_plan(size);
            assert!(
                (1..=MAX_CHUNKS).contains(&total),
                "size {size}: {total} Chunks"
            );
            assert!(
                chunk_size * total <= size,
                "size {size}: Plan deckt die Datei nicht"
            );
            if total > 1 {
                assert!(chunk_size >= MIN_CHUNK_BYTES, "size {size}: Chunk zu klein");
                assert!(chunk_size <= MAX_CHUNK_BYTES, "size {size}: Chunk zu gross");
                assert_eq!(
                    total,
                    size / chunk_size,
                    "size {size}: total != floor(size/chunk)"
                );
                let last = size - (total - 1) * chunk_size;
                assert!(
                    last <= 128 * MIB,
                    "size {size}: letzter Chunk {last} ueber 128 MB"
                );
            } else {
                assert_eq!(
                    chunk_size, size,
                    "size {size}: Ein-Chunk-Plan muss die ganze Datei umfassen"
                );
            }
        }
    }

    #[test]
    fn caption_baut_ein_feld_und_kuerzt_utf16() {
        let c = build_caption("Titel", "Text", &["deadlock".into(), "haze".into()]);
        assert_eq!(c, "Titel\n\nText\n\n#deadlock #haze");
        assert_eq!(build_caption("", "Nur Text", &[]), "Nur Text");
        let lang = build_caption(&"a".repeat(3000), "Text", &[]);
        assert_eq!(lang.chars().count(), CAPTION_MAX);
    }

    #[test]
    fn individuelle_freigabe_prueft_alle_pflichtangaben() {
        let info: CreatorInfo = serde_json::from_value(
            creator_info_body(json!(["SELF_ONLY", "PUBLIC_TO_EVERYONE"]))["data"].clone(),
        )
        .unwrap();
        let base = post_options();
        assert!(base.validate(&info, 15.0).is_ok());
        let changes: [fn(&mut TikTokPostOptions); 8] = [
            |o: &mut TikTokPostOptions| o.consent = false,
            |o: &mut TikTokPostOptions| o.privacy_level.clear(),
            |o: &mut TikTokPostOptions| o.creator_username = "other".into(),
            |o: &mut TikTokPostOptions| o.allow_comment = true,
            |o: &mut TikTokPostOptions| o.allow_stitch = true,
            |o: &mut TikTokPostOptions| o.commercial_content = true,
            |o: &mut TikTokPostOptions| {
                o.commercial_content = true;
                o.brand_content_toggle = true;
            },
            |o: &mut TikTokPostOptions| o.caption = "😀".repeat(1101),
        ];
        for mutate in changes {
            let mut options = base.clone();
            mutate(&mut options);
            assert!(matches!(
                options.validate(&info, 15.0),
                Err(UploadError::Validation(_))
            ));
        }
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY, 301.0] {
            assert!(base.validate(&info, duration).is_err());
        }
        let mut branded = base.clone();
        branded.commercial_content = true;
        branded.brand_content_toggle = true;
        branded.privacy_level = "PUBLIC_TO_EVERYONE".into();
        assert!(branded.validate(&info, 15.0).is_ok());
    }

    #[tokio::test]
    async fn entwurf_geht_ohne_freigabe_ins_postfach() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/inbox/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "publish_id": "draft-7", "upload_url": format!("{}/upload/", server.uri()) },
                "error": { "code": "ok", "message": "", "log_id": "l3" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/upload/"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;
        let checkpoint = RecordingCheckpoint {
            ids: Default::default(),
            fail: false,
        };
        let video = temp_video("tb_tiktok_draft.mp4", 16).await;
        let id = TikTokUploader::new("tok")
            .with_api_base(server.uri())
            .upload_video_checkpointed(&video, "Titel", "Beschreibung", &[], &checkpoint)
            .await
            .unwrap();
        assert_eq!(id, "draft-7");
        assert_eq!(*checkpoint.ids.lock().unwrap(), vec!["draft-7".to_string()]);
        let requests = server.received_requests().await.unwrap();
        let paths: Vec<&str> = requests.iter().map(|r| r.url.path()).collect();
        assert_eq!(paths, vec!["/post/publish/inbox/video/init/", "/upload/"]);
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(body.get("post_info").is_none());
        assert_eq!(body["source_info"]["source"], json!("FILE_UPLOAD"));
        assert_eq!(body["source_info"]["video_size"], json!(16));
        assert_eq!(body["source_info"]["total_chunk_count"], json!(1));
        tokio::fs::remove_file(video).await.unwrap();
    }

    fn direct_post(privacy: &str) -> DirectPost {
        DirectPost {
            caption: "Titel\n\n#deadlock".into(),
            privacy_level: privacy.into(),
            allow_comment: true,
            allow_duet: false,
            allow_stitch: true,
        }
    }

    #[tokio::test]
    async fn direktpost_sendet_caption_und_respektiert_kontosperren() {
        let server = MockServer::start().await;
        mount_creator_info(&server, json!(["PUBLIC_TO_EVERYONE", "SELF_ONLY"])).await;
        Mock::given(method("POST"))
            .and(path("/post/publish/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "publish_id": "direct-1", "upload_url": format!("{}/upload/", server.uri()) },
                "error": { "code": "ok" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/upload/"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;
        let checkpoint = RecordingCheckpoint {
            ids: Default::default(),
            fail: false,
        };
        let video = temp_video("tb_tiktok_direct.mp4", 16).await;
        let id = TikTokUploader::new("tok")
            .with_api_base(server.uri())
            .with_direct_post(direct_post("PUBLIC_TO_EVERYONE"))
            .with_known_duration(15.0)
            .upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint)
            .await
            .unwrap();
        assert_eq!(id, "direct-1");
        let requests = server.received_requests().await.unwrap();
        let init = requests
            .iter()
            .find(|r| r.url.path() == "/post/publish/video/init/")
            .unwrap();
        let body: Value = serde_json::from_slice(&init.body).unwrap();
        assert_eq!(body["post_info"]["title"], json!("Titel\n\n#deadlock"));
        assert_eq!(
            body["post_info"]["privacy_level"],
            json!("PUBLIC_TO_EVERYONE")
        );
        assert_eq!(body["post_info"]["disable_comment"], json!(true));
        assert_eq!(body["post_info"]["disable_duet"], json!(true));
        assert_eq!(body["post_info"]["disable_stitch"], json!(true));
        assert_eq!(body["source_info"]["video_size"], json!(16));
        tokio::fs::remove_file(video).await.unwrap();
    }

    #[tokio::test]
    async fn direktpost_mit_gesperrter_sichtbarkeit_startet_nicht() {
        let server = MockServer::start().await;
        mount_creator_info(&server, json!(["SELF_ONLY"])).await;
        let checkpoint = RecordingCheckpoint {
            ids: Default::default(),
            fail: false,
        };
        let video = temp_video("tb_tiktok_direct_private.mp4", 16).await;
        let error = TikTokUploader::new("tok")
            .with_api_base(server.uri())
            .with_direct_post(direct_post("PUBLIC_TO_EVERYONE"))
            .with_known_duration(15.0)
            .upload_video_checkpointed(&video, "Titel", "", &[], &checkpoint)
            .await
            .unwrap_err();
        assert!(
            matches!(error, UploadError::NotStarted(ref inner) if matches!(**inner, UploadError::Validation(_)))
        );
        assert!(checkpoint.ids.lock().unwrap().is_empty());
        tokio::fs::remove_file(video).await.unwrap();
    }

    #[test]
    fn entwurf_braucht_den_upload_scope() {
        let credentials = |scopes: &str| crate::credentials::SocialMediaCredentials {
            id: 7,
            platform: "tiktok".into(),
            streamer_login: None,
            access_token: "tok".into(),
            refresh_token: None,
            client_id: None,
            client_secret: None,
            expires_at: None,
            scopes: Some(scopes.into()),
            platform_user_id: Some("account-1".into()),
            platform_username: None,
        };
        assert!(draft_uploader(&credentials("user.info.basic,video.upload")).is_ok());
        assert!(matches!(
            draft_uploader(&credentials("user.info.basic,video.publish")),
            Err(UploadError::NotStarted(cause)) if matches!(*cause, UploadError::NotAuthenticated)
        ));
    }

    #[tokio::test]
    async fn error_code_bei_200_ist_kein_erfolg() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/inbox/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {},
                "error": { "code": "spam_risk_too_many_posts", "message": "zu viele Posts", "log_id": "l3" }
            })))
            .mount(&server)
            .await;

        let video = temp_video("tb_tiktok_errorcode.mp4", 16).await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let err = uploader
            .upload_video(&video, "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::QuotaExceeded(_)),
            "erwartet QuotaExceeded, war {err:?}"
        );
    }

    #[tokio::test]
    async fn init_fehlercode_bricht_vor_dem_put_ab() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/inbox/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {},
                "error": { "code": "invalid_param", "message": "kaputt", "log_id": "l4" }
            })))
            .mount(&server)
            .await;

        let video = temp_video("tb_tiktok_initfehler.mp4", 16).await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let err = uploader
            .upload_video(&video, "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::Api(_)),
            "erwartet Api, war {err:?}"
        );
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| r.url.path() != "/upload/"));
    }

    #[tokio::test]
    async fn upload_ohne_token_und_fehlerpfade() {
        let video = temp_video("tb_tiktok_fehler.mp4", 16).await;

        let no_tok = TikTokUploader::new("");
        assert!(matches!(
            no_tok.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::NotAuthenticated)
        ));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/inbox/video/init/"))
            .respond_with(
                ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "forbidden"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        assert!(matches!(
            uploader.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::Api(_))
        ));

        assert!(matches!(
            uploader
                .upload_video("/nope/missing.mp4", "t", "d", &[])
                .await,
            Err(UploadError::Validation(_))
        ));
    }

    #[tokio::test]
    async fn fuenf_xx_wird_dreimal_versucht() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/creator_info/query/"))
            .respond_with(ResponseTemplate::new(503).set_body_string("busy"))
            .expect(3)
            .mount(&server)
            .await;

        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let err = uploader.query_creator_info().await.unwrap_err();
        assert!(
            matches!(err, UploadError::Api(_)),
            "erwartet Api, war {err:?}"
        );
    }

    #[tokio::test]
    async fn status_laeuft_per_post_mit_publish_id() {
        let server = MockServer::start().await;
        mount_creator_info(&server, json!(["SELF_ONLY", "PUBLIC_TO_EVERYONE"])).await;
        Mock::given(method("POST"))
            .and(path("/post/publish/status/fetch/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "status": "PROCESSING_UPLOAD" },
                "error": { "code": "ok", "message": "", "log_id": "l5" }
            })))
            .mount(&server)
            .await;

        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let status = uploader.get_video_status("pub-42").await;
        assert_eq!(status["status"], "PROCESSING_UPLOAD");

        let requests = server.received_requests().await.unwrap();
        let req = requests
            .iter()
            .find(|r| r.url.path() == "/post/publish/status/fetch/")
            .unwrap();
        assert_eq!(req.method.as_str(), "POST");
        assert!(
            req.url.query().is_none(),
            "publish_id gehoert in den Body, nicht in die Query"
        );
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        assert_eq!(body["publish_id"], json!("pub-42"));
    }

    #[tokio::test]
    async fn status_fehler_bleibt_leeres_objekt() {
        let server = MockServer::start().await;
        mount_creator_info(&server, json!(["SELF_ONLY", "PUBLIC_TO_EVERYONE"])).await;
        Mock::given(method("POST"))
            .and(path("/post/publish/status/fetch/"))
            .respond_with(ResponseTemplate::new(400).set_body_string("nope"))
            .mount(&server)
            .await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        assert_eq!(uploader.get_video_status("pub-42").await, json!({}));
    }

    #[tokio::test]
    async fn analytics_meldet_fehlenden_scope_statt_nullen() {
        let uploader = TikTokUploader::new("tok");
        let err = uploader
            .fetch_video_analytics("pub-42", "d1")
            .await
            .unwrap_err();
        match err {
            UploadError::NotImplemented(msg) => assert!(msg.contains("video.list")),
            other => panic!("erwartet NotImplemented, war {other:?}"),
        }
    }
}
