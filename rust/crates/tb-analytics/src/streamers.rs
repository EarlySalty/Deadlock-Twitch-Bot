//! Query für `GET /twitch/api/v2/streamers` (Admin).

use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
pub struct StreamerListRow {
    pub twitch_login: String,
    pub is_live: i32,
    pub viewer_count: i32,
}

/// Lädt alle aktiven Partner mit Live-Status.
pub async fn active_streamers(pool: &PgPool) -> Result<Vec<StreamerListRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            sp.twitch_login,
            COALESCE(ls.is_live, 0)            AS is_live,
            COALESCE(ls.last_viewer_count, 0)  AS viewer_count
        FROM twitch_streamers_partner_state sp
        LEFT JOIN twitch_live_state ls
               ON LOWER(ls.streamer_login) = LOWER(sp.twitch_login)
        WHERE sp.is_partner_active = 1
        ORDER BY
            COALESCE(ls.is_live, 0)           DESC,
            COALESCE(ls.last_viewer_count, 0) DESC,
            LOWER(sp.twitch_login)            ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen fehlgeschlagen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamers_partner_state (
                twitch_login      TEXT NOT NULL PRIMARY KEY,
                is_partner_active INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers_partner_state fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login    TEXT NOT NULL PRIMARY KEY,
                is_live           INTEGER NOT NULL DEFAULT 0,
                last_viewer_count INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state fehlgeschlagen");
        pool
    }

    #[tokio::test]
    async fn leere_tabelle_gibt_leere_liste() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_streamers_leer").await;
        let rows = active_streamers(&pool).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn aktive_partner_werden_zurueckgegeben() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_streamers_mit_daten").await;
        sqlx::query("TRUNCATE twitch_streamers_partner_state, twitch_live_state")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('streamer_a', 1), ('streamer_b', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_viewer_count) VALUES ('streamer_a', 1, 500)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let rows = active_streamers(&pool).await.unwrap();
        // Nur streamer_a ist aktiv (is_partner_active = 1)
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].twitch_login, "streamer_a");
        assert_eq!(rows[0].is_live, 1);
        assert_eq!(rows[0].viewer_count, 500);
    }
}
