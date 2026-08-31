//! Hermetische StateStore-Tests gegen den Wegwerf-Container
//! (`TB_TEST_DATABASE_URL`, siehe `rust/scripts/test_db.sh up`). Schema pro
//! Test (Parallel-Isolation), DDL nach prod-verifiziertem Stand:
//! `oauth_state_tokens` mit text-Spalten + timestamptz, `platform`-gated.

use std::str::FromStr;

use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::state_store::{RaidOAuthState, StateStore, PLATFORM_RAID};

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = test_dsn() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`");
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
        "CREATE TABLE oauth_state_tokens (
            state_token TEXT PRIMARY KEY,
            platform TEXT,
            streamer_login TEXT,
            redirect_uri TEXT,
            pkce_verifier TEXT,
            created_at TIMESTAMPTZ DEFAULT NOW(),
            expires_at TIMESTAMPTZ,
            consumed_at TIMESTAMPTZ
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn sample() -> RaidOAuthState {
    RaidOAuthState {
        requested_login: "drag".to_string(),
        scope_profile: "raid".to_string(),
        expected_twitch_login: Some("drag".to_string()),
        expected_twitch_user_id: Some("42".to_string()),
        discord_user_id: Some("99".to_string()),
    }
}

#[tokio::test]
async fn persist_lookup_consume_roundtrip() {
    let pool = pool_or_skip!("t6a_state_roundtrip");
    let store = StateStore::new(pool.clone(), "https://cb/raid");
    let now = Utc::now();

    store.persist("st-1", &sample(), now).await.unwrap();

    // Lookup verbraucht nicht.
    let looked = store.lookup("st-1", now).await.unwrap().expect("vorhanden");
    assert_eq!(looked, sample());
    assert!(
        store.lookup("st-1", now).await.unwrap().is_some(),
        "lookup non-destruktiv"
    );

    // platform-Gate: ein social-media-Eintrag mit gleichem Token-Namespace stört nicht.
    sqlx::query(
        "INSERT INTO oauth_state_tokens (state_token, platform, streamer_login, expires_at)
         VALUES ($1, 'instagram', 'fremd', NOW() + INTERVAL '10 minutes')",
    )
    .bind(tb_crypto::token_lookup_key("sm-1"))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        store.lookup("sm-1", now).await.unwrap().is_none(),
        "fremde Plattform unsichtbar"
    );

    // Consume entfernt atomar.
    let consumed = store.consume("st-1", now).await.unwrap().expect("consume");
    assert_eq!(consumed, sample());
    assert!(
        store.lookup("st-1", now).await.unwrap().is_none(),
        "nach consume weg"
    );
    assert!(
        store.consume("st-1", now).await.unwrap().is_none(),
        "zweiter consume leer"
    );
}

#[tokio::test]
async fn abgelaufene_tokens_unsichtbar_und_cleanup_nur_eigene_plattform() {
    let pool = pool_or_skip!("t6a_state_expiry");
    let store = StateStore::new(pool.clone(), "https://cb/raid");
    let now = Utc::now();

    store.persist("frisch", &sample(), now).await.unwrap();
    // Abgelaufener raid-Token + fremder Plattform-Token.
    sqlx::query(
        "INSERT INTO oauth_state_tokens (state_token, platform, streamer_login, expires_at)
         VALUES ($1, $2, 'drag', $3), ($4, 'youtube', 'fremd', $3)",
    )
    .bind(tb_crypto::token_lookup_key("alt"))
    .bind(PLATFORM_RAID)
    .bind(now - Duration::seconds(60))
    .bind(tb_crypto::token_lookup_key("sm-alt"))
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        store.lookup("alt", now).await.unwrap().is_none(),
        "abgelaufen unsichtbar"
    );

    // Cleanup löscht nur abgelaufene raid-Tokens.
    assert_eq!(store.cleanup_expired(now).await.unwrap(), 1);
    let rest: Vec<(String, String)> =
        sqlx::query_as("SELECT state_token, platform FROM oauth_state_tokens ORDER BY state_token")
            .fetch_all(&pool)
            .await
            .unwrap();
    let mut expected = vec![
        (
            tb_crypto::token_lookup_key("frisch"),
            PLATFORM_RAID.to_string(),
        ),
        (tb_crypto::token_lookup_key("sm-alt"), "youtube".to_string()),
    ];
    expected.sort();
    assert_eq!(
        rest, expected,
        "fremde Plattform + frischer raid-Token bleiben"
    );
}
