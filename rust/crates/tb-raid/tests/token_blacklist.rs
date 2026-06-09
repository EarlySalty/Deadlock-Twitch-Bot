//! Hermetische Tests des Token-Lockout-Stores (`twitch_token_blacklist`,
//! Alt-Stil: TEXT-Timestamps, INTEGER-Flags, error_count DEFAULT 1).

use std::str::FromStr;

use chrono::{Duration, SecondsFormat, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::token_refresher::TokenBlacklist;
use tb_raid::TokenBlacklistStore;

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
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
    // Prod-Typen: TEXT-Timestamps, INTEGER-Flags, error_count DEFAULT 1.
    sqlx::query(
        "CREATE TABLE twitch_token_blacklist (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, error_message TEXT,
            error_count INTEGER DEFAULT 1, first_error_at TEXT, last_error_at TEXT,
            notified INTEGER DEFAULT 0, grace_expires_at TEXT,
            user_dm_sent INTEGER DEFAULT 0, reminder_sent INTEGER DEFAULT 0,
            role_removed INTEGER DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn iso_ago(hours: i64) -> String {
    (Utc::now() - Duration::hours(hours)).to_rfc3339_opts(SecondsFormat::Secs, false)
}

#[tokio::test]
async fn add_legt_an_inkrementiert_und_resettet_nach_fenster() {
    let pool = pool_or_skip!("t6b_bl_add");
    let store = TokenBlacklistStore::new(pool.clone());

    // Neuer Eintrag → error_count DEFAULT 1, Grace gesetzt.
    store.add_to_blacklist("42", "drag", "boom").await;
    let (count, grace): (i32, Option<String>) = sqlx::query_as(
        "SELECT error_count, grace_expires_at FROM twitch_token_blacklist WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert!(grace.is_some());

    // Zweiter Fehler innerhalb des Fensters → Counter +1.
    store.add_to_blacklist("42", "drag", "boom2").await;
    let count: i32 = sqlx::query_scalar(
        "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);

    // Letzter Fehler > 12 h her → Reset auf 1.
    sqlx::query("UPDATE twitch_token_blacklist SET error_count=2, last_error_at=$1 WHERE twitch_user_id='42'")
        .bind(iso_ago(13))
        .execute(&pool)
        .await
        .unwrap();
    store.add_to_blacklist("42", "drag", "spaeter").await;
    let count: i32 = sqlx::query_scalar(
        "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "ausserhalb 12h-Fenster → Reset");
}

#[tokio::test]
async fn is_blacklisted_ab_schwelle_und_recent_failure_im_cooldown() {
    let pool = pool_or_skip!("t6b_bl_checks");
    let store = TokenBlacklistStore::new(pool.clone());

    // error_count 2, letzter Fehler 1 h her → recent failure, NICHT blacklisted.
    sqlx::query(
        "INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login, error_count, last_error_at)
         VALUES ('42', 'drag', 2, $1)",
    )
    .bind(iso_ago(1))
    .execute(&pool)
    .await
    .unwrap();
    assert!(!store.is_blacklisted("42").await);
    assert!(store.has_recent_failure("42").await);

    // Letzter Fehler 3 h her (> 2h Cooldown) → kein recent failure.
    sqlx::query("UPDATE twitch_token_blacklist SET last_error_at=$1 WHERE twitch_user_id='42'")
        .bind(iso_ago(3))
        .execute(&pool)
        .await
        .unwrap();
    assert!(!store.has_recent_failure("42").await);

    // error_count 3 → blacklisted; recent_failure dann false (separat behandelt).
    sqlx::query("UPDATE twitch_token_blacklist SET error_count=3, last_error_at=$1 WHERE twitch_user_id='42'")
        .bind(iso_ago(1))
        .execute(&pool)
        .await
        .unwrap();
    assert!(store.is_blacklisted("42").await);
    assert!(
        !store.has_recent_failure("42").await,
        "voll blacklisted → kein recent-failure-Cooldown"
    );

    // Unbekannt → beides false.
    assert!(!store.is_blacklisted("x").await);
    assert!(!store.has_recent_failure("x").await);
}

#[tokio::test]
async fn clear_loescht_den_eintrag() {
    let pool = pool_or_skip!("t6b_bl_clear");
    let store = TokenBlacklistStore::new(pool.clone());
    store.add_to_blacklist("42", "drag", "boom").await;
    store.clear_failure_count("42").await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_token_blacklist")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
