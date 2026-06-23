//! DB-Tests fuer den gueltigen Token-Provider.
//!
//! Fokus: Chatters/Presence braucht Broadcaster-Tokens ohne `raid_enabled`-Gate,
//! aber nur mit dem zwingenden `moderator:read:chatters`-Scope.

use std::str::FromStr;
use std::sync::Arc;

use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher, KID};
use tb_raid::{
    RaidAuthStore, RaidTokenRefresher, RefreshError, TokenBlacklist, TokenOwnerInfo, TokenProvider,
    TokenResponse, TwitchTokenClient,
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
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            access_token TEXT,
            refresh_token TEXT,
            token_expires_at TIMESTAMPTZ,
            scopes TEXT,
            raid_enabled BOOLEAN DEFAULT TRUE,
            needs_reauth BOOLEAN DEFAULT FALSE,
            access_token_enc BYTEA,
            refresh_token_enc BYTEA,
            enc_version INTEGER,
            enc_kid TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

struct NoopTokenClient;

#[async_trait::async_trait]
impl TwitchTokenClient for NoopTokenClient {
    async fn refresh(&self, _refresh_token: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!("Test-Token ist frisch und darf nicht refreshen")
    }

    async fn exchange_code(&self, _code: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!("exchange_code im Provider-Test ungenutzt")
    }

    async fn token_owner(&self, _access_token: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!("token_owner im Provider-Test ungenutzt")
    }
}

struct NoBlacklist;

#[async_trait::async_trait]
impl TokenBlacklist for NoBlacklist {
    async fn is_blacklisted(&self, _twitch_user_id: &str) -> bool {
        false
    }

    async fn has_recent_failure(&self, _twitch_user_id: &str) -> bool {
        false
    }

    async fn add_to_blacklist(&self, _twitch_user_id: &str, _login: &str, _error_message: &str) {}

    async fn clear_failure_count(&self, _twitch_user_id: &str) {}
}

fn cipher() -> Arc<FieldCipher> {
    Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap())
}

fn provider(pool: PgPool) -> TokenProvider {
    let cipher = cipher();
    let blacklist = Arc::new(NoBlacklist);
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        Arc::new(NoopTokenClient),
        blacklist.clone(),
    );
    TokenProvider::new(RaidAuthStore::new(pool, cipher), refresher, blacklist)
}

async fn seed_token(
    pool: &PgPool,
    user_id: &str,
    login: &str,
    access: &str,
    raid_enabled: bool,
    scopes: &str,
) {
    let cipher = cipher();
    let access_enc = cipher
        .encrypt_field(access, &aad::raid_auth("access_token", user_id, 1))
        .unwrap();
    let refresh_enc = cipher
        .encrypt_field("refresh", &aad::raid_auth("refresh_token", user_id, 1))
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth
            (twitch_user_id, twitch_login, access_token, refresh_token,
             token_expires_at, scopes, raid_enabled, needs_reauth,
             access_token_enc, refresh_token_enc, enc_version, enc_kid)
         VALUES ($1, $2, 'ENC', 'ENC', $3, $4, $5, FALSE, $6, $7, 1, 'v1')",
    )
    .bind(user_id)
    .bind(login)
    .bind(Utc::now() + Duration::hours(2))
    .bind(scopes)
    .bind(raid_enabled)
    .bind(access_enc)
    .bind(refresh_enc)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn unrestricted_scope_token_entkoppelt_chatters_von_raid_enabled() {
    let pool = pool_or_skip!("t_provider_chatters_unrestricted");
    seed_token(
        &pool,
        "42",
        "chatters",
        "access-chatters",
        false,
        "channel:manage:raids Moderator:Read:Chatters",
    )
    .await;
    let provider = provider(pool);

    assert!(
        provider
            .get_valid_token("42", Utc::now())
            .await
            .unwrap()
            .is_none(),
        "Raid-Pfad bleibt raid_enabled-gegatet"
    );
    assert_eq!(
        provider
            .get_valid_token_unrestricted_with_scope(
                "42",
                Utc::now(),
                "moderator:read:chatters",
            )
            .await
            .unwrap()
            .as_deref(),
        Some("access-chatters"),
        "Chatters-Fallback nutzt Broadcaster-Token trotz raid_enabled=false"
    );
}

#[tokio::test]
async fn unrestricted_scope_token_liefert_ohne_chatters_scope_none() {
    let pool = pool_or_skip!("t_provider_chatters_scope_missing");
    seed_token(
        &pool,
        "99",
        "noscope",
        "access-noscope",
        false,
        "channel:manage:raids channel:bot",
    )
    .await;
    let provider = provider(pool);

    assert!(
        provider
            .get_valid_token_unrestricted_with_scope(
                "99",
                Utc::now(),
                "moderator:read:chatters",
            )
            .await
            .unwrap()
            .is_none(),
        "Token ohne Chatters-Scope darf nicht in einen wirkungslosen 403-Pfad laufen"
    );
}
