//! Upload-Worker (Port von `bot/social_media/upload_worker.py`).
//!
//! Verarbeitet die Upload-Queue: pending-Jobs holen → je Streamer den passenden
//! Uploader auflösen (Credentials, global-Fallback, Cache) → Twitch-Clip per
//! yt-dlp laden → ins Hochformat schneiden → zur Plattform hochladen →
//! Queue-Status setzen. Approval-Gate: ohne Freigabe wird der Job `failed`
//! ('approval_required'). An/Aus 1:1 — in Python dauerhaft an (kein Gate).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::approval::is_clip_approved_for;
use crate::clip_queue::{get_upload_queue, update_upload_status, UploadQueueItem};
use crate::credentials::{CredentialManager, SocialMediaCredentials};
use crate::posting_plan::acquire_release_lock;
use crate::preparation::{
    ClipPreparationService, IsolatedClipDownloader, PreparationError, VideoClipRenderer,
};
use crate::uploaders::instagram::InstagramUploader;
use crate::uploaders::tiktok::TikTokUploader;
use crate::uploaders::youtube::{YouTubeRefreshCreds, YouTubeUploader, GOOGLE_TOKEN_URL};
use crate::uploaders::PlatformUploader;
#[cfg(test)]
use crate::uploaders::UploadError;
use crate::video_processor::VideoProcessor;

const STALE_AFTER_SECS: i64 = 30 * 60;
const INITIAL_DELAY_SECS: u64 = 10;
const DEFAULT_INTERVAL_SECS: u64 = 60;
const DEFAULT_MAX_PARALLEL: usize = 2;

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error(transparent)]
    Prepare(#[from] PreparationError),
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

/// Parst die Queue-Hashtags (JSON-Array-String) in eine Liste.
fn parse_hashtags(raw: Option<&str>) -> Vec<String> {
    raw.filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

/// Bindet kurze Provider-Endzustände an dieselbe Streamer-Sperre wie
/// Provider-Start und Kill-Switch. Der Netzwerkaufruf ist zu diesem Zeitpunkt
/// bereits beendet; die Transaktion umfasst ausschließlich den DB-Abschluss.
async fn lock_provider_outcome(
    connection: &mut PgConnection,
    queue_id: i64,
) -> Result<bool, sqlx::Error> {
    let streamer_login: Option<String> = sqlx::query_scalar(
        "SELECT c.streamer_login FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id \
         WHERE q.id = $1",
    )
    .bind(queue_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(streamer_login) = streamer_login else {
        return Ok(false);
    };
    acquire_release_lock(connection, &streamer_login).await?;
    Ok(true)
}

/// Cheap-clone-barer Verarbeitungskontext (für nebenläufige Uploads).
#[derive(Clone)]
struct UploadTask {
    pool: PgPool,
    preparation: ClipPreparationService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UploadGate {
    Live,
    PrepareOnly,
    Discarded,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TikTokConfirmationError {
    Rejected,
    Uncertain,
}

impl UploadTask {
    /// Verarbeitet einen Queue-Job; liefert `true` bei Erfolg.
    async fn process(&self, item: UploadQueueItem, uploader: Arc<dyn PlatformUploader>) -> bool {
        let clip_db_id = item.clip_db_id;
        if !self.release_gate_allows_upload(&item).await {
            return false;
        }
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
                self.park_claimed(
                    &item,
                    "approval_check_failed",
                    chrono::Duration::minutes(15),
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
                    self.update_upload_status_logged(
                        &item,
                        "failed",
                        None,
                        Some("completed_write_failed"),
                        "completed_write_failed_existing",
                    )
                    .await;
                }
                return true;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(queue_id = item.id, %e, "uploaded-flag check failed before upload");
                self.park_claimed(
                    &item,
                    "uploaded_flag_check_failed",
                    chrono::Duration::minutes(15),
                )
                .await;
                return false;
            }
        }
        match self.do_upload(&item).await {
            Ok(converted) => {
                if uploader.validate_video(&converted.path).is_err() {
                    tracing::warn!(queue_id = item.id, platform = %item.platform, "Lokale Provider-Validierung ist fehlgeschlagen");
                    self.update_upload_status_logged(
                        &item,
                        "failed",
                        None,
                        Some("video_validation_failed"),
                        "video_validation_failed",
                    )
                    .await;
                    return false;
                }
                // Kurze, commitbare Provider-Lease: Gate prüfen + eindeutigen
                // Startmarker schreiben, dann erst nach COMMIT ins Netzwerk.
                // Dadurch hält kein DB-Lock über dem Provider-Aufruf.
                let provider_lease = match self.begin_provider_upload(&item, &converted).await {
                    Ok(Some(lease)) => lease,
                    Ok(None) => return false,
                    Err(error) => {
                        tracing::error!(%error, queue_id = item.id, "Provider-Start konnte nicht gesichert werden");
                        self.park_claimed(
                            &item,
                            "release_gate_failed",
                            chrono::Duration::minutes(15),
                        )
                        .await;
                        return false;
                    }
                };
                let upload_result = uploader
                    .upload_video(
                        &converted.path,
                        &converted.title,
                        &converted.description,
                        &converted.hashtags,
                    )
                    .await;
                match upload_result {
                    Ok(external_id) => {
                        match self
                            .record_provider_acceptance(&item, &provider_lease, &external_id)
                            .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                tracing::error!(
                                    queue_id = item.id,
                                    "Provider-Annahme gehört nicht mehr zur aktuellen Lease"
                                );
                                return false;
                            }
                            Err(error) => {
                                tracing::error!(%error, queue_id = item.id, "Provider-Annahme konnte nicht als Abgleichanker gespeichert werden");
                                self.mark_provider_reconciliation_required(
                                    &item,
                                    &provider_lease,
                                    "provider_acceptance_write_uncertain",
                                )
                                .await;
                                return false;
                            }
                        }
                        // TikTok liefert nur eine `publish_id`, also "zur
                        // Verarbeitung angenommen". Wer das als Erfolg verbucht,
                        // merkt nie, wenn TikTok den Post danach ablehnt.
                        let external_id = if item.platform == "tiktok" {
                            match self
                                .warte_auf_tiktok(&item, uploader.as_ref(), &external_id)
                                .await
                            {
                                Ok(id) => id,
                                Err(TikTokConfirmationError::Rejected) => {
                                    self.mark_provider_rejected(
                                        &item,
                                        &provider_lease,
                                        "provider_rejected",
                                    )
                                    .await;
                                    tracing::warn!(queue_id = item.id, platform = %item.platform, code = "provider_rejected", "TikTok hat die Veröffentlichung abgelehnt");
                                    return false;
                                }
                                Err(TikTokConfirmationError::Uncertain) => {
                                    self.mark_provider_reconciliation_required(
                                        &item,
                                        &provider_lease,
                                        "tiktok_publish_uncertain",
                                    )
                                    .await;
                                    tracing::warn!(queue_id = item.id, platform = %item.platform, "TikTok-Ergebnis ist unklar; kein automatischer Wiederholungsversuch");
                                    return false;
                                }
                            }
                        } else {
                            external_id
                        };
                        // TikTok liefert nach dem Poll ggf. erst die echte
                        // Post-ID. Sie wird vor dem Completion-Schritt erneut
                        // lease-gebunden persistiert, damit ein Crash genau
                        // zwischen Poll und Abschluss einen brauchbaren
                        // Reconciliation-Anker hinterlässt.
                        match self
                            .record_provider_acceptance(&item, &provider_lease, &external_id)
                            .await
                        {
                            Ok(true) => {}
                            Ok(false) => return false,
                            Err(error) => {
                                tracing::error!(queue_id = item.id, code = "provider_acceptance_write_uncertain", error = %error, "Finale Provider-ID konnte nicht gespeichert werden");
                                self.mark_provider_reconciliation_required(
                                    &item,
                                    &provider_lease,
                                    "provider_acceptance_write_uncertain",
                                )
                                .await;
                                return false;
                            }
                        }
                        match crate::clip_queue::complete_provider_upload(
                            &self.pool,
                            item.id,
                            &provider_lease,
                            Some(&external_id),
                        )
                        .await
                        {
                            Ok(true) => return true,
                            Ok(false) => {
                                tracing::error!(
                                    queue_id = item.id,
                                    "Provider-Erfolg gehört nicht mehr zur aktuellen Lease"
                                );
                            }
                            Err(e) => {
                                tracing::error!(queue_id = item.id, %e, "completed-write failed after successful upload");
                                self.mark_provider_reconciliation_required(
                                    &item,
                                    &provider_lease,
                                    "completed_write_uncertain",
                                )
                                .await;
                            }
                        }
                        false
                    }
                    Err(e) => {
                        self.mark_provider_reconciliation_required(
                            &item,
                            &provider_lease,
                            "provider_result_uncertain",
                        )
                        .await;
                        let _ = e;
                        tracing::warn!(queue_id = item.id, platform = %item.platform, "Provider-Ergebnis ist unklar; kein automatischer Wiederholungsversuch");
                        false
                    }
                }
            }
            Err(WorkerError::Prepare(PreparationError::Busy(_))) => {
                self.park_claimed(&item, "preparation_busy", chrono::Duration::minutes(2))
                    .await;
                false
            }
            Err(WorkerError::Prepare(
                error @ (PreparationError::Db(_)
                | PreparationError::Io(_)
                | PreparationError::Download(_)),
            )) => {
                tracing::warn!(
                    queue_id = item.id,
                    clip_db_id,
                    code = error.code(),
                    "Vorübergehende Clip-Aufbereitung fehlgeschlagen"
                );
                self.park_claimed(
                    &item,
                    "preparation_transient",
                    chrono::Duration::minutes(15),
                )
                .await;
                false
            }
            Err(WorkerError::Prepare(error)) => {
                let code = error.code();
                tracing::warn!(
                    queue_id = item.id,
                    clip_db_id,
                    code,
                    "Clip-Aufbereitung dauerhaft fehlgeschlagen"
                );
                self.update_upload_status_logged(
                    &item,
                    "failed",
                    None,
                    Some(code),
                    "upload_worker_failed",
                )
                .await;
                false
            }
        }
    }

    /// Lädt (falls nötig) den Clip und konvertiert ihn; liefert die Upload-Daten.
    async fn do_upload(&self, item: &UploadQueueItem) -> Result<Converted, WorkerError> {
        self.update_upload_status_logged(item, "processing", None, None, "processing_start")
            .await;

        let prepared = self.preparation.prepare(item.clip_db_id).await?;
        let converted_path = prepared
            .ready_path()
            .ok_or_else(|| {
                PreparationError::Renderer(
                    "Aufbereitung meldet kein gespeichertes Ready-MP4".to_string(),
                )
            })?
            .to_string();
        let render_fingerprint = prepared
            .render_fingerprint
            .clone()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                PreparationError::Renderer(
                    "Aufbereitung meldet keinen Render-Fingerprint".to_string(),
                )
            })?;
        self.update_upload_status_logged(item, "processing", None, None, "processing_converted")
            .await;

        let title = item
            .title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| item.clip_title.clone())
            .unwrap_or_default();
        Ok(Converted {
            path: converted_path,
            render_fingerprint,
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

    /// Reserviert genau einen externen Provider-Aufruf unter derselben kurzen
    /// Streamer-Sperre wie der Kill-Switch. Nach dem Commit ist die Lease eine
    /// absichtlich nicht zeitbasiert reclaimbare, möglicherweise bereits beim
    /// Provider sichtbare Operation.
    async fn begin_provider_upload(
        &self,
        item: &UploadQueueItem,
        converted: &Converted,
    ) -> Result<Option<String>, sqlx::Error> {
        let streamer_login = item
            .streamer_login
            .as_deref()
            .map(str::trim)
            .filter(|login| !login.is_empty());
        let Some(streamer_login) = streamer_login else {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', \
                 last_error = 'streamer_missing', last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
            )
            .bind(item.id)
            .execute(&self.pool)
            .await?;
            return Ok(None);
        };

        let mut transaction = self.pool.begin().await?;
        acquire_release_lock(transaction.as_mut(), streamer_login).await?;
        let preparation: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT state, render_fingerprint, render_path \
             FROM social_media_clip_preparation WHERE clip_db_id = $1 FOR UPDATE",
        )
        .bind(item.clip_db_id)
        .fetch_optional(transaction.as_mut())
        .await?;
        let gate = self
            .release_gate_in(transaction.as_mut(), item.clip_db_id)
            .await?;
        let queue_locked: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM twitch_clips_upload_queue \
             WHERE id = $1 AND clip_id = $2 AND platform = $3 \
               AND status = 'processing' AND provider_started_at IS NULL FOR UPDATE",
        )
        .bind(item.id)
        .bind(item.clip_db_id)
        .bind(&item.platform)
        .fetch_optional(transaction.as_mut())
        .await?;
        if queue_locked.is_none() {
            transaction.commit().await?;
            return Ok(None);
        }
        let provider_calls_enabled: bool = sqlx::query_scalar(
            "SELECT COALESCE(( \
                 SELECT a.provider_calls_enabled FROM social_media_platform_auth a \
                  WHERE a.platform = $1 AND a.enabled = 1 AND ( \
                        LOWER(a.streamer_login) = LOWER($2) OR a.streamer_login IS NULL) \
                  ORDER BY CASE WHEN LOWER(a.streamer_login) = LOWER($2) THEN 1 ELSE 0 END DESC, \
                           a.id DESC LIMIT 1 \
             ), FALSE)",
        )
        .bind(&item.platform)
        .bind(streamer_login)
        .fetch_one(transaction.as_mut())
        .await?;
        // TikTok Direct Post bleibt unabhängig von OAuth, globalem Live-Modus
        // und provider_calls_enabled gesperrt, bis eine explizite per-Clip-
        // Consent-/Privacy-/Interaktions-UX gespeichert und hier geprüft wird.
        if item.platform == "tiktok" {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', \
                 last_error = 'tiktok_consent_required', last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
            )
            .bind(item.id)
            .execute(transaction.as_mut())
            .await?;
            transaction.commit().await?;
            return Ok(None);
        }
        if !provider_calls_enabled {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', \
                 last_error = 'platform_release_blocked', last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
            )
            .bind(item.id)
            .execute(transaction.as_mut())
            .await?;
            transaction.commit().await?;
            return Ok(None);
        }
        let cadence_enabled: bool = sqlx::query_scalar(
            "SELECT COALESCE((SELECT auto_post AND posts_per_week > 0 AND max_posts_per_day > 0 \
                FROM social_media_platform_schedule \
               WHERE LOWER(streamer_login) = LOWER($1) AND platform = $2), FALSE)",
        )
        .bind(streamer_login)
        .bind(&item.platform)
        .fetch_one(transaction.as_mut())
        .await?;
        if !cadence_enabled {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', \
                 last_error = 'platform_paused', last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
            )
            .bind(item.id)
            .execute(transaction.as_mut())
            .await?;
            transaction.commit().await?;
            return Ok(None);
        }
        if gate != UploadGate::Live {
            let (status, error) = match gate {
                UploadGate::PrepareOnly => ("pending", "release_disabled"),
                UploadGate::Discarded => ("failed", "clip_discarded"),
                UploadGate::Missing => ("failed", "clip_not_found"),
                UploadGate::Live => unreachable!(),
            };
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = $2, last_error = $3, \
                 last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
            )
            .bind(item.id)
            .bind(status)
            .bind(error)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(None);
        }

        let approval: Option<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT state, approved_platforms::text, approved_render_fingerprint \
             FROM social_media_clip_approval WHERE clip_db_id = $1 FOR UPDATE",
        )
        .bind(item.clip_db_id)
        .fetch_optional(transaction.as_mut())
        .await?;
        let approved_for_render = match (preparation, approval) {
            (
                Some((preparation_state, Some(render_fingerprint), Some(render_path))),
                Some((approval_state, approved_platforms, Some(approved_fingerprint))),
            ) => {
                let platforms = parse_hashtags(Some(&approved_platforms));
                preparation_state == "preview_ready"
                    && approval_state == "approved"
                    && platforms.contains(&item.platform)
                    && render_fingerprint == approved_fingerprint
                    && render_fingerprint == converted.render_fingerprint
                    && render_path == converted.path
                    && crate::preparation::stored_file_is_regular_nonempty(&render_path)
            }
            _ => false,
        };
        if !approved_for_render {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', \
                 last_error = 'approval_or_preview_changed', last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
            )
            .bind(item.id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(None);
        }

        let lease_token = Uuid::new_v4().to_string();
        let updated = sqlx::query(
            "UPDATE twitch_clips_upload_queue q \
             SET provider_started_at = CURRENT_TIMESTAMP, provider_lease_token = $2, \
                 last_attempt_at = CURRENT_TIMESTAMP, last_error = NULL \
             WHERE q.id = $1 AND q.status = 'processing' \
               AND q.provider_started_at IS NULL \
               AND NOT EXISTS ( \
                   SELECT 1 FROM twitch_clips_upload_queue other \
                   WHERE other.clip_id = q.clip_id AND other.platform = q.platform \
                     AND other.id <> q.id AND other.provider_started_at IS NOT NULL \
                     AND other.status IN ('processing', 'reconciliation_required') \
               )",
        )
        .bind(item.id)
        .bind(&lease_token)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            sqlx::query(
                "UPDATE twitch_clips_upload_queue SET status = 'failed', \
                 last_error = 'provider_attempt_exists', last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
            )
            .bind(item.id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(None);
        }
        transaction.commit().await?;
        Ok(Some(lease_token))
    }

    async fn mark_provider_reconciliation_required(
        &self,
        item: &UploadQueueItem,
        provider_lease_token: &str,
        error_code: &'static str,
    ) {
        let result = async {
            let mut transaction = self.pool.begin().await?;
            if !lock_provider_outcome(transaction.as_mut(), item.id).await? {
                transaction.commit().await?;
                return Ok::<(), sqlx::Error>(());
            }
            sqlx::query(
                "UPDATE twitch_clips_upload_queue \
                 SET status = 'reconciliation_required', last_error = $3, \
                     last_attempt_at = CURRENT_TIMESTAMP \
                 WHERE id = $1 AND provider_lease_token = $2 \
                   AND provider_started_at IS NOT NULL AND status = 'processing'",
            )
            .bind(item.id)
            .bind(provider_lease_token)
            .bind(error_code)
            .execute(transaction.as_mut())
            .await?;
            transaction.commit().await?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            tracing::error!(%error, queue_id = item.id, error_code, "Unklarer Provider-Versuch konnte nicht geparkt werden");
        }
    }

    async fn mark_provider_rejected(
        &self,
        item: &UploadQueueItem,
        provider_lease_token: &str,
        error_code: &'static str,
    ) {
        let result = async {
            let mut transaction = self.pool.begin().await?;
            if !lock_provider_outcome(transaction.as_mut(), item.id).await? {
                transaction.commit().await?;
                return Ok::<(), sqlx::Error>(());
            }
            sqlx::query(
                "UPDATE twitch_clips_upload_queue \
                 SET status = 'failed', last_error = $3, completed_at = clock_timestamp(), \
                     last_attempt_at = clock_timestamp() \
                 WHERE id = $1 AND provider_lease_token = $2 \
                   AND provider_started_at IS NOT NULL AND status = 'processing'",
            )
            .bind(item.id)
            .bind(provider_lease_token)
            .bind(error_code)
            .execute(transaction.as_mut())
            .await?;
            transaction.commit().await?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            tracing::error!(%error, queue_id = item.id, error_code, "Provider-Ablehnung konnte nicht gespeichert werden");
        }
    }

    async fn record_provider_acceptance(
        &self,
        item: &UploadQueueItem,
        provider_lease_token: &str,
        external_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        if !lock_provider_outcome(transaction.as_mut(), item.id).await? {
            transaction.commit().await?;
            return Ok(false);
        }
        let updated = sqlx::query(
            "UPDATE twitch_clips_upload_queue SET provider_external_id = $3, \
             provider_accepted_at = CURRENT_TIMESTAMP \
             WHERE id = $1 AND provider_lease_token = $2 \
               AND provider_started_at IS NOT NULL AND status = 'processing'",
        )
        .bind(item.id)
        .bind(provider_lease_token)
        .bind(external_id)
        .execute(transaction.as_mut())
        .await?;
        transaction.commit().await?;
        Ok(updated.rows_affected() == 1)
    }

    async fn release_gate(&self, clip_db_id: i64) -> Result<UploadGate, sqlx::Error> {
        let row: Option<(bool, String)> = sqlx::query_as(
            "SELECT c.discarded_at IS NOT NULL, COALESCE(s.release_mode, 'prepare_only') \
               FROM twitch_clips_social_media c \
               LEFT JOIN social_media_streamer_settings s \
                 ON LOWER(s.streamer_login) = LOWER(c.streamer_login) \
              WHERE c.id = $1",
        )
        .bind(clip_db_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(match row {
            None => UploadGate::Missing,
            Some((true, _)) => UploadGate::Discarded,
            Some((false, mode)) if mode == "live" => UploadGate::Live,
            Some(_) => UploadGate::PrepareOnly,
        })
    }

    async fn release_gate_in(
        &self,
        connection: &mut PgConnection,
        clip_db_id: i64,
    ) -> Result<UploadGate, sqlx::Error> {
        let row: Option<(bool, String)> = sqlx::query_as(
            "SELECT c.discarded_at IS NOT NULL, COALESCE(s.release_mode, 'prepare_only') \
               FROM twitch_clips_social_media c \
               LEFT JOIN social_media_streamer_settings s \
                 ON LOWER(s.streamer_login) = LOWER(c.streamer_login) \
              WHERE c.id = $1 FOR UPDATE OF c",
        )
        .bind(clip_db_id)
        .fetch_optional(&mut *connection)
        .await?;
        Ok(match row {
            None => UploadGate::Missing,
            Some((true, _)) => UploadGate::Discarded,
            Some((false, mode)) if mode == "live" => UploadGate::Live,
            Some(_) => UploadGate::PrepareOnly,
        })
    }

    async fn release_gate_allows_upload(&self, item: &UploadQueueItem) -> bool {
        let (allowed, status, error) = match self.release_gate(item.clip_db_id).await {
            Ok(UploadGate::Live) => return true,
            Ok(UploadGate::PrepareOnly) => (false, "pending", "release_disabled"),
            Ok(UploadGate::Discarded) => (false, "failed", "clip_discarded"),
            Ok(UploadGate::Missing) => (false, "failed", "clip_not_found"),
            Err(error) => {
                tracing::error!(%error, queue_id = item.id, "Release-Gate konnte nicht geprüft werden");
                (false, "pending", "release_gate_failed")
            }
        };
        if let Err(db_error) = sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $2, last_error = $3, \
             last_attempt_at = CURRENT_TIMESTAMP \
             WHERE id = $1 AND status <> 'completed' AND provider_started_at IS NULL",
        )
        .bind(item.id)
        .bind(status)
        .bind(error)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%db_error, queue_id = item.id, "Release-Gate-Status konnte nicht gespeichert werden");
        }
        allowed
    }

    async fn park_claimed(&self, item: &UploadQueueItem, error: &str, delay: chrono::Duration) {
        let scheduled_at = (Utc::now() + delay).to_rfc3339();
        if let Err(db_error) = sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = 'pending', last_error = $2, \
             scheduled_at = $3::text::timestamptz, last_attempt_at = CURRENT_TIMESTAMP \
             WHERE id = $1 AND status = 'processing' AND provider_started_at IS NULL",
        )
        .bind(item.id)
        .bind(error)
        .bind(scheduled_at)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%db_error, queue_id = item.id, error, "Geclaimter Upload konnte nicht zurückgestellt werden");
        }
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
    ) -> Result<String, TikTokConfirmationError> {
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
                    return Ok(post_id);
                }
                "FAILED" => {
                    return Err(TikTokConfirmationError::Rejected);
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
                return Err(TikTokConfirmationError::Uncertain);
            }
            tokio::time::sleep(Self::TIKTOK_BESTAETIGUNG_ABSTAND).await;
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
}

struct Converted {
    path: String,
    render_fingerprint: String,
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
                preparation: ClipPreparationService::new(pool.clone()).with_renderer(Arc::new(
                    VideoClipRenderer::new(VideoProcessor::new(
                        "/usr/bin/ffmpeg",
                        "/usr/bin/ffprobe",
                    )),
                )),
                pool,
            },
            credentials,
            max_parallel: DEFAULT_MAX_PARALLEL,
            interval: Duration::from_secs(DEFAULT_INTERVAL_SECS),
        }
    }

    pub fn with_isolated_downloader(mut self) -> Self {
        self.task.preparation = self
            .task
            .preparation
            .clone()
            .with_downloader(Arc::new(IsolatedClipDownloader::new()));
        self
    }

    pub fn with_clips_dir(mut self, dir: impl Into<String>) -> Self {
        self.task.preparation = self.task.preparation.clone().with_clips_dir(dir.into());
        self
    }

    pub fn with_video_processor(mut self, vp: VideoProcessor) -> Self {
        self.task.preparation = self
            .task
            .preparation
            .clone()
            .with_renderer(Arc::new(VideoClipRenderer::new(vp)));
        self
    }

    /// Löst den Uploader für einen Job auf (Cache nach (Plattform, Credential-ID)).
    async fn resolve_uploader(
        &self,
        platform: &str,
        streamer_login: Option<&str>,
        cache: &mut HashMap<(String, i32), Option<Arc<dyn PlatformUploader>>>,
    ) -> Option<Arc<dyn PlatformUploader>> {
        let creds = self
            .credentials
            .get_credentials(platform, streamer_login)
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
    pub async fn run_once(&self) {
        // `get_upload_queue` claimt atomar. Deshalb nie mehr Jobs holen als in
        // diesem Lauf wirklich verarbeitet oder sichtbar zurückgestellt werden.
        let scan_limit = self.max_parallel as i64;
        let stale_cutoff = (Utc::now() - chrono::Duration::seconds(STALE_AFTER_SECS)).to_rfc3339();
        let queue = match get_upload_queue(
            &self.task.pool,
            None,
            "pending",
            scan_limit,
            Some(&stale_cutoff),
        )
        .await
        {
            Ok(queue) => queue,
            Err(error) => {
                tracing::error!(
                    code = "upload_queue_claim_failed",
                    database_code = ?error
                        .as_database_error()
                        .and_then(|database| database.code()),
                    "Social-Media-Upload-Queue konnte nicht sicher geclaimt werden"
                );
                return;
            }
        };
        if queue.is_empty() {
            return;
        }

        let mut cache: HashMap<(String, i32), Option<Arc<dyn PlatformUploader>>> = HashMap::new();
        let mut batch: Vec<(UploadQueueItem, Arc<dyn PlatformUploader>)> = Vec::new();
        for item in queue {
            if let Some(uploader) = self
                .resolve_uploader(&item.platform, item.streamer_login.as_deref(), &mut cache)
                .await
            {
                batch.push((item, uploader));
                if batch.len() >= self.max_parallel {
                    break;
                }
            } else {
                self.task
                    .park_claimed(&item, "credentials_missing", chrono::Duration::minutes(30))
                    .await;
            }
        }
        if batch.is_empty() {
            return;
        }

        let mut set = tokio::task::JoinSet::new();
        let mut recovery = HashMap::new();
        for (item, uploader) in batch {
            let recovery_item = item.clone();
            let task = self.task.clone();
            let abort = set.spawn(async move { task.process(item, uploader).await });
            recovery.insert(abort.id(), recovery_item);
        }
        while let Some(result) = set.join_next_with_id().await {
            match result {
                Ok((id, _)) => {
                    recovery.remove(&id);
                }
                Err(error) => {
                    let item = recovery.remove(&error.id());
                    tracing::error!(%error, "Upload-Worker: Upload-Task fehlerhaft beendet");
                    if let Some(item) = item {
                        self.task
                            .park_claimed(
                                &item,
                                "upload_task_panicked",
                                chrono::Duration::minutes(15),
                            )
                            .await;
                    }
                }
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

#[cfg(test)]
mod tests {
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
        assert_eq!(crate::preparation::PREPARATION_MAX_DURATION_SECS, 60);
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
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
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
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1, provider_calls_enabled BOOLEAN NOT NULL DEFAULT TRUE)",
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, clip_title TEXT, streamer_login TEXT NOT NULL, local_file_path TEXT, upload_local_path TEXT, converted_file_path TEXT, downloaded_at TIMESTAMPTZ, status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT, tiktok_uploaded_at TIMESTAMPTZ, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ, discarded_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, release_mode TEXT NOT NULL DEFAULT 'live')",
            "INSERT INTO social_media_streamer_settings (streamer_login) VALUES ('nani')",
            "CREATE TABLE social_media_platform_schedule (streamer_login TEXT NOT NULL, platform TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT TRUE, posts_per_week INTEGER NOT NULL DEFAULT 4, max_posts_per_day INTEGER NOT NULL DEFAULT 1, PRIMARY KEY (streamer_login, platform))",
            "INSERT INTO social_media_platform_schedule (streamer_login, platform) VALUES ('nani','tiktok'),('nani','youtube'),('nani','instagram')",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY REFERENCES twitch_clips_social_media(id), state TEXT NOT NULL DEFAULT 'pending', lease_token TEXT, source_fingerprint TEXT, render_fingerprint TEXT, render_path TEXT, error_code TEXT, error_message TEXT, requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ, approved_render_fingerprint TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)",
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
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
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
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, clip_title TEXT, streamer_login TEXT NOT NULL, local_file_path TEXT, upload_local_path TEXT, converted_file_path TEXT, downloaded_at TIMESTAMPTZ, status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ, discarded_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, release_mode TEXT NOT NULL DEFAULT 'live')",
            "INSERT INTO social_media_streamer_settings (streamer_login) VALUES ('nani')",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY REFERENCES twitch_clips_social_media(id), state TEXT NOT NULL DEFAULT 'pending', lease_token TEXT, source_fingerprint TEXT, render_fingerprint TEXT, render_path TEXT, error_code TEXT, error_message TEXT, requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ, approved_render_fingerprint TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)",
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

    struct CopyRenderer;

    #[async_trait::async_trait]
    impl crate::preparation::ClipRenderer for CopyRenderer {
        async fn compose_and_trim(
            &self,
            source: &std::path::Path,
            destination: &std::path::Path,
            _layout: &crate::layout::StreamerLayout,
            max_duration_secs: i64,
        ) -> Result<(), PreparationError> {
            assert_eq!(max_duration_secs, 60);
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(source, destination).await?;
            Ok(())
        }
    }

    fn task(pool: PgPool) -> UploadTask {
        let clips_dir = unique_temp_dir("upload_task");
        UploadTask {
            preparation: ClipPreparationService::new(pool.clone())
                .with_clips_dir(clips_dir)
                .with_renderer(Arc::new(CopyRenderer)),
            pool,
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
    async fn credentials_missing_stellt_claim_sichtbar_und_verzoegert_zurueck() {
        let Some(pool) = make_pool("t_sm_upload_credentials_missing").await else {
            return;
        };
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) \
             VALUES ('missing-creds', 'https://clips.twitch.tv/FancyClip', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'tiktok', 'processing') RETURNING id",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        let item = upload_item(queue_id, clip, None);

        task(pool.clone())
            .park_claimed(&item, "credentials_missing", chrono::Duration::minutes(30))
            .await;

        let (status, error, delayed): (String, Option<String>, bool) = sqlx::query_as(
            "SELECT status, last_error, scheduled_at > CURRENT_TIMESTAMP \
             FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(error.as_deref(), Some("credentials_missing"));
        assert!(delayed);
    }

    #[tokio::test]
    async fn tiktok_providerstart_bleibt_trotz_live_und_providerfreigabe_gesperrt() {
        let Some(pool) = make_pool("t_sm_upload_tiktok_consent_gate").await else {
            return;
        };
        let dir = unique_temp_dir("tiktok_consent_gate");
        let render = dir.join("render.mp4");
        std::fs::write(&render, b"rendered").unwrap();
        let render_path = render.to_string_lossy().into_owned();
        let clip: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) \
             VALUES ('consent-gate', 'https://clips.test/consent-gate', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_auth \
             (platform, streamer_login, provider_calls_enabled) \
             VALUES ('tiktok', 'nani', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation \
             (clip_db_id, state, render_fingerprint, render_path) \
             VALUES ($1, 'preview_ready', 'render-v1', $2)",
        )
        .bind(clip)
        .bind(&render_path)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_approval \
             (clip_db_id, state, approved_platforms, approved_render_fingerprint) \
             VALUES ($1, 'approved', '[\"tiktok\"]'::jsonb, 'render-v1')",
        )
        .bind(i32::try_from(clip).unwrap())
        .execute(&pool)
        .await
        .unwrap();
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, 'tiktok', 'processing') RETURNING id",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        let item = upload_item(queue_id, clip, Some(render_path.clone()));
        let converted = Converted {
            path: render_path,
            render_fingerprint: "render-v1".to_string(),
            title: String::new(),
            description: String::new(),
            hashtags: Vec::new(),
        };

        assert!(task(pool.clone())
            .begin_provider_upload(&item, &converted)
            .await
            .unwrap()
            .is_none());
        let (status, error, started): (String, Option<String>, bool) = sqlx::query_as(
            "SELECT status, last_error, provider_started_at IS NOT NULL \
             FROM twitch_clips_upload_queue WHERE id = $1",
        )
        .bind(queue_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(error.as_deref(), Some("tiktok_consent_required"));
        assert!(!started);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn completed_write_failure_after_success_marks_failed() {
        let Some(pool) = make_completed_write_error_pool("t_sm_upload_completed_write_fail").await
        else {
            return;
        };
        let dir = unique_temp_dir("completed_write_fail");
        let input_path = dir.join("clip.mp4");
        let converted_path = dir.join("clip_tiktok_vertical.mp4");
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
        assert_eq!(status, "failed");
        assert_eq!(attempts, 1);
        assert!(err
            .as_deref()
            .unwrap_or("")
            .starts_with("completed_write_failed:"));

        let _ = std::fs::remove_dir_all(dir);
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
