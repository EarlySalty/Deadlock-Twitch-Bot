//! Query für `GET /twitch/api/v2/streamers` (Admin).

use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
pub struct StreamerListRow {
    pub twitch_login: String,
    /// `true` wenn der Streamer in `twitch_streamers_partner_state` als aktiver
    /// Partner geführt wird; `false` für reine 90-Tage-Recent-Logins.
    pub is_partner: bool,
    pub is_live: i32,
    pub viewer_count: i32,
}

/// Lädt aktive Partner **und** kürzlich aktive Nicht-Partner mit Live-Status.
///
/// Parität zu Python `_load_api_v2_streamers_data` (`api_v2.py:574-623`): UNION
/// aus aktiven Partnern (`twitch_streamers_partner_state.is_partner_active = 1`)
/// und Recent-Logins (`twitch_stream_sessions.started_at >= NOW() - 90 Tage`).
/// `is_partner` ergibt sich aus der Partner-Mitgliedschaft, nicht hartkodiert.
/// Die Rust-seitige Live-/Viewer-Sortierung bleibt erhalten.
pub async fn active_streamers(pool: &PgPool) -> Result<Vec<StreamerListRow>, sqlx::Error> {
    sqlx::query_as!(
        StreamerListRow,
        r#"
        WITH partner_logins AS (
            SELECT LOWER(twitch_login) AS login
            FROM twitch_streamers_partner_state
            WHERE is_partner_active = 1
        ),
        recent_logins AS (
            SELECT DISTINCT LOWER(streamer_login) AS login
            FROM twitch_stream_sessions
            WHERE started_at >= NOW() - INTERVAL '90 days'
        ),
        all_logins AS (
            SELECT login FROM partner_logins
            UNION
            SELECT login FROM recent_logins
        )
        SELECT
            COALESCE(a.login, '')              AS "twitch_login!",
            (p.login IS NOT NULL)              AS "is_partner!",
            COALESCE(ls.is_live, 0)            AS "is_live!",
            COALESCE(ls.last_viewer_count, 0)  AS "viewer_count!"
        FROM all_logins a
        LEFT JOIN partner_logins p
               ON p.login = a.login
        LEFT JOIN twitch_live_state ls
               ON LOWER(ls.streamer_login) = a.login
        WHERE a.login <> ''
        ORDER BY
            COALESCE(ls.is_live, 0)           DESC,
            COALESCE(ls.last_viewer_count, 0) DESC,
            a.login                           ASC
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
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id             BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                started_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_stream_sessions fehlgeschlagen");
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
        sqlx::query(
            "TRUNCATE twitch_streamers_partner_state, twitch_live_state, twitch_stream_sessions",
        )
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
        // Nur streamer_a ist aktiver Partner; streamer_b ist nicht aktiv und hat
        // keine Recent-Session → bleibt draußen.
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].twitch_login, "streamer_a");
        assert!(rows[0].is_partner);
        assert_eq!(rows[0].is_live, 1);
        assert_eq!(rows[0].viewer_count, 500);
    }

    #[tokio::test]
    async fn recent_nicht_partner_erscheint_mit_is_partner_false() {
        // P2.84: Ein Nicht-Partner mit Session in den letzten 90 Tagen muss
        // mit isPartner=false in der Liste auftauchen.
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_streamers_recent_union").await;
        sqlx::query(
            "TRUNCATE twitch_streamers_partner_state, twitch_live_state, twitch_stream_sessions",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Aktiver Partner
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('partner_x', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Recent Non-Partner (Session vor 10 Tagen) + alter Non-Partner (vor 200 Tagen)
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at) VALUES \
             ('Recent_NonPartner', NOW() - INTERVAL '10 days'), \
             ('alt_nonpartner', NOW() - INTERVAL '200 days')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let rows = active_streamers(&pool).await.unwrap();
        let logins: Vec<&str> = rows.iter().map(|r| r.twitch_login.as_str()).collect();
        assert!(logins.contains(&"partner_x"));
        assert!(
            logins.contains(&"recent_nonpartner"),
            "Recent Non-Partner (lowercase) muss erscheinen"
        );
        assert!(
            !logins.contains(&"alt_nonpartner"),
            "Login >90 Tage darf nicht erscheinen"
        );

        let recent = rows
            .iter()
            .find(|r| r.twitch_login == "recent_nonpartner")
            .unwrap();
        assert!(
            !recent.is_partner,
            "Recent Non-Partner muss isPartner=false haben"
        );
        let partner = rows.iter().find(|r| r.twitch_login == "partner_x").unwrap();
        assert!(partner.is_partner);
    }
}
