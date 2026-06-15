use sqlx::PgPool;

use super::model::{ClipRecord, StreamerFetchResult};
use crate::layout::apply_default_layout;

/// DB-Zugriff für den Clip-Fetcher.
///
/// Jede Methode ist ein einzelner, klar benannter Datenbankaufruf.
/// Keine Business-Logik — das gehört in `ClipFetchService`.
#[derive(Clone)]
pub struct ClipRepository {
    pool: PgPool,
}

impl ClipRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Gibt alle Logins aktiver Partner zurück (nicht departnered, nicht archiviert).
    pub async fn active_partner_logins(&self) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT tp.twitch_login
              FROM twitch_partners tp
             WHERE tp.departnered_at IS NULL
               AND tp.admin_archived_at IS NULL
             ORDER BY tp.twitch_login ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Stellt sicher, dass der Streamer in `twitch_streamers` als monitoring-only
    /// existiert (FK-Pflicht vor dem Clip-Insert).
    ///
    /// Parität zu Python (`clip_manager.register_clip`): bei vorhandenem Streamer
    /// wird ausschließlich `is_monitored_only` per COALESCE aufgefüllt — die
    /// `twitch_user_id` eines bereits bekannten Streamers bleibt unangetastet
    /// (kein Backfill), exakt wie das Original. Ein früherer Rust-Backfill der
    /// User-ID wich vom 1:1-Verhalten ab (social_media-5).
    pub async fn ensure_monitored_streamer(
        &self,
        login: &str,
        twitch_user_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only)
            VALUES ($1, $2, 1)
            ON CONFLICT (twitch_login) DO UPDATE SET
                is_monitored_only = COALESCE(twitch_streamers.is_monitored_only, 1)
            "#,
        )
        .bind(login)
        .bind(twitch_user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Registriert einen Clip — gibt (db_id, is_new) zurück.
    ///
    /// Bei Konflikt (clip_id bereits vorhanden) wird die vorhandene ID zurückgegeben,
    /// `is_new` ist dann `false`.
    ///
    /// Parität zu Python (`clip_manager.register_clip`): nach JEDEM Register —
    /// sowohl bei bereits existierendem als auch bei neuem Clip — wird
    /// `apply_default_layout` aufgerufen, damit `layout_override_json` mit dem
    /// Streamer-Default vorbelegt wird (COALESCE schützt bestehende Overrides).
    /// Der frühere Rust-Fetch-Pfad rief das nie auf → per-Fetch eingelesene Clips
    /// hatten `layout_override_json = NULL` statt des Defaults (social_media-1).
    ///
    /// Migrationsbug-Fix: `twitch_clips_social_media.id` ist im Baseline-Schema
    /// `integer` (INT4); der frühere `i64`-Decode hätte in Prod einen
    /// sqlx-ColumnDecode-Fehler (INT4→i64) geworfen. Rückgabetyp ist daher `i32`
    /// (passend zum Schema, identisch zu `clip_manager`).
    pub async fn register_clip(&self, rec: &ClipRecord) -> Result<(i32, bool), sqlx::Error> {
        // Prüfe ob der Clip bereits existiert.
        let existing: Option<i32> = sqlx::query_scalar(
            "SELECT id FROM twitch_clips_social_media WHERE clip_id = $1",
        )
        .bind(&rec.clip_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(id) = existing {
            self.apply_layout(id, &rec.streamer_login).await;
            return Ok((id, false));
        }

        let id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO twitch_clips_social_media
                (clip_id, clip_url, clip_title, clip_thumbnail_url,
                 streamer_login, twitch_user_id, created_at, duration_seconds,
                 view_count, game_name, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending')
            RETURNING id
            "#,
        )
        .bind(&rec.clip_id)
        .bind(&rec.clip_url)
        .bind(&rec.clip_title)
        .bind(&rec.thumbnail_url)
        .bind(&rec.streamer_login)
        .bind(&rec.twitch_user_id)
        .bind(&rec.created_at)
        .bind(rec.duration_seconds)
        .bind(rec.view_count)
        .bind(&rec.game_name)
        .fetch_one(&self.pool)
        .await?;

        self.apply_layout(id, &rec.streamer_login).await;
        Ok((id, true))
    }

    /// Belegt das Clip-Override mit dem Streamer-Default (best-effort, mirror
    /// Python: Layout-Fehler brechen den Register-Pfad nicht ab).
    async fn apply_layout(&self, clip_db_id: i32, streamer_login: &str) {
        if let Err(e) = apply_default_layout(&self.pool, clip_db_id, streamer_login).await {
            tracing::warn!("clip_fetch: apply_default_layout für Clip {clip_db_id} fehlgeschlagen: {e}");
        }
    }

    /// Schreibt einen Eintrag in `clip_fetch_history` (Erfolg oder Fehler).
    pub async fn record_history(&self, result: &StreamerFetchResult) -> Result<(), sqlx::Error> {
        if let Some(err) = &result.error {
            sqlx::query(
                r#"
                INSERT INTO clip_fetch_history (streamer_login, clips_found, clips_new, error)
                VALUES ($1, 0, 0, $2)
                "#,
            )
            .bind(&result.login)
            .bind(err)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                INSERT INTO clip_fetch_history
                    (streamer_login, clips_found, clips_new, fetch_duration_ms)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(&result.login)
            .bind(result.clips_found)
            .bind(result.clips_new)
            .bind(result.duration_ms)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
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
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT, is_monitored_only INTEGER)",
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT UNIQUE, clip_url TEXT, clip_title TEXT, clip_thumbnail_url TEXT, streamer_login TEXT, twitch_user_id TEXT, created_at TEXT, duration_seconds DOUBLE PRECISION, view_count BIGINT DEFAULT 0, game_name TEXT, status TEXT DEFAULT 'pending', layout_override_json JSONB)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    fn rec(clip_id: &str, login: &str) -> ClipRecord {
        ClipRecord {
            clip_id: clip_id.to_string(),
            clip_url: "https://clips.twitch.tv/x".to_string(),
            clip_title: "Insane".to_string(),
            thumbnail_url: None,
            streamer_login: login.to_string(),
            twitch_user_id: "999".to_string(),
            created_at: "2026-06-15T00:00:00Z".to_string(),
            duration_seconds: 28.0,
            view_count: 5,
            game_name: Some("Deadlock".to_string()),
        }
    }

    async fn layout_override(pool: &PgPool, clip_db_id: i32) -> Option<String> {
        sqlx::query_scalar("SELECT layout_override_json::text FROM twitch_clips_social_media WHERE id = $1")
            .bind(clip_db_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    // social_media-1: register_clip belegt layout_override_json sowohl für neue
    // als auch für bereits existierende Clips (Python-Parität: apply_default_layout
    // in beiden Zweigen).
    #[tokio::test]
    async fn register_clip_belegt_default_layout_in_beiden_zweigen() {
        let Some(pool) = make_pool("t_sm_repo_layout").await else { return };
        let repo = ClipRepository::new(pool.clone());

        // Neuer Clip → layout_override gesetzt (Streamer ohne eigenes Layout → globaler Default).
        let (id, is_new) = repo.register_clip(&rec("c1", "nani")).await.unwrap();
        assert!(is_new);
        assert!(layout_override(&pool, id).await.is_some(), "neuer Clip muss layout_override haben");

        // Existierender Clip → erneuter Register ruft apply_default_layout (COALESCE
        // schützt bestehendes Override), Override bleibt gesetzt.
        let (id2, is_new2) = repo.register_clip(&rec("c1", "nani")).await.unwrap();
        assert_eq!(id, id2);
        assert!(!is_new2);
        assert!(layout_override(&pool, id2).await.is_some(), "existierender Clip behält layout_override");
    }

    // social_media-5: ensure_monitored_streamer backfillt twitch_user_id eines
    // bereits bekannten Streamers NICHT (1:1 zu Python).
    #[tokio::test]
    async fn ensure_streamer_backfillt_user_id_nicht() {
        let Some(pool) = make_pool("t_sm_repo_streamer").await else { return };
        let repo = ClipRepository::new(pool.clone());

        // Streamer existiert mit NULL user_id (z.B. anders eingelegt).
        sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only) VALUES ('nani', NULL, 1)")
            .execute(&pool).await.unwrap();

        repo.ensure_monitored_streamer("nani", "12345").await.unwrap();

        let uid: Option<String> = sqlx::query_scalar("SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = 'nani'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(uid, None, "vorhandene NULL user_id darf nicht gebackfillt werden (Python-Parität)");

        // Neuer Streamer → INSERT setzt user_id ganz normal.
        repo.ensure_monitored_streamer("ghost", "777").await.unwrap();
        let uid2: Option<String> = sqlx::query_scalar("SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = 'ghost'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(uid2.as_deref(), Some("777"));
    }
}
