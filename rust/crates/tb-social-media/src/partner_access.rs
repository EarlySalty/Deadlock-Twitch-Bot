//! Partner-Freigabe-Guard für das Social-Media-Dashboard.
//!
//! Zentraler Guard, der prüft ob ein Streamer für Social-Media-Posts
//! freigegeben ist. Jeder Schreibpfad muss durch diesen einen Guard.
//!
//! Die Tabelle `social_media_partner_access` wird von [`ensure_schema`]
//! in [`crate::schema`] angelegt.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Datensatz aus `social_media_partner_access`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartnerAccessEntry {
    pub streamer_login: String,
    pub granted: bool,
    pub granted_by: Option<String>,
    pub granted_at: chrono::DateTime<chrono::Utc>,
}

/// Zentraler Guard: `true` wenn der Streamer für Social-Media-Posts
/// freigegeben ist (`granted = true` in `social_media_partner_access`).
///
/// Prüft case-insensitive (LOWER). Bei DB-Fehlern `false` (fail-closed —
/// kein versehentlicher Zugang).
pub async fn is_partner_granted(pool: &PgPool, streamer_login: &str) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE((SELECT granted FROM social_media_partner_access WHERE LOWER(streamer_login) = LOWER($1)), FALSE)",
    )
    .bind(streamer_login.trim())
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}

/// Liste aller Streamer mit Freigabestatus (alphabetisch nach Login).
pub async fn list_partner_access(pool: &PgPool) -> Result<Vec<PartnerAccessEntry>, sqlx::Error> {
    sqlx::query_as!(
        PartnerAccessEntry,
        "SELECT streamer_login, granted, granted_by, granted_at
         FROM social_media_partner_access
         ORDER BY streamer_login"
    )
    .fetch_all(pool)
    .await
}

/// Freigabe für einen Streamer setzen oder entfernen.
///
/// - `granted = true` → Streamer wird freigegeben
/// - `granted = false` → Streamer wird gesperrt
///
/// Der Streamer muss in `twitch_streamers` existieren (FK-Constraint).
pub async fn set_partner_access(
    pool: &PgPool,
    streamer_login: &str,
    granted: bool,
    granted_by: Option<&str>,
) -> Result<PartnerAccessEntry, sqlx::Error> {
    let login = streamer_login.trim();
    let actor = granted_by.unwrap_or("system");

    sqlx::query(
        "INSERT INTO social_media_partner_access (streamer_login, granted, granted_by, granted_at)
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP)
         ON CONFLICT (streamer_login)
         DO UPDATE SET granted = $2, granted_by = $3, granted_at = CURRENT_TIMESTAMP",
    )
    .bind(login)
    .bind(granted)
    .bind(actor)
    .execute(pool)
    .await?;

    let entry = sqlx::query_as!(
        PartnerAccessEntry,
        "SELECT streamer_login, granted, granted_by, granted_at
         FROM social_media_partner_access
         WHERE LOWER(streamer_login) = LOWER($1)",
        login
    )
    .fetch_one(pool)
    .await?;

    Ok(entry)
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
        let opts =
            PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        // Basistabelle + partner_access
        sqlx::query("CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE social_media_partner_access (
                streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login) ON DELETE CASCADE,
                granted BOOLEAN NOT NULL DEFAULT FALSE,
                granted_by TEXT,
                granted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn guard_blockt_nicht_freigegebene() {
        let Some(pool) = make_pool("t_sm_pa_guard_block").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ('nani')")
            .execute(&pool)
            .await
            .unwrap();
        // Kein Eintrag → nicht freigegeben
        assert!(!is_partner_granted(&pool, "nani").await);
    }

    #[tokio::test]
    async fn guard_laesst_freigegebene_durch() {
        let Some(pool) = make_pool("t_sm_pa_guard_allow").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ('earlysalty')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted, granted_by) VALUES ('earlysalty', TRUE, 'system')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(is_partner_granted(&pool, "earlysalty").await);
        assert!(is_partner_granted(&pool, "EarlySalty").await); // case-insensitive
    }

    #[tokio::test]
    async fn guard_blockt_explizit_nicht_freigegebene() {
        let Some(pool) = make_pool("t_sm_pa_guard_explicit").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ('testuser')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted, granted_by) VALUES ('testuser', FALSE, 'admin')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!is_partner_granted(&pool, "testuser").await);
    }

    #[tokio::test]
    async fn set_partner_access_erstellt_und_updated() {
        let Some(pool) = make_pool("t_sm_pa_set").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ('nani')")
            .execute(&pool)
            .await
            .unwrap();

        // Erstellen
        let entry = set_partner_access(&pool, "nani", true, Some("admin")).await.unwrap();
        assert_eq!(entry.streamer_login, "nani");
        assert!(entry.granted);
        assert_eq!(entry.granted_by.as_deref(), Some("admin"));

        // Updaten
        let entry = set_partner_access(&pool, "nani", false, Some("admin")).await.unwrap();
        assert!(!entry.granted);

        // Liste
        let list = list_partner_access(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].streamer_login, "nani");
    }
}
