//! Hermetische Tests der Raid-Ziel-Blacklist (`twitch_raid_blacklist`,
//! PK target_login, alle Spalten TEXT).

use std::str::FromStr;

use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::RaidBlacklistStore;

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
    sqlx::query(
        "CREATE TABLE twitch_raid_blacklist (
            target_login TEXT PRIMARY KEY, target_id TEXT, reason TEXT, added_at TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

#[tokio::test]
async fn add_match_per_id_oder_login_und_drift_cleanup() {
    let pool = pool_or_skip!("t6b_raidbl");
    let store = RaidBlacklistStore::new(pool.clone());

    store
        .add(Some("42"), "Drag", "scam", Utc::now())
        .await
        .unwrap();
    // Match per Login (case-insensitiv) UND per ID.
    assert!(store.is_blacklisted(None, "drag").await.unwrap());
    assert!(store.is_blacklisted(Some("42"), "anderer").await.unwrap());
    assert!(!store.is_blacklisted(Some("99"), "fremd").await.unwrap());

    // Drift: gleiche ID 42 wandert auf neuen Login → alte Login-Zeile weg.
    store
        .add(Some("42"), "drag_neu", "rename", Utc::now())
        .await
        .unwrap();
    let logins: Vec<String> =
        sqlx::query_scalar("SELECT target_login FROM twitch_raid_blacklist ORDER BY target_login")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        logins,
        vec!["drag_neu".to_string()],
        "alte Login-Zeile gleicher ID entfernt"
    );

    // Reason-Update via UPSERT.
    store
        .add(None, "drag_neu", "neuer_grund", Utc::now())
        .await
        .unwrap();
    let reason: String = sqlx::query_scalar(
        "SELECT reason FROM twitch_raid_blacklist WHERE target_login='drag_neu'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reason, "neuer_grund");

    // Leerer Login → no-op.
    store
        .add(Some("x"), "  ", "egal", Utc::now())
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_blacklist")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Remove.
    assert!(store.remove("DRAG_NEU").await.unwrap());
    assert!(!store.is_blacklisted(None, "drag_neu").await.unwrap());
}

#[tokio::test]
async fn load_all_liefert_id_und_login_sets_normalisiert() {
    let pool = pool_or_skip!("t6b_raidbl_all");
    let store = RaidBlacklistStore::new(pool.clone());

    store
        .add(Some("42"), "Drag", "scam", Utc::now())
        .await
        .unwrap();
    store
        .add(None, "NurLogin", "spam", Utc::now())
        .await
        .unwrap();
    // Zeile mit leerer ID (Altbestand): nur der Login zählt.
    sqlx::query(
        "INSERT INTO twitch_raid_blacklist (target_login, target_id, reason, added_at)
         VALUES ('altbestand', '', 'x', 'y')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let (ids, logins) = store.load_all().await.unwrap();
    assert_eq!(ids, ["42".to_string()].into_iter().collect());
    assert_eq!(
        logins,
        [
            "drag".to_string(),
            "nurlogin".to_string(),
            "altbestand".to_string()
        ]
        .into_iter()
        .collect(),
        "Logins lowercase-normalisiert, leere IDs nicht im ID-Set"
    );
}
