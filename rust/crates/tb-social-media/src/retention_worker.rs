//! Retention-Worker (Port von `bot/social_media/retention_worker.py`).
//!
//! Löscht abgelaufene Social-Media-Clips — aber erst, wenn sie entweder
//! verworfen (`discarded_at`) ODER auf allen aktiven Plattformen veröffentlicht
//! sind. Pro Treffer wird die lokale Datei entfernt und die Clip-Zeile gelöscht.
//! An/Aus 1:1: in Python dauerhaft an (kein Gate), Intervall 30min.

use std::collections::BTreeSet;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use crate::retention::{
    delete_clips_by_ids, is_clip_published_on_all_active_platforms,
    iter_expired_clips_for_retention, ExpiredClip,
};
use crate::upload_worker::vertical_output_path;

fn clip_file_paths(clip: &ExpiredClip) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for input in [
        clip.upload_local_path.as_deref(),
        clip.local_file_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|path| !path.is_empty())
    {
        paths.insert(input.to_string());
        for platform in crate::posting_plan::PLATFORMS {
            paths.insert(vertical_output_path(input, platform));
        }
    }
    if let Some(preview) = clip
        .preview_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        paths.insert(preview.to_string());
    }
    paths
}

async fn remove_clip_files(clip: &ExpiredClip) -> std::io::Result<()> {
    for path in clip_file_paths(clip) {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, clip_id = clip.id, %path, "Social-Media-Retention: Datei konnte nicht gelöscht werden");
                return Err(error);
            }
        }
    }
    Ok(())
}

const INTERVAL_SECS: u64 = 30 * 60;
const INITIAL_DELAY_SECS: u64 = 30;

/// Worker, der abgelaufene Clips aufräumt.
pub struct RetentionWorker {
    pool: PgPool,
    interval: Duration,
}

impl RetentionWorker {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            interval: Duration::from_secs(INTERVAL_SECS),
        }
    }

    /// Ein Durchlauf (Python `_cleanup_expired_clips`).
    pub async fn run_once(&self) {
        let now = Utc::now().to_rfc3339();
        let candidates = iter_expired_clips_for_retention(&self.pool, &now).await;
        let mut deleted_ids: Vec<i64> = Vec::new();

        for clip in candidates {
            // Noch nicht verworfen UND nicht voll veröffentlicht → noch behalten.
            if clip.discarded_at.is_none()
                && !is_clip_published_on_all_active_platforms(&self.pool, clip.id).await
            {
                continue;
            }

            if remove_clip_files(&clip).await.is_err() {
                continue;
            }
            deleted_ids.push(clip.id);
        }

        delete_clips_by_ids(&self.pool, &deleted_ids).await;
    }

    /// Hintergrund-Loop (30s Initial-Delay + 30min-Intervall). Noch nicht in
    /// tb-bot gespawnt (Wiring = Cutover-Slice).
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
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, streamer_login TEXT NOT NULL, twitch_user_id TEXT DEFAULT '42', source_kind TEXT NOT NULL DEFAULT 'twitch', upload_local_path TEXT, local_file_path TEXT, preview_path TEXT, status TEXT DEFAULT 'pending', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), retention_until TIMESTAMPTZ, discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, twitch_user_id TEXT, enabled INTEGER DEFAULT 1)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn cleanup_loescht_nur_fertige_clips() {
        let Some(pool) = make_pool("t_sm_retention_worker").await else {
            return;
        };
        // Aktive Plattform tiktok für 'nani'.
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login, twitch_user_id) VALUES ('tiktok', 'nani', '42')").execute(&pool).await.unwrap();

        // Clip A: abgelaufen + verworfen + reale Datei → wird gelöscht.
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("upload.mp4");
        let local_a = dir.path().join("local.mp4");
        let preview_a = dir.path().join("preview.mp4");
        let mut files = vec![file_a.clone(), local_a.clone(), preview_a.clone()];
        for input in [&file_a, &local_a] {
            for platform in crate::posting_plan::PLATFORMS {
                files.push(vertical_output_path(&input.to_string_lossy(), platform).into());
            }
        }
        for file in &files {
            tokio::fs::write(file, b"x").await.unwrap();
        }
        let _a: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, upload_local_path, local_file_path, preview_path, discarded_at, retention_until) VALUES ('a', 'https://clips.test/a', 'nani', $1, $2, $3, NOW(), NOW() - INTERVAL '1 day') RETURNING id")
            .bind(file_a.to_string_lossy().into_owned())
            .bind(local_a.to_string_lossy().into_owned())
            .bind(preview_a.to_string_lossy().into_owned())
            .fetch_one(&pool).await.unwrap();

        // Clip B: abgelaufen, NICHT verworfen, tiktok aktiv aber nicht hochgeladen → behalten.
        let b: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('b', 'https://clips.test/b', 'nani', NOW() - INTERVAL '1 day') RETURNING id").fetch_one(&pool).await.unwrap();

        // Clip C: abgelaufen, NICHT verworfen, tiktok hochgeladen → voll veröffentlicht → gelöscht.
        let _c: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until, uploaded_tiktok) VALUES ('c', 'https://clips.test/c', 'nani', NOW() - INTERVAL '1 day', TRUE) RETURNING id").fetch_one(&pool).await.unwrap();

        // Clip D: in der Zukunft → gar kein Kandidat.
        let d: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('d', 'https://clips.test/d', 'nani', NOW() + INTERVAL '5 days') RETURNING id").fetch_one(&pool).await.unwrap();

        RetentionWorker::new(pool.clone()).run_once().await;

        let remaining: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM twitch_clips_social_media ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, vec![b, d]); // A + C gelöscht, B + D bleiben
        assert!(files.iter().all(|file| !file.exists()));
    }

    #[tokio::test]
    async fn cleanup_behaelt_zeile_wenn_renderdatei_nicht_loeschbar() {
        let Some(pool) = make_pool("t_sm_retention_render_error").await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.mp4");
        let render = vertical_output_path(&input.to_string_lossy(), "youtube");
        tokio::fs::write(&input, b"source").await.unwrap();
        tokio::fs::create_dir(&render).await.unwrap();
        let id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, local_file_path, discarded_at, retention_until) VALUES ('blocked', 'https://clips.test/blocked', 'nani', $1, NOW(), NOW() - INTERVAL '1 day') RETURNING id")
            .bind(input.to_string_lossy().into_owned()).fetch_one(&pool).await.unwrap();
        RetentionWorker::new(pool.clone()).run_once().await;
        let remaining: Vec<i64> = sqlx::query_scalar("SELECT id FROM twitch_clips_social_media")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, vec![id]);
        tokio::fs::remove_dir(&render).await.unwrap();
        RetentionWorker::new(pool.clone()).run_once().await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_social_media")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
