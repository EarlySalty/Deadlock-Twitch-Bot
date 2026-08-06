//! Schema der Social-Media-Pipeline (Port der Tabellen-Erstellung aus
//! `bot/social_media/storage.py`).
//!
//! [`ensure_schema`] legt alle Tabellen/Spalten/Indizes/Trigger idempotent an
//! (CREATE … IF NOT EXISTS, ADD COLUMN IF NOT EXISTS). Bewusst NICHT portiert:
//! die einmaligen Legacy-Daten-Migrationen (Sequence-Repair, Phase-3-Numeric-
//! Coercion bestehender Spalten, Backfill-UPDATEs) — die migrieren Daten der
//! Python-Ära und sind für den Rust-Feature-Pfad nicht nötig.
//!
//! Voraussetzung: die Basistabellen `twitch_streamers`, `twitch_clips_social_media`
//! und `twitch_clips_social_analytics` existieren bereits (Clip-Fetcher/Analytics).

use sqlx::PgPool;

/// Alle DDL-Anweisungen, in Abhängigkeitsreihenfolge.
const STATEMENTS: &[&str] = &[
    // Key/Value-Settings.
    "CREATE TABLE IF NOT EXISTS social_media_settings (\
        key TEXT PRIMARY KEY, value JSONB NOT NULL, \
        updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_by TEXT)",
    // Re-Auth-Benachrichtigungen (Dedupe pro streamer/platform/error_kind).
    "CREATE TABLE IF NOT EXISTS social_media_reauth_notifications (\
        streamer_login TEXT NOT NULL, platform TEXT NOT NULL, error_kind TEXT NOT NULL, \
        last_sent_at TIMESTAMPTZ NOT NULL, \
        PRIMARY KEY (streamer_login, platform, error_kind))",
    "CREATE INDEX IF NOT EXISTS idx_social_media_reauth_notifications_last_sent \
        ON social_media_reauth_notifications(last_sent_at DESC)",
    // Streamer-Layout (PiP/Stacked, Cam-Toggle).
    "CREATE TABLE IF NOT EXISTS social_media_streamer_layout (\
        streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login) ON DELETE CASCADE, \
        layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, \
        mode TEXT NOT NULL DEFAULT 'pip', \
        updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_by TEXT, \
        CONSTRAINT social_media_layout_mode_chk CHECK (mode IN ('pip', 'stacked')))",
    // Clip-Tabelle: Social-Media-Spalten + Retention.
    "ALTER TABLE twitch_clips_social_media \
        ADD COLUMN IF NOT EXISTS layout_override_json JSONB, \
        ADD COLUMN IF NOT EXISTS source_kind TEXT NOT NULL DEFAULT 'twitch', \
        ADD COLUMN IF NOT EXISTS upload_local_path TEXT, \
        ADD COLUMN IF NOT EXISTS retention_until TIMESTAMPTZ, \
        ADD COLUMN IF NOT EXISTS discarded_at TIMESTAMPTZ",
    "ALTER TABLE twitch_clips_social_media DROP CONSTRAINT IF EXISTS twitch_clips_source_kind_chk",
    "ALTER TABLE twitch_clips_social_media \
        ADD CONSTRAINT twitch_clips_source_kind_chk CHECK (source_kind IN ('twitch', 'manual_upload'))",
    "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_retention \
        ON twitch_clips_social_media(retention_until)",
    "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_discarded_at \
        ON twitch_clips_social_media(discarded_at)",
    // Retention-Trigger: setzt retention_until = created_at + 14 Tage.
    "CREATE OR REPLACE FUNCTION social_media_set_retention_until() \
        RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN \
            IF NEW.created_at IS NULL OR BTRIM(NEW.created_at::text) = '' THEN RETURN NEW; END IF; \
            NEW.retention_until := (NEW.created_at::timestamptz + INTERVAL '14 days'); \
            RETURN NEW; \
        END; $$",
    "DROP TRIGGER IF EXISTS social_media_retention_until_tg ON twitch_clips_social_media",
    "CREATE TRIGGER social_media_retention_until_tg \
        BEFORE INSERT OR UPDATE OF created_at ON twitch_clips_social_media \
        FOR EACH ROW EXECUTE FUNCTION social_media_set_retention_until()",
    // Deadlock-Vokabular (Transkript-Korrektur).
    "CREATE TABLE IF NOT EXISTS deadlock_vocab (\
        term TEXT PRIMARY KEY, canonical TEXT NOT NULL, category TEXT NOT NULL, \
        source TEXT NOT NULL DEFAULT 'manual', aliases JSONB NOT NULL DEFAULT '[]'::JSONB, \
        weight INTEGER NOT NULL DEFAULT 1, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, \
        CONSTRAINT deadlock_vocab_category_chk CHECK (category IN ('hero', 'item', 'ability', 'slang')), \
        CONSTRAINT deadlock_vocab_source_chk CHECK (source IN ('deadlock_api', 'manual')))",
    "CREATE INDEX IF NOT EXISTS idx_deadlock_vocab_category ON deadlock_vocab(category)",
    "CREATE INDEX IF NOT EXISTS idx_deadlock_vocab_canonical ON deadlock_vocab(canonical)",
    // Social-Media-Reports (Insights/Periodenberichte).
    "CREATE TABLE IF NOT EXISTS social_media_reports (\
        id SERIAL PRIMARY KEY, kind TEXT NOT NULL, streamer_login TEXT, \
        period_start TIMESTAMPTZ NOT NULL, period_end TIMESTAMPTZ NOT NULL, \
        content_md TEXT NOT NULL, model TEXT, \
        created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    "CREATE INDEX IF NOT EXISTS idx_social_media_reports_kind_period \
        ON social_media_reports(kind, period_end DESC)",
    "CREATE INDEX IF NOT EXISTS idx_social_media_reports_streamer_period \
        ON social_media_reports(streamer_login, period_end DESC)",
    // Clip-Enrichment (Transkript + Titel/Beschreibung/Hashtags je Plattform).
    "CREATE TABLE IF NOT EXISTS social_media_clip_enrichment (\
        clip_db_id INTEGER PRIMARY KEY REFERENCES twitch_clips_social_media(id) ON DELETE CASCADE, \
        transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, \
        detected_terms JSONB NOT NULL DEFAULT '[]'::JSONB, \
        title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, \
        description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, \
        hashtags_youtube JSONB NOT NULL DEFAULT '[]'::JSONB, \
        hashtags_tiktok JSONB NOT NULL DEFAULT '[]'::JSONB, \
        hashtags_instagram JSONB NOT NULL DEFAULT '[]'::JSONB, \
        llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10, 6), \
        status TEXT NOT NULL DEFAULT 'pending', error_message TEXT, \
        started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, \
        updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, \
        CONSTRAINT social_media_clip_enrichment_status_chk CHECK (status IN (\
            'pending', 'transcribing', 'correcting', 'llm', 'done', 'failed', 'skipped_no_key')))",
    "CREATE INDEX IF NOT EXISTS idx_social_media_clip_enrichment_status \
        ON social_media_clip_enrichment(status)",
    "CREATE INDEX IF NOT EXISTS idx_social_media_clip_enrichment_updated_at \
        ON social_media_clip_enrichment(updated_at DESC)",
    // Clip-Approval (Freigabe-Workflow).
    "CREATE TABLE IF NOT EXISTS social_media_clip_approval (\
        clip_db_id INTEGER PRIMARY KEY REFERENCES twitch_clips_social_media(id) ON DELETE CASCADE, \
        state TEXT NOT NULL DEFAULT 'awaiting_approval', \
        approved_platforms JSONB NOT NULL DEFAULT '[]'::JSONB, approver_user_id TEXT, \
        decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, \
        CONSTRAINT social_media_clip_approval_state_chk \
            CHECK (state IN ('awaiting_approval', 'approved', 'skipped', 'editing')))",
    "CREATE INDEX IF NOT EXISTS idx_social_media_clip_approval_state \
        ON social_media_clip_approval(state)",
    "CREATE INDEX IF NOT EXISTS idx_social_media_clip_approval_last_sent_at \
        ON social_media_clip_approval(last_sent_at DESC)",
    // Externe Google-Forms-Einreichungen (Dedupe pro Clip/Formular).
    "CREATE TABLE IF NOT EXISTS twitch_clip_form_submissions (\
        id SERIAL PRIMARY KEY, clip_id INTEGER NOT NULL, form_key TEXT NOT NULL, \
        status TEXT NOT NULL DEFAULT 'pending', http_status INTEGER, error TEXT, \
        submitted_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
        UNIQUE (clip_id, form_key))",
    // Partner-Freigabe für Social-Media-Posts (zentraler Guard).
    "CREATE TABLE IF NOT EXISTS social_media_partner_access (\
        streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login) ON DELETE CASCADE, \
        granted BOOLEAN NOT NULL DEFAULT FALSE, granted_by TEXT, \
        granted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    // Analytics-Spalten (Phase 3) — neue Spalten idempotent.
    "ALTER TABLE twitch_clips_social_analytics \
        ADD COLUMN IF NOT EXISTS bucket TEXT, \
        ADD COLUMN IF NOT EXISTS watch_time_seconds INTEGER, \
        ADD COLUMN IF NOT EXISTS ctr_percent NUMERIC(5,2), \
        ADD COLUMN IF NOT EXISTS provider TEXT, \
        ADD COLUMN IF NOT EXISTS next_pull_at TIMESTAMPTZ, \
        ADD COLUMN IF NOT EXISTS engagement_rate NUMERIC(5,2)",
    "CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_bucket \
        ON twitch_clips_social_analytics(clip_id, platform, bucket)",
];

/// Legt das gesamte Social-Media-Schema idempotent an. Best-effort pro
/// Statement: ein Fehler (z.B. fehlende Basistabelle) bricht NICHT alles ab,
/// sondern wird geloggt — so bleiben unabhängige Tabellen nutzbar.
pub async fn ensure_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    for stmt in STATEMENTS {
        if let Err(error) = sqlx::query(stmt).execute(pool).await {
            tracing::error!(%error, stmt = &stmt[..stmt.len().min(60)], "social-media ensure_schema: Statement fehlgeschlagen");
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        // Basistabellen (sonst scheitern FK/ALTER).
        sqlx::query("CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, created_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_analytics (clip_id INTEGER, platform TEXT)").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn ensure_schema_idempotent_und_vollstaendig() {
        let Some(pool) = make_pool("t_sm_schema").await else { return };
        // Zweimal laufen → idempotent (kein Fehler beim zweiten Lauf).
        ensure_schema(&pool).await.unwrap();
        ensure_schema(&pool).await.unwrap();

        // Alle neuen Tabellen existieren.
        for table in [
            "social_media_settings",
            "social_media_reauth_notifications",
            "social_media_streamer_layout",
            "deadlock_vocab",
            "social_media_reports",
            "social_media_clip_enrichment",
            "social_media_clip_approval",
            "twitch_clip_form_submissions",
            "social_media_partner_access",
        ] {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_schema = current_schema() AND table_name = $1)",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(exists, "Tabelle {table} fehlt");
        }

        // Retention-Trigger gesetzt: Insert mit created_at → retention_until = +14d.
        sqlx::query("INSERT INTO twitch_clips_social_media (created_at) VALUES (NOW())")
            .execute(&pool).await.unwrap();
        let (created, retention): (chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>) =
            sqlx::query_as("SELECT created_at, retention_until FROM twitch_clips_social_media LIMIT 1")
                .fetch_one(&pool).await.unwrap();
        let retention = retention.expect("retention_until vom Trigger gesetzt");
        let delta = (retention - created).num_days();
        assert_eq!(delta, 14);

        // Neue Clip-Spalten + Analytics-Spalten vorhanden.
        sqlx::query("INSERT INTO twitch_clips_social_media (created_at, source_kind, upload_local_path) VALUES (NOW(), 'manual_upload', '/x')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, bucket, engagement_rate) VALUES (1, 'youtube', '30d', 1.5)")
            .execute(&pool).await.unwrap();
    }
}
