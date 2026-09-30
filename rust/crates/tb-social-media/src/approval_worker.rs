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

use crate::approval::{
    auto_approve_if_allowed_checked, ensure_queued_uploads, iter_approved_clips_pending_queue,
    iter_auto_approval_retry_candidates, iter_clips_ohne_enrichment,
    mark_clip_awaiting_approval_checked, vermerke_nachreih_versuch_checked, ApprovalError,
};

const INTERVAL_SECS: u64 = 60;
const INITIAL_DELAY_SECS: u64 = 20;
const BATCH_SIZE: i64 = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApprovalWorkerRunStats {
    pub awaiting_marked: usize,
    pub auto_approved: usize,
    pub uploads_queued: usize,
}

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
    pub async fn run_once(&self) -> Result<ApprovalWorkerRunStats, ApprovalError> {
        let mut stats = ApprovalWorkerRunStats::default();
        // Clips ohne Enrichment (alles ausser Deadlock) treten hier in den
        // Workflow ein; angereicherte Clips bringt die Enrichment-Pipeline mit.
        for clip_db_id in iter_clips_ohne_enrichment(&self.pool, self.batch_size).await? {
            mark_clip_awaiting_approval_checked(&self.pool, clip_db_id).await?;
            stats.awaiting_marked += 1;
            match auto_approve_if_allowed_checked(&self.pool, clip_db_id).await {
                Ok(platforms) if !platforms.is_empty() => stats.auto_approved += 1,
                Ok(_) | Err(ApprovalError::PreviewNotReady) => {}
                Err(error) => return Err(error),
            }
        }
        for clip_db_id in iter_auto_approval_retry_candidates(&self.pool, self.batch_size).await? {
            if !auto_approve_if_allowed_checked(&self.pool, clip_db_id)
                .await?
                .is_empty()
            {
                stats.auto_approved += 1;
            }
        }
        for clip_db_id in iter_approved_clips_pending_queue(&self.pool, self.batch_size).await? {
            stats.uploads_queued += ensure_queued_uploads(&self.pool, clip_db_id).await?.len();
            // Versucht ist versucht: der Stempel schiebt den Clip ans Ende der
            // Rotation. Ohne ihn stehen Freigaben, die nie eine Queue-Zeile
            // bekommen koennen (Plattform schon hochgeladen, Planungshorizont
            // voll), fuer immer auf den vorderen Plaetzen und verdraengen jede
            // neue Freigabe aus dem Zehnerfenster. Auch nach einem
            // erfolgreichen Lauf gesetzt: ein vollstaendig eingereihter Clip
            // faellt ohnehin aus dem Fenster, ein halb eingereihter gehoert
            // hinter die, die noch nicht dran waren.
            vermerke_nachreih_versuch_checked(&self.pool, clip_db_id).await?;
        }
        Ok(stats)
    }

    /// Hintergrund-Loop (20s Initial-Delay + 60s-Intervall). Noch nicht in
    /// tb-bot gespawnt (Wiring = Cutover-Slice).
    pub async fn run(&self) {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            match self.run_once().await {
                Ok(stats) => tracing::debug!(
                    awaiting_marked = stats.awaiting_marked,
                    auto_approved = stats.auto_approved,
                    uploads_queued = stats.uploads_queued,
                    "Social-Media-Approval-Worker-Durchlauf abgeschlossen"
                ),
                Err(error) => tracing::error!(
                    %error,
                    code = "approval_worker_run_failed",
                    "Social-Media-Approval-Worker-Durchlauf fehlgeschlagen"
                ),
            }
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
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, status TEXT DEFAULT 'pending', streamer_login TEXT NOT NULL DEFAULT 'nani', category_key TEXT NOT NULL DEFAULT 'misc', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE social_media_category (category_key TEXT PRIMARY KEY, enrichment_enabled BOOLEAN NOT NULL DEFAULT FALSE)",
            "INSERT INTO social_media_category (category_key) VALUES ('misc')",
            "CREATE TABLE social_media_category_settings (category_key TEXT NOT NULL, streamer_login TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT FALSE, PRIMARY KEY (category_key, streamer_login))",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ, approved_render_fingerprint TEXT)",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY, state TEXT NOT NULL DEFAULT 'pending', render_fingerprint TEXT, render_path TEXT, completed_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER NOT NULL DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, approval_mode TEXT NOT NULL DEFAULT 'manual', timezone TEXT NOT NULL DEFAULT 'Europe/Berlin', release_mode TEXT NOT NULL DEFAULT 'prepare_only')",
            "CREATE TABLE social_media_platform_schedule (streamer_login TEXT NOT NULL, platform TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT FALSE, posts_per_week INTEGER NOT NULL DEFAULT 4, max_posts_per_day INTEGER NOT NULL DEFAULT 1, post_times JSONB NOT NULL DEFAULT '[\"18:00\"]'::jsonb, PRIMARY KEY (streamer_login, platform))",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn seed_ready_preview(pool: &PgPool, clip_db_id: i32) {
        sqlx::query(
            "INSERT INTO social_media_clip_preparation \
             (clip_db_id, state, render_fingerprint, render_path) \
             VALUES ($1, 'preview_ready', 'render-v1', $2)",
        )
        .bind(clip_db_id)
        .bind(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/approval_worker.rs"
        ))
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn queued_nur_approved_clips() {
        let Some(pool) = make_pool("t_sm_approval_worker").await else {
            return;
        };
        // Clip A: approved für YouTube, mit Enrichment-Titel.
        let a: i32 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        seed_ready_preview(&pool, a).await;
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, approved_render_fingerprint, decided_at) VALUES ($1, 'approved', '[\"youtube\"]'::jsonb, 'render-v1', NOW())").bind(a).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, title_youtube) VALUES ($1, 'YT-Titel')").bind(a).execute(&pool).await.unwrap();
        // Clip B: nur awaiting → wird nicht eingereiht.
        let b: i32 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state) VALUES ($1, 'awaiting_approval')").bind(b).execute(&pool).await.unwrap();

        ApprovalWorker::new(pool.clone()).run_once().await.unwrap();

        // A: genau eine YouTube-Queue-Zeile mit Enrichment-Titel.
        let (platform, title): (String, Option<String>) = sqlx::query_as(
            "SELECT platform, title FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(a)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(platform, "youtube");
        assert_eq!(title.as_deref(), Some("YT-Titel"));
        // B: keine Queue-Zeile.
        let n_b: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(b)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n_b, 0);

        // Idempotent: zweiter Lauf legt keine Duplikate an.
        ApprovalWorker::new(pool.clone()).run_once().await.unwrap();
        let n_a: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(a)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n_a, 1);
    }

    #[tokio::test]
    async fn kandidaten_db_fehler_wird_als_run_fehler_sichtbar() {
        let Some(pool) = make_pool("t_sm_approval_worker_query_error").await else {
            return;
        };
        sqlx::query("DROP TABLE social_media_clip_approval")
            .execute(&pool)
            .await
            .unwrap();

        assert!(ApprovalWorker::new(pool).run_once().await.is_err());
    }

    #[tokio::test]
    async fn gespeichertes_enrichment_ohne_approval_wird_dauerhaft_nachgeholt() {
        let Some(pool) = make_pool("t_sm_approval_worker_enrichment_recovery").await else {
            return;
        };
        sqlx::query(
            "UPDATE social_media_category SET enrichment_enabled = TRUE \
             WHERE category_key = 'misc'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let clip: i32 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        seed_ready_preview(&pool, clip).await;
        // `save_llm_output` wurde bereits committed, danach sind sowohl das
        // Status-Update als auch der direkte Approval-Schreibpfad ausgefallen.
        // Die gespeicherte Provider-Markierung ist der dauerhafte
        // Wiederaufnahmepunkt; ein reines `status = 'done'` wäre hier zu eng.
        sqlx::query(
            "INSERT INTO social_media_clip_enrichment (clip_db_id, status, llm_provider) \
             VALUES ($1, 'processing', 'test-provider')",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();

        let stats = ApprovalWorker::new(pool.clone()).run_once().await.unwrap();
        assert_eq!(stats.awaiting_marked, 1);
        let state: String = sqlx::query_scalar(
            "SELECT state FROM social_media_clip_approval WHERE clip_db_id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, "awaiting_approval");
        assert_eq!(
            iter_auto_approval_retry_candidates(&pool, 10)
                .await
                .unwrap(),
            vec![clip],
            "auch ein nachgelagerter Auto-Approval-Fehler muss ohne done-Status retrybar bleiben"
        );
    }

    /// Legt einen Clip an, der fuer `plattformen` freigegeben ist. `vor_minuten`
    /// steuert das Alter der Entscheidung, `schon_hochgeladen` setzt das
    /// Upload-Flag der Plattform youtube.
    async fn seed_freigabe(
        pool: &PgPool,
        plattformen: &str,
        vor_minuten: i32,
        schon_hochgeladen: bool,
    ) -> i32 {
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (uploaded_youtube) VALUES ($1) RETURNING id",
        )
        .bind(schon_hochgeladen)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_approval \
                 (clip_db_id, state, approved_platforms, approved_render_fingerprint, decided_at) \
             VALUES ($1, 'approved', $2::text::jsonb, 'render-v1', \
                     now() - make_interval(mins => $3))",
        )
        .bind(clip)
        .bind(plattformen)
        .bind(vor_minuten)
        .execute(pool)
        .await
        .unwrap();
        seed_ready_preview(pool, clip).await;
        clip
    }

    /// Die Blockade: zehn Freigaben, die nie eine Queue-Zeile bekommen koennen,
    /// duerfen keine frische Freigabe aussperren.
    ///
    /// Der Ablauf, der das in Produktion ausloest: ein Clip wird fuer youtube
    /// und tiktok freigegeben, danach setzt jemand youtube ueber "als
    /// hochgeladen markieren". `ensure_queued_uploads` springt fuer youtube bei
    /// `upload_already_exists` raus, es entsteht nie eine youtube-Zeile, und
    /// `iter_approved_clips_pending_queue` fuehrt den Clip deshalb weiter als
    /// offen. Nach reinem `decided_at ASC` stand so ein Dauergast fuer immer
    /// vorn: zehn davon fuellten das Zehnerfenster, und keine neue Freigabe
    /// wurde je wieder eingereiht, ohne Log, ohne Fehler, nur per Hand in der
    /// Datenbank aufloesbar.
    ///
    /// Mit dem Stempel `letzter_nachreih_versuch` wird aus der Rangliste eine
    /// Rotation: der erste Lauf stempelt die zehn Dauergaeste, danach steht die
    /// frische Freigabe mit ihrem `NULL`-Stempel vor ihnen und kommt dran.
    #[tokio::test]
    async fn dauergaeste_sperren_die_frische_freigabe_nicht_aus() {
        let Some(pool) = make_pool("t_sm_approval_dauergast").await else {
            return;
        };

        // Zehn Dauergaeste: freigegeben fuer youtube, youtube steht aber schon
        // auf "hochgeladen". Es kann nie eine Queue-Zeile entstehen.
        let mut dauergaeste = Vec::new();
        for minuten in 0..10 {
            dauergaeste.push(seed_freigabe(&pool, "[\"youtube\"]", 600 - minuten, true).await);
        }
        // Die frische Freigabe: juengste Entscheidung, wartet auf ihre
        // Instagram-Zeile.
        let frisch = seed_freigabe(&pool, "[\"instagram\"]", 0, false).await;

        // Erster Lauf: die zehn Dauergaeste fuellen das Fenster und richten
        // nichts aus. Wichtig ist nur, dass sie danach gestempelt sind.
        ApprovalWorker::new(pool.clone()).run_once().await.unwrap();
        let dauergast_zeilen: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = ANY($1)",
        )
        .bind(&dauergaeste[..])
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            dauergast_zeilen, 0,
            "ein Dauergast kann keine Queue-Zeile bekommen, sonst prueft der Test den falschen Fall"
        );

        // Zweiter Lauf: jetzt ist die frische Freigabe an der Reihe.
        ApprovalWorker::new(pool.clone()).run_once().await.unwrap();
        let (anzahl, platform): (i64, Option<String>) = sqlx::query_as(
            "SELECT COUNT(*), MIN(platform) FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(frisch)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            anzahl, 1,
            "die frische Freigabe muss eingereiht werden, auch wenn zehn Dauergaeste im Fenster stehen"
        );
        assert_eq!(platform.as_deref(), Some("instagram"));

        // Und die Dauergaeste stehen weiter offen, ohne jemanden zu blockieren.
        let noch_offen: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = ANY($1)",
        )
        .bind(&dauergaeste[..])
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(noch_offen, 0);
    }
}
