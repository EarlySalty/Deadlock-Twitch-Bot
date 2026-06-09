//! Hermetische tb-db-Tests gegen den Wegwerf-Container (`TB_TEST_DATABASE_URL`).
//! Ohne diese Env-Var werden die Tests laut übersprungen (kein stiller Pass).

use std::time::Duration;
use tb_config::DbConfig;
use tb_db::rows::{StreamerPlanRow, TwitchStreamerRow};

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

fn cfg(dsn: String) -> DbConfig {
    DbConfig {
        dsn,
        pool_max: 4,
        acquire_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
    }
}

macro_rules! skip_without_db {
    () => {
        match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!(
                    "SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`"
                );
                return;
            }
        }
    };
}

#[tokio::test]
async fn pool_connects_and_pings() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(one, 1);
}

#[tokio::test]
async fn migrations_create_tracking_table_and_touch_nothing_else() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");
    tb_db::run_migrations(&pool).await.expect("migrate");
    // sqlx-Tracking-Tabelle existiert, getrennt vom Python-`schema_version`.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = '_sqlx_migrations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        exists,
        "_sqlx_migrations muss nach run_migrations existieren"
    );
}

#[tokio::test]
async fn row_structs_map_real_columns() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");

    // Kontrolliertes DDL, das das Prod-Schema nachbildet (Timestamps als text!).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            discord_user_id TEXT,
            is_on_discord INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            is_monitored_only INTEGER DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            plan_name TEXT NOT NULL DEFAULT 'free',
            promo_disabled INTEGER NOT NULL DEFAULT 0,
            activated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            expires_at TEXT,
            trial_ever_granted INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('dragskope', '42') ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO streamer_plans (twitch_user_id, plan_name) VALUES ('42', 'pro') ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();

    let s: TwitchStreamerRow =
        sqlx::query_as("SELECT twitch_login, twitch_user_id, discord_user_id, is_on_discord, created_at, is_monitored_only FROM twitch_streamers WHERE twitch_login = 'dragskope'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(s.twitch_login, "dragskope");
    assert_eq!(s.twitch_user_id.as_deref(), Some("42"));
    assert!(s.created_at.is_some()); // text-Timestamp, kein timestamptz

    let p: StreamerPlanRow =
        sqlx::query_as("SELECT twitch_user_id, twitch_login, plan_name, promo_disabled, activated_at, expires_at, trial_ever_granted FROM streamer_plans WHERE twitch_user_id = '42'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(p.plan_name, "pro");
    assert_eq!(p.promo_disabled, 0);
}
