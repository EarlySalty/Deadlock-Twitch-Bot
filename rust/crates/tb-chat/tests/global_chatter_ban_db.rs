//! DB-Tests für GlobalChatterBanEnforcer — twitch_chatter_global_ban (Entscheidung).
//!
//! Die AKTION (Delete+Ban+Notice+Records) läuft seit dem Pipeline-Refactor über
//! `ModerationEngine::auto_ban_and_cleanup` (wie Python) und wird in den
//! moderation_db-Tests abgedeckt — hier wird nur die Entscheidung getestet.
//!
//! Schema-isoliert, prod-treue DDL. Läuft nur wenn TB_TEST_DATABASE_URL gesetzt.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_chat::global_chatter_ban::GlobalChatterBanEnforcer;
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
    for ddl in [
        // twitch_chatter_global_ban — added_at = TIMESTAMPTZ (Prod-Schema)
        "CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban (
            chatter_login  TEXT PRIMARY KEY,
            chatter_id     TEXT,
            reason         TEXT,
            added_by       TEXT,
            added_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
        "CREATE TABLE IF NOT EXISTS twitch_ban_events (
            id BIGSERIAL PRIMARY KEY,
            twitch_user_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            target_login TEXT,
            target_id TEXT,
            received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
        "CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban_applied (
            chatter_login TEXT NOT NULL,
            broadcaster_id TEXT NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (chatter_login, broadcaster_id)
        )",
    ] {
        sqlx::query(ddl).execute(pool).await.unwrap();
    }
}

fn make_event(chatter_login: &str, chatter_id: &str) -> ChatMessageEvent {
    ChatMessageEvent {
        broadcaster_user_id: "broadcaster-123".to_string(),
        broadcaster_user_login: "streamer".to_string(),
        broadcaster_user_name: "streamer".to_string(),
        chatter_user_id: chatter_id.to_string(),
        chatter_user_login: chatter_login.to_string(),
        chatter_user_name: chatter_login.to_string(),
        message_id: "msg-gcb-1".to_string(),
        message: ChatMessageBody {
            text: "hallo".to_string(),
            fragments: vec![],
        },
        badges: vec![],
        color: String::new(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nicht_gebannter_chatter_wird_durchgelassen() {
    let pool = pool_or_skip!("gcb_pass");
    let enforcer = GlobalChatterBanEnforcer::new(pool);
    let event = make_event("harmlosername", "uid-harmlos");
    assert!(
        !enforcer.is_banned(&event).await,
        "Nicht-gebannter Chatter soll nicht als gebannt gelten"
    );
}

#[tokio::test]
async fn gebannter_chatter_wird_erkannt() {
    let pool = pool_or_skip!("gcb_ban");

    // Chatter zur globalen Bannliste hinzufügen (pg.py Z. 4119–4134)
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason, added_by)
         VALUES ('boser_user', 'uid-boes', 'Test-Ban', 'test')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let enforcer = GlobalChatterBanEnforcer::new(pool);
    let event = make_event("boser_user", "uid-boes");
    assert_eq!(
        enforcer.ban_reason(&event).await.as_deref(),
        Some("Test-Ban")
    );
    assert!(
        enforcer.is_banned(&event).await,
        "Gebannter Chatter soll erkannt werden"
    );
}

#[tokio::test]
async fn ban_per_id_auch_ohne_login_treffer() {
    let pool = pool_or_skip!("gcb_ban_by_id");

    // Nur anhand der ID (anderer Login in DB) — pg.py Z. 4125–4128
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason, added_by)
         VALUES ('alter_login', 'uid-spezifisch', 'ID-Ban', 'test')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let enforcer = GlobalChatterBanEnforcer::new(pool);
    // Event hat anderen Login, aber gleiche ID
    let event = make_event("neuer_login", "uid-spezifisch");
    assert!(
        enforcer.is_banned(&event).await,
        "Ban per ID soll auch bei anderem Login greifen"
    );
}

#[tokio::test]
async fn positiver_treffer_wird_gecacht() {
    let pool = pool_or_skip!("gcb_cache");

    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason, added_by)
         VALUES ('gecachter', 'uid-cache', 'Cache-Test', 'test')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let enforcer = GlobalChatterBanEnforcer::new(pool.clone());
    let event = make_event("gecachter", "uid-cache");
    assert!(
        enforcer.is_banned(&event).await,
        "Erster Aufruf: DB-Treffer"
    );

    // Eintrag aus der DB löschen — der 300s-Positiv-Cache muss weiter greifen
    // (moderation.py Z. 736–738)
    sqlx::query("DELETE FROM twitch_chatter_global_ban WHERE chatter_login = 'gecachter'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        enforcer.is_banned(&event).await,
        "Zweiter Aufruf: Cache-Treffer trotz DB-Löschung"
    );
}

#[tokio::test]
async fn channel_unban_stoppt_sofort_reban_trotz_cache_bis_zum_sweep() {
    let pool = pool_or_skip!("gcb_unban_override");

    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason, added_by)
         VALUES ('alter_login', 'uid-unban', 'Global', 'test')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let enforcer = GlobalChatterBanEnforcer::new(pool.clone());
    let event = make_event("neuer_login", "uid-unban");
    assert!(
        enforcer.is_banned(&event).await,
        "Erster Treffer füllt den Cache"
    );

    sqlx::query(
        "INSERT INTO twitch_ban_events
             (twitch_user_id, event_type, target_login, target_id, received_at)
         VALUES
             ('broadcaster-123', 'ban', 'neuer_login', 'uid-unban', NOW() - INTERVAL '2 minutes'),
             ('broadcaster-123', 'unban', 'neuer_login', 'uid-unban', NOW() - INTERVAL '1 minute')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        !enforcer.is_banned(&event).await,
        "Streamer-Unban unterdrückt den Sofort-Reban auch bei positivem Cache"
    );

    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban_applied
             (chatter_login, broadcaster_id, applied_at)
         VALUES ('alter_login', 'broadcaster-123', NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        enforcer.is_banned(&event).await,
        "Nach Sweep-Applied darf der globale Ban wieder greifen"
    );
}
