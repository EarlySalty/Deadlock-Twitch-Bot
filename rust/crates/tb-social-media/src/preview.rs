use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use crate::clip_prep_worker::{download_atomic, register_local_file, ClipDownloader, YtDlpDownloader};
use crate::render::render_clip_vertical;
use crate::video_processor::VideoProcessor;

pub const PREVIEW_PENDING: &str = "pending";
pub const PREVIEW_RENDERING: &str = "rendering";
pub const PREVIEW_READY: &str = "ready";
pub const PREVIEW_ERROR: &str = "error";

const PREVIEW_MAX_SECS: i64 = 60;
const INTERVAL_SECS: u64 = 20;
const INITIAL_DELAY_SECS: u64 = 15;
const BATCH_SIZE: i64 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewStatus {
    pub status: Option<String>,
    pub error: Option<String>,
    pub path: Option<String>,
}

/// Stoesst den Vorschau-Render fuer einen Clip an: Status auf `pending`, alter
/// Fehler und Pfad geloescht. Der Vorschau-Worker uebernimmt das Rendern.
pub async fn request_preview(pool: &PgPool, clip_db_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE twitch_clips_social_media \
            SET preview_status = 'pending', preview_error = NULL, \
                preview_updated_at = $1::text::timestamptz WHERE id = $2",
        Utc::now().to_rfc3339(),
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_preview(pool: &PgPool, clip_db_id: i64) -> Option<PreviewStatus> {
    let row = sqlx::query!(
        "SELECT preview_status, preview_error, preview_path FROM twitch_clips_social_media WHERE id = $1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    Some(PreviewStatus {
        status: row.preview_status,
        error: row.preview_error,
        path: row.preview_path,
    })
}

async fn finish_ready(pool: &PgPool, clip_db_id: i64, path: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE twitch_clips_social_media \
            SET preview_status = 'ready', preview_path = $1, preview_error = NULL, \
                preview_updated_at = $2::text::timestamptz WHERE id = $3",
        path,
        Utc::now().to_rfc3339(),
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn finish_error(pool: &PgPool, clip_db_id: i64, message: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE twitch_clips_social_media \
            SET preview_status = 'error', preview_error = $1, \
                preview_updated_at = $2::text::timestamptz WHERE id = $3",
        message,
        Utc::now().to_rfc3339(),
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

struct PreviewJob {
    clip_db_id: i64,
    clip_url: String,
    local_file_path: Option<String>,
}

async fn claim_pending(pool: &PgPool, limit: i64) -> Vec<PreviewJob> {
    let rows = sqlx::query!(
        "UPDATE twitch_clips_social_media \
            SET preview_status = 'rendering', preview_updated_at = NOW() \
          WHERE id IN ( \
              SELECT id FROM twitch_clips_social_media \
               WHERE preview_status = 'pending' \
                  OR (preview_status = 'rendering' AND preview_updated_at < NOW() - INTERVAL '15 minutes') \
               ORDER BY preview_updated_at ASC NULLS FIRST LIMIT $1 FOR UPDATE SKIP LOCKED) \
          RETURNING id AS \"id!\", clip_url, local_file_path",
        limit.max(1)
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|r| PreviewJob {
            clip_db_id: r.id,
            clip_url: r.clip_url,
            local_file_path: r.local_file_path,
        })
        .collect()
}

pub struct PreviewWorker {
    pool: PgPool,
    downloader: Arc<dyn ClipDownloader>,
    video_processor: VideoProcessor,
    clips_dir: String,
    batch_size: i64,
    interval: Duration,
}

impl PreviewWorker {
    pub fn new(pool: PgPool, yt_dlp_path: impl Into<String>, clips_dir: impl Into<String>) -> Self {
        Self {
            pool,
            downloader: Arc::new(YtDlpDownloader::new(yt_dlp_path)),
            video_processor: VideoProcessor::default(),
            clips_dir: clips_dir.into(),
            batch_size: BATCH_SIZE,
            interval: Duration::from_secs(INTERVAL_SECS),
        }
    }

    pub fn with_downloader(mut self, downloader: Arc<dyn ClipDownloader>) -> Self {
        self.downloader = downloader;
        self
    }

    pub fn with_video_processor(mut self, vp: VideoProcessor) -> Self {
        self.video_processor = vp;
        self
    }

    async fn ensure_local_file(&self, job: &PreviewJob) -> Result<String, String> {
        if let Some(path) = job.local_file_path.as_deref().filter(|s| !s.is_empty()) {
            if Path::new(path).exists() {
                return Ok(path.to_string());
            }
        }
        let dest = format!("{}/{}.mp4", self.clips_dir, job.clip_db_id);
        download_atomic(self.downloader.as_ref(), &job.clip_url, &dest).await?;
        // Auch der Vorschau-Download registriert die Quelldatei, sonst bliebe sie
        // fuer einen Clip, den der Prep-Worker nie anfasst (Kategorie other), nach
        // dem Retention-Lauf verwaist liegen (INV-07).
        if let Err(e) = register_local_file(&self.pool, job.clip_db_id, &dest).await {
            tracing::warn!(%e, clip_db_id = job.clip_db_id, "Vorschau: local_file_path nicht gespeichert");
        }
        Ok(dest)
    }

    async fn render_one(&self, job: &PreviewJob) {
        let input = match self.ensure_local_file(job).await {
            Ok(path) => path,
            Err(e) => {
                let _ = finish_error(&self.pool, job.clip_db_id, &format!("download: {e}")).await;
                return;
            }
        };
        let output = format!("{}/{}_preview.mp4", self.clips_dir, job.clip_db_id);
        match render_clip_vertical(
            &self.video_processor,
            &self.pool,
            job.clip_db_id,
            &input,
            &output,
            PREVIEW_MAX_SECS,
        )
        .await
        {
            Ok(()) => {
                let abs = std::fs::canonicalize(&output)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or(output);
                if let Err(e) = finish_ready(&self.pool, job.clip_db_id, &abs).await {
                    tracing::warn!(%e, clip_db_id = job.clip_db_id, "Vorschau: Ready-Status nicht gespeichert");
                }
            }
            Err(e) => {
                let _ = finish_error(&self.pool, job.clip_db_id, &format!("render: {e}")).await;
            }
        }
    }

    pub async fn run_once(&self) {
        let jobs = claim_pending(&self.pool, self.batch_size).await;
        for job in jobs {
            self.render_one(&job).await;
        }
    }

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
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_url TEXT, streamer_login TEXT, local_file_path TEXT, preview_path TEXT, preview_status TEXT, preview_error TEXT, preview_updated_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn vorschau_zustandsmaschine() {
        let Some(pool) = make_pool("t_sm_preview_state").await else {
            return;
        };
        let id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_url, streamer_login) VALUES ('https://clips.twitch.tv/x', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // Angestossen -> pending.
        request_preview(&pool, id).await.unwrap();
        assert_eq!(get_preview(&pool, id).await.unwrap().status.as_deref(), Some(PREVIEW_PENDING));

        // Vom Worker beansprucht -> rendering.
        let jobs = claim_pending(&pool, 5).await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].clip_db_id, id);
        assert_eq!(get_preview(&pool, id).await.unwrap().status.as_deref(), Some(PREVIEW_RENDERING));
        // Ein zweiter Claim findet nichts mehr.
        assert!(claim_pending(&pool, 5).await.is_empty());

        // Fertig -> ready + Pfad.
        finish_ready(&pool, id, "/clips/1_preview.mp4").await.unwrap();
        let st = get_preview(&pool, id).await.unwrap();
        assert_eq!(st.status.as_deref(), Some(PREVIEW_READY));
        assert_eq!(st.path.as_deref(), Some("/clips/1_preview.mp4"));

        // Fehlerpfad.
        finish_error(&pool, id, "boom").await.unwrap();
        let st = get_preview(&pool, id).await.unwrap();
        assert_eq!(st.status.as_deref(), Some(PREVIEW_ERROR));
        assert_eq!(st.error.as_deref(), Some("boom"));
    }
}
