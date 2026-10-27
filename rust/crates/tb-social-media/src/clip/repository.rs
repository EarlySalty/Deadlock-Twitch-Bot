use sqlx::{PgConnection, PgPool};

use super::model::{ClipRecord, StreamerFetchResult};
use crate::layout::apply_default_layout;

fn int4_metric(field: &str, value: i64) -> Result<i32, sqlx::Error> {
    i32::try_from(value).map_err(|error| {
        sqlx::Error::InvalidArgument(format!("{field}={value} does not fit into int4: {error}"))
    })
}

/// DB-Zugriff für den Clip-Fetcher.
///
/// Jede Methode ist ein einzelner, klar benannter Datenbankaufruf.
/// Keine Business-Logik — das gehört in `ClipFetchService`.
#[derive(Clone)]
pub struct ClipRepository {
    pool: PgPool,
}

impl ClipRepository {
    pub async fn pool_managed(&self, owner: &str) -> Result<bool, crate::clip_pool::PoolError> {
        crate::clip_pool::is_managed(&self.pool, owner).await
    }

    pub async fn social_access_granted(&self, owner: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM social_media_partner_access WHERE twitch_user_id = $1 AND granted)",
        )
        .bind(owner)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn start_pool_import(
        &self,
        owner: &str,
        login: &str,
    ) -> Result<crate::clip_pool::ImportJob, crate::clip_pool::PoolError> {
        crate::clip_pool::start_import(&self.pool, owner, login).await
    }

    pub async fn register_recent_pool(
        &self,
        owner: &str,
        clips: &[ClipRecord],
    ) -> Result<(i32, i32), crate::clip_pool::PoolError> {
        crate::clip_pool::register_recent(&self.pool, owner, clips).await
    }

    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Gibt alle Logins aktiver Partner zurück (nicht departnered, nicht archiviert).
    pub async fn active_partner_logins(&self) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query_scalar!(
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

    /// Stellt sicher, dass der Streamer in `twitch_streamers` existiert
    /// (FK-Pflicht vor dem Clip-Insert).
    ///
    /// Parität zu Python (`clip_manager.register_clip`): die `twitch_user_id`
    /// eines bereits bekannten Streamers bleibt unangetastet (kein Backfill).
    /// Monitoring-only ergibt sich aus fehlendem Partner-Eintrag.
    pub async fn ensure_monitored_streamer(
        &self,
        login: &str,
        twitch_user_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
            VALUES ($1, $2)
            ON CONFLICT (twitch_login) DO NOTHING
            "#,
            login,
            twitch_user_id
        )
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
    pub async fn register_clip(&self, rec: &ClipRecord) -> Result<(i64, bool), sqlx::Error> {
        let created_at = chrono::DateTime::parse_from_rfc3339(&rec.created_at)
            .map(|value| value.with_timezone(&chrono::Utc))
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        let view_count = int4_metric("view_count", rec.view_count)?;

        let category_key = crate::posting_plan::resolve_category(
            &self.pool,
            rec.game_id.as_deref(),
            rec.game_name.as_deref(),
        )
        .await;
        let mut tx = self.pool.begin().await?;
        let result =
            register_on(&mut tx, rec, created_at, view_count, &category_key, false).await?;
        tx.commit().await?;
        self.apply_layout(result.0, &rec.streamer_login).await;
        Ok(result)
    }

    /// Belegt das Clip-Override mit dem Streamer-Default (best-effort, mirror
    /// Python: Layout-Fehler brechen den Register-Pfad nicht ab).
    async fn apply_layout(&self, clip_db_id: i64, streamer_login: &str) {
        if let Err(e) = apply_default_layout(&self.pool, clip_db_id, streamer_login).await {
            tracing::warn!(
                "clip_fetch: apply_default_layout für Clip {clip_db_id} fehlgeschlagen: {e}"
            );
        }
    }

    /// Bucht einen Clip auf das Monatskontingent des Streamers.
    ///
    /// Absichtlich ein eigener Aufruf und nicht Teil von `register_clip`: ob ein
    /// Clip verbraucht, haengt nicht am Clip, sondern daran, wer ihn geholt hat.
    /// Der Hintergrund-Fetcher ruft das nie, der vom Streamer ausgeloeste
    /// Dashboard-Fetch schon (`ClipFetchService::mit_kontingent_verbrauch`).
    /// `COALESCE` schuetzt eine bereits gebuchte Zeile vor der Umbuchung in
    /// einen spaeteren Monat.
    pub async fn mark_kontingent_verbrauch(&self, clip_db_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE twitch_clips_social_media AS c \
                SET kontingent_verbraucht_at = COALESCE(kontingent_verbraucht_at, NOW()) \
              WHERE id = $1 AND (to_jsonb(c)->>'pool_managed_at' IS NULL OR c.source_kind = 'manual_upload')",
        )
        .bind(clip_db_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Schreibt einen Eintrag in `clip_fetch_history` (Erfolg oder Fehler).
    pub async fn record_history(&self, result: &StreamerFetchResult) -> Result<(), sqlx::Error> {
        if let Some(err) = &result.error {
            sqlx::query!(
                r#"
                INSERT INTO clip_fetch_history (streamer_login, clips_found, clips_new, error)
                VALUES ($1, 0, 0, $2)
                "#,
                &result.login,
                err.as_str()
            )
            .execute(&self.pool)
            .await?;
        } else {
            let duration_ms = int4_metric("fetch_duration_ms", result.duration_ms)?;
            sqlx::query!(
                r#"
                INSERT INTO clip_fetch_history
                    (streamer_login, clips_found, clips_new, fetch_duration_ms)
                VALUES ($1, $2, $3, $4)
                "#,
                &result.login,
                result.clips_found,
                result.clips_new,
                duration_ms
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
}

pub(crate) async fn register_on(
    connection: &mut PgConnection,
    rec: &ClipRecord,
    created_at: chrono::DateTime<chrono::Utc>,
    view_count: i32,
    category_key: &str,
    pool_only: bool,
) -> Result<(i64, bool), sqlx::Error> {
    let row: Option<(i64, bool)> = sqlx::query_as(
        "INSERT INTO twitch_clips_social_media AS c
            (clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login, twitch_user_id,
             created_at, duration_seconds, view_count, game_name, game_id, category_key, status, vod_id, vod_offset_s)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         ON CONFLICT (clip_id) DO UPDATE SET clip_id = c.clip_id
         WHERE c.twitch_user_id = EXCLUDED.twitch_user_id
         RETURNING c.id::bigint, (xmax = 0)",
    )
    .bind(&rec.clip_id).bind(&rec.clip_url).bind(&rec.clip_title).bind(&rec.thumbnail_url)
    .bind(&rec.streamer_login).bind(&rec.twitch_user_id).bind(created_at).bind(rec.duration_seconds)
    .bind(view_count).bind(&rec.game_name).bind(&rec.game_id).bind(category_key)
    .bind(if pool_only { "pool" } else { "pending" }).bind(&rec.vod_id).bind(rec.vod_offset_s)
    .fetch_optional(connection).await?;
    row.ok_or_else(|| sqlx::Error::Protocol("Der Clip gehört zu einem anderen Kanal".into()))
}

pub(crate) async fn initialize_layout_on(
    connection: &mut PgConnection,
    clip_id: i64,
    owner: &str,
) -> Result<(), sqlx::Error> {
    let needs_layout: bool = sqlx::query_scalar("SELECT layout_override_json IS NULL FROM twitch_clips_social_media WHERE id = $1 AND twitch_user_id = $2")
        .bind(clip_id).bind(owner).fetch_one(&mut *connection).await?;
    if !needs_layout {
        return Ok(());
    }
    let raw: Option<(serde_json::Value, bool, String)> = sqlx::query_as(
        "SELECT layout_json, cam_enabled, mode FROM social_media_streamer_layout WHERE twitch_user_id = $1",
    ).bind(owner).fetch_optional(&mut *connection).await?;
    let layout = match raw {
        Some((raw, cam_enabled, mode)) => {
            crate::layout::StreamerLayout::from_stored_value(&raw, Some(cam_enabled), Some(&mode))
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?
        }
        None => {
            let mut layout = crate::layout::default_streamer_layout();
            layout.cam_enabled = false;
            layout.mode = "blur_pad".into();
            layout
        }
    };
    sqlx::query("UPDATE twitch_clips_social_media SET layout_override_json = COALESCE(layout_override_json, $2) WHERE id = $1")
        .bind(clip_id).bind(layout.to_override_json()).execute(connection).await?;
    Ok(())
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
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT UNIQUE, clip_url TEXT, clip_title TEXT, clip_thumbnail_url TEXT, streamer_login TEXT, twitch_user_id TEXT, created_at TIMESTAMPTZ, duration_seconds DOUBLE PRECISION, view_count BIGINT DEFAULT 0, game_name TEXT, game_id TEXT, category_key TEXT NOT NULL DEFAULT 'other', status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', vod_id TEXT, vod_offset_s INTEGER, kontingent_verbraucht_at TIMESTAMPTZ, layout_override_json JSONB)",
            "CREATE TABLE social_media_category (category_key TEXT PRIMARY KEY, display_name TEXT NOT NULL, twitch_game_id TEXT, match_game_names TEXT[] NOT NULL DEFAULT '{}', enrichment_enabled BOOLEAN NOT NULL DEFAULT FALSE, sort_order INTEGER NOT NULL DEFAULT 100)",
            "INSERT INTO social_media_category (category_key, display_name, match_game_names, enrichment_enabled, sort_order) VALUES ('deadlock', 'Deadlock', ARRAY['deadlock'], TRUE, 10), ('other', 'Andere Spiele', ARRAY[]::TEXT[], FALSE, 900)",
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
            broadcaster_name: None,
            created_at: "2026-06-15T00:00:00Z".to_string(),
            duration_seconds: 28.0,
            view_count: 5,
            game_name: Some("Deadlock".to_string()),
            game_id: Some("1422200164".to_string()),
            vod_id: None,
            vod_offset_s: None,
        }
    }

    async fn layout_override(pool: &PgPool, clip_db_id: i64) -> Option<String> {
        sqlx::query_scalar(
            "SELECT layout_override_json::text FROM twitch_clips_social_media WHERE id = $1",
        )
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
        let Some(pool) = make_pool("t_sm_repo_layout").await else {
            return;
        };
        let repo = ClipRepository::new(pool.clone());

        // Neuer Clip → layout_override gesetzt (Streamer ohne eigenes Layout → globaler Default).
        let (id, is_new) = repo.register_clip(&rec("c1", "nani")).await.unwrap();
        assert!(is_new);
        assert!(
            layout_override(&pool, id).await.is_some(),
            "neuer Clip muss layout_override haben"
        );

        // Existierender Clip → erneuter Register ruft apply_default_layout (COALESCE
        // schützt bestehendes Override), Override bleibt gesetzt.
        let (id2, is_new2) = repo.register_clip(&rec("c1", "nani")).await.unwrap();
        assert_eq!(id, id2);
        assert!(!is_new2);
        assert!(
            layout_override(&pool, id2).await.is_some(),
            "existierender Clip behält layout_override"
        );
    }

    #[tokio::test]
    async fn register_clip_akzeptiert_live_typen_bigint_und_timestamptz() {
        let Some(pool) = make_pool("t_sm_repo_live_types").await else {
            return;
        };
        let repo = ClipRepository::new(pool.clone());

        let (id, is_new) = repo
            .register_clip(&rec("live-types", "nani"))
            .await
            .unwrap();
        assert!(is_new);
        assert!(id > 0);
        let created_at: chrono::DateTime<chrono::Utc> =
            sqlx::query_scalar("SELECT created_at FROM twitch_clips_social_media WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(created_at.to_rfc3339(), "2026-06-15T00:00:00+00:00");
    }

    #[tokio::test]
    async fn ensure_streamer_backfillt_user_id_nicht() {
        let Some(pool) = make_pool("t_sm_repo_streamer").await else {
            return;
        };
        let repo = ClipRepository::new(pool.clone());

        // Streamer existiert mit NULL user_id (z.B. anders eingelegt).
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('nani', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        repo.ensure_monitored_streamer("nani", "12345")
            .await
            .unwrap();

        let uid: Option<String> = sqlx::query_scalar(
            "SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = 'nani'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            uid, None,
            "vorhandene NULL user_id darf nicht gebackfillt werden (Python-Parität)"
        );

        // Neuer Streamer → INSERT setzt user_id ganz normal.
        repo.ensure_monitored_streamer("ghost", "777")
            .await
            .unwrap();
        let uid2: Option<String> = sqlx::query_scalar(
            "SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = 'ghost'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(uid2.as_deref(), Some("777"));
    }
}
