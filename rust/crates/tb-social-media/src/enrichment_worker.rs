//! Enrichment-Worker (Port von `bot/social_media/enrichment_worker.py`).
//!
//! Reichert pending Clips im Hintergrund per Vokabel-Korrektur + LLM an. Holt
//! batchweise die offenen Clips und schickt sie durch die
//! [`ClipEnrichmentPipeline`]. Der Transcriber bleibt injizierbar, wird aber per
//! Grillme-Entscheidung (Block 15) NICHT gesetzt — Transkription ist deaktiviert
//! (kein OpenAI), die Stage wird übersprungen. LLM wird injiziert.
//! An/Aus 1:1: dauerhaft an, Intervall 90s, Batch 3.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use crate::enrich_pipeline::{ClipEnrichmentPipeline, EnrichmentLlm, Transcriber};
use crate::enrichment::iter_pending_enrichments;

const INTERVAL_SECS: u64 = 90;
const INITIAL_DELAY_SECS: u64 = 45;
const BATCH_SIZE: i64 = 3;

/// Hintergrund-Worker für Clip-Enrichment.
pub struct EnrichmentWorker {
    pool: PgPool,
    pipeline: ClipEnrichmentPipeline,
    transcriber: Option<Arc<dyn Transcriber>>,
    llm: Arc<dyn EnrichmentLlm>,
    batch_size: i64,
    interval: Duration,
}

impl EnrichmentWorker {
    pub fn new(pool: PgPool, llm: Arc<dyn EnrichmentLlm>) -> Self {
        Self {
            pipeline: ClipEnrichmentPipeline::new(pool.clone()),
            pool,
            transcriber: None,
            llm,
            batch_size: BATCH_SIZE,
            interval: Duration::from_secs(INTERVAL_SECS),
        }
    }

    /// Setzt einen Transcriber. Aktuell ungenutzt (Transkription per
    /// Grillme-Entscheidung deaktiviert), bleibt als Infra-Anker für einen
    /// späteren nicht-OpenAI-Transkriptionsweg.
    pub fn with_transcriber(mut self, transcriber: Arc<dyn Transcriber>) -> Self {
        self.transcriber = Some(transcriber);
        self
    }

    /// Ein Durchlauf (Python `_process_pending`).
    pub async fn run_once(&self) {
        let pending = iter_pending_enrichments(&self.pool, self.batch_size).await;
        if pending.is_empty() {
            return;
        }
        for clip_db_id in pending {
            // Pipeline-Fehler sind best-effort geloggt; ein Clip bricht den
            // Batch nicht ab (mirror Pythons try/except je Clip).
            if let Err(error) = self
                .pipeline
                .run(
                    clip_db_id,
                    self.transcriber.as_deref(),
                    self.llm.as_ref(),
                    false,
                )
                .await
            {
                tracing::warn!(
                    %error,
                    clip_db_id,
                    "Clip-Enrichment-Worker: Pipeline-Durchlauf fehlgeschlagen"
                );
            }
        }
    }

    /// Hintergrund-Loop (45s Initial-Delay + 90s-Intervall). Noch nicht in
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
    use crate::enrich_pipeline::EnrichmentLlm;
    use crate::llm::{LlmError, LlmRequest, LlmResponse};
    use async_trait::async_trait;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    // LLM-Mock, der immer scheitert → Pipeline endet bei skipped_no_key
    // (Transkription übersprungen, weil kein Transcriber).
    struct FailingLlm;
    #[async_trait]
    impl EnrichmentLlm for FailingLlm {
        async fn generate(&self, _: &LlmRequest) -> Result<LlmResponse, LlmError> {
            Err(LlmError::ProviderUnavailable("test".to_string()))
        }
    }

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
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT, streamer_login TEXT, clip_title TEXT, duration_seconds DOUBLE PRECISION, game_name TEXT, upload_local_path TEXT, local_file_path TEXT, discarded_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', aliases JSONB NOT NULL DEFAULT '[]'::jsonb, weight INTEGER NOT NULL DEFAULT 1, updated_at TIMESTAMPTZ DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn enrichment_status(pool: &PgPool, clip: i32) -> Option<String> {
        sqlx::query_scalar("SELECT status FROM social_media_clip_enrichment WHERE clip_db_id = $1")
            .bind(clip)
            .fetch_optional(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn worker_verarbeitet_nur_pending() {
        let Some(pool) = make_pool("t_sm_enrichment_worker").await else {
            return;
        };
        // A: ohne Enrichment, mit Datei → Kandidat.
        let a: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, upload_local_path) VALUES ('a', 'nani', '/a.mp4') RETURNING id").fetch_one(&pool).await.unwrap();
        // B: Enrichment-Status failed → Kandidat.
        let b: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, local_file_path) VALUES ('b', 'nani', '/b.mp4') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_enrichment (clip_db_id, status) VALUES ($1, 'failed')",
        )
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
        // C: bereits done → kein Kandidat.
        let c: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, upload_local_path) VALUES ('c', 'nani', '/c.mp4') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_enrichment (clip_db_id, status) VALUES ($1, 'done')",
        )
        .bind(c)
        .execute(&pool)
        .await
        .unwrap();
        // D: ohne Datei → kein Kandidat.
        let d: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('d', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // iter_pending liefert genau A + B.
        let pending = iter_pending_enrichments(&pool, 10).await;
        assert!(pending.contains(&a) && pending.contains(&b));
        assert!(!pending.contains(&c) && !pending.contains(&d));

        // Worker ohne Transcriber + scheiterndes LLM → A/B enden bei skipped_no_key.
        let worker = EnrichmentWorker::new(pool.clone(), Arc::new(FailingLlm));
        worker.run_once().await;
        assert_eq!(
            enrichment_status(&pool, a).await.as_deref(),
            Some("skipped_no_key")
        );
        assert_eq!(
            enrichment_status(&pool, b).await.as_deref(),
            Some("skipped_no_key")
        );
        assert_eq!(enrichment_status(&pool, c).await.as_deref(), Some("done")); // unangetastet
    }
}
