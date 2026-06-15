//! Upload-Worker (Port von `bot/social_media/upload_worker.py`).
//!
//! Verarbeitet die Upload-Queue: pending-Jobs holen → je Streamer den passenden
//! Uploader auflösen (Credentials, global-Fallback, Cache) → Twitch-Clip per
//! yt-dlp laden → ins Hochformat schneiden → zur Plattform hochladen →
//! Queue-Status setzen. Approval-Gate: ohne Freigabe wird der Job `failed`
//! ('approval_required'). An/Aus 1:1 — in Python dauerhaft an (kein Gate).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use crate::approval::is_clip_approved_for;
use crate::clip_queue::{get_upload_queue, update_upload_status, UploadQueueItem};
use crate::credentials::{CredentialManager, SocialMediaCredentials};
use crate::uploaders::instagram::InstagramUploader;
use crate::uploaders::tiktok::TikTokUploader;
use crate::uploaders::youtube::{YouTubeRefreshCreds, YouTubeUploader, GOOGLE_TOKEN_URL};
use crate::uploaders::{PlatformUploader, UploadError};
use crate::video_processor::{VideoProcessor, VideoProcessorError};

const STALE_AFTER_SECS: i64 = 30 * 60;
const INITIAL_DELAY_SECS: u64 = 10;
const DEFAULT_INTERVAL_SECS: u64 = 60;
const DEFAULT_MAX_PARALLEL: usize = 2;
const TARGET_WIDTH: i64 = 1080;
const TARGET_HEIGHT: i64 = 1920;

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("yt-dlp failed: {0}")]
    Download(String),
    #[error(transparent)]
    Convert(#[from] VideoProcessorError),
    #[error(transparent)]
    Upload(#[from] UploadError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Baut den passenden Uploader aus den Credentials (mirror `_build_uploader`).
/// `None`, wenn Pflichtfelder fehlen oder die Plattform unbekannt ist.
fn build_uploader(platform: &str, creds: &SocialMediaCredentials) -> Option<Arc<dyn PlatformUploader>> {
    let has_client_id = creds.client_id.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
    match platform {
        "tiktok" => {
            if !has_client_id || creds.access_token.is_empty() {
                return None;
            }
            Some(Arc::new(TikTokUploader::new(creds.access_token.clone())))
        }
        "youtube" => {
            if !has_client_id || creds.access_token.is_empty() {
                return None;
            }
            Some(Arc::new(youtube_uploader(creds)))
        }
        "instagram" => {
            let user_id = creds.platform_user_id.as_deref().filter(|s| !s.is_empty())?;
            if creds.access_token.is_empty() {
                return None;
            }
            Some(Arc::new(InstagramUploader::new(creds.access_token.clone(), user_id.to_string())))
        }
        _ => None,
    }
}

/// Baut den YouTube-Uploader und hängt — falls die Credentials Refresh-Token +
/// Client-ID + Client-Secret tragen — die inline 401-Selbstheilung an
/// (uploaders-1). Ohne vollständige Refresh-Daten bleibt es beim reinen
/// Access-Token (Refresh dann nur proaktiv über den refresh_worker).
pub(crate) fn youtube_uploader(creds: &SocialMediaCredentials) -> YouTubeUploader {
    let uploader = YouTubeUploader::new(creds.access_token.clone());
    match (
        creds.refresh_token.as_deref().filter(|s| !s.is_empty()),
        creds.client_id.as_deref().filter(|s| !s.is_empty()),
        creds.client_secret.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(refresh_token), Some(client_id), Some(client_secret)) => uploader.with_refresh(YouTubeRefreshCreds {
            refresh_token: refresh_token.to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            token_url: GOOGLE_TOKEN_URL.to_string(),
        }),
        _ => uploader,
    }
}

/// Maximale Clip-Länge je Plattform (Python: tiktok/youtube 60, instagram 90).
fn max_duration_for(platform: &str) -> i64 {
    match platform {
        "instagram" => 90,
        _ => 60,
    }
}

/// Ausgabepfad der vertikalen Variante (mirror `input.replace(".mp4", ...)`).
fn vertical_output_path(input_path: &str, platform: &str) -> String {
    input_path.replace(".mp4", &format!("_{platform}_vertical.mp4"))
}

/// Parst die Queue-Hashtags (JSON-Array-String) in eine Liste.
fn parse_hashtags(raw: Option<&str>) -> Vec<String> {
    raw.filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

/// Cheap-clone-barer Verarbeitungskontext (für nebenläufige Uploads).
#[derive(Clone)]
struct UploadTask {
    pool: PgPool,
    video_processor: VideoProcessor,
    yt_dlp_path: String,
    clips_dir: String,
}

impl UploadTask {
    /// Verarbeitet einen Queue-Job; liefert `true` bei Erfolg.
    async fn process(&self, item: UploadQueueItem, uploader: Arc<dyn PlatformUploader>) -> bool {
        let clip_db_id = item.clip_db_id;
        // Approval-Gate: ohne Freigabe direkt failed.
        if !is_clip_approved_for(&self.pool, clip_db_id, &item.platform).await {
            let _ = update_upload_status(&self.pool, item.id, "failed", None, Some("approval_required")).await;
            return false;
        }
        match self.do_upload(&item).await {
            Ok(converted) => {
                match uploader
                    .upload_video(&converted.path, &converted.title, &converted.description, &converted.hashtags)
                    .await
                {
                    Ok(external_id) => {
                        let _ = update_upload_status(&self.pool, item.id, "completed", Some(&external_id), None).await;
                        true
                    }
                    Err(e) => {
                        let _ = update_upload_status(&self.pool, item.id, "failed", None, Some(&e.to_string())).await;
                        false
                    }
                }
            }
            Err(e) => {
                let _ = update_upload_status(&self.pool, item.id, "failed", None, Some(&e.to_string())).await;
                false
            }
        }
    }

    /// Lädt (falls nötig) den Clip und konvertiert ihn; liefert die Upload-Daten.
    async fn do_upload(&self, item: &UploadQueueItem) -> Result<Converted, WorkerError> {
        let _ = update_upload_status(&self.pool, item.id, "processing", None, None).await;

        let mut local_path = item.local_file_path.clone().unwrap_or_default();
        if local_path.is_empty() || !Path::new(&local_path).exists() {
            local_path = self.download_clip(item.clip_url.as_deref().unwrap_or(""), item.clip_db_id).await?;
            let _ = update_upload_status(&self.pool, item.id, "processing", None, None).await;
        }

        let converted_path = self.convert_to_vertical(&local_path, &item.platform).await?;
        let _ = update_upload_status(&self.pool, item.id, "processing", None, None).await;

        let title = item
            .title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| item.clip_title.clone())
            .unwrap_or_default();
        Ok(Converted {
            path: converted_path,
            title,
            description: item.description.clone().unwrap_or_default(),
            hashtags: parse_hashtags(item.hashtags.as_deref()),
        })
    }

    async fn download_clip(&self, clip_url: &str, clip_db_id: i32) -> Result<String, WorkerError> {
        tokio::fs::create_dir_all(&self.clips_dir).await?;
        let output_path = format!("{}/{}.mp4", self.clips_dir, clip_db_id);
        if Path::new(&output_path).exists() {
            return Ok(output_path);
        }
        let output = tokio::process::Command::new(&self.yt_dlp_path)
            .args(["-f", "best", "-o", &output_path, clip_url])
            .output()
            .await?;
        if !output.status.success() {
            return Err(WorkerError::Download(String::from_utf8_lossy(&output.stderr).trim().to_string()));
        }
        if !Path::new(&output_path).exists() {
            return Err(WorkerError::Download(format!("Downloaded file not found: {output_path}")));
        }
        let _ = sqlx::query("UPDATE twitch_clips_social_media SET local_file_path = $1, downloaded_at = $2 WHERE id = $3")
            .bind(&output_path)
            .bind(Utc::now().to_rfc3339())
            .bind(clip_db_id)
            .execute(&self.pool)
            .await;
        Ok(output_path)
    }

    async fn convert_to_vertical(&self, input_path: &str, platform: &str) -> Result<String, WorkerError> {
        let output_path = vertical_output_path(input_path, platform);
        if Path::new(&output_path).exists() {
            return Ok(output_path);
        }
        self.video_processor
            .convert_and_trim(input_path, &output_path, max_duration_for(platform), TARGET_WIDTH, TARGET_HEIGHT)
            .await?;
        Ok(output_path)
    }
}

struct Converted {
    path: String,
    title: String,
    description: String,
    hashtags: Vec<String>,
}

/// Upload-Worker: hält den Verarbeitungskontext + Credential-Auflösung.
pub struct UploadWorker {
    task: UploadTask,
    credentials: CredentialManager,
    max_parallel: usize,
    interval: Duration,
}

impl UploadWorker {
    pub fn new(pool: PgPool, credentials: CredentialManager) -> Self {
        Self {
            task: UploadTask {
                pool,
                video_processor: VideoProcessor::default(),
                yt_dlp_path: "yt-dlp".to_string(),
                clips_dir: "data/clips".to_string(),
            },
            credentials,
            max_parallel: DEFAULT_MAX_PARALLEL,
            interval: Duration::from_secs(DEFAULT_INTERVAL_SECS),
        }
    }

    pub fn with_yt_dlp(mut self, path: impl Into<String>) -> Self {
        self.task.yt_dlp_path = path.into();
        self
    }

    pub fn with_clips_dir(mut self, dir: impl Into<String>) -> Self {
        self.task.clips_dir = dir.into();
        self
    }

    pub fn with_video_processor(mut self, vp: VideoProcessor) -> Self {
        self.task.video_processor = vp;
        self
    }

    /// Löst den Uploader für einen Job auf (Cache nach (Plattform, Credential-ID)).
    async fn resolve_uploader(
        &self,
        platform: &str,
        streamer_login: Option<&str>,
        cache: &mut HashMap<(String, i32), Option<Arc<dyn PlatformUploader>>>,
    ) -> Option<Arc<dyn PlatformUploader>> {
        let creds = self.credentials.get_credentials(platform, streamer_login).await?;
        let key = (platform.to_string(), creds.id);
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }
        let uploader = build_uploader(platform, &creds);
        cache.insert(key, uploader.clone());
        uploader
    }

    /// Ein Durchlauf: Queue scannen, Batch (max_parallel) bilden, nebenläufig
    /// hochladen.
    pub async fn run_once(&self) {
        let scan_limit = (self.max_parallel * 10).max(self.max_parallel) as i64;
        let stale_cutoff = (Utc::now() - chrono::Duration::seconds(STALE_AFTER_SECS)).to_rfc3339();
        let queue = get_upload_queue(&self.task.pool, None, "pending", scan_limit, Some(&stale_cutoff)).await;
        if queue.is_empty() {
            return;
        }

        let mut cache: HashMap<(String, i32), Option<Arc<dyn PlatformUploader>>> = HashMap::new();
        let mut batch: Vec<(UploadQueueItem, Arc<dyn PlatformUploader>)> = Vec::new();
        for item in queue {
            if let Some(uploader) = self.resolve_uploader(&item.platform, item.streamer_login.as_deref(), &mut cache).await {
                batch.push((item, uploader));
                if batch.len() >= self.max_parallel {
                    break;
                }
            }
        }
        if batch.is_empty() {
            return;
        }

        let mut set = tokio::task::JoinSet::new();
        for (item, uploader) in batch {
            let task = self.task.clone();
            set.spawn(async move { task.process(item, uploader).await });
        }
        while set.join_next().await.is_some() {}
    }

    /// Hintergrund-Loop (Initial-Delay + interval). Noch nicht in tb-bot
    /// gespawnt (Wiring = Cutover-Slice).
    pub async fn run(&self) {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            self.run_once().await;
            tokio::time::sleep(self.interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn creds(platform: &str, client_id: Option<&str>, token: &str, user_id: Option<&str>) -> SocialMediaCredentials {
        SocialMediaCredentials {
            id: 1,
            platform: platform.to_string(),
            streamer_login: None,
            access_token: token.to_string(),
            refresh_token: None,
            client_id: client_id.map(str::to_string),
            client_secret: None,
            expires_at: None,
            scopes: None,
            platform_user_id: user_id.map(str::to_string),
            platform_username: None,
        }
    }

    #[test]
    fn build_uploader_pflichtfelder() {
        assert_eq!(build_uploader("tiktok", &creds("tiktok", Some("ck"), "tok", None)).unwrap().platform_name(), "tiktok");
        assert!(build_uploader("tiktok", &creds("tiktok", None, "tok", None)).is_none()); // client_id fehlt
        assert!(build_uploader("youtube", &creds("youtube", Some("ci"), "", None)).is_none()); // token leer
        assert_eq!(build_uploader("youtube", &creds("youtube", Some("ci"), "tok", None)).unwrap().platform_name(), "youtube");
        assert_eq!(build_uploader("instagram", &creds("instagram", None, "tok", Some("123"))).unwrap().platform_name(), "instagram");
        assert!(build_uploader("instagram", &creds("instagram", None, "tok", None)).is_none()); // user_id fehlt
        assert!(build_uploader("snapchat", &creds("snapchat", Some("x"), "tok", None)).is_none()); // unbekannt
    }

    #[test]
    fn helper_funktionen() {
        assert_eq!(max_duration_for("tiktok"), 60);
        assert_eq!(max_duration_for("youtube"), 60);
        assert_eq!(max_duration_for("instagram"), 90);
        assert_eq!(vertical_output_path("data/clips/5.mp4", "tiktok"), "data/clips/5_tiktok_vertical.mp4");
        assert_eq!(parse_hashtags(Some("[\"a\",\"b\"]")), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(parse_hashtags(None), Vec::<String>::new());
        assert_eq!(parse_hashtags(Some("")), Vec::<String>::new());
    }

    // Mock-Uploader, der nie aufgerufen werden sollte (Approval-Reject-Pfad).
    struct NeverUploader;
    #[async_trait::async_trait]
    impl PlatformUploader for NeverUploader {
        fn platform_name(&self) -> &str {
            "tiktok"
        }
        fn validate_video(&self, _: &str) -> Result<(), UploadError> {
            Ok(())
        }
        async fn upload_video(&self, _: &str, _: &str, _: &str, _: &[String]) -> Result<String, UploadError> {
            panic!("upload_video darf bei fehlender Freigabe nicht laufen");
        }
        async fn get_video_status(&self, _: &str) -> Value {
            Value::Null
        }
        async fn fetch_video_analytics(&self, _: &str, _: &str) -> Result<crate::uploaders::AnalyticsSnapshot, UploadError> {
            unreachable!()
        }
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, status TEXT DEFAULT 'pending')",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TEXT, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TEXT, created_at TEXT DEFAULT CURRENT_TIMESTAMP, completed_at TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn approval_gate_markiert_failed() {
        let Some(pool) = make_pool("t_sm_upload_worker").await else { return };
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id").fetch_one(&pool).await.unwrap();
        let queue_id: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, 'tiktok', 'pending') RETURNING id").bind(clip).fetch_one(&pool).await.unwrap();

        let task = UploadTask {
            pool: pool.clone(),
            video_processor: VideoProcessor::default(),
            yt_dlp_path: "yt-dlp".to_string(),
            clips_dir: std::env::temp_dir().to_string_lossy().into_owned(),
        };
        let item = UploadQueueItem {
            id: queue_id,
            clip_db_id: clip,
            platform: "tiktok".to_string(),
            status: "pending".to_string(),
            priority: 0,
            title: None,
            description: None,
            hashtags: None,
            scheduled_at: None,
            attempts: 0,
            twitch_clip_id: None,
            clip_url: Some("https://clips.twitch.tv/x".to_string()),
            clip_title: Some("Titel".to_string()),
            streamer_login: None,
            local_file_path: None,
            converted_file_path: None,
        };
        // Keine Approval-Zeile → nicht freigegeben → failed/approval_required,
        // ohne dass der Uploader (NeverUploader) je aufgerufen wird.
        let ok = task.process(item, Arc::new(NeverUploader)).await;
        assert!(!ok);
        let (status, err): (String, Option<String>) = sqlx::query_as("SELECT status, last_error FROM twitch_clips_upload_queue WHERE id = $1").bind(queue_id).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "failed");
        assert_eq!(err.as_deref(), Some("approval_required"));
    }
}
