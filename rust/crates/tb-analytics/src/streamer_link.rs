//! DB-Queries für die Streamer-Discord-Verknüpfung.
//!
//! Liefert alle nicht-archivierten Streamer ohne Discord-Verknüpfung — Quelle
//! für den automatischen Discord-Namens-Abgleich (Matcher im Discord-Bot).
//!
//! Parität zu `bot/storage/pg.py::list_unlinked_streamers`.
//! Kein HTTP, kein Serde.

use sqlx::PgPool;

/// Ein Eintrag aus der Link-Kandidaten-Liste.
#[derive(Debug, sqlx::FromRow)]
pub struct UnlinkedStreamer {
    pub twitch_login: String,
    /// NULL wenn weder `s.twitch_user_id` noch `i.twitch_user_id` gesetzt ist.
    pub twitch_user_id: Option<String>,
    /// `COALESCE(s.is_monitored_only, 0)` — immer 0 oder 1, nie NULL.
    pub is_monitored_only: i32,
}

/// Aktive Partner ohne Discord-Verknüpfung.
///
/// Nur Streamer, die in `twitch_partners` als aktiver Partner geführt werden
/// (nicht departnered, nicht admin-archived), werden zurückgegeben.
/// Rein gescrapte Kanäle oder allgemeine `twitch_streamers`-Einträge ohne
/// Partnerstatus erscheinen hier nicht — der Matcher soll nur echte Partner
/// automatisch verknüpfen und darüber berichten.
pub async fn list_unlinked(pool: &PgPool) -> Result<Vec<UnlinkedStreamer>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT s.twitch_login,
               COALESCE(NULLIF(s.twitch_user_id, ''), i.twitch_user_id) AS twitch_user_id,
               COALESCE(s.is_monitored_only, 0)                         AS is_monitored_only
          FROM twitch_streamers s
         INNER JOIN twitch_partners tp
            ON tp.twitch_login = s.twitch_login
           AND tp.departnered_at IS NULL
           AND tp.admin_archived_at IS NULL
          LEFT JOIN twitch_streamer_identities i
            ON i.twitch_user_id = s.twitch_user_id
         WHERE (s.discord_user_id IS NULL OR s.discord_user_id = '')
           AND (i.discord_user_id IS NULL OR i.discord_user_id = '')
           AND s.archived_at IS NULL
         ORDER BY s.twitch_login
        "#,
    )
    .fetch_all(pool)
    .await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    /// Schema-isolierter Pool mit prod-treuer DDL.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");

        // prod-treue DDL: alle Spaltentypen wie in bot/storage/pg.py
        sqlx::query(
            r#"
            CREATE TABLE twitch_streamers (
                twitch_login        TEXT PRIMARY KEY,
                twitch_user_id      TEXT,
                discord_user_id     TEXT,
                discord_display_name TEXT,
                is_on_discord       INTEGER DEFAULT 0,
                created_at          TEXT DEFAULT CURRENT_TIMESTAMP,
                archived_at         TEXT,
                is_monitored_only   INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers");

        sqlx::query(
            r#"
            CREATE TABLE twitch_streamer_identities (
                twitch_user_id       TEXT PRIMARY KEY,
                twitch_login         TEXT NOT NULL,
                discord_user_id      TEXT,
                discord_display_name TEXT,
                is_on_discord        INTEGER DEFAULT 0,
                created_at           TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at           TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamer_identities");

        pool
    }

    #[tokio::test]
    async fn leer_gibt_leere_liste() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_leer").await;
        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn ohne_discord_id_erscheint_in_liste() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_erscheint").await;

        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ($1, $2)",
        )
        .bind("streamer_a")
        .bind("uid_a")
        .execute(&pool)
        .await
        .expect("insert");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].twitch_login, "streamer_a");
        assert_eq!(rows[0].twitch_user_id.as_deref(), Some("uid_a"));
        assert_eq!(rows[0].is_monitored_only, 0);
    }

    #[tokio::test]
    async fn mit_discord_id_in_streamers_wird_ausgeblendet() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_discord_in_streamers").await;

        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, discord_user_id) VALUES ($1, $2, $3)",
        )
        .bind("verknuepft")
        .bind("uid_v")
        .bind("discord_123")
        .execute(&pool)
        .await
        .expect("insert");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert!(rows.is_empty(), "verknüpfter Streamer darf nicht erscheinen");
    }

    #[tokio::test]
    async fn mit_discord_id_in_identities_wird_ausgeblendet() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_discord_in_identities").await;

        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ($1, $2)",
        )
        .bind("identity_linked")
        .bind("uid_il")
        .execute(&pool)
        .await
        .expect("insert streamer");

        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id) VALUES ($1, $2, $3)",
        )
        .bind("uid_il")
        .bind("identity_linked")
        .bind("discord_456")
        .execute(&pool)
        .await
        .expect("insert identity");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert!(
            rows.is_empty(),
            "Streamer mit discord_user_id in identities darf nicht erscheinen"
        );
    }

    #[tokio::test]
    async fn archivierter_streamer_wird_ausgeblendet() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_archiviert").await;

        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, archived_at) VALUES ($1, $2)",
        )
        .bind("alt_archiv")
        .bind("2026-01-01 00:00:00")
        .execute(&pool)
        .await
        .expect("insert archiviert");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert!(rows.is_empty(), "archivierter Streamer darf nicht erscheinen");
    }

    #[tokio::test]
    async fn is_monitored_only_default_null_wird_zu_null() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_monitored").await;

        // is_monitored_only explizit NULL setzen (override Default)
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, is_monitored_only) VALUES ($1, NULL)",
        )
        .bind("monitor_streamer")
        .execute(&pool)
        .await
        .expect("insert monitored");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert_eq!(rows.len(), 1);
        // COALESCE(NULL, 0) = 0
        assert_eq!(
            rows[0].is_monitored_only, 0,
            "NULL is_monitored_only muss zu 0 werden"
        );
    }

    #[tokio::test]
    async fn is_monitored_only_1_wird_weitergegeben() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_monitored_1").await;

        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, is_monitored_only) VALUES ($1, 1)",
        )
        .bind("nur_monitor")
        .execute(&pool)
        .await
        .expect("insert is_monitored_only=1");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].is_monitored_only, 1);
    }

    #[tokio::test]
    async fn twitch_user_id_null_kein_falscher_identity_join() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_null_uid").await;

        // Streamer ohne user_id, keine identity
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login) VALUES ($1)",
        )
        .bind("kein_uid")
        .execute(&pool)
        .await
        .expect("insert ohne uid");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].twitch_login, "kein_uid");
        assert!(rows[0].twitch_user_id.is_none(), "twitch_user_id muss None sein");
    }

    #[tokio::test]
    async fn reihenfolge_alphabetisch() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_order").await;

        for login in ["z_streamer", "a_streamer", "m_streamer"] {
            sqlx::query("INSERT INTO twitch_streamers (twitch_login) VALUES ($1)")
                .bind(login)
                .execute(&pool)
                .await
                .expect("insert");
        }

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].twitch_login, "a_streamer");
        assert_eq!(rows[1].twitch_login, "m_streamer");
        assert_eq!(rows[2].twitch_login, "z_streamer");
    }

    #[tokio::test]
    async fn leer_discord_id_string_gilt_als_unverknuepft() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sl_leer_discord").await;

        // discord_user_id = '' soll als unverknüpft gelten (Parität zu Python: OR = '')
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, discord_user_id) VALUES ($1, $2)",
        )
        .bind("leer_discord")
        .bind("")
        .execute(&pool)
        .await
        .expect("insert leer discord_user_id");

        let rows = list_unlinked(&pool).await.expect("list_unlinked");
        assert_eq!(rows.len(), 1, "leerer discord_user_id-String gilt als unverknüpft");
    }
}
