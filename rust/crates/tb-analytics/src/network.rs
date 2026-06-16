//! Queries für `GET /twitch/api/v2/public/network`.
//!
//! `twitch_streamers_partner_state` ist eine VIEW (in der Migration angelegt via
//! `DROP VIEW IF EXISTS … CREATE VIEW …`). Sie projiziert `twitch_login` und
//! `is_partner_active` aus `twitch_partners` (einzige Wahrheitsquelle für Partner-Status).

use sqlx::PgPool;

/// Eine Zeile aus dem Netzwerk-Query.
///
/// `is_live` kommt als `i32` (0/1) aus der DB (COALESCE auf INTEGER-Spalte).
/// Die JSON-Serialisierung wandelt es in `bool` um (→ Handler).
#[derive(Debug, sqlx::FromRow)]
pub struct NetworkStreamerRow {
    pub twitch_login: String,
    pub is_live: i32,
    pub viewer_count: i32,
}

/// Lädt alle aktiven Partner, sortiert nach Live-Status und Viewer-Anzahl.
pub async fn network_streamers(pool: &PgPool) -> Result<Vec<NetworkStreamerRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            sp.twitch_login,
            COALESCE(ls.is_live, 0)           AS is_live,
            COALESCE(ls.last_viewer_count, 0) AS viewer_count
        FROM twitch_streamers_partner_state sp
        LEFT JOIN twitch_live_state ls
               ON LOWER(ls.streamer_login) = LOWER(sp.twitch_login)
        WHERE sp.is_partner_active = 1
        ORDER BY COALESCE(ls.is_live, 0) DESC,
                 COALESCE(ls.last_viewer_count, 0) DESC,
                 LOWER(sp.twitch_login) ASC
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

    /// Pool mit max 1 Connection + eigenem Schema: Basistabelle + VIEW + live_state.
    /// Parallele Tests kollidieren nicht.
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
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login    TEXT PRIMARY KEY,
                is_live           INTEGER NOT NULL DEFAULT 0,
                last_viewer_count INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL live_state fehlgeschlagen");

        // Basistabelle für die View
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS _partner_state_base (
                twitch_login      TEXT PRIMARY KEY,
                is_partner_active INTEGER NOT NULL DEFAULT 1
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL partner_state_base fehlgeschlagen");

        // View anlegen (idempotent via OR REPLACE)
        sqlx::query(
            r#"
            CREATE OR REPLACE VIEW twitch_streamers_partner_state AS
            SELECT twitch_login, is_partner_active
            FROM _partner_state_base
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL view fehlgeschlagen");

        pool
    }

    #[tokio::test]
    async fn network_leeres_ergebnis() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_network_leer").await;
        sqlx::query("TRUNCATE _partner_state_base CASCADE")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("TRUNCATE twitch_live_state")
            .execute(&pool)
            .await
            .unwrap();

        let rows = network_streamers(&pool).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn network_sortierung_und_is_live_konvertierung() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_network_sort").await;
        sqlx::query("TRUNCATE _partner_state_base CASCADE")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("TRUNCATE twitch_live_state")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO _partner_state_base (twitch_login, is_partner_active) \
             VALUES ('dragskope', 1), ('anderer', 1), ('offline_streamer', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_viewer_count) \
             VALUES ('dragskope', 1, 500), ('anderer', 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let rows = network_streamers(&pool).await.unwrap();
        assert_eq!(rows.len(), 3);
        // dragskope zuerst (is_live=1, 500 viewer)
        assert_eq!(rows[0].twitch_login, "dragskope");
        assert_eq!(rows[0].is_live, 1);
        assert_eq!(rows[0].viewer_count, 500);
        // offline_streamer ohne live_state → COALESCE → 0/0
        let offline = rows
            .iter()
            .find(|r| r.twitch_login == "offline_streamer")
            .unwrap();
        assert_eq!(offline.is_live, 0);
        assert_eq!(offline.viewer_count, 0);
    }
}
