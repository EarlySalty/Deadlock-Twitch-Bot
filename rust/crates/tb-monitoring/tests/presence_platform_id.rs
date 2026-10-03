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

async fn wait_for_writer_lock(pool: &sqlx::PgPool, advisory: bool) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let query = if advisory {
            "SELECT EXISTS(SELECT 1 FROM pg_locks
             WHERE locktype='advisory' AND objid::bigint=74104 AND NOT granted)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity
             WHERE datname=current_database() AND wait_event_type='Lock'
               AND wait_event IN ('transactionid', 'tuple')
               AND query ILIKE '%twitch_chatter_rollup%')"
        };
        let waiting: bool = sqlx::query_scalar(query).fetch_one(pool).await.unwrap();
        if waiting {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Echter Writer-Lock fehlt"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

async fn chat_poller_race(chat_first: bool) {
    use sqlx::postgres::PgPoolOptions;
    use tb_analytics::community_points::{aggregate_day, berlin_day, list_viewer_points};
    use tb_chat::chatter_tracking::ChatterTracker;
    use tb_chat::types::{ChatMessageBody, ChatMessageEvent};

    let mut db = test_postgres::TestPostgres::start_with_timescaledb().await;
    db.pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with((*db.pool.connect_options()).clone())
        .await
        .unwrap();
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations")
        .run(&db.pool)
        .await
        .unwrap();
    let start = Utc::now() - chrono::Duration::minutes(10);
    sqlx::raw_sql("INSERT INTO twitch_streamers(twitch_login,twitch_user_id) VALUES ('alpha','100');
        INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES ('100','alpha','active');")
        .execute(&db.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_stream_sessions(id,streamer_login,started_at,twitch_user_id)
        VALUES (42,'alpha',$1,'100')",
    )
    .bind(start)
    .execute(&db.pool)
    .await
    .unwrap();
    let streamer = LiveStreamer {
        twitch_user_id: "100".into(),
        streamer_login: "alpha".into(),
        active_session_id: 42,
        is_partner_active: true,
    };
    let viewers = vec![("alice".to_owned(), Some("123".to_owned()))];
    let tick = Utc::now();
    record_chatters_for_streamer(
        &db.pool,
        &streamer,
        &viewers,
        None,
        tick - chrono::Duration::minutes(2),
    )
    .await
    .unwrap();
    let day = berlin_day(tick);
    aggregate_day(&db.pool, day, tick).await.unwrap();
    let before = list_viewer_points(&db.pool, None, 100).await.unwrap();
    let cursor =
        chrono::DateTime::parse_from_rfc3339(before.next_updated_since.as_deref().unwrap())
            .unwrap()
            .with_timezone(&Utc);
    sqlx::raw_sql(
        "CREATE FUNCTION fixture_writer_pause() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_advisory_xact_lock(74104); RETURN NULL; END $$;",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    let trigger = if chat_first {
        "CREATE TRIGGER fixture_writer_pause AFTER UPDATE ON twitch_chatter_rollup
         FOR EACH ROW WHEN (NEW.total_messages=1) EXECUTE FUNCTION fixture_writer_pause()"
    } else {
        "CREATE TRIGGER fixture_writer_pause AFTER UPDATE ON twitch_chatter_rollup
         FOR EACH ROW WHEN (NEW.total_messages=0) EXECUTE FUNCTION fixture_writer_pause()"
    };
    sqlx::raw_sql(trigger).execute(&db.pool).await.unwrap();
    let mut pause = db.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(74104)")
        .execute(&mut *pause)
        .await
        .unwrap();
    let event = ChatMessageEvent {
        broadcaster_user_id: "100".into(),
        broadcaster_user_login: "alpha".into(),
        chatter_user_id: "123".into(),
        chatter_user_login: "alice".into(),
        message_id: "community-race-message".into(),
        message: ChatMessageBody {
            text: "Neue Nachricht während des Poller-Ticks".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let chat_pool = db.pool.clone();
    let spawn_chat =
        || tokio::spawn(async move { ChatterTracker::new(chat_pool).track(&event).await });
    let poller_pool = db.pool.clone();
    let spawn_poller = || {
        tokio::spawn(async move {
            record_chatters_for_streamer(&poller_pool, &streamer, &viewers, None, tick).await
        })
    };
    let (chat, poller) = if chat_first {
        let chat = spawn_chat();
        wait_for_writer_lock(&db.pool, true).await;
        let poller = spawn_poller();
        wait_for_writer_lock(&db.pool, false).await;
        (chat, poller)
    } else {
        let poller = spawn_poller();
        wait_for_writer_lock(&db.pool, true).await;
        let chat = spawn_chat();
        wait_for_writer_lock(&db.pool, false).await;
        (chat, poller)
    };
    pause.commit().await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), chat)
            .await
            .expect("Chat blockiert nach Freigabe")
            .unwrap()
            .is_some()
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), poller)
        .await
        .expect("Poller blockiert nach Freigabe")
        .unwrap()
        .unwrap();
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT
        (SELECT COUNT(*) FROM twitch_chat_messages WHERE message_id='community-race-message'),
        (SELECT COUNT(*) FROM twitch_viewer_presence_ticks WHERE viewer_twitch_user_id='123')",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        counts,
        (1, 2),
        "Beide Writer müssen vollständig persistieren"
    );
    aggregate_day(&db.pool, day, Utc::now()).await.unwrap();
    let after = list_viewer_points(&db.pool, Some(cursor), 100)
        .await
        .unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0].chat_messages, 1);
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_community_points_dirty_days WHERE day=$1")
            .bind(day)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(
        pending, 0,
        "Committed Rohgenerationen müssen bestätigt werden"
    );
}

#[tokio::test]
async fn chat_vor_poller_persistiert_ohne_lockzyklus_und_aktualisiert_cursor() {
    chat_poller_race(true).await;
}

#[tokio::test]
async fn poller_vor_chat_persistiert_ohne_lockzyklus_und_aktualisiert_cursor() {
    chat_poller_race(false).await;
}
