//! Approval-Worker, Queue-Seite (Port von
//! `bot/social_media/approval_worker.py`, `_queue_approved_uploads`).
//!
//! Zieht freigegebene Clips in die Upload-Queue: holt batchweise die als
//! `approved` markierten Clips und reiht ihre noch nicht vorhandenen Uploads
//! ein. Die zweite Hälfte des Python-Workers (`_dispatch_pending_dms`, Versand
//! der Approval-DMs) ist **B10 (Discord-DMs, von Nani ausgeschlossen)** und
//! nicht portiert. An/Aus 1:1: dauerhaft an, Intervall 60s, Batch 10.

use std::time::Duration;

use sqlx::PgPool;

use crate::approval::{ensure_queued_uploads, iter_approved_clips_pending_queue};

const INTERVAL_SECS: u64 = 60;
const INITIAL_DELAY_SECS: u64 = 20;
const BATCH_SIZE: i64 = 10;

/// Worker, der freigegebene Uploads in die Queue zieht.
pub struct ApprovalWorker {
    pool: PgPool,
    batch_size: i64,
    interval: Duration,
}

impl ApprovalWorker {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            batch_size: BATCH_SIZE,
            interval: Duration::from_secs(INTERVAL_SECS),
        }
    }

    /// Ein Durchlauf (Python `_queue_approved_uploads`).
    pub async fn run_once(&self) {
        for clip_db_id in iter_approved_clips_pending_queue(&self.pool, self.batch_size).await {
            // best-effort je Clip (Python try/except, ein Fehler bricht den
            // Batch nicht ab).
            let _ = ensure_queued_uploads(&self.pool, clip_db_id).await;
        }
    }

    /// Hintergrund-Loop (20s Initial-Delay + 60s-Intervall). Noch nicht in
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
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, status TEXT DEFAULT 'pending', uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn queued_nur_approved_clips() {
        let Some(pool) = make_pool("t_sm_approval_worker").await else {
            return;
        };
        // Clip A: approved für tiktok, mit Enrichment-Titel.
        let a: i32 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, decided_at) VALUES ($1, 'approved', '[\"tiktok\"]'::jsonb, NOW())").bind(a).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, title_tiktok) VALUES ($1, 'TT-Titel')").bind(a).execute(&pool).await.unwrap();
        // Clip B: nur awaiting → wird nicht eingereiht.
        let b: i32 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state) VALUES ($1, 'awaiting_approval')").bind(b).execute(&pool).await.unwrap();

        ApprovalWorker::new(pool.clone()).run_once().await;

        // A: genau eine tiktok-Queue-Zeile mit Enrichment-Titel.
        let (platform, title): (String, Option<String>) = sqlx::query_as(
            "SELECT platform, title FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(a)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(platform, "tiktok");
        assert_eq!(title.as_deref(), Some("TT-Titel"));
        // B: keine Queue-Zeile.
        let n_b: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(b)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n_b, 0);

        // Idempotent: zweiter Lauf legt keine Duplikate an.
        ApprovalWorker::new(pool.clone()).run_once().await;
        let n_a: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(a)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n_a, 1);
    }
}
