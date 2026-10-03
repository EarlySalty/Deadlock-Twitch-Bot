//! Prüft die Helix-ID beim Schreiben und den atomaren Fehlerpfad.
#[path = "../../../test-support/postgres.rs"]
mod test_postgres;
use chrono::Utc;
use tb_monitoring::{chatters_poller::LiveStreamer, record_chatters_for_streamer};
#[tokio::test]
async fn presence_schreibt_helix_id_und_rollt_bei_tickfehler_zurueck() {
    let db = test_postgres::TestPostgres::start().await;
    for ddl in [
        "CREATE TABLE twitch_session_chatters (
            session_id BIGINT NOT NULL,
            streamer_login TEXT NOT NULL,
            chatter_login TEXT NOT NULL,
            chatter_id TEXT,
            first_message_at TIMESTAMPTZ NOT NULL,
            messages INTEGER NOT NULL DEFAULT 0,
            is_first_time_streamer BOOLEAN NOT NULL DEFAULT FALSE,
            seen_via_chatters_api BOOLEAN NOT NULL DEFAULT FALSE,
            last_seen_at TIMESTAMPTZ,
            confirmed_first_ever BOOLEAN NOT NULL DEFAULT FALSE,
            PRIMARY KEY (session_id, chatter_login)
        )",
        "CREATE TABLE twitch_chatter_rollup (
            streamer_login TEXT NOT NULL,
            chatter_login TEXT NOT NULL,
            chatter_id TEXT,
            first_seen_at TIMESTAMPTZ NOT NULL,
            last_seen_at TIMESTAMPTZ NOT NULL,
            total_messages INTEGER NOT NULL DEFAULT 0,
            total_sessions INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (streamer_login, chatter_login)
        )",
        "CREATE TABLE twitch_viewer_presence_ticks (
            session_id BIGINT NOT NULL,
            streamer_login TEXT NOT NULL,
            viewer_login TEXT NOT NULL,
            viewer_twitch_user_id TEXT,
            tick_at TIMESTAMPTZ NOT NULL,
            PRIMARY KEY (session_id, viewer_login, tick_at)
        )",
    ] {
        sqlx::query(ddl).execute(&db.pool).await.unwrap();
    }
    let streamer = LiveStreamer {
        twitch_user_id: "100".into(),
        streamer_login: "alpha".into(),
        active_session_id: 42,
        is_partner_active: true,
    };
    let now = Utc::now();
    let viewers = vec![("alice".into(), Some("123".into()))];
    record_chatters_for_streamer(&db.pool, &streamer, &viewers, None, now)
        .await
        .unwrap();
    let id: String =
        sqlx::query_scalar("SELECT viewer_twitch_user_id FROM twitch_viewer_presence_ticks")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(id, "123");
    sqlx::query("ALTER TABLE twitch_viewer_presence_ticks ADD CONSTRAINT reject_bob CHECK (viewer_login <> 'bob')").execute(&db.pool).await.unwrap();
    let rejected = vec![("bob".into(), Some("456".into()))];
    assert!(
        record_chatters_for_streamer(&db.pool, &streamer, &rejected, None, now)
            .await
            .is_err()
    );
    for table in ["twitch_session_chatters", "twitch_chatter_rollup"] {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table} WHERE chatter_login = 'bob'"
        )))
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }
}
