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
use serde_json::Value;
use sqlx::PgPool;

use crate::approval::is_clip_approved_for;
use crate::clip_queue::{
    get_upload_queue, reschedule_upload, update_upload_status, UploadQueueItem, VertagungsKonto,
};
use crate::credentials::{CredentialManager, SocialMediaCredentials};
use crate::render::render_clip_vertical;
use crate::uploaders::instagram::InstagramUploader;
use crate::uploaders::tiktok::TikTokUploader;
use crate::uploaders::youtube::{YouTubeRefreshCreds, YouTubeUploader, GOOGLE_TOKEN_URL};
use crate::uploaders::{PlatformUploader, UploadError};
use crate::video_processor::{VideoProcessor, VideoProcessorError};

const STALE_AFTER_SECS: i64 = 30 * 60;
const INITIAL_DELAY_SECS: u64 = 10;
const DEFAULT_INTERVAL_SECS: u64 = 60;
const DEFAULT_MAX_PARALLEL: usize = 2;

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
fn build_uploader(
    platform: &str,
    creds: &SocialMediaCredentials,
) -> Option<Arc<dyn PlatformUploader>> {
    let has_client_id = creds
        .client_id
        .as_deref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
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
            let user_id = creds
                .platform_user_id
                .as_deref()
                .filter(|s| !s.is_empty())?;
            if creds.access_token.is_empty() {
                return None;
            }
            Some(Arc::new(InstagramUploader::new(
                creds.access_token.clone(),
                user_id.to_string(),
            )))
        }
        _ => None,
    }
}

/// Baut den YouTube-Uploader und hängt — falls die Credentials Refresh-Token +
/// Client-ID + Client-Secret tragen — die inline 401-Selbstheilung an
/// (uploaders-1). Ohne vollständige Refresh-Daten bleibt es beim reinen
/// Access-Token (Refresh dann nur proaktiv über den refresh_worker).
pub fn youtube_uploader(creds: &SocialMediaCredentials) -> YouTubeUploader {
    let uploader = YouTubeUploader::new(creds.access_token.clone());
    match (
        creds.refresh_token.as_deref().filter(|s| !s.is_empty()),
        creds.client_id.as_deref().filter(|s| !s.is_empty()),
        creds.client_secret.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(refresh_token), Some(client_id), Some(client_secret)) => {
            uploader.with_refresh(YouTubeRefreshCreds {
                refresh_token: refresh_token.to_string(),
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
                token_url: GOOGLE_TOKEN_URL.to_string(),
            })
        }
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

fn vertical_output_path(input_path: &str, platform: &str) -> String {
    input_path.replace(".mp4", &format!("_{platform}_branded_v2.mp4"))
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
        match is_clip_approved_for(&self.pool, clip_db_id, &item.platform).await {
            Ok(true) => {}
            Ok(false) => {
                self.update_upload_status_logged(
                    &item,
                    "failed",
                    None,
                    Some("approval_required"),
                    "approval_required",
                )
                .await;
                return false;
            }
            Err(e) => {
                tracing::error!(queue_id = item.id, clip_db_id, %e, "approval check failed before upload");
                let err = format!("approval_check_failed: {e}");
                self.update_upload_status_logged(
                    &item,
                    "failed",
                    None,
                    Some(&err),
                    "approval_check_failed",
                )
                .await;
                return false;
            }
        }
        match self.existing_upload(&item).await {
            Ok(Some(existing)) => {
                if let Err(e) = update_upload_status(
                    &self.pool,
                    item.id,
                    "completed",
                    existing.external_id.as_deref(),
                    None,
                )
                .await
                {
                    tracing::error!(queue_id = item.id, %e, "completed-write failed for existing uploaded clip");
                    let err = format!("completed_write_failed: {e}");
                    self.update_upload_status_logged(
                        &item,
                        "failed",
                        None,
                        Some(&err),
                        "completed_write_failed_existing",
                    )
                    .await;
                }
                return true;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(queue_id = item.id, %e, "uploaded-flag check failed before upload");
                let err = format!("uploaded_flag_check_failed: {e}");
                self.update_upload_status_logged(
                    &item,
                    "failed",
                    None,
                    Some(&err),
                    "uploaded_flag_check_failed",
                )
                .await;
                return false;
            }
        }
        match self.do_upload(&item).await {
            Ok(converted) => {
                let uploaded = if item.platform == "tiktok" {
                    match crate::tiktok_recovery::reserve(&self.pool, item.id).await {
                        Ok(true) => {}
                        Ok(false) => return false,
                        Err(error) => {
                            tracing::warn!(queue_id = item.id, %error, "TikTok-Übertragung konnte nicht reserviert werden");
                            return false;
                        }
                    }
                    uploader
                        .upload_video_checkpointed(
                            &converted.path,
                            &converted.title,
                            &converted.description,
                            &converted.hashtags,
                            &crate::tiktok_recovery::Checkpoint {
                                pool: &self.pool,
                                queue_id: item.id,
                            },
                        )
                        .await
                } else {
                    uploader
                        .upload_video(
                            &converted.path,
                            &converted.title,
                            &converted.description,
                            &converted.hashtags,
                        )
                        .await
                };
                match uploaded {
                    Ok(external_id) => {
                        let external_id = if item.platform == "tiktok" {
                            match self
                                .warte_auf_tiktok(&item, uploader.as_ref(), &external_id)
                                .await
                            {
                                Ok(TikTokOutcome::Published(id)) => id,
                                Ok(TikTokOutcome::Inbox(id)) => {
                                    self.update_upload_status_logged(
                                        &item,
                                        "inbox",
                                        Some(&id),
                                        None,
                                        "tiktok_inbox",
                                    )
                                    .await;
                                    return true;
                                }
                                Ok(TikTokOutcome::Pending(id)) => {
                                    self.update_upload_status_logged(
                                        &item,
                                        "inbox_pending",
                                        Some(&id),
                                        None,
                                        "tiktok_processing",
                                    )
                                    .await;
                                    return true;
                                }
                                Err(_) => {
                                    if let Err(error) = crate::tiktok_recovery::rejected(
                                        &self.pool,
                                        item.id,
                                        &external_id,
                                    )
                                    .await
                                    {
                                        tracing::warn!(queue_id = item.id, %error, "TikTok-Ablehnung konnte nicht gespeichert werden");
                                    }
                                    return false;
                                }
                            }
                        } else {
                            external_id
                        };
                        if let Err(e) = update_upload_status(
                            &self.pool,
                            item.id,
                            "completed",
                            Some(&external_id),
                            None,
                        )
                        .await
                        {
                            tracing::error!(queue_id = item.id, %e, "completed-write failed after successful upload");
                            let err = format!("completed_write_failed: {e}");
                            self.update_upload_status_logged(
                                &item,
                                "failed",
                                None,
                                Some(&err),
                                "completed_write_failed_after_upload",
                            )
                            .await;
                        }
                        true
                    }
                    Err(e) => {
                        if item.platform == "tiktok" {
                            if let UploadError::NotStarted(cause) = &e {
                                match crate::tiktok_recovery::release_not_started(
                                    &self.pool, item.id,
                                )
                                .await
                                {
                                    Ok(true) => {
                                        self.handle_upload_error(&item, cause, "tiktok_not_started")
                                            .await
                                    }
                                    Ok(false) => {}
                                    Err(error) => {
                                        tracing::warn!(queue_id = item.id, %error, "Nicht gestarteter TikTok-Vorgang konnte nicht freigegeben werden")
                                    }
                                }
                            } else if let Err(error) =
                                crate::tiktok_recovery::uncertain(&self.pool, item.id).await
                            {
                                tracing::warn!(queue_id = item.id, %error, "Unklarer TikTok-Vorgang konnte nicht vermerkt werden");
                            }
                        } else {
                            self.handle_upload_error(&item, &e, "platform_upload_failed")
                                .await;
                        }
                        false
                    }
                }
            }
            Err(e) => {
                let err = e.to_string();
                self.update_upload_status_logged(
                    &item,
                    "failed",
                    None,
                    Some(&err),
                    "upload_worker_failed",
                )
                .await;
                false
            }
        }
    }

    /// Lädt (falls nötig) den Clip und konvertiert ihn; liefert die Upload-Daten.
    async fn do_upload(&self, item: &UploadQueueItem) -> Result<Converted, WorkerError> {
        if item.platform != "tiktok" {
            self.update_upload_status_logged(item, "processing", None, None, "processing_start")
                .await;
        }

        let mut local_path = item.local_file_path.clone().unwrap_or_default();
        if local_path.is_empty() || !Path::new(&local_path).exists() {
            local_path = self
                .download_clip(item.clip_url.as_deref().unwrap_or(""), item.clip_db_id)
                .await?;
            if item.platform != "tiktok" {
                self.update_upload_status_logged(
                    item,
                    "processing",
                    None,
                    None,
                    "processing_downloaded",
                )
                .await;
            }
        }

        let converted_path = self
            .convert_to_vertical(item.clip_db_id, &local_path, &item.platform)
            .await?;
        if item.platform != "tiktok" {
            self.update_upload_status_logged(
                item,
                "processing",
                None,
                None,
                "processing_converted",
            )
            .await;
        }

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

    async fn existing_upload(
        &self,
        item: &UploadQueueItem,
    ) -> Result<Option<ExistingUpload>, sqlx::Error> {
        let sql = match item.platform.as_str() {
            "tiktok" => "SELECT uploaded_tiktok, tiktok_video_id FROM twitch_clips_social_media WHERE id = $1",
            "youtube" => "SELECT uploaded_youtube, youtube_video_id FROM twitch_clips_social_media WHERE id = $1",
            "instagram" => "SELECT uploaded_instagram, instagram_media_id FROM twitch_clips_social_media WHERE id = $1",
            _ => return Ok(None),
        };
        let row: Option<(Option<bool>, Option<String>)> = sqlx::query_as(sql)
            .bind(item.clip_db_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|(uploaded, external_id)| {
            uploaded
                .unwrap_or(false)
                .then_some(ExistingUpload { external_id })
        }))
    }

    async fn download_clip(&self, clip_url: &str, clip_db_id: i64) -> Result<String, WorkerError> {
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
            return Err(WorkerError::Download(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        if !Path::new(&output_path).exists() {
            return Err(WorkerError::Download(format!(
                "Downloaded file not found: {output_path}"
            )));
        }
        if let Err(error) = sqlx::query!(
            "UPDATE twitch_clips_social_media SET local_file_path = $1, downloaded_at = $2::text::timestamptz WHERE id = $3",
            &output_path,
            Utc::now().to_rfc3339(),
            clip_db_id
        )
            .execute(&self.pool)
            .await
        {
            tracing::warn!(
                %error,
                clip_db_id,
                path = %output_path,
                "Upload-Worker: lokaler Clip-Pfad konnte nicht gespeichert werden"
            );
        }
        Ok(output_path)
    }

    /// Wie lange auf die Veroeffentlichungsbestaetigung von TikTok gewartet
    /// wird, und in welchem Abstand nachgefragt wird.
    const TIKTOK_BESTAETIGUNG_MAX: Duration = Duration::from_secs(180);
    const TIKTOK_BESTAETIGUNG_ABSTAND: Duration = Duration::from_secs(10);

    /// Fragt TikTok, ob aus der `publish_id` wirklich ein Post geworden ist.
    /// `PUBLISH_COMPLETE` liefert die echte Post-ID zurueck, `FAILED` den Grund.
    /// Antwortet TikTok innerhalb des Zeitfensters gar nicht abschliessend,
    /// bleibt es bei der `publish_id`: ein zweiter Upload waere schlimmer als
    /// eine fehlende Bestaetigung, weil er den Clip doppelt posten wuerde.
    async fn warte_auf_tiktok(
        &self,
        item: &UploadQueueItem,
        uploader: &dyn PlatformUploader,
        publish_id: &str,
    ) -> Result<TikTokOutcome, UploadError> {
        let start = std::time::Instant::now();
        loop {
            let status = uploader.get_video_status(publish_id).await;
            let zustand = status
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            match zustand.as_str() {
                "PUBLISH_COMPLETE" => {
                    let post_id = status
                        .get("publicaly_available_post_id")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .map(|v| match v.as_str() {
                            Some(text) => text.to_string(),
                            None => v.to_string(),
                        })
                        .unwrap_or_else(|| publish_id.to_string());
                    return Ok(TikTokOutcome::Published(post_id));
                }
                "SEND_TO_USER_INBOX" => return Ok(TikTokOutcome::Inbox(publish_id.to_string())),
                "FAILED" => {
                    let grund = status
                        .get("fail_reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unbekannt");
                    return Err(UploadError::Api(format!(
                        "TikTok hat den Post abgelehnt: {grund}"
                    )));
                }
                _ => {}
            }
            if start.elapsed() >= Self::TIKTOK_BESTAETIGUNG_MAX {
                tracing::warn!(
                    queue_id = item.id,
                    publish_id,
                    letzter_zustand = %zustand,
                    "TikTok hat den Post im Zeitfenster nicht bestaetigt; \
                     Eintrag bleibt bei der publish_id, kein zweiter Upload"
                );
                return Ok(TikTokOutcome::Pending(publish_id.to_string()));
            }
            tokio::time::sleep(Self::TIKTOK_BESTAETIGUNG_ABSTAND).await;
        }
    }

    /// Wie viele Anlaeufe ein Job bekommt, bevor er wirklich als kaputt gilt.
    /// Ein erschoepftes Kontingent zaehlt nicht mit, dort geht es nur um den
    /// Termin.
    const MAX_VERSUCHE: i32 = 5;

    /// Wie oft ein Job wegen vollem Tageskontingent vertagt werden darf.
    ///
    /// Eigenes, deutlich hoeheres Konto als [`Self::MAX_VERSUCHE`]: ein voller
    /// Tag beim Anbieter ist Alltag und darf die Versuchsgrenze nicht anfassen.
    /// Endlos vertagen darf sich ein Job aber auch nicht, sonst haengt ein
    /// dauerhaft abgelehnter Upload (falsche Projekt-Quota, gesperrte App) fuer
    /// immer in der Warteschlange. Bei 24 Stunden Abstand sind 30 Vertagungen
    /// rund ein Monat; wer danach immer noch am Kontingent scheitert, hat kein
    /// Tagesproblem.
    const MAX_KONTINGENT_VERTAGUNGEN: i32 = 30;

    /// Auf welches Konto die Vertagung dieses Fehlers geht.
    fn konto_fuer(error: &UploadError) -> VertagungsKonto {
        match error {
            UploadError::NotStarted(cause) => Self::konto_fuer(cause),
            UploadError::QuotaExceeded(_) => VertagungsKonto::Kontingent,
            _ => VertagungsKonto::Versuch,
        }
    }

    /// `true`, wenn das zustaendige Konto mit dieser Vertagung voll waere.
    /// Ohne IO, damit die Grenzen ohne Datenbank pruefbar bleiben.
    fn konto_erschoepft(
        konto: VertagungsKonto,
        attempts: i32,
        kontingent_vertagungen: i32,
    ) -> bool {
        match konto {
            VertagungsKonto::Versuch => attempts + 1 >= Self::MAX_VERSUCHE,
            VertagungsKonto::Kontingent => {
                kontingent_vertagungen + 1 >= Self::MAX_KONTINGENT_VERTAGUNGEN
            }
        }
    }

    /// Trennt "kaputt" von "spaeter nochmal". Frueher landete jeder Fehler auf
    /// `failed`, und `failed` holt die Warteschlange nie wieder ab: ein einziges
    /// 502 oder ein volles Tageskontingent hat den Clip endgueltig verbrannt.
    async fn handle_upload_error(
        &self,
        item: &UploadQueueItem,
        error: &UploadError,
        context: &'static str,
    ) {
        let text = error.to_string();
        let verzoegerung = verzoegerung_fuer(error);
        // Ein volles Tageskontingent sagt nichts ueber den Clip. Es darf
        // deshalb das Versuchskonto weder pruefen noch verbrauchen: sonst
        // haben vier Kontingent-Vertagungen den Clip so weit aufgezehrt, dass
        // die erste voruebergehende Stoerung ihn endgueltig verbrennt.
        // Umgekehrt bekommt es ein eigenes Konto, damit ein dauerhaft
        // abgelehnter Job nicht ewig alle 24 Stunden wiederkehrt.
        let konto = Self::konto_fuer(error);
        let erschoepft = Self::konto_erschoepft(konto, item.attempts, item.quota_deferrals);
        match verzoegerung {
            Some(dauer) if !erschoepft => {
                let naechster = (Utc::now() + dauer).to_rfc3339();
                if let Err(db_error) =
                    reschedule_upload(&self.pool, item.id, &naechster, Some(&text), konto).await
                {
                    tracing::warn!(
                        error = %db_error,
                        queue_id = item.id,
                        platform = %item.platform,
                        "Neuer Termin konnte nicht gesetzt werden"
                    );
                    return;
                }
                tracing::info!(
                    queue_id = item.id,
                    platform = %item.platform,
                    naechster_versuch = %naechster,
                    grund = %text,
                    "Upload vertagt statt verworfen"
                );
            }
            _ => {
                self.update_upload_status_logged(item, "failed", None, Some(&text), context)
                    .await;
            }
        }
    }

    async fn update_upload_status_logged(
        &self,
        item: &UploadQueueItem,
        status: &str,
        external_video_id: Option<&str>,
        error_message: Option<&str>,
        context: &'static str,
    ) {
        if let Err(error) = update_upload_status(
            &self.pool,
            item.id,
            status,
            external_video_id,
            error_message,
        )
        .await
        {
            tracing::warn!(
                %error,
                queue_id = item.id,
                clip_db_id = item.clip_db_id,
                platform = %item.platform,
                status,
                context,
                "Upload-Status konnte nicht aktualisiert werden"
            );
        }
    }

    async fn convert_to_vertical(
        &self,
        clip_db_id: i64,
        input_path: &str,
        platform: &str,
    ) -> Result<String, WorkerError> {
        let output_path = vertical_output_path(input_path, platform);
        render_clip_vertical(
            &self.video_processor,
            &self.pool,
            clip_db_id,
            input_path,
            &output_path,
            max_duration_for(platform),
        )
        .await?;
        Ok(output_path)
    }
}

enum TikTokOutcome {
    Published(String),
    Inbox(String),
    Pending(String),
}

struct Converted {
    path: String,
    title: String,
    description: String,
    hashtags: Vec<String>,
}

struct ExistingUpload {
    external_id: Option<String>,
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
        clip_db_id: i64,
        cache: &mut HashMap<(String, i32), Option<Arc<dyn PlatformUploader>>>,
    ) -> Option<Arc<dyn PlatformUploader>> {
        let twitch_user_id = sqlx::query_scalar::<_, Option<String>>(
            "SELECT twitch_user_id FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip_db_id)
        .fetch_optional(&self.task.pool)
        .await
        .ok()
        .flatten()
        .flatten()?;
        let creds = self
            .credentials
            .get_credentials_for_id(platform, Some(&twitch_user_id))
            .await?;
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
    async fn refresh_tiktok_inbox(&self) {
        let rows: Vec<(i64, i64, Option<String>, String)> = match sqlx::query_as(
            "SELECT q.id, c.id, q.tiktok_publish_id, q.status \
             FROM twitch_clips_upload_queue q \
             JOIN twitch_clips_social_media c ON c.id = q.clip_id \
             WHERE q.platform = 'tiktok' AND q.status IN ('inbox', 'inbox_pending') \
               AND (q.last_attempt_at IS NULL OR q.last_attempt_at::timestamptz < NOW() - INTERVAL '10 minutes') \
             ORDER BY q.last_attempt_at ASC NULLS FIRST LIMIT 2",
        )
        .fetch_all(&self.task.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "TikTok-Postfachstatus konnte nicht geladen werden");
                return;
            }
        };
        let mut cache = HashMap::new();
        for (queue_id, clip_db_id, publish_id, previous) in rows {
            let Some(publish_id) = publish_id else {
                let next_check = (Utc::now() + chrono::Duration::days(1)).to_rfc3339();
                if let Err(error) = sqlx::query(
                    "UPDATE twitch_clips_upload_queue SET last_attempt_at = $1::text::timestamptz, last_error = $2 WHERE id = $3 AND status IN ('inbox', 'inbox_pending')",
                )
                .bind(next_check)
                .bind("Bitte prüfe dein TikTok-Postfach. Der Status dieses Clips konnte nicht bestätigt werden; ein zweiter Upload bleibt gesperrt.")
                .bind(queue_id)
                .execute(&self.task.pool)
                .await
                {
                    tracing::warn!(%error, queue_id, "TikTok-Postfachstatus ohne Vorgangsnummer konnte nicht vertagt werden");
                }
                continue;
            };
            let Some(uploader) = self
                .resolve_uploader("tiktok", clip_db_id, &mut cache)
                .await
            else {
                if let Err(error) = update_upload_status(
                    &self.task.pool,
                    queue_id,
                    &previous,
                    Some(&publish_id),
                    None,
                )
                .await
                {
                    tracing::warn!(%error, queue_id, "TikTok-Postfachstatus ohne Zugang konnte nicht vertagt werden");
                }
                continue;
            };
            let response = uploader.get_video_status(&publish_id).await;
            let status = response.get("status").and_then(Value::as_str).unwrap_or("");
            let (next, external_id, reason) = match status {
                "PUBLISH_COMPLETE" => {
                    let post_id = response
                        .get("publicaly_available_post_id")
                        .and_then(Value::as_array)
                        .and_then(|ids| ids.first())
                        .and_then(Value::as_str)
                        .unwrap_or(&publish_id);
                    ("completed", Some(post_id), None)
                }
                "SEND_TO_USER_INBOX" => ("inbox", Some(publish_id.as_str()), None),
                "FAILED" => {
                    if let Err(error) =
                        crate::tiktok_recovery::rejected(&self.task.pool, queue_id, &publish_id)
                            .await
                    {
                        tracing::warn!(queue_id, %error, "TikTok-Ablehnung konnte nicht gespeichert werden");
                    }
                    continue;
                }
                _ => (previous.as_str(), Some(publish_id.as_str()), None),
            };
            if let Err(error) =
                update_upload_status(&self.task.pool, queue_id, next, external_id, reason).await
            {
                tracing::warn!(%error, queue_id, "TikTok-Postfachstatus konnte nicht gespeichert werden");
            }
        }
    }

    pub async fn run_once(&self) {
        self.refresh_tiktok_inbox().await;
        let scan_limit = (self.max_parallel * 10).max(self.max_parallel) as i64;
        let stale_cutoff = (Utc::now() - chrono::Duration::seconds(STALE_AFTER_SECS)).to_rfc3339();
        let queue = get_upload_queue(
            &self.task.pool,
            None,
            "pending",
            scan_limit,
            Some(&stale_cutoff),
        )
        .await;
        if queue.is_empty() {
            return;
        }

        let mut cache: HashMap<(String, i32), Option<Arc<dyn PlatformUploader>>> = HashMap::new();
        let mut batch: Vec<(UploadQueueItem, Arc<dyn PlatformUploader>)> = Vec::new();
        for item in queue {
            if let Some(uploader) = self
                .resolve_uploader(&item.platform, item.clip_db_id, &mut cache)
                .await
            {
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
        while let Some(result) = set.join_next().await {
            if let Err(error) = result {
                tracing::error!(%error, "Upload-Worker: Upload-Task fehlerhaft beendet");
            }
        }
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

/// Wartezeit bis zum naechsten Anlauf, oder `None`, wenn der Fehler beim
/// naechsten Mal genauso auftritt. Bewusst als freie Funktion, damit die
/// Einordnung ohne Datenbank pruefbar ist.
fn verzoegerung_fuer(error: &UploadError) -> Option<chrono::Duration> {
    match error {
        UploadError::NotStarted(cause) => verzoegerung_fuer(cause),
        // Kontingent voll: das heilt keine Wiederholung in fuenf Minuten, aber
        // morgen ist es wieder da. Der Clip selbst ist in Ordnung.
        UploadError::QuotaExceeded(_) => Some(chrono::Duration::hours(24)),
        // Voruebergehende Stoerung der Gegenseite oder der Leitung.
        UploadError::Request(_) => Some(chrono::Duration::minutes(15)),
        // Abgelaufener Zugang: der Refresh-Worker laeuft alle fuenf Minuten,
        // ein Anlauf spaeter kann also schon wieder gehen.
        UploadError::NotAuthenticated => Some(chrono::Duration::minutes(30)),
        // Validierung, API-Ablehnung, IO, nicht implementiert: bleibt kaputt.
        UploadError::Validation(_)
        | UploadError::Api(_)
        | UploadError::NotImplemented(_)
        | UploadError::Io(_) => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fehler_einordnung_trennt_vertagen_von_verwerfen() {
        use super::verzoegerung_fuer;
        use crate::uploaders::UploadError;
        assert_eq!(
            verzoegerung_fuer(&UploadError::QuotaExceeded("voll".into())),
            Some(chrono::Duration::hours(24))
        );
        assert_eq!(
            verzoegerung_fuer(&UploadError::Request("timeout".into())),
            Some(chrono::Duration::minutes(15))
        );
        assert_eq!(
            verzoegerung_fuer(&UploadError::NotAuthenticated),
            Some(chrono::Duration::minutes(30))
        );
        // Diese vier bleiben beim naechsten Anlauf genauso kaputt.
        assert_eq!(
            verzoegerung_fuer(&UploadError::Validation("zu lang".into())),
            None
        );
        assert_eq!(
            verzoegerung_fuer(&UploadError::Api("invalidTitle".into())),
            None
        );
        assert_eq!(
            verzoegerung_fuer(&UploadError::NotImplemented("kein Scope".into())),
            None
        );
    }

    use super::*;
    use serde_json::Value;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn creds(
        platform: &str,
        client_id: Option<&str>,
        token: &str,
        user_id: Option<&str>,
    ) -> SocialMediaCredentials {
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
        assert_eq!(
            build_uploader("tiktok", &creds("tiktok", Some("ck"), "tok", None))
                .unwrap()
                .platform_name(),
            "tiktok"
        );
        assert!(build_uploader("tiktok", &creds("tiktok", None, "tok", None)).is_none()); // client_id fehlt
        assert!(build_uploader("youtube", &creds("youtube", Some("ci"), "", None)).is_none()); // token leer
        assert_eq!(
            build_uploader("youtube", &creds("youtube", Some("ci"), "tok", None))
                .unwrap()
                .platform_name(),
            "youtube"
        );
        assert_eq!(
            build_uploader("instagram", &creds("instagram", None, "tok", Some("123")))
                .unwrap()
                .platform_name(),
            "instagram"
        );
        assert!(build_uploader("instagram", &creds("instagram", None, "tok", None)).is_none()); // user_id fehlt
        assert!(build_uploader("snapchat", &creds("snapchat", Some("x"), "tok", None)).is_none());
        // unbekannt
    }

    #[test]
    fn helper_funktionen() {
        assert_eq!(max_duration_for("tiktok"), 60);
        assert_eq!(max_duration_for("youtube"), 60);
        assert_eq!(max_duration_for("instagram"), 90);
        assert_eq!(
            vertical_output_path("data/clips/5.mp4", "tiktok"),
            "data/clips/5_tiktok_branded_v2.mp4"
        );
        assert_ne!(
            vertical_output_path("data/clips/5.mp4", "tiktok"),
            "data/clips/5_tiktok_vertical.mp4"
        );
        assert_eq!(
            parse_hashtags(Some("[\"a\",\"b\"]")),
            vec!["a".to_string(), "b".to_string()]
        );
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
        async fn upload_video(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &[String],
        ) -> Result<String, UploadError> {
            panic!("upload_video darf bei fehlender Freigabe nicht laufen");
        }
        async fn get_video_status(&self, _: &str) -> Value {
            Value::Null
        }
        async fn fetch_video_analytics(
            &self,
            _: &str,
            _: &str,
        ) -> Result<crate::uploaders::AnalyticsSnapshot, UploadError> {
            unreachable!()
        }
    }

    struct OkUploader {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl PlatformUploader for OkUploader {
        fn platform_name(&self) -> &str {
            "tiktok"
        }
        fn validate_video(&self, _: &str) -> Result<(), UploadError> {
            Ok(())
        }
        async fn upload_video(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &[String],
        ) -> Result<String, UploadError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("new_vid".to_string())
        }
        async fn upload_video_checkpointed(
            &self,
            video_path: &str,
            title: &str,
            description: &str,
            hashtags: &[String],
            checkpoint: &dyn crate::uploaders::UploadCheckpoint,
        ) -> Result<String, UploadError> {
            checkpoint.record_publish_id("new_vid").await?;
            self.upload_video(video_path, title, description, hashtags)
                .await
        }
        async fn get_video_status(&self, _: &str) -> Value {
            serde_json::json!({"status": "PUBLISH_COMPLETE", "publicaly_available_post_id": ["new_vid"]})
        }
        async fn fetch_video_analytics(
            &self,
            _: &str,
            _: &str,
        ) -> Result<crate::uploaders::AnalyticsSnapshot, UploadError> {
            unreachable!()
        }
    }

    struct InboxUploader;

    struct InterruptedUploader {
        calls: AtomicUsize,
        has_publish_id: bool,
    }

    #[async_trait::async_trait]
    impl PlatformUploader for InterruptedUploader {
        fn platform_name(&self) -> &str {
            "tiktok"
        }
        fn validate_video(&self, _: &str) -> Result<(), UploadError> {
            Ok(())
        }
        async fn upload_video(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &[String],
        ) -> Result<String, UploadError> {
            panic!("TikTok muss den gesicherten Uploadpfad verwenden")
        }
        async fn upload_video_checkpointed(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &[String],
            checkpoint: &dyn crate::uploaders::UploadCheckpoint,
        ) -> Result<String, UploadError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.has_publish_id {
                checkpoint.record_publish_id("uncertain-1").await?;
            }
            Err(UploadError::Request("Test: Antwort verloren".into()))
        }
        async fn get_video_status(&self, id: &str) -> Value {
            assert_eq!(id, "uncertain-1");
            serde_json::json!({"status": "PUBLISH_COMPLETE", "publicaly_available_post_id": ["published-1"]})
        }
        async fn fetch_video_analytics(
            &self,
            _: &str,
            _: &str,
        ) -> Result<crate::uploaders::AnalyticsSnapshot, UploadError> {
            unreachable!()
        }
    }

    #[async_trait::async_trait]
    impl PlatformUploader for InboxUploader {
        fn platform_name(&self) -> &str {
            "tiktok"
        }
        fn validate_video(&self, _: &str) -> Result<(), UploadError> {
            Ok(())
        }
        async fn upload_video(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &[String],
        ) -> Result<String, UploadError> {
            Ok("publish_123".to_string())
        }
        async fn upload_video_checkpointed(
            &self,
            video_path: &str,
            title: &str,
            description: &str,
            hashtags: &[String],
            checkpoint: &dyn crate::uploaders::UploadCheckpoint,
        ) -> Result<String, UploadError> {
            checkpoint.record_publish_id("publish_123").await?;
            self.upload_video(video_path, title, description, hashtags)
                .await
        }
        async fn get_video_status(&self, _: &str) -> Value {
            serde_json::json!({"status": "SEND_TO_USER_INBOX"})
        }
        async fn fetch_video_analytics(
            &self,
            _: &str,
            _: &str,
        ) -> Result<crate::uploaders::AnalyticsSnapshot, UploadError> {
            unreachable!()
        }
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        // Gemeinsame Notbremse statt einer Kopie je Testmodul: ohne DSN
        // meldet ein uebersprungener DB-Test sonst gruen, ohne etwas
        // geprueft zu haben.
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, twitch_user_id TEXT, enabled INTEGER DEFAULT 1, access_token_enc BYTEA, refresh_token_enc BYTEA, client_id TEXT, client_secret_enc BYTEA, token_expires_at TEXT, scopes TEXT, platform_user_id TEXT, platform_username TEXT, enc_version INTEGER, authorized_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, clip_title TEXT, custom_title TEXT, layout_override_json JSONB, streamer_login TEXT NOT NULL, twitch_user_id TEXT DEFAULT '42', local_file_path TEXT, converted_file_path TEXT, status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT, tiktok_uploaded_at TIMESTAMPTZ, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ, discarded_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, tiktok_publish_id TEXT, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn make_completed_write_error_pool(schema: &str) -> Option<PgPool> {
        // Gemeinsame Notbremse statt einer Kopie je Testmodul: ohne DSN
        // meldet ein uebersprungener DB-Test sonst gruen, ohne etwas
        // geprueft zu haben.
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, clip_title TEXT, custom_title TEXT, layout_override_json JSONB, streamer_login TEXT NOT NULL, twitch_user_id TEXT DEFAULT '42', local_file_path TEXT, converted_file_path TEXT, status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ, discarded_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, tiktok_publish_id TEXT, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn approve_tiktok(pool: &PgPool, clip: i32) {
        sqlx::query(
            "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) \
             VALUES ($1, 'approved', '[\"tiktok\"]'::jsonb)",
        )
        .bind(clip)
        .execute(pool)
        .await
        .unwrap();
    }

    fn task(pool: PgPool) -> UploadTask {
        static TOOLS: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
        let (ffmpeg, ffprobe) = TOOLS.get_or_init(|| {
            use std::os::unix::fs::PermissionsExt;
            let dir = unique_temp_dir("upload-worker-tools");
            let ffmpeg = dir.join("ffmpeg");
            let ffprobe = dir.join("ffprobe");
            std::fs::write(
                &ffmpeg,
                r#"#!/bin/sh
for out do :; done
printf '%s\n' "$*" > "$out"
cat overlay.ass >> "$out"
"#,
            )
            .unwrap();
            std::fs::write(
                &ffprobe,
                r#"#!/bin/sh
printf '%s\n' '{"streams":[{"codec_type":"video","width":1920,"height":1080,"duration":"15"}]}'
"#,
            )
            .unwrap();
            for file in [&ffmpeg, &ffprobe] {
                std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            (
                ffmpeg.to_string_lossy().into_owned(),
                ffprobe.to_string_lossy().into_owned(),
            )
        });
        UploadTask {
            pool,
            video_processor: VideoProcessor::new(ffmpeg, ffprobe),
            yt_dlp_path: "yt-dlp".to_string(),
            clips_dir: std::env::temp_dir().to_string_lossy().into_owned(),
        }
    }

    fn upload_item(queue_id: i64, clip: i64, local_file_path: Option<String>) -> UploadQueueItem {
        UploadQueueItem {
            id: queue_id,
            clip_db_id: clip,
            platform: "tiktok".to_string(),
            status: "processing".to_string(),
            priority: 0,
            title: Some("Title".to_string()),
            description: Some("Description".to_string()),
            hashtags: Some("[\"deadlock\"]".to_string()),
            scheduled_at: None,
            attempts: 0,
            quota_deferrals: 0,
            twitch_clip_id: Some("c1".to_string()),
            clip_url: None,
            clip_title: Some("Clip title".to_string()),
            streamer_login: Some("nani".to_string()),
            local_file_path,
            converted_file_path: None,
        }
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("tb_sm_{name}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn approval_gate_markiert_failed() {
        let Some(pool) = make_pool("t_sm_upload_worker").await else {
            return;
        };
        let clip: i64 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) VALUES ('approval-1', 'https://clips.test/approval-1', 'nani') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let queue_id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, 'tiktok', 'pending') RETURNING id").bind(clip).fetch_one(&pool).await.unwrap();

        let task = task(pool.clone());
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
            quota_deferrals: 0,
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
        let (status, err): (String, Option<String>) = sqlx::query_as(
            "SELECT status, last_error FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(err.as_deref(), Some("approval_required"));
    }

    #[tokio::test]
    async fn upload_renders_current_layout_title_and_subtitles_again() {
        let Some(pool) = make_pool("t_sm_upload_fresh_render").await else {
            return;
        };
        for ddl in [
            "CREATE TABLE social_media_streamer_layout (twitch_user_id TEXT DEFAULT '42', streamer_login TEXT PRIMARY KEY, layout_json JSONB, cam_enabled BOOLEAN, mode TEXT)",
            "CREATE TABLE social_media_streamer_settings (twitch_user_id TEXT DEFAULT '42', streamer_login TEXT PRIMARY KEY, subtitles_enabled BOOLEAN DEFAULT TRUE)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT, category TEXT, source TEXT, aliases JSONB, weight INTEGER, updated_at TIMESTAMPTZ)",
        ] { sqlx::query(ddl).execute(&pool).await.unwrap(); }
        let dir = unique_temp_dir("fresh-render");
        let input = dir.join("clip.mp4");
        std::fs::write(&input, b"input").unwrap();
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, clip_title) VALUES ('render', 'https://clips.test/render', 'nani', 'Alter Titel') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let mut layout = crate::layout::default_streamer_layout();
        crate::layout::set_clip_layout_override(&pool, clip, Some(&layout))
            .await
            .unwrap();
        let worker = task(pool.clone());
        let output = worker
            .convert_to_vertical(clip, input.to_str().unwrap(), "tiktok")
            .await
            .unwrap();
        let first = std::fs::read_to_string(&output).unwrap();
        assert!(first.contains("Alter Titel"));
        layout.cam_position.h = 500;
        crate::layout::set_clip_layout_override(&pool, clip, Some(&layout))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_clips_social_media SET custom_title = 'Neuer Titel' WHERE id = $1",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, transcript_segments) VALUES ($1, $2)")
            .bind(i32::try_from(clip).unwrap()).bind(serde_json::json!([{"start_seconds":0.0,"end_seconds":2.0,"text":"Neue Worte"}])).execute(&pool).await.unwrap();
        let output = worker
            .convert_to_vertical(clip, input.to_str().unwrap(), "tiktok")
            .await
            .unwrap();
        let next = std::fs::read_to_string(&output).unwrap();
        assert!(next.contains("Neuer Titel"));
        assert!(!next.contains("Alter Titel"));
        assert!(next.contains("Neue Worte"));
        assert!(next.contains("1080:500"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn tiktok_uncertain_transfer_survives_retry_reclaim_and_restart() {
        for (schema, has_publish_id) in [
            ("t_sm_tiktok_lost_init", false),
            ("t_sm_tiktok_lost_chunk", true),
        ] {
            let Some(pool) = make_pool(schema).await else {
                return;
            };
            let dir = unique_temp_dir(schema);
            let input = dir.join("clip.mp4");
            std::fs::write(&input, b"input").unwrap();
            std::fs::write(dir.join("clip_tiktok_branded_v2.mp4"), b"converted").unwrap();
            let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, local_file_path) VALUES ('lost', 'https://clips.test/lost', 'nani', $1) RETURNING id")
                .bind(input.to_string_lossy().as_ref()).fetch_one(&pool).await.unwrap();
            approve_tiktok(&pool, i32::try_from(clip).unwrap()).await;
            let queue_id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id, platform) VALUES ($1, 'tiktok') RETURNING id")
                .bind(clip).fetch_one(&pool).await.unwrap();
            let uploader = Arc::new(InterruptedUploader {
                calls: AtomicUsize::new(0),
                has_publish_id,
            });
            let item = upload_item(queue_id, clip, Some(input.to_string_lossy().into_owned()));
            assert!(
                !task(pool.clone())
                    .process(item.clone(), uploader.clone())
                    .await
            );
            sqlx::query("UPDATE twitch_clips_upload_queue SET last_attempt_at = NOW() - INTERVAL '2 days' WHERE id = $1")
                .bind(queue_id).execute(&pool).await.unwrap();
            assert!(
                get_upload_queue(&pool, None, "pending", 10, Some(&Utc::now().to_rfc3339()))
                    .await
                    .is_empty()
            );
            let again =
                crate::clip_queue::queue_upload(&pool, clip, "tiktok", None, None, None, None, 0)
                    .await
                    .unwrap();
            assert_eq!(again, queue_id);
            assert!(
                !task(pool.clone())
                    .process(item.clone(), uploader.clone())
                    .await
            );
            assert_eq!(uploader.calls.load(Ordering::SeqCst), 1);
            let (status, id, published): (String, Option<String>, bool) = sqlx::query_as("SELECT q.status, q.tiktok_publish_id, c.uploaded_tiktok FROM twitch_clips_upload_queue q JOIN twitch_clips_social_media c ON c.id = q.clip_id WHERE q.id = $1")
                .bind(queue_id).fetch_one(&pool).await.unwrap();
            assert_eq!(status, "inbox_pending");
            assert_eq!(id.as_deref(), has_publish_id.then_some("uncertain-1"));
            assert!(!published);
            if let Some(id) = id {
                let result = task(pool.clone())
                    .warte_auf_tiktok(&item, uploader.as_ref(), &id)
                    .await
                    .unwrap();
                let TikTokOutcome::Published(post_id) = result else {
                    panic!("Status wurde nicht erkannt")
                };
                update_upload_status(&pool, queue_id, "completed", Some(&post_id), None)
                    .await
                    .unwrap();
                let published: bool = sqlx::query_scalar(
                    "SELECT uploaded_tiktok FROM twitch_clips_social_media WHERE id = $1",
                )
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert!(published);
                assert_eq!(uploader.calls.load(Ordering::SeqCst), 1);
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test]
    async fn tiktok_inbox_bleibt_unveroeffentlicht() {
        let Some(pool) = make_pool("t_sm_upload_inbox").await else {
            return;
        };
        let dir = unique_temp_dir("inbox");
        let input_path = dir.join("clip.mp4");
        let converted_path = dir.join("clip_tiktok_branded_v2.mp4");
        std::fs::write(&input_path, b"input").unwrap();
        std::fs::write(&converted_path, b"converted").unwrap();
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, local_file_path) \
             VALUES ('inbox-1', 'https://clips.test/inbox-1', 'nani', $1) RETURNING id",
        )
        .bind(input_path.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();
        approve_tiktok(&pool, i32::try_from(clip).unwrap()).await;
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'tiktok', 'processing') RETURNING id",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        let ok = task(pool.clone())
            .process(
                upload_item(
                    queue_id,
                    clip,
                    Some(input_path.to_string_lossy().into_owned()),
                ),
                Arc::new(InboxUploader),
            )
            .await;
        assert!(ok);
        let (status, publish_id, uploaded): (String, Option<String>, Option<bool>) =
            sqlx::query_as(
                "SELECT q.status, c.tiktok_video_id, c.uploaded_tiktok \
             FROM twitch_clips_upload_queue q \
             JOIN twitch_clips_social_media c ON c.id = q.clip_id WHERE q.id = $1",
            )
            .bind(queue_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "inbox");
        assert_eq!(publish_id.as_deref(), Some("publish_123"));
        assert_eq!(uploaded, Some(false));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn tiktok_postfach_prueft_weitere_clips_trotz_defekter_eintraege() {
        let Some(pool) = make_pool("t_sm_upload_inbox_fairness").await else {
            return;
        };
        let mut queue_ids = Vec::new();
        for (index, age) in ["3 hours", "2 hours", "1 hour"].iter().enumerate() {
            let publish_id = (index > 0).then(|| format!("publish_{index}"));
            let clip: i64 = sqlx::query_scalar(
                "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, tiktok_video_id) VALUES ($1, 'https://clips.test/poll', 'nani', $2) RETURNING id",
            )
            .bind(format!("poll-{index}"))
            .bind(&publish_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            let queue_id: i64 = sqlx::query_scalar(
                "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, last_attempt_at, tiktok_publish_id) VALUES ($1, 'tiktok', 'inbox', NOW() - $2::interval, $3) RETURNING id",
            )
            .bind(clip)
            .bind(age)
            .bind(&publish_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            queue_ids.push(queue_id);
        }
        let cipher =
            Arc::new(tb_crypto::FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap());
        let worker = UploadWorker::new(pool.clone(), CredentialManager::new(pool.clone(), cipher));
        worker.refresh_tiktok_inbox().await;
        let (first, reason, delayed): (String, Option<String>, bool) = sqlx::query_as(
            "SELECT status, last_error, last_attempt_at > NOW() + INTERVAL '23 hours' FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_ids[0])
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(first, "inbox");
        assert!(reason.is_some());
        assert!(delayed);
        let second_updated: bool = sqlx::query_scalar(
            "SELECT last_attempt_at > NOW() - INTERVAL '1 minute' FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_ids[1])
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(second_updated);
        worker.refresh_tiktok_inbox().await;
        let third_updated: bool = sqlx::query_scalar(
            "SELECT last_attempt_at > NOW() - INTERVAL '1 minute' FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_ids[2])
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(third_updated);
    }

    #[tokio::test]
    async fn completed_write_failure_keeps_tiktok_recoverable() {
        let Some(pool) = make_completed_write_error_pool("t_sm_upload_completed_write_fail").await
        else {
            return;
        };
        let dir = unique_temp_dir("completed_write_fail");
        let input_path = dir.join("clip.mp4");
        let converted_path = dir.join("clip_tiktok_branded_v2.mp4");
        std::fs::write(&input_path, b"input").unwrap();
        std::fs::write(&converted_path, b"converted").unwrap();
        let input_path_s = input_path.to_string_lossy().into_owned();

        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, local_file_path) \
             VALUES ('c1', 'https://clips.test/c1', 'nani', $1) RETURNING id",
        )
        .bind(&input_path_s)
        .fetch_one(&pool)
        .await
        .unwrap();
        approve_tiktok(&pool, i32::try_from(clip).unwrap()).await;
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'tiktok', 'processing') RETURNING id",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let ok = task(pool.clone())
            .process(
                upload_item(queue_id, clip, Some(input_path_s)),
                Arc::new(OkUploader {
                    calls: calls.clone(),
                }),
            )
            .await;
        assert!(ok);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (status, attempts, err): (String, i32, Option<String>) = sqlx::query_as(
            "SELECT status, attempts, last_error FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "inbox_pending");
        assert_eq!(attempts, 0);
        assert!(err.is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    /// Ein erschoepftes Tageskontingent vertagt, verbraucht aber keinen
    /// Anlauf. Vorher zaehlte jede Kontingent-Vertagung mit, und nach vier
    /// davon hat die erste voruebergehende Stoerung den Clip endgueltig auf
    /// `failed` gesetzt, obwohl er nie wirklich abgelehnt wurde.
    #[tokio::test]
    async fn kontingent_vertagt_ohne_versuch_zu_verbrauchen() {
        let Some(pool) = make_pool("t_sm_upload_kontingent").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) \
             VALUES ('q1', 'https://clips.test/q1', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'youtube', 'processing') RETURNING id",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();

        let task = task(pool.clone());
        let mut item = upload_item(queue_id, clip, None);
        for _ in 0..4 {
            task.handle_upload_error(
                &item,
                &UploadError::QuotaExceeded("quotaExceeded".into()),
                "test_quota",
            )
            .await;
        }
        let (status, attempts): (String, i32) =
            sqlx::query_as("SELECT status, attempts FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(queue_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(
            attempts, 0,
            "ein volles Tageskontingent darf das Versuchskonto nicht aufzehren"
        );

        // Und danach steht der volle Vorrat fuer echte Stoerungen bereit.
        item.attempts = attempts;
        task.handle_upload_error(
            &item,
            &UploadError::Request("timeout".into()),
            "test_request",
        )
        .await;
        let (status, attempts): (String, i32) =
            sqlx::query_as("SELECT status, attempts FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(queue_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(attempts, 1);
    }

    /// Kontingent-Vertagungen haben ein eigenes Konto, und dieses Konto ist
    /// endlich. Frueher vertagte sich ein dauerhaft abgelehnter Job alle 24
    /// Stunden endlos, weil `erschoepft` fuer Kontingent-Fehler immer `false`
    /// war.
    #[test]
    fn kontingent_hat_eigene_endliche_obergrenze() {
        use super::UploadTask;
        use crate::clip_queue::VertagungsKonto;
        use crate::uploaders::UploadError;

        let kontingent = UploadTask::konto_fuer(&UploadError::QuotaExceeded("voll".into()));
        assert_eq!(kontingent, VertagungsKonto::Kontingent);
        assert_eq!(
            UploadTask::konto_fuer(&UploadError::Request("timeout".into())),
            VertagungsKonto::Versuch
        );

        // Das Kontingent-Konto ist deutlich groesser als das Versuchskonto.
        const { assert!(UploadTask::MAX_KONTINGENT_VERTAGUNGEN > UploadTask::MAX_VERSUCHE) };

        // Viele Kontingent-Vertagungen lassen das Versuchskonto unberuehrt.
        assert!(!UploadTask::konto_erschoepft(
            VertagungsKonto::Kontingent,
            UploadTask::MAX_VERSUCHE + 10,
            0
        ));

        // Unterhalb der Grenze wird weiter vertagt.
        assert!(!UploadTask::konto_erschoepft(
            VertagungsKonto::Kontingent,
            0,
            UploadTask::MAX_KONTINGENT_VERTAGUNGEN - 2
        ));
        // An der Grenze ist Schluss.
        assert!(UploadTask::konto_erschoepft(
            VertagungsKonto::Kontingent,
            0,
            UploadTask::MAX_KONTINGENT_VERTAGUNGEN - 1
        ));

        // Das Versuchskonto haengt weiter nur an `attempts`.
        assert!(!UploadTask::konto_erschoepft(
            VertagungsKonto::Versuch,
            UploadTask::MAX_VERSUCHE - 2,
            UploadTask::MAX_KONTINGENT_VERTAGUNGEN
        ));
        assert!(UploadTask::konto_erschoepft(
            VertagungsKonto::Versuch,
            UploadTask::MAX_VERSUCHE - 1,
            0
        ));
    }

    /// Der Weg durch die Datenbank: das Kontingent-Konto laeuft mit, und wenn es
    /// voll ist, geht der Job auf `failed` statt sich weiter zu vertagen.
    #[tokio::test]
    async fn kontingent_konto_laeuft_voll_und_verwirft() {
        let Some(pool) = make_pool("t_sm_upload_kontingent_grenze").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) \
             VALUES ('q2', 'https://clips.test/q2', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'youtube', 'processing') RETURNING id",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();

        let task = task(pool.clone());
        let mut item = upload_item(queue_id, clip, None);
        item.platform = "youtube".to_string();
        for _ in 0..UploadTask::MAX_KONTINGENT_VERTAGUNGEN {
            task.handle_upload_error(
                &item,
                &UploadError::QuotaExceeded("quotaExceeded".into()),
                "test_quota_grenze",
            )
            .await;
            let (attempts, vertagungen): (i32, i32) = sqlx::query_as(
                "SELECT attempts, quota_deferrals FROM twitch_clips_upload_queue WHERE id = $1",
            )
            .bind(queue_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            item.attempts = attempts;
            item.quota_deferrals = vertagungen;
        }

        let (status, attempts, vertagungen): (String, i32, i32) = sqlx::query_as(
            "SELECT status, attempts, quota_deferrals FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            status, "failed",
            "nach der Kontingent-Obergrenze darf sich der Job nicht weiter vertagen"
        );
        assert_eq!(
            attempts, 1,
            "nur das abschliessende Verwerfen zaehlt einen Versuch, \
             die 29 Vertagungen davor nicht"
        );
        assert_eq!(vertagungen, UploadTask::MAX_KONTINGENT_VERTAGUNGEN - 1);
    }

    #[tokio::test]
    async fn existing_uploaded_flag_skips_platform_upload() {
        let Some(pool) = make_pool("t_sm_upload_existing_flag").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_tiktok, tiktok_video_id) \
             VALUES ('c1', 'https://clips.test/c1', 'nani', TRUE, 'old_vid') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        approve_tiktok(&pool, i32::try_from(clip).unwrap()).await;
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'tiktok', 'processing') RETURNING id",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let ok = task(pool.clone())
            .process(
                upload_item(queue_id, clip, None),
                Arc::new(OkUploader {
                    calls: calls.clone(),
                }),
            )
            .await;
        assert!(ok);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let qstatus: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(queue_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(qstatus, "completed");
        let (uploaded, video_id): (bool, Option<String>) = sqlx::query_as(
            "SELECT uploaded_tiktok, tiktok_video_id FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(uploaded);
        assert_eq!(video_id.as_deref(), Some("old_vid"));
    }
}
