use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

pub const DEFAULT_CLIPS_DIR: &str = "data/clips";
const INTERVAL_SECS: u64 = 120;
const INITIAL_DELAY_SECS: u64 = 30;
const BATCH_SIZE: i64 = 3;

#[derive(Debug, thiserror::Error)]
pub enum PrepError {
    #[error("download failed: {0}")]
    Download(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[async_trait]
pub trait ClipDownloader: Send + Sync {
    async fn download(&self, clip_url: &str, dest: &Path) -> Result<(), String>;
}

pub struct YtDlpDownloader {
    yt_dlp_path: String,
}

impl YtDlpDownloader {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            yt_dlp_path: path.into(),
        }
    }
}

#[async_trait]
impl ClipDownloader for YtDlpDownloader {
    async fn download(&self, clip_url: &str, dest: &Path) -> Result<(), String> {
        let output = tokio::process::Command::new(&self.yt_dlp_path)
            .args([
                "-f",
                "best",
                "-o",
                &dest.to_string_lossy(),
                clip_url,
            ])
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }
}

/// Laedt einen Clip atomar nach `dest_path`: erst in eine eindeutige Temp-Datei
/// im selben Ordner, dann `rename` auf das Ziel. So schreiben zwei nebenlaeufige
/// Downloads (Prep- und Vorschau-Worker teilen `data/clips`) nie in dieselbe
/// halbfertige Datei; das Ziel ist immer eine vollstaendige Datei.
pub async fn download_atomic(
    downloader: &dyn ClipDownloader,
    clip_url: &str,
    dest_path: &str,
) -> Result<(), String> {
    if Path::new(dest_path).exists() {
        return Ok(());
    }
    if let Some(parent) = Path::new(dest_path).parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    let tmp = format!("{dest_path}.dl-{}.part", tb_crypto::random_hex_token(8));
    downloader.download(clip_url, Path::new(&tmp)).await?;
    if !Path::new(&tmp).exists() {
        return Err(format!("Downloaded file not found: {tmp}"));
    }
    if let Err(e) = tokio::fs::rename(&tmp, dest_path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.to_string());
    }
    Ok(())
}

/// Traegt den lokalen Pfad in `twitch_clips_social_media` ein und loescht einen
/// etwaigen frueheren Download-Fehler. Damit greift der Enrichment-Selektor und
/// die Retention raeumt die Datei spaeter mit (INV-07).
pub async fn register_local_file(
    pool: &PgPool,
    clip_db_id: i64,
    path: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE twitch_clips_social_media \
            SET local_file_path = $1, downloaded_at = $2::text::timestamptz, download_failed_at = NULL \
          WHERE id = $3",
        path,
        Utc::now().to_rfc3339(),
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_download_failed(pool: &PgPool, clip_db_id: i64) {
    if let Err(error) = sqlx::query!(
        "UPDATE twitch_clips_social_media SET download_failed_at = NOW() WHERE id = $1",
        clip_db_id
    )
    .execute(pool)
    .await
    {
        tracing::warn!(%error, clip_db_id, "Clip-Prep: Download-Fehler konnte nicht vermerkt werden");
    }
}

pub struct ClipPrepWorker {
    pool: PgPool,
    downloader: Arc<dyn ClipDownloader>,
    clips_dir: String,
    batch_size: i64,
    interval: Duration,
}

impl ClipPrepWorker {
    pub fn new(pool: PgPool, yt_dlp_path: impl Into<String>) -> Self {
        Self {
            pool,
            downloader: Arc::new(YtDlpDownloader::new(yt_dlp_path)),
            clips_dir: DEFAULT_CLIPS_DIR.to_string(),
            batch_size: BATCH_SIZE,
            interval: Duration::from_secs(INTERVAL_SECS),
        }
    }

    pub fn with_downloader(mut self, downloader: Arc<dyn ClipDownloader>) -> Self {
        self.downloader = downloader;
        self
    }

    pub fn with_clips_dir(mut self, dir: impl Into<String>) -> Self {
        self.clips_dir = dir.into();
        self
    }

    async fn iter_clips_needing_download(&self, limit: i64) -> Vec<(i64, String)> {
        let rows = sqlx::query!(
            "SELECT c.id AS \"id!\", c.clip_url AS \"clip_url!\" \
               FROM twitch_clips_social_media c \
               JOIN social_media_category k ON k.category_key = c.category_key \
              WHERE c.discarded_at IS NULL \
                AND k.enrichment_enabled \
                AND COALESCE(c.upload_local_path, c.local_file_path) IS NULL \
                AND c.clip_url IS NOT NULL AND c.clip_url <> '' \
                AND (c.download_failed_at IS NULL OR c.download_failed_at < NOW() - INTERVAL '6 hours') \
              ORDER BY c.created_at DESC LIMIT $1",
            limit.max(1)
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_else(|error| {
            tracing::error!(%error, "Clip-Prep: offene Downloads konnten nicht gelesen werden");
            Vec::new()
        });
        rows.into_iter().map(|r| (r.id, r.clip_url)).collect()
    }

    async fn download_one(&self, clip_db_id: i64, clip_url: &str) -> Result<(), PrepError> {
        let output_path = format!("{}/{}.mp4", self.clips_dir, clip_db_id);
        download_atomic(self.downloader.as_ref(), clip_url, &output_path)
            .await
            .map_err(PrepError::Download)?;
        register_local_file(&self.pool, clip_db_id, &output_path).await?;
        Ok(())
    }

    pub async fn prepare_once(&self) -> usize {
        let clips = self.iter_clips_needing_download(self.batch_size).await;
        let mut geladen = 0usize;
        for (clip_db_id, clip_url) in clips {
            match self.download_one(clip_db_id, &clip_url).await {
                Ok(()) => geladen += 1,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        clip_db_id,
                        "Clip-Prep: Vorab-Download fehlgeschlagen"
                    );
                    // Fehlschlag festhalten, damit ein dauerhaft untauglicher Clip
                    // nicht alle 120s erneut gezogen wird und den Batch blockiert.
                    mark_download_failed(&self.pool, clip_db_id).await;
                }
            }
        }
        geladen
    }

    pub async fn run(&self) {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            self.prepare_once().await;
            tokio::time::sleep(self.interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrichment::iter_pending_enrichments;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    struct FakeDownloader;
    #[async_trait]
    impl ClipDownloader for FakeDownloader {
        async fn download(&self, _clip_url: &str, dest: &Path) -> Result<(), String> {
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
            }
            tokio::fs::write(dest, b"fake-mp4").await.map_err(|e| e.to_string())?;
            Ok(())
        }
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
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
            "CREATE TABLE social_media_category (category_key TEXT PRIMARY KEY, display_name TEXT NOT NULL, twitch_game_id TEXT, match_game_names JSONB NOT NULL DEFAULT '[]'::jsonb, enrichment_enabled BOOLEAN NOT NULL DEFAULT FALSE, sort_order INTEGER NOT NULL DEFAULT 0)",
            "INSERT INTO social_media_category (category_key, display_name, enrichment_enabled, sort_order) VALUES ('deadlock', 'Deadlock', TRUE, 0), ('other', 'Andere Spiele', FALSE, 1)",
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT, clip_url TEXT, streamer_login TEXT, game_id TEXT, upload_local_path TEXT, local_file_path TEXT, downloaded_at TIMESTAMPTZ, download_failed_at TIMESTAMPTZ, discarded_at TIMESTAMPTZ, category_key TEXT NOT NULL DEFAULT 'deadlock' REFERENCES social_media_category (category_key), created_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, status TEXT DEFAULT 'pending')",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn prep_laedt_deadlock_clip_und_macht_ihn_enrichbar() {
        let Some(pool) = make_pool("t_sm_prep_download").await else {
            return;
        };
        // Deadlock-Clip ohne lokale Datei, mit URL.
        let id: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, game_id, category_key) VALUES ('c1', 'https://clips.twitch.tv/x', 'earlysalty', '2132205352', 'deadlock') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // other-Clip ohne Datei: darf NICHT geladen werden (enrichment_enabled=false).
        let other: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, category_key) VALUES ('c2', 'https://clips.twitch.tv/y', 'earlysalty', 'other') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Vor dem Prep-Lauf: kein Kandidat fuer Enrichment (keine lokale Datei).
        assert!(iter_pending_enrichments(&pool, 10).await.is_empty());

        let tmp = std::env::temp_dir().join(format!("prep_test_{}", std::process::id()));
        let worker = ClipPrepWorker::new(pool.clone(), "yt-dlp")
            .with_downloader(Arc::new(FakeDownloader))
            .with_clips_dir(tmp.to_string_lossy().into_owned());
        let geladen = worker.prepare_once().await;
        assert_eq!(geladen, 1, "genau der Deadlock-Clip wird geladen");

        // local_file_path ist gesetzt, downloaded_at gefuellt.
        let path: Option<String> = sqlx::query_scalar(
            "SELECT local_file_path FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(path.is_some(), "Deadlock-Clip hat local_file_path");
        assert!(Path::new(path.as_deref().unwrap()).exists(), "Datei liegt auf der Platte");

        // other bleibt ohne Datei.
        let other_path: Option<String> = sqlx::query_scalar(
            "SELECT local_file_path FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(other)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(other_path.is_none(), "other-Clip wird nicht geladen");

        // Jetzt selektiert iter_pending_enrichments genau den Deadlock-Clip.
        let pending = iter_pending_enrichments(&pool, 10).await;
        assert_eq!(pending, vec![id]);

        // Zweiter Lauf laedt nichts erneut (Pfad gesetzt).
        assert_eq!(worker.prepare_once().await, 0);

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
