use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use crate::clip_prep_worker::{
    download_atomic, register_local_file, ClipDownloader, YtDlpDownloader, DEFAULT_CLIPS_DIR,
};
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

pub async fn request_preview(pool: &PgPool, clip_db_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let (status, path): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT preview_status, preview_path FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE",
    )
    .bind(clip_db_id)
    .fetch_one(&mut *tx)
    .await?;
    let in_progress = matches!(status.as_deref(), Some(PREVIEW_PENDING | PREVIEW_RENDERING));
    let ready = status.as_deref() == Some(PREVIEW_READY)
        && valid_preview_path(clip_db_id, path.as_deref())
            .await
            .is_some();
    if !in_progress && !ready {
        sqlx::query(
            "UPDATE twitch_clips_social_media SET preview_status = 'pending', preview_error = NULL, preview_path = NULL, preview_updated_at = NOW() WHERE id = $1",
        )
        .bind(clip_db_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
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
    if row.preview_status.as_deref() == Some(PREVIEW_READY) {
        return Some(
            match valid_preview_path(clip_db_id, row.preview_path.as_deref()).await {
                Some(path) => PreviewStatus {
                    status: row.preview_status,
                    error: None,
                    path: Some(path),
                },
                None => PreviewStatus {
                    status: None,
                    error: None,
                    path: None,
                },
            },
        );
    }
    Some(PreviewStatus {
        status: row.preview_status,
        error: row.preview_error,
        path: None,
    })
}

async fn valid_preview_path(clip_db_id: i64, path: Option<&str>) -> Option<String> {
    let path = path?;
    let expected = format!("{clip_db_id}_preview_manual_v1.mp4");
    if Path::new(path).file_name()? != expected.as_str() {
        return None;
    }
    let path = resolve_preview_path(clip_db_id, path.to_string());
    let file = tokio::fs::File::open(&path).await.ok()?;
    let metadata = file.metadata().await.ok()?;
    if !metadata.is_file() || metadata.len() == 0 {
        return None;
    }
    let info = VideoProcessor::default().get_video_info(&path).await.ok()?;
    (info.duration.is_finite() && info.duration > 0.0).then_some(path)
}

fn resolve_preview_path(clip_db_id: i64, path: String) -> String {
    if Path::new(&path).exists() {
        return path;
    }
    let expected = format!("{clip_db_id}_preview_manual_v1.mp4");
    let stored = Path::new(&path);
    if stored
        .file_name()
        .is_some_and(|name| name == expected.as_str())
        && stored
            .parent()
            .is_some_and(|parent| parent.ends_with("data/clips"))
    {
        return format!("{DEFAULT_CLIPS_DIR}/{expected}");
    }
    path
}

async fn finish_ready(
    pool: &PgPool,
    clip_db_id: i64,
    path: &str,
    claimed_at: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE twitch_clips_social_media SET preview_status = 'ready', preview_path = $1, preview_error = NULL, preview_updated_at = NOW() WHERE id = $2 AND ($3::text IS NULL OR (preview_status = 'rendering' AND preview_updated_at = $3::text::timestamptz))",
    )
    .bind(path)
    .bind(clip_db_id)
    .bind(claimed_at)
    .execute(pool)
    .await?;
    Ok(())
}

async fn finish_error(
    pool: &PgPool,
    clip_db_id: i64,
    message: &str,
    claimed_at: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE twitch_clips_social_media SET preview_status = 'error', preview_error = $1, preview_updated_at = NOW() WHERE id = $2 AND ($3::text IS NULL OR (preview_status = 'rendering' AND preview_updated_at = $3::text::timestamptz))",
    )
    .bind(message)
    .bind(clip_db_id)
    .bind(claimed_at)
    .execute(pool)
    .await?;
    Ok(())
}

struct PreviewJob {
    clip_db_id: i64,
    clip_url: String,
    local_file_path: Option<String>,
    claimed_at: String,
}

async fn claim_pending(pool: &PgPool, limit: i64) -> Vec<PreviewJob> {
    let rows: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
        "UPDATE twitch_clips_social_media \
            SET preview_status = 'rendering', preview_updated_at = NOW() \
          WHERE id IN ( \
              SELECT id FROM twitch_clips_social_media \
               WHERE (preview_status = 'pending' \
                  OR (preview_status = 'rendering' AND (preview_updated_at IS NULL OR preview_updated_at < NOW() - INTERVAL '15 minutes'))) \
                 AND pg_try_advisory_xact_lock(hashtext('tb-social-media-preview'), hashtext(id::text)) \
               ORDER BY preview_updated_at ASC NULLS FIRST LIMIT $1 FOR UPDATE SKIP LOCKED) \
          RETURNING id, clip_url, local_file_path, preview_updated_at::text",
    )
    .bind(limit.max(1))
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(
            |(clip_db_id, clip_url, local_file_path, claimed_at)| PreviewJob {
                clip_db_id,
                clip_url,
                local_file_path,
                claimed_at,
            },
        )
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
        if let Err(e) = register_local_file(&self.pool, job.clip_db_id, &dest).await {
            tracing::warn!(%e, clip_db_id = job.clip_db_id, "Vorschau: local_file_path nicht gespeichert");
        }
        Ok(dest)
    }

    async fn render_one(&self, job: &PreviewJob) {
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::warn!(%error, clip_db_id = job.clip_db_id, "Vorschau: Rendersperre nicht verfügbar");
                return;
            }
        };
        let locked: bool = sqlx::query_scalar(
            "SELECT pg_try_advisory_xact_lock(hashtext('tb-social-media-preview'), hashtext($1))",
        )
        .bind(job.clip_db_id.to_string())
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(false);
        if !locked {
            return;
        }
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM twitch_clips_social_media WHERE id = $1 AND preview_status = 'rendering' AND preview_updated_at = $2::text::timestamptz)",
        )
        .bind(job.clip_db_id)
        .bind(&job.claimed_at)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(false);
        if !current {
            return;
        }
        let input = match self.ensure_local_file(job).await {
            Ok(path) => path,
            Err(e) => {
                let _ = finish_error(
                    &self.pool,
                    job.clip_db_id,
                    &format!("download: {e}"),
                    Some(&job.claimed_at),
                )
                .await;
                return;
            }
        };
        let output = format!(
            "{}/{}_preview_manual_v1.mp4",
            self.clips_dir, job.clip_db_id
        );
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
                if let Err(e) =
                    finish_ready(&self.pool, job.clip_db_id, &output, Some(&job.claimed_at)).await
                {
                    tracing::warn!(%e, clip_db_id = job.clip_db_id, "Vorschau: Ready-Status nicht gespeichert");
                }
            }
            Err(e) => {
                let _ = finish_error(
                    &self.pool,
                    job.clip_db_id,
                    &format!("render: {e}"),
                    Some(&job.claimed_at),
                )
                .await;
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
        sqlx::query(
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_url TEXT, streamer_login TEXT, local_file_path TEXT, preview_path TEXT, preview_status TEXT, preview_error TEXT, preview_updated_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[test]
    fn alter_release_pfad_zeigt_auf_den_aktuellen_clips_mount() {
        let old = "/opt/deadlock/twitch/releases/obsolete/data/clips/42_preview_manual_v1.mp4";
        assert_eq!(
            resolve_preview_path(42, old.to_string()),
            "data/clips/42_preview_manual_v1.mp4"
        );
        assert_eq!(resolve_preview_path(43, old.to_string()), old);
    }

    async fn preview_file(dir: &Path, id: i64) -> String {
        let path = dir.join(format!("{id}_preview_manual_v1.mp4"));
        let output = tokio::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=size=16x16:rate=1",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-y",
            ])
            .arg(&path)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        path.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn vorschau_prueft_echte_artefakte() {
        let dir = tempfile::tempdir().unwrap();
        let path = preview_file(dir.path(), 42).await;
        assert_eq!(
            valid_preview_path(42, Some(&path)).await,
            Some(path.clone())
        );
        assert!(valid_preview_path(43, Some(&path)).await.is_none());
        tokio::fs::write(&path, b"broken").await.unwrap();
        assert!(valid_preview_path(42, Some(&path)).await.is_none());
        tokio::fs::write(&path, b"").await.unwrap();
        assert!(valid_preview_path(42, Some(&path)).await.is_none());
        tokio::fs::remove_file(&path).await.unwrap();
        assert!(valid_preview_path(42, Some(&path)).await.is_none());
    }

    #[tokio::test]
    async fn vorschau_zustandsmaschine() {
        let Some(pool) = make_pool("t_sm_preview_state").await else {
            return;
        };
        let id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_url, streamer_login) VALUES ('https://clips.twitch.tv/x', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();
        request_preview(&pool, id).await.unwrap();
        let first: String = sqlx::query_scalar(
            "SELECT preview_updated_at::text FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let (a, b) = tokio::join!(request_preview(&pool, id), request_preview(&pool, id));
        a.unwrap();
        b.unwrap();
        let unchanged: String = sqlx::query_scalar(
            "SELECT preview_updated_at::text FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(first, unchanged);
        assert_eq!(
            get_preview(&pool, id).await.unwrap().status.as_deref(),
            Some(PREVIEW_PENDING)
        );
        let jobs = claim_pending(&pool, 5).await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].clip_db_id, id);
        request_preview(&pool, id).await.unwrap();
        assert_eq!(
            get_preview(&pool, id).await.unwrap().status.as_deref(),
            Some(PREVIEW_RENDERING)
        );
        assert!(claim_pending(&pool, 5).await.is_empty());
        let legacy_path = format!("/clips/{id}_preview.mp4");
        finish_ready(&pool, id, &legacy_path, Some(&jobs[0].claimed_at))
            .await
            .unwrap();
        let stale = get_preview(&pool, id).await.unwrap();
        assert!(stale.status.is_none());
        assert!(stale.path.is_none());
        let stored: String =
            sqlx::query_scalar("SELECT preview_path FROM twitch_clips_social_media WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, legacy_path);
        request_preview(&pool, id).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = preview_file(dir.path(), id).await;
        finish_ready(&pool, id, &path, Some(&jobs[0].claimed_at))
            .await
            .unwrap();
        assert_eq!(
            get_preview(&pool, id).await.unwrap().status.as_deref(),
            Some(PREVIEW_PENDING)
        );
        let current = claim_pending(&pool, 1).await;
        finish_ready(&pool, id, &path, Some(&current[0].claimed_at))
            .await
            .unwrap();
        request_preview(&pool, id).await.unwrap();
        let state = get_preview(&pool, id).await.unwrap();
        assert_eq!(state.status.as_deref(), Some(PREVIEW_READY));
        assert_eq!(state.path.as_deref(), Some(path.as_str()));
        assert!(claim_pending(&pool, 1).await.is_empty());
        tokio::fs::remove_file(&path).await.unwrap();
        assert!(get_preview(&pool, id).await.unwrap().status.is_none());
        request_preview(&pool, id).await.unwrap();
        let current = claim_pending(&pool, 1).await;
        finish_error(&pool, id, "boom", Some(&current[0].claimed_at))
            .await
            .unwrap();
        let state = get_preview(&pool, id).await.unwrap();
        assert_eq!(state.status.as_deref(), Some(PREVIEW_ERROR));
        assert_eq!(state.error.as_deref(), Some("boom"));
        request_preview(&pool, id).await.unwrap();
        assert_eq!(
            get_preview(&pool, id).await.unwrap().status.as_deref(),
            Some(PREVIEW_PENDING)
        );
        sqlx::query("UPDATE twitch_clips_social_media SET preview_status = 'rendering', preview_updated_at = NULL WHERE id = $1").bind(id).execute(&pool).await.unwrap();
        assert_eq!(claim_pending(&pool, 1).await.len(), 1);
        sqlx::query("UPDATE twitch_clips_social_media SET preview_updated_at = '2000-01-01'::timestamptz WHERE id = $1").bind(id).execute(&pool).await.unwrap();
        let mut lock = pool.begin().await.unwrap();
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtext('tb-social-media-preview'), hashtext($1))",
        )
        .bind(id.to_string())
        .execute(&mut *lock)
        .await
        .unwrap();
        assert!(claim_pending(&pool, 1).await.is_empty());
        lock.commit().await.unwrap();
        assert_eq!(claim_pending(&pool, 1).await.len(), 1);
    }
}
