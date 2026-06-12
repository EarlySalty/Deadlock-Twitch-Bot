use sqlx::PgPool;

use super::model::{ClipRecord, StreamerFetchResult};

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
    /// existiert (FK-Pflicht vor dem Clip-Insert). ON CONFLICT backfüllt nur NULL-Werte.
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
                twitch_user_id    = COALESCE(twitch_streamers.twitch_user_id, EXCLUDED.twitch_user_id),
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
    pub async fn register_clip(&self, rec: &ClipRecord) -> Result<(i64, bool), sqlx::Error> {
        // Prüfe ob der Clip bereits existiert.
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM twitch_clips_social_media WHERE clip_id = $1",
        )
        .bind(&rec.clip_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(id) = existing {
            return Ok((id, false));
        }

        let id: i64 = sqlx::query_scalar(
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

        Ok((id, true))
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
