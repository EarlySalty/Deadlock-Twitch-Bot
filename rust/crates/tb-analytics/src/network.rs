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
    /// Zuletzt gemeldete Twitch-Kategorie (`twitch_live_state.last_game`).
    /// Ohne diese Spalte kann die Landing "live" nicht von "live in Deadlock"
    /// unterscheiden und muesste jeden Live-Kanal als Deadlock-Stream ausgeben.
    pub last_game: Option<String>,
    /// Deadlock-Streams der letzten 30 Tage. Grundlage der Partner-Sortierung
    /// auf der Landing; ohne den Wert faellt sie auf "live zuerst" zurueck.
    pub dl_streams_30d: i64,
    /// Schnitt-Zuschauer dieser Deadlock-Streams, `None` ohne Sessions.
    pub dl_avg_viewers_30d: Option<f64>,
}

/// Lädt alle aktiven Partner, sortiert nach Live-Status und Viewer-Anzahl.
///
/// Das 30-Tage-Aggregat läuft bei jedem Aufruf des unauthentifizierten
/// Endpoints mit, den offene Tabs alle 45 s pollen. Es stützt sich auf
/// vorhandene Indizes: `idx_twitch_sessions_login (streamer_login, started_at)`
/// aus dem Baseline-Schema und `(LOWER(streamer_login), started_at, ended_at)`
/// aus `20260719120000_public_streamer_comparison_indexes.sql`. Wenn das
/// Netzwerk deutlich wächst, ist hier der Punkt für einen Cache.
pub async fn network_streamers(pool: &PgPool) -> Result<Vec<NetworkStreamerRow>, sqlx::Error> {
    sqlx::query_as!(
        NetworkStreamerRow,
        r#"
        SELECT
            COALESCE(sp.twitch_login, '')     AS "twitch_login!",
            COALESCE(ls.is_live, 0)           AS "is_live!",
            COALESCE(ls.last_viewer_count, 0) AS "viewer_count!",
            ls.last_game                      AS "last_game?",
            COALESCE(agg.dl_streams, 0)       AS "dl_streams_30d!",
            agg.dl_avg_viewers                AS "dl_avg_viewers_30d?"
        FROM twitch_streamers_partner_state sp
        LEFT JOIN twitch_live_state ls
               ON LOWER(ls.streamer_login) = LOWER(sp.twitch_login)
        LEFT JOIN (
            SELECT LOWER(streamer_login)                         AS login,
                   COUNT(*) FILTER (WHERE had_deadlock_in_session) AS dl_streams,
                   AVG(avg_viewers) FILTER (WHERE had_deadlock_in_session) AS dl_avg_viewers
            FROM twitch_stream_sessions
            WHERE started_at >= now() - interval '30 days'
            GROUP BY LOWER(streamer_login)
        ) agg ON agg.login = LOWER(sp.twitch_login)
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
        // Erst droppen, dann anlegen: ein Schema aus einem frueheren Lauf
        // behaelt sonst die alte twitch_live_state ohne last_game (CREATE
        // TABLE IF NOT EXISTS greift dann nicht) und der Query faellt in 42703.
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen fehlgeschlagen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
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
                last_viewer_count INTEGER NOT NULL DEFAULT 0,
                last_game         TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL live_state fehlgeschlagen");

        // Sessions-Tabelle fuer die 30-Tage-Aggregate. Ohne sie laeuft der
        // Query in "relation does not exist" statt in ein leeres Aggregat.
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id                      BIGSERIAL PRIMARY KEY,
                streamer_login          TEXT NOT NULL,
                started_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
                had_deadlock_in_session BOOLEAN NOT NULL DEFAULT false,
                avg_viewers             DOUBLE PRECISION
            )"#,
        )
        .execute(&pool)
        .await
        .expect("DDL stream_sessions");

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
