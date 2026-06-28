//! DB-Schicht für die Telemetry-Routen (live-announcements + link-clicks).
//!
//! Portiert aus `bot/dashboard/mixin.py` (`_dashboard_live_active_announcements`,
//! `_dashboard_live_link_click`) und `bot/storage/pg.py` (DDL).
//!
//! Alle Felder entsprechen exakt dem Postgres-Schema:
//! - `twitch_live_state`: TEXT-Spalten; `is_live`/`last_viewer_count`/
//!   `active_session_id` als INTEGER; `had_deadlock_in_session` als INTEGER.
//! - `twitch_link_clicks`: TEXT-Spalten; `id` SERIAL.
//!
//! Kein HTTP, kein Serde — nur reine Query-Logik.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

// ── Typen ─────────────────────────────────────────────────────────────────────

/// Ein normalisierter Live-Announcement-Eintrag (Parität zu
/// `normalize_live_announcement_item` in `policy.py`).
#[derive(Debug, sqlx::FromRow)]
pub struct LiveAnnouncementRow {
    pub streamer_login: String,
    /// message_id als TEXT aus der DB; muss zu i64 parsebar sein.
    pub last_discord_message_id: Option<String>,
    pub last_tracking_token: Option<String>,
}

// ── Live Active Announcements ─────────────────────────────────────────────────

/// Lädt alle aktiven Announcements aus `twitch_live_state`.
///
/// Parität zu `_dashboard_live_active_announcements` in `bot/dashboard/mixin.py`:
/// - nur Zeilen mit `last_discord_message_id IS NOT NULL` und
///   `last_tracking_token IS NOT NULL`
/// - ORDER BY LOWER(streamer_login)
pub async fn list_active_announcements(
    pool: &PgPool,
) -> Result<Vec<LiveAnnouncementRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT ls.streamer_login,
               ls.last_discord_message_id,
               ls.last_tracking_token
        FROM twitch_live_state ls
        WHERE ls.last_discord_message_id IS NOT NULL
          AND ls.last_tracking_token IS NOT NULL
        ORDER BY LOWER(ls.streamer_login)
        "#,
    )
    .fetch_all(pool)
    .await
}

// ── Link Click Persist ─────────────────────────────────────────────────────────

/// Schreibt einen Link-Click in `twitch_link_clicks`.
///
/// Parität zu `_dashboard_live_link_click` in `bot/dashboard/mixin.py`:
/// - `clicked_at` als TIMESTAMPTZ (sqlx bindet DateTime<Utc> direkt)
/// - `ref_code` aus Umgebung / Konstante ("DE-Deadlock-Discord")
/// - alle IDs als TEXT (guild_id nullable)
#[allow(clippy::too_many_arguments)]
pub async fn insert_link_click(
    pool: &PgPool,
    clicked_at: DateTime<Utc>,
    streamer_login: &str,
    tracking_token: &str,
    discord_user_id: &str,
    discord_username: &str,
    guild_id: Option<&str>,
    channel_id: &str,
    message_id: &str,
    ref_code: Option<&str>,
    source_hint: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO twitch_link_clicks (
            clicked_at,
            streamer_login,
            tracking_token,
            discord_user_id,
            discord_username,
            guild_id,
            channel_id,
            message_id,
            ref_code,
            source_hint
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(clicked_at)
    .bind(streamer_login)
    .bind(tracking_token)
    .bind(discord_user_id)
    .bind(discord_username)
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_id)
    .bind(ref_code)
    .bind(source_hint)
    .execute(pool)
    .await?;
    Ok(())
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

    /// Schema-isolierter Pool mit prod-treuer DDL für die relevanten Tabellen.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");

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
            .expect("search_path");

        // Parität zu bot/storage/pg.py DDL
        sqlx::query(
            r#"
            CREATE TABLE twitch_live_state (
                twitch_user_id              TEXT PRIMARY KEY,
                streamer_login              TEXT NOT NULL,
                last_stream_id              TEXT,
                last_started_at             TEXT,
                last_title                  TEXT,
                last_game_id                TEXT,
                last_discord_message_id     TEXT,
                last_notified_at            TEXT,
                is_live                     INTEGER DEFAULT 0,
                last_seen_at                TEXT,
                last_game                   TEXT,
                last_viewer_count           INTEGER DEFAULT 0,
                last_tracking_token         TEXT,
                active_session_id           BIGINT,
                had_deadlock_in_session     INTEGER DEFAULT 0,
                last_deadlock_seen_at       TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");

        sqlx::query(
            r#"
            CREATE TABLE twitch_link_clicks (
                id               SERIAL PRIMARY KEY,
                clicked_at       TIMESTAMPTZ DEFAULT NOW(),
                streamer_login   TEXT NOT NULL,
                tracking_token   TEXT,
                discord_user_id  TEXT,
                discord_username TEXT,
                guild_id         TEXT,
                channel_id       TEXT,
                message_id       TEXT,
                ref_code         TEXT,
                source_hint      TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_link_clicks");

        pool
    }

    #[tokio::test]
    async fn list_active_announcements_leer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_tr_list_leer").await;

        let rows = list_active_announcements(&pool).await.expect("query");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn list_active_announcements_filtert_nulls() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_tr_list_filter").await;

        // Zeile mit NULL message_id → darf nicht erscheinen
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_discord_message_id, last_tracking_token) VALUES ($1,$2,$3,$4)"
        )
        .bind("uid_a").bind("streamer_a").bind::<Option<String>>(None).bind("tok_a")
        .execute(&pool).await.expect("insert a");

        // Zeile mit NULL tracking_token → darf nicht erscheinen
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_discord_message_id, last_tracking_token) VALUES ($1,$2,$3,$4)"
        )
        .bind("uid_b").bind("streamer_b").bind("999").bind::<Option<String>>(None)
        .execute(&pool).await.expect("insert b");

        // Valide Zeile → muss erscheinen
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_discord_message_id, last_tracking_token) VALUES ($1,$2,$3,$4)"
        )
        .bind("uid_c").bind("streamer_c").bind("12345").bind("tok_c")
        .execute(&pool).await.expect("insert c");

        let rows = list_active_announcements(&pool).await.expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].streamer_login, "streamer_c");
        assert_eq!(rows[0].last_discord_message_id.as_deref(), Some("12345"));
        assert_eq!(rows[0].last_tracking_token.as_deref(), Some("tok_c"));
    }

    #[tokio::test]
    async fn list_active_announcements_benoetigt_keine_config_tabelle() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_tr_list_no_config").await;

        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_discord_message_id, last_tracking_token) VALUES ($1,$2,$3,$4)"
        )
        .bind("uid_x").bind("streamer_x").bind("555").bind("tok_x")
        .execute(&pool).await.expect("insert live");

        let rows = list_active_announcements(&pool).await.expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].streamer_login, "streamer_x");
        assert_eq!(rows[0].last_discord_message_id.as_deref(), Some("555"));
        assert_eq!(rows[0].last_tracking_token.as_deref(), Some("tok_x"));
    }

    #[tokio::test]
    async fn list_active_announcements_geordnet() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_tr_list_order").await;

        for (uid, login, msg, tok) in [
            ("u1", "zebra_streamer", "1", "t1"),
            ("u2", "alpha_streamer", "2", "t2"),
        ] {
            sqlx::query(
                "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_discord_message_id, last_tracking_token) VALUES ($1,$2,$3,$4)"
            )
            .bind(uid).bind(login).bind(msg).bind(tok)
            .execute(&pool).await.expect("insert");
        }

        let rows = list_active_announcements(&pool).await.expect("query");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].streamer_login, "alpha_streamer", "ORDER BY LOWER(streamer_login)");
        assert_eq!(rows[1].streamer_login, "zebra_streamer");
    }

    #[tokio::test]
    async fn insert_link_click_schreibt_alle_felder() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_tr_click_felder").await;

        insert_link_click(
            &pool,
            chrono::DateTime::parse_from_rfc3339("2026-06-11T12:00:00+00:00").unwrap().with_timezone(&chrono::Utc),
            "dragscope",
            "tok_abc",
            "123456789",
            "CoolUser",
            Some("987654321"),
            "111111111",
            "222222222",
            Some("DE-Deadlock-Discord"),
            "discord_button",
        )
        .await
        .expect("insert");

        let row: (String, String, String, String, Option<String>, String, String, Option<String>, String) = sqlx::query_as(
            "SELECT streamer_login, tracking_token, discord_user_id, discord_username, guild_id, channel_id, message_id, ref_code, source_hint FROM twitch_link_clicks LIMIT 1"
        )
        .fetch_one(&pool)
        .await
        .expect("select");

        assert_eq!(row.0, "dragscope");
        assert_eq!(row.1, "tok_abc");
        assert_eq!(row.2, "123456789");
        assert_eq!(row.3, "CoolUser");
        assert_eq!(row.4.as_deref(), Some("987654321"));
        assert_eq!(row.5, "111111111");
        assert_eq!(row.6, "222222222");
        assert_eq!(row.7.as_deref(), Some("DE-Deadlock-Discord"));
        assert_eq!(row.8, "discord_button");
    }

    #[tokio::test]
    async fn insert_link_click_guild_id_nullable() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_tr_click_nullable").await;

        insert_link_click(
            &pool,
            chrono::DateTime::parse_from_rfc3339("2026-06-11T13:00:00+00:00").unwrap().with_timezone(&chrono::Utc),
            "streamer_y",
            "tok_xyz",
            "111",
            "User",
            None, // kein guild_id
            "222",
            "333",
            None,
            "some_source",
        )
        .await
        .expect("insert");

        let (guild_id, ref_code): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT guild_id, ref_code FROM twitch_link_clicks LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("select");

        assert!(guild_id.is_none());
        assert!(ref_code.is_none());
    }
}
