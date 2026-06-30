//! Retention-Worker (Port von `bot/social_media/retention_worker.py`).
//!
//! Löscht abgelaufene Social-Media-Clips — aber erst, wenn sie entweder
//! verworfen (`discarded_at`) ODER auf allen aktiven Plattformen veröffentlicht
//! sind. Pro Treffer wird die lokale Datei entfernt und die Clip-Zeile gelöscht.
//! An/Aus 1:1: in Python dauerhaft an (kein Gate), Intervall 30min.

use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;

use crate::retention::{
    delete_clips_by_ids, is_clip_published_on_all_active_platforms,
    iter_expired_clips_for_retention,
};

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

            // Pythons `upload_local_path or local_file_path or ""` (Leerstring = falsy).
            let file_path = clip
                .upload_local_path
                .as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| clip.local_file_path.as_deref().filter(|s| !s.is_empty()))
                .unwrap_or("")
                .trim()
                .to_string();

            if !file_path.is_empty() {
                match tokio::fs::remove_file(&file_path).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {} // missing_ok
                    Err(_) => continue, // Datei nicht löschbar → Clip-Zeile behalten
                }
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
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
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
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT, streamer_login TEXT, source_kind TEXT, upload_local_path TEXT, local_file_path TEXT, status TEXT DEFAULT 'pending', retention_until TIMESTAMPTZ, discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)",
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
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok', 'nani')").execute(&pool).await.unwrap();

        // Clip A: abgelaufen + verworfen + reale Datei → wird gelöscht.
        let file_a = std::env::temp_dir().join("tb_retention_a.mp4");
        tokio::fs::write(&file_a, b"x").await.unwrap();
        let _a: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, upload_local_path, discarded_at, retention_until) VALUES ('a', 'nani', $1, NOW(), NOW() - INTERVAL '1 day') RETURNING id")
            .bind(file_a.to_string_lossy().into_owned()).fetch_one(&pool).await.unwrap();

        // Clip B: abgelaufen, NICHT verworfen, tiktok aktiv aber nicht hochgeladen → behalten.
        let b: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, retention_until) VALUES ('b', 'nani', NOW() - INTERVAL '1 day') RETURNING id").fetch_one(&pool).await.unwrap();

        // Clip C: abgelaufen, NICHT verworfen, tiktok hochgeladen → voll veröffentlicht → gelöscht.
        let _c: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, retention_until, uploaded_tiktok) VALUES ('c', 'nani', NOW() - INTERVAL '1 day', TRUE) RETURNING id").fetch_one(&pool).await.unwrap();

        // Clip D: in der Zukunft → gar kein Kandidat.
        let d: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, retention_until) VALUES ('d', 'nani', NOW() + INTERVAL '5 days') RETURNING id").fetch_one(&pool).await.unwrap();

        RetentionWorker::new(pool.clone()).run_once().await;

        let remaining: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM twitch_clips_social_media ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, vec![b as i64, d as i64]); // A + C gelöscht, B + D bleiben
        assert!(!file_a.exists()); // Datei von A entfernt
    }
}
