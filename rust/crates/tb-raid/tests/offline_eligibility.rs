//! Hermetische Tests der Quell-Eligibility fürs Auto-Raid
//! (`twitch_partners` + `twitch_raid_auth`).

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::OfflineEligibilityStore;

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
    // Prod-Typen: raid_bot_enabled INTEGER (0/1), raid_enabled BOOLEAN.
    sqlx::query(
        "CREATE TABLE twitch_partners (
            id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL, status TEXT NOT NULL,
            raid_bot_enabled INTEGER DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY, raid_enabled BOOLEAN, authorized_at TIMESTAMPTZ
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn insert_partner(pool: &PgPool, user_id: &str, status: &str, raid_bot_enabled: i32) {
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
         VALUES ($1, $1, $2, $3)",
    )
    .bind(user_id)
    .bind(status)
    .bind(raid_bot_enabled)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_auth(pool: &PgPool, user_id: &str, raid_enabled: bool) {
    sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, raid_enabled) VALUES ($1, $2)")
        .bind(user_id)
        .bind(raid_enabled)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn voll_freigeschalteter_partner_darf_auto_raiden() {
    let pool = pool_or_skip!("t6f_elig_ok");
    insert_partner(&pool, "42", "active", 1).await;
    insert_auth(&pool, "42", true).await;

    let elig = OfflineEligibilityStore::new(pool).load("42").await.unwrap();
    assert!(elig.active_partner);
    assert!(elig.auth_row_found);
    assert!(elig.raid_bot_enabled);
    assert!(elig.raid_auth_enabled);
    assert!(elig.can_auto_raid());
    assert_eq!(elig.skip_reason(), None);
}

#[tokio::test]
async fn unbekannter_streamer_wird_uebersprungen() {
    let pool = pool_or_skip!("t6f_elig_unknown");
    let elig = OfflineEligibilityStore::new(pool).load("99").await.unwrap();
    assert!(!elig.active_partner);
    assert!(!elig.auth_row_found);
    assert_eq!(elig.skip_reason(), Some("not_found"));
}

#[tokio::test]
async fn archivierter_partner_mit_auth_zaehlt_nicht_als_aktiv() {
    let pool = pool_or_skip!("t6f_elig_arch");
    insert_partner(&pool, "42", "archived", 1).await;
    insert_auth(&pool, "42", true).await;

    let elig = OfflineEligibilityStore::new(pool).load("42").await.unwrap();
    assert!(!elig.active_partner);
    assert!(elig.auth_row_found);
    assert!(!elig.can_auto_raid());
    assert_eq!(elig.skip_reason(), Some("not_active_partner"));
}

#[tokio::test]
async fn raid_bot_setting_aus_blockiert() {
    let pool = pool_or_skip!("t6f_elig_setting");
    insert_partner(&pool, "42", "active", 0).await;
    insert_auth(&pool, "42", true).await;

    let elig = OfflineEligibilityStore::new(pool).load("42").await.unwrap();
    assert!(elig.active_partner);
    assert!(!elig.raid_bot_enabled);
    assert_eq!(elig.skip_reason(), Some("setting_disabled"));
}

#[tokio::test]
async fn fehlende_oder_deaktivierte_auth_blockiert() {
    let pool = pool_or_skip!("t6f_elig_auth");
    insert_partner(&pool, "42", "active", 1).await;
    insert_auth(&pool, "42", false).await;
    insert_partner(&pool, "77", "active", 1).await; // gar keine Auth-Zeile

    let store = OfflineEligibilityStore::new(pool);
    let elig = store.load("42").await.unwrap();
    assert!(elig.auth_row_found);
    assert!(!elig.raid_auth_enabled);
    assert_eq!(elig.skip_reason(), Some("no_auth"));

    let elig77 = store.load("77").await.unwrap();
    assert!(!elig77.auth_row_found);
    assert_eq!(elig77.skip_reason(), Some("no_auth"));
}

#[tokio::test]
async fn neueste_aktive_zeile_gewinnt_und_leere_id_ist_not_found() {
    let pool = pool_or_skip!("t6f_elig_latest");
    // Re-Partnerung: alte Zeile ohne Setting, neue aktive Zeile mit Setting.
    insert_partner(&pool, "42", "active", 0).await;
    insert_partner(&pool, "42", "active", 1).await;
    insert_auth(&pool, "42", true).await;

    let store = OfflineEligibilityStore::new(pool);
    let elig = store.load("42").await.unwrap();
    assert!(
        elig.raid_bot_enabled,
        "neueste aktive Zeile (id DESC) zählt"
    );
    assert!(elig.can_auto_raid());

    let leer = store.load("  ").await.unwrap();
    assert_eq!(leer.skip_reason(), Some("not_found"));
}
