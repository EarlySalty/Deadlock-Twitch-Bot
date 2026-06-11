//! End-to-End-Test der Raid-Ausführung: TokenProvider (Store+Refresher+
//! Blacklist) → RaidExecutor (RaidApi-Port) → Raid-History. Stub-RaidApi +
//! Stub-TwitchTokenClient, echte Stores gegen den Test-Container.

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher, KID};
use tb_raid::{
    RaidApi, RaidAuthStore, RaidExecutor, RaidHistoryStore, RaidOutcome, RaidRequest,
    RaidTokenRefresher, RefreshError, TokenBlacklistStore, TokenOwnerInfo, TokenProvider, TokenResponse,
    TwitchTokenClient,
};

const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

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
        "CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, access_token TEXT, refresh_token TEXT,
            token_expires_at TIMESTAMPTZ, scopes TEXT, raid_enabled BOOLEAN DEFAULT TRUE,
            needs_reauth BOOLEAN DEFAULT FALSE, access_token_enc BYTEA, refresh_token_enc BYTEA,
            enc_version INTEGER, enc_kid TEXT, last_refreshed_at TIMESTAMPTZ )",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE twitch_raid_history (
            id BIGSERIAL PRIMARY KEY, from_broadcaster_id TEXT, from_broadcaster_login TEXT,
            to_broadcaster_id TEXT, to_broadcaster_login TEXT, viewer_count INTEGER,
            stream_duration_sec INTEGER, reason TEXT, executed_at TIMESTAMPTZ, success BOOLEAN,
            error_message TEXT, target_stream_started_at TIMESTAMPTZ, candidates_count INTEGER )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE twitch_token_blacklist (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, error_message TEXT,
            error_count INTEGER DEFAULT 1, first_error_at TEXT, last_error_at TEXT,
            notified INTEGER DEFAULT 0, grace_expires_at TEXT )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn cipher() -> Arc<FieldCipher> {
    Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap())
}

async fn seed_valid(pool: &PgPool, cipher: &FieldCipher, user_id: &str, expires_in_min: i64) {
    let acc = cipher
        .encrypt_field("acc-tok", &aad::raid_auth("access_token", user_id, 1))
        .unwrap();
    let refr = cipher
        .encrypt_field("ref-tok", &aad::raid_auth("refresh_token", user_id, 1))
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth
            (twitch_user_id, twitch_login, raid_enabled, enc_version, enc_kid,
             access_token_enc, refresh_token_enc, token_expires_at)
         VALUES ($1, 'src', TRUE, 1, 'v1', $2, $3, $4)",
    )
    .bind(user_id)
    .bind(acc)
    .bind(refr)
    .bind(Utc::now() + Duration::minutes(expires_in_min))
    .execute(pool)
    .await
    .unwrap();
}

// ── Stubs ──
struct StubTokenClient;
#[async_trait::async_trait]
impl TwitchTokenClient for StubTokenClient {
    async fn refresh(&self, _t: &str) -> Result<TokenResponse, RefreshError> {
        Err(RefreshError::Other("nicht erwartet".into()))
    }
    async fn exchange_code(&self, _c: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _a: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!("token_owner im Test ungenutzt")
    }
}

struct StubRaidApi {
    fail_with: Option<String>,
    called_with: Mutex<Option<(String, String, String)>>,
}
#[async_trait::async_trait]
impl RaidApi for StubRaidApi {
    async fn start_raid(&self, from: &str, to: &str, token: &str) -> Result<(), String> {
        *self.called_with.lock().unwrap() = Some((from.into(), to.into(), token.into()));
        match &self.fail_with {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

fn provider(pool: &PgPool, cipher: Arc<FieldCipher>) -> Arc<TokenProvider> {
    let blacklist = Arc::new(TokenBlacklistStore::new(pool.clone()));
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        Arc::new(StubTokenClient),
        blacklist.clone(),
    );
    Arc::new(TokenProvider::new(
        RaidAuthStore::new(pool.clone(), cipher),
        refresher,
        blacklist,
    ))
}

fn request() -> RaidRequest {
    RaidRequest {
        from_broadcaster_id: "100".into(),
        from_broadcaster_login: "src".into(),
        to_broadcaster_id: "200".into(),
        to_broadcaster_login: "dst".into(),
        viewer_count: 42,
        stream_duration_sec: 3600,
        target_stream_started_at: Some(Utc::now()),
        candidates_count: 3,
        reason: "auto_raid_on_offline".into(),
    }
}

#[tokio::test]
async fn gueltiger_token_startet_raid_und_schreibt_erfolg() {
    let pool = pool_or_skip!("t6d_exec_ok");
    let cipher = cipher();
    seed_valid(&pool, &cipher, "100", 60).await;
    let api = Arc::new(StubRaidApi {
        fail_with: None,
        called_with: Mutex::new(None),
    });
    let exec = RaidExecutor::new(
        api.clone(),
        provider(&pool, cipher),
        RaidHistoryStore::new(pool.clone()),
    );

    let outcome = exec.execute(&request(), Utc::now()).await.unwrap();
    assert_eq!(outcome, RaidOutcome::Started);
    // RaidApi mit entschlüsseltem User-Token aufgerufen.
    assert_eq!(
        api.called_with.lock().unwrap().clone(),
        Some(("100".into(), "200".into(), "acc-tok".into()))
    );

    let (success, err): (bool, Option<String>) = sqlx::query_as(
        "SELECT success, error_message FROM twitch_raid_history WHERE from_broadcaster_id='100'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(success);
    assert!(err.is_none());
}

#[tokio::test]
async fn kein_token_schreibt_fehlschlag_ohne_api_aufruf() {
    let pool = pool_or_skip!("t6d_exec_notoken");
    let cipher = cipher();
    // Geblacklistet (error_count 3) → kein Token.
    seed_valid(&pool, &cipher, "100", 60).await;
    sqlx::query(
        "INSERT INTO twitch_token_blacklist (twitch_user_id, error_count) VALUES ('100', 3)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let api = Arc::new(StubRaidApi {
        fail_with: None,
        called_with: Mutex::new(None),
    });
    let exec = RaidExecutor::new(
        api.clone(),
        provider(&pool, cipher),
        RaidHistoryStore::new(pool.clone()),
    );

    let outcome = exec.execute(&request(), Utc::now()).await.unwrap();
    assert!(matches!(outcome, RaidOutcome::Failed(ref e) if e.contains("No valid token")));
    assert!(
        api.called_with.lock().unwrap().is_none(),
        "RaidApi nicht aufgerufen"
    );
    let (success, err): (bool, Option<String>) = sqlx::query_as(
        "SELECT success, error_message FROM twitch_raid_history WHERE from_broadcaster_id='100'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!success);
    assert!(err.unwrap().contains("No valid token"));
}

#[tokio::test]
async fn api_fehler_wird_in_history_geschrieben() {
    let pool = pool_or_skip!("t6d_exec_apierr");
    let cipher = cipher();
    seed_valid(&pool, &cipher, "100", 60).await;
    let api = Arc::new(StubRaidApi {
        fail_with: Some("HTTP 429: rate limited".into()),
        called_with: Mutex::new(None),
    });
    let exec = RaidExecutor::new(
        api,
        provider(&pool, cipher),
        RaidHistoryStore::new(pool.clone()),
    );

    let outcome = exec.execute(&request(), Utc::now()).await.unwrap();
    assert!(matches!(outcome, RaidOutcome::Failed(ref e) if e.contains("429")));
    let err: Option<String> = sqlx::query_scalar(
        "SELECT error_message FROM twitch_raid_history WHERE from_broadcaster_id='100'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(err.unwrap().contains("429"));
}
