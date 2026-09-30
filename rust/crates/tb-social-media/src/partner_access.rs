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

pub async fn is_partner_id_granted(pool: &PgPool, twitch_user_id: &str) -> bool {
    if twitch_user_id.is_empty() || !twitch_user_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE((SELECT granted FROM social_media_partner_access WHERE twitch_user_id = $1), FALSE)",
    )
    .bind(twitch_user_id)
    .fetch_one(pool)
    .await
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

    let mut tx = pool.begin().await?;
    let twitch_user_id: String = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) \
         AND twitch_user_id ~ '^[0-9]+$' FOR SHARE",
    )
    .bind(login)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM social_media_partner_access WHERE twitch_user_id = $1 AND streamer_login <> $2")
        .bind(&twitch_user_id).bind(login).execute(&mut *tx).await?;

    sqlx::query(
        "INSERT INTO social_media_partner_access (streamer_login, granted, granted_by, granted_at, twitch_user_id)
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP, $4)
         ON CONFLICT (streamer_login)
         DO UPDATE SET granted = $2, granted_by = $3, granted_at = CURRENT_TIMESTAMP, twitch_user_id = $4",
    )
    .bind(login)
    .bind(granted)
    .bind(actor)
    .bind(&twitch_user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

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
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        // Basistabelle + partner_access
        sqlx::query("CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT DEFAULT '42')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE social_media_partner_access (
                streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login) ON DELETE CASCADE,
                twitch_user_id TEXT,
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
        assert!(!is_partner_id_granted(&pool, "42").await);
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
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, twitch_user_id, granted, granted_by) VALUES ('earlysalty', '42', TRUE, 'system')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(is_partner_id_granted(&pool, "42").await);
        assert!(is_partner_id_granted(&pool, "42").await); // case-insensitive
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
        assert!(!is_partner_id_granted(&pool, "99").await);
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
        let entry = set_partner_access(&pool, "nani", true, Some("admin"))
            .await
            .unwrap();
        assert_eq!(entry.streamer_login, "nani");
        assert!(entry.granted);
        assert_eq!(entry.granted_by.as_deref(), Some("admin"));

        // Updaten
        let entry = set_partner_access(&pool, "nani", false, Some("admin"))
            .await
            .unwrap();
        assert!(!entry.granted);

        // Liste
        let list = list_partner_access(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].streamer_login, "nani");
    }
}
