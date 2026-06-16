//! DB-Tests für ChatterTracker — twitch_session_chatters + twitch_chatter_rollup.
//!
//! Schema-isoliert, prod-treue DDL. Läuft nur wenn TB_TEST_DATABASE_URL gesetzt.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::chatter_tracking::ChatterTracker;
use tb_chat::types::{ChatMessageBody, ChatMessageEvent};

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        pool_in_schema(&dsn, $schema).await
    }};
}

async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .unwrap();
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    apply_ddl(&pool).await;
    pool
}

async fn apply_ddl(pool: &PgPool) {
    // twitch_streamers_partner_state (vereinfacht, nur benötigte Spalten)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_streamers_partner_state (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            is_partner_active INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_streamers (für monitored-only Gate)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_live_state — is_live = integer, last_game = text, active_session_id = bigint
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            is_live INTEGER NOT NULL DEFAULT 0,
            last_game TEXT,
            active_session_id BIGINT
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_chatter_rollup — total_messages/total_sessions = integer
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_chatter_rollup (
            streamer_login TEXT NOT NULL,
            chatter_login  TEXT NOT NULL,
            chatter_id     TEXT,
            first_seen_at  TIMESTAMPTZ NOT NULL,
            last_seen_at   TIMESTAMPTZ NOT NULL,
            total_messages INTEGER NOT NULL DEFAULT 0,
            total_sessions INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (streamer_login, chatter_login)
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_session_chatters — messages/is_first_time_streamer/seen_via_chatters_api = bool
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_session_chatters (
            session_id              BIGINT NOT NULL,
            streamer_login          TEXT NOT NULL,
            chatter_login           TEXT NOT NULL,
            chatter_id              TEXT,
            first_message_at        TIMESTAMPTZ,
            messages                INTEGER NOT NULL DEFAULT 0,
            is_first_time_streamer  BOOLEAN,
            seen_via_chatters_api   BOOLEAN NOT NULL DEFAULT FALSE,
            last_seen_at            TIMESTAMPTZ,
            PRIMARY KEY (session_id, chatter_login)
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_stream_sessions — Session-Resolver liest die offene Session (bot.py Z. 2168)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
            id             BIGINT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            started_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            ended_at       TIMESTAMPTZ,
            game_name      TEXT
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_chat_messages — Roh-Nachrichten (Prod: message_ts = timestamptz)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_chat_messages (
            id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
            session_id     BIGINT,
            streamer_login TEXT,
            chatter_login  TEXT,
            chatter_id     TEXT,
            message_id     TEXT,
            message_ts     TIMESTAMPTZ,
            is_command     BOOLEAN,
            content        TEXT
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    // twitch_raw_chat_ingest_health — Prod: ALLE Timestamp-Spalten sind TEXT!
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_raw_chat_ingest_health (
            streamer_login                TEXT PRIMARY KEY,
            last_raw_chat_message_at      TEXT,
            last_raw_chat_insert_ok_at    TEXT,
            last_raw_chat_insert_error_at TEXT,
            last_raw_chat_error           TEXT,
            raw_chat_lag_seconds          INTEGER,
            updated_at                    TEXT
        )",
    )
    .execute(pool)
    .await
    .unwrap();
}

fn make_event(broadcaster_login: &str, chatter_login: &str) -> ChatMessageEvent {
    ChatMessageEvent {
        broadcaster_user_id: "broadcaster-id-1".to_string(),
        broadcaster_user_login: broadcaster_login.to_string(),
        broadcaster_user_name: broadcaster_login.to_string(),
        chatter_user_id: "chatter-id-1".to_string(),
        chatter_user_login: chatter_login.to_string(),
        chatter_user_name: chatter_login.to_string(),
        message_id: "msg-1".to_string(),
        message: ChatMessageBody {
            text: "Hallo Kanal".to_string(),
            fragments: vec![],
        },
        badges: vec![],
        color: String::new(),
        ..Default::default()
    }
}

/// Hilfsfunktion: Partner + Live-State + offene Session für einen Kanal anlegen.
async fn setup_active_partner(pool: &PgPool, login: &str, session_id: i64) {
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active)
         VALUES ($1, 1) ON CONFLICT DO NOTHING",
    )
    .bind(login)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game, active_session_id)
         VALUES ($1, $2, 1, 'deadlock', $3)
         ON CONFLICT (twitch_user_id) DO UPDATE
         SET is_live = 1, last_game = 'deadlock', active_session_id = $3",
    )
    .bind(format!("uid-{login}"))
    .bind(login)
    .bind(session_id)
    .execute(pool)
    .await
    .unwrap();

    open_session(pool, login, session_id).await;
}

/// Offene Session in twitch_stream_sessions anlegen (Resolver-Quelle).
async fn open_session(pool: &PgPool, login: &str, session_id: i64) {
    sqlx::query(
        "INSERT INTO twitch_stream_sessions (id, streamer_login, game_name)
         VALUES ($1, $2, 'Deadlock') ON CONFLICT DO NOTHING",
    )
    .bind(session_id)
    .bind(login)
    .execute(pool)
    .await
    .unwrap();
}

/// Offene Session beenden.
async fn end_session(pool: &PgPool, session_id: i64) {
    sqlx::query("UPDATE twitch_stream_sessions SET ended_at = NOW() WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await
        .unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn erstmalige_nachricht_legt_session_und_rollup_an() {
    let pool = pool_or_skip!("ct_first_msg");
    setup_active_partner(&pool, "streamer1", 100).await;
    let tracker = ChatterTracker::new(pool.clone());

    let event = make_event("streamer1", "neuerchatter");
    tracker.track(&event).await;

    // Rollup muss existieren
    let (msgs, sessions): (i32, i32) = sqlx::query_as(
        "SELECT total_messages, total_sessions FROM twitch_chatter_rollup
         WHERE streamer_login = 'streamer1' AND chatter_login = 'neuerchatter'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(msgs, 1);
    assert_eq!(sessions, 1);

    // Session-Chatters muss existieren
    let (sc_msgs, first_time): (i32, Option<bool>) = sqlx::query_as(
        "SELECT messages, is_first_time_streamer FROM twitch_session_chatters
         WHERE session_id = 100 AND chatter_login = 'neuerchatter'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(sc_msgs, 1);
    // Erstmalig → is_first_time_streamer = true
    assert_eq!(first_time, Some(true));

    // Roh-Nachricht muss persistiert sein (moderation.py Z. 2214–2238)
    let (raw_content, raw_is_command): (String, bool) = sqlx::query_as(
        "SELECT content, is_command FROM twitch_chat_messages
         WHERE session_id = 100 AND chatter_login = 'neuerchatter'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(raw_content, "Hallo Kanal");
    assert!(!raw_is_command);

    // Health-Heartbeat: message_at + insert_ok_at gesetzt, Fehler NULL,
    // Format = ISO-Sekunden (TEXT-Spalte, Python isoformat timespec=seconds)
    let (msg_at, ok_at, err): (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT last_raw_chat_message_at, last_raw_chat_insert_ok_at, last_raw_chat_error
         FROM twitch_raw_chat_ingest_health WHERE streamer_login = 'streamer1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let msg_at = msg_at.expect("message_at gesetzt");
    assert!(
        msg_at.ends_with("+00:00") && msg_at.len() == 25,
        "ISO-Sekunden: {msg_at}"
    );
    assert!(ok_at.is_some(), "insert_ok_at gesetzt");
    assert!(err.is_none(), "kein Fehler");
}

#[tokio::test]
async fn zweite_nachricht_inkrementiert_zaehler() {
    let pool = pool_or_skip!("ct_second_msg");
    setup_active_partner(&pool, "streamer2", 200).await;
    let tracker = ChatterTracker::new(pool.clone());

    let event = make_event("streamer2", "bekannterchatter");
    tracker.track(&event).await;
    tracker.track(&event).await;

    let (msgs, sessions): (i32, i32) = sqlx::query_as(
        "SELECT total_messages, total_sessions FROM twitch_chatter_rollup
         WHERE streamer_login = 'streamer2' AND chatter_login = 'bekannterchatter'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(msgs, 2, "Rollup: 2 Nachrichten");
    // Beide Nachrichten in gleicher Session → sessions bleibt 1
    assert_eq!(sessions, 1, "Rollup: nur 1 Session (gleiche session_id)");

    let sc_msgs: i32 = sqlx::query_scalar(
        "SELECT messages FROM twitch_session_chatters
         WHERE session_id = 200 AND chatter_login = 'bekannterchatter'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(sc_msgs, 2);
}

#[tokio::test]
async fn known_bot_wird_ignoriert() {
    let pool = pool_or_skip!("ct_known_bot");
    setup_active_partner(&pool, "streamer3", 300).await;
    let tracker = ChatterTracker::new(pool.clone());

    // nightbot ist ein known chat bot (chat_bots.py Z. 13)
    let event = make_event("streamer3", "nightbot");
    tracker.track(&event).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_rollup WHERE chatter_login = 'nightbot'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "known bot darf nicht getrackt werden");
}

#[tokio::test]
async fn non_deadlock_game_wird_ignoriert() {
    let pool = pool_or_skip!("ct_wrong_game");
    // Partner anlegen aber falsches Spiel
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active)
         VALUES ('wronggame', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game, active_session_id)
         VALUES ('uid-wronggame', 'wronggame', 1, 'valorant', 400)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Offene Session existiert — der Skip muss am GAME-Gate passieren, nicht am Session-Gate
    open_session(&pool, "wronggame", 400).await;

    let tracker = ChatterTracker::with_persist_all_games(pool.clone(), false);
    let event = make_event("wronggame", "einchatter");
    tracker.track(&event).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_rollup WHERE streamer_login = 'wronggame'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "Nicht-Deadlock-Kanal wird nicht getrackt");

    // Aber der Health-Heartbeat (message_at) wurde geschrieben (Python Z. 2193)
    let hb: Option<String> = sqlx::query_scalar(
        "SELECT last_raw_chat_message_at FROM twitch_raw_chat_ingest_health
         WHERE streamer_login = 'wronggame'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hb.is_some(), "Heartbeat trotz Game-Gate-Block");
}

#[tokio::test]
async fn kein_partner_wird_ignoriert() {
    let pool = pool_or_skip!("ct_no_partner");
    // Kein Eintrag in partner_state → nicht getrackt
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game, active_session_id)
         VALUES ('uid-nop', 'nop', 1, 'deadlock', 500)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let tracker = ChatterTracker::new(pool.clone());
    let event = make_event("nop", "jemand");
    tracker.track(&event).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_rollup WHERE streamer_login = 'nop'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "Kein Partner → nicht getrackt");
}

#[tokio::test]
async fn neue_session_inkrementiert_total_sessions() {
    let pool = pool_or_skip!("ct_new_session");
    setup_active_partner(&pool, "streamer4", 600).await;
    let tracker = ChatterTracker::new(pool.clone());

    // Erste Nachricht in Session 600
    let event = make_event("streamer4", "chatterx");
    tracker.track(&event).await;

    // Session-Wechsel: 600 beenden, 601 öffnen. Der Tracker cached die
    // aufgelöste Session 60s (wie Python) — frische Instanz simuliert den
    // Cache-Ablauf.
    end_session(&pool, 600).await;
    open_session(&pool, "streamer4", 601).await;
    let tracker = ChatterTracker::new(pool.clone());

    // Zweite Nachricht in Session 601
    let event2 = make_event("streamer4", "chatterx");
    tracker.track(&event2).await;

    let (msgs, sessions): (i32, i32) = sqlx::query_as(
        "SELECT total_messages, total_sessions FROM twitch_chatter_rollup
         WHERE streamer_login = 'streamer4' AND chatter_login = 'chatterx'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(msgs, 2);
    assert_eq!(sessions, 2, "Neue Session → total_sessions = 2");
}
