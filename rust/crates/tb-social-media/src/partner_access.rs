//! Partner-Freigabe-Guard für das Social-Media-Dashboard.
//!
//! Zentraler Guard, der prüft ob ein Streamer für Social-Media-Posts
//! freigegeben ist. Jeder Schreibpfad muss durch diesen einen Guard.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Datensatz aus `social_media_partner_access`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PartnerAccessEntry {
    pub twitch_user_id: String,
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
    sqlx::query_as::<_, PartnerAccessEntry>(
        "SELECT twitch_user_id, streamer_login, granted, granted_by, granted_at
         FROM social_media_partner_access
         ORDER BY streamer_login",
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
    twitch_user_id: &str,
    granted: bool,
    granted_by: Option<&str>,
) -> Result<PartnerAccessEntry, sqlx::Error> {
    if twitch_user_id.is_empty() || !twitch_user_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(sqlx::Error::Protocol("Ungültige Twitch-ID".into()));
    }
    let actor = granted_by.unwrap_or("system");

    let mut tx = pool.begin().await?;
    // Der Name wird nur für den bestehenden Anzeige-/Fremdschlüssel gelesen.
    // Die vom Admin gewählte ID bleibt die einzige Zielidentität.
    let login: String = sqlx::query_scalar(
        "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = $1 FOR SHARE",
    )
    .bind(twitch_user_id)
    .fetch_one(&mut *tx)
    .await?;

    let entry = sqlx::query_as::<_, PartnerAccessEntry>(
        "INSERT INTO social_media_partner_access (streamer_login, granted, granted_by, granted_at, twitch_user_id)
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP, $4)
         ON CONFLICT (twitch_user_id)
         DO UPDATE SET streamer_login = EXCLUDED.streamer_login, granted = EXCLUDED.granted,
                       granted_by = EXCLUDED.granted_by, granted_at = EXCLUDED.granted_at
         RETURNING twitch_user_id, streamer_login, granted, granted_by, granted_at",
    )
    .bind(&login)
    .bind(granted)
    .bind(actor)
    .bind(twitch_user_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

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
                streamer_login TEXT NOT NULL,
                twitch_user_id TEXT PRIMARY KEY CHECK (twitch_user_id ~ '^[0-9]+$'),
                granted BOOLEAN NOT NULL DEFAULT FALSE,
                granted_by TEXT,
                granted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE UNIQUE INDEX access_identity ON social_media_partner_access(twitch_user_id) WHERE twitch_user_id IS NOT NULL")
            .execute(&pool).await.unwrap();
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
        sqlx::query("INSERT INTO social_media_partner_access (streamer_login, twitch_user_id, granted, granted_by) VALUES ('testuser', '42', FALSE, 'admin')")
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
        let entry = set_partner_access(&pool, "42", true, Some("admin"))
            .await
            .unwrap();
        assert_eq!(entry.streamer_login, "nani");
        assert!(entry.granted);
        assert_eq!(entry.granted_by.as_deref(), Some("admin"));

        // Updaten
        let entry = set_partner_access(&pool, "42", false, Some("admin"))
            .await
            .unwrap();
        assert!(!entry.granted);

        // Liste
        let list = list_partner_access(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].streamer_login, "nani");
    }
    #[tokio::test]
    async fn grant_and_revoke_keep_selected_id_after_login_reassignment() {
        let Some(pool) = make_pool("t_sm_pa_reassignment").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('name_l', '11')",
        )
        .execute(&pool)
        .await
        .unwrap();
        set_partner_access(&pool, "11", true, Some("admin"))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_streamers SET twitch_user_id = '22' WHERE twitch_login = 'name_l'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('new_a', '11')",
        )
        .execute(&pool)
        .await
        .unwrap();
        set_partner_access(&pool, "22", false, Some("admin"))
            .await
            .unwrap();
        assert!(is_partner_id_granted(&pool, "11").await);
        assert!(!is_partner_id_granted(&pool, "22").await);
        let a = set_partner_access(&pool, "11", true, Some("admin"))
            .await
            .unwrap();
        assert_eq!(a.twitch_user_id, "11");
        assert_eq!(a.streamer_login, "new_a");
        set_partner_access(&pool, "22", true, Some("admin"))
            .await
            .unwrap();
        set_partner_access(&pool, "11", false, Some("admin"))
            .await
            .unwrap();
        assert!(!is_partner_id_granted(&pool, "11").await);
        assert!(is_partner_id_granted(&pool, "22").await);
        let entries = list_partner_access(&pool).await.unwrap();
        assert!(entries
            .iter()
            .any(|entry| entry.twitch_user_id == "11" && !entry.granted));
        assert!(entries
            .iter()
            .any(|entry| entry.twitch_user_id == "22" && entry.granted));
    }

    #[tokio::test]
    async fn grants_require_id_and_never_resolve_login() {
        let Some(pool) = make_pool("t_sm_pa_required_id").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('legacy', '11')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(sqlx::query("INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('legacy', TRUE)")
            .execute(&pool).await.is_err());
        assert!(set_partner_access(&pool, "legacy", true, Some("admin"))
            .await
            .is_err());
        assert!(set_partner_access(&pool, "999", true, Some("admin"))
            .await
            .is_err());
        let entry = set_partner_access(&pool, "11", true, Some("admin"))
            .await
            .unwrap();
        assert_eq!(entry.twitch_user_id, "11");
        assert!(is_partner_id_granted(&pool, "11").await);
        assert!(!is_partner_id_granted(&pool, "legacy").await);
    }
}
