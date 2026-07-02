//! Hermetische Tests des Outreach-Boost-Stores (`twitch_partner_outreach`,
//! alle Spalten TEXT wie in Prod).

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::{OutreachBoostStore, OUTREACH_BOOST_LOOKBACK_HOURS};

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
        "CREATE TABLE twitch_partner_outreach (
            streamer_login TEXT, streamer_user_id TEXT, detected_at TEXT,
            contacted_at TEXT, status TEXT, cooldown_until TEXT, notes TEXT,
            raid_used_at TEXT, conversation_status TEXT )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE twitch_partners (
            twitch_user_id TEXT, twitch_login TEXT, status TEXT )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn insert(pool: &PgPool, login: &str, status: &str, contacted_offset_h: i32, used: bool) {
    sqlx::query(
        "INSERT INTO twitch_partner_outreach (streamer_login, status, detected_at, contacted_at, raid_used_at)
         VALUES ($1, $2, (NOW() - ($3 || ' hours')::interval)::text,
                 CASE WHEN $2 = 'queued' THEN NULL ELSE (NOW() - ($3 || ' hours')::interval)::text END,
                 CASE WHEN $4 THEN NOW()::text ELSE NULL END)",
    )
    .bind(login)
    .bind(status)
    .bind(contacted_offset_h.to_string())
    .bind(used)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn lader_filtert_status_frische_und_verbraucht() {
    let pool = pool_or_skip!("t6g_boost_load");
    insert(&pool, "Frisch", "sent", 2, false).await;
    insert(&pool, "Queued", "queued", 2, false).await;
    insert(&pool, "alt", "sent", 72, false).await; // außerhalb 48h
    insert(&pool, "verbraucht", "sent", 2, true).await;
    insert(&pool, "nur_erkannt", "detected", 2, false).await;
    insert(&pool, "partner", "sent", 2, false).await;
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status)
         VALUES ('p1', 'partner', 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let store = OutreachBoostStore::new(pool);
    let logins = store
        .load_boost_logins(OUTREACH_BOOST_LOOKBACK_HOURS)
        .await
        .unwrap();
    assert_eq!(
        logins,
        ["frisch".to_string(), "partner".to_string()]
            .into_iter()
            .collect(),
        "nur frisch+sent+kontaktiert+unverbraucht, Login lowercase"
    );
}

#[tokio::test]
async fn mark_used_ist_cas_und_einmalig() {
    let pool = pool_or_skip!("t6g_boost_mark");
    insert(&pool, "ziel", "sent", 2, false).await;

    let store = OutreachBoostStore::new(pool.clone());
    assert!(store.mark_used("ZIEL").await.unwrap(), "erster Mark greift");
    assert!(
        !store.mark_used("ziel").await.unwrap(),
        "zweiter Mark: CAS schlägt fehl"
    );
    let used: Option<String> = sqlx::query_scalar(
        "SELECT raid_used_at FROM twitch_partner_outreach WHERE streamer_login='ziel'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(used.is_some());
    assert!(!store.mark_used("unbekannt").await.unwrap());
}
