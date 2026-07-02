//! Hermetische Tests des Token-Refresh-Schreibpfads. Stub-Client + Stub-
//! Blacklist (kein Netz); echte Round-Trip-Verifikation über RaidAuthStore.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher, KID};
use tb_raid::{
    RaidAuthStore, RaidTokenRefresher, RefreshError, RefreshOutcome, TokenBlacklist,
    TokenOwnerInfo, TokenResponse, TwitchTokenClient,
};

const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = test_dsn() else {
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
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
            access_token TEXT, refresh_token TEXT,
            token_expires_at TIMESTAMPTZ, scopes TEXT,
            raid_enabled BOOLEAN DEFAULT TRUE, needs_reauth BOOLEAN DEFAULT FALSE,
            access_token_enc BYTEA, refresh_token_enc BYTEA,
            enc_version INTEGER, enc_kid TEXT, last_refreshed_at TIMESTAMPTZ
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn cipher() -> Arc<FieldCipher> {
    Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap())
}

async fn seed_row(pool: &PgPool, cipher: &FieldCipher, user_id: &str) {
    let acc = cipher
        .encrypt_field("alt-access", &aad::raid_auth("access_token", user_id, 1))
        .unwrap();
    let refr = cipher
        .encrypt_field("alt-refresh", &aad::raid_auth("refresh_token", user_id, 1))
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth
            (twitch_user_id, twitch_login, raid_enabled, enc_version, enc_kid,
             access_token_enc, refresh_token_enc, token_expires_at)
         VALUES ($1, 'drag', TRUE, 1, 'v1', $2, $3, NOW())",
    )
    .bind(user_id)
    .bind(acc)
    .bind(refr)
    .execute(pool)
    .await
    .unwrap();
}

// ── Stubs ─────────────────────────────────────────────────────────────────────

struct StubClient {
    result: Mutex<Option<Result<TokenResponse, RefreshError>>>,
}
impl StubClient {
    fn ok(access: &str, refresh: &str) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Some(Ok(TokenResponse {
                access_token: access.to_string(),
                refresh_token: refresh.to_string(),
                expires_in: 3600,
                scopes: vec![],
            }))),
        })
    }
    fn err(e: RefreshError) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Some(Err(e))),
        })
    }
}
#[async_trait::async_trait]
impl TwitchTokenClient for StubClient {
    async fn refresh(&self, _refresh_token: &str) -> Result<TokenResponse, RefreshError> {
        self.result
            .lock()
            .unwrap()
            .take()
            .expect("einmal aufgerufen")
    }
    async fn exchange_code(&self, _code: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!("exchange_code im Refresher-Test ungenutzt")
    }
    async fn token_owner(&self, _a: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!("token_owner im Test ungenutzt")
    }
}

#[derive(Default)]
struct StubBlacklist {
    blacklisted: AtomicBool,
    recent_failure: AtomicBool,
    added: Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl TokenBlacklist for StubBlacklist {
    async fn is_blacklisted(&self, _u: &str) -> bool {
        self.blacklisted.load(Ordering::SeqCst)
    }
    async fn has_recent_failure(&self, _u: &str) -> bool {
        self.recent_failure.load(Ordering::SeqCst)
    }
    async fn add_to_blacklist(&self, twitch_user_id: &str, _login: &str, _err: &str) {
        self.added.lock().unwrap().push(twitch_user_id.to_string());
    }
    async fn clear_failure_count(&self, _twitch_user_id: &str) {}
}

/// Wie `seed_row`, aber mit `token_expires_at` in der Vergangenheit und einem
/// explizit gesetzten Refresh-Token-Wert.
async fn seed_row_expired(pool: &PgPool, cipher: &FieldCipher, user_id: &str, refresh_val: &str) {
    let acc = cipher
        .encrypt_field("alt-access", &aad::raid_auth("access_token", user_id, 1))
        .unwrap();
    let refr = cipher
        .encrypt_field(refresh_val, &aad::raid_auth("refresh_token", user_id, 1))
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth
            (twitch_user_id, twitch_login, raid_enabled, enc_version, enc_kid,
             access_token_enc, refresh_token_enc, token_expires_at)
         VALUES ($1, 'drag', TRUE, 1, 'v1', $2, $3,
                 NOW() - INTERVAL '1 hour')",
    )
    .bind(user_id)
    .bind(acc)
    .bind(refr)
    .execute(pool)
    .await
    .unwrap();
}

/// StubClient der den tatsächlich übergebenen Refresh-Token aufzeichnet.
struct CapturingStubClient {
    received_token: Mutex<Option<String>>,
    response: TokenResponse,
}
impl CapturingStubClient {
    fn new(access: &str, refresh: &str) -> Arc<Self> {
        Arc::new(Self {
            received_token: Mutex::new(None),
            response: TokenResponse {
                access_token: access.to_string(),
                refresh_token: refresh.to_string(),
                expires_in: 3600,
                scopes: vec![],
            },
        })
    }
    fn received(&self) -> Option<String> {
        self.received_token.lock().unwrap().clone()
    }
}
#[async_trait::async_trait]
impl TwitchTokenClient for CapturingStubClient {
    async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, RefreshError> {
        *self.received_token.lock().unwrap() = Some(refresh_token.to_string());
        Ok(self.response.clone())
    }
    async fn exchange_code(&self, _code: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _a: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!("token_owner im Test ungenutzt")
    }
}

/// P2.33: Stub-Client, der den 15-Min-`invalid_client`-Cooldown meldet. Jeder
/// `refresh`-Aufruf ist ein Test-Fehler — `refresh_all_due` MUSS kurzschließen,
/// bevor ein Refresh ausgelöst wird.
struct BlockedStubClient;
#[async_trait::async_trait]
impl TwitchTokenClient for BlockedStubClient {
    async fn refresh(&self, _refresh_token: &str) -> Result<TokenResponse, RefreshError> {
        panic!("refresh darf bei aktivem invalid_client-Cooldown NICHT aufgerufen werden");
    }
    async fn exchange_code(&self, _code: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _a: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!("token_owner im Test ungenutzt")
    }
    fn is_client_auth_blocked(&self) -> bool {
        true
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// P2.33: Bei aktivem `invalid_client`-Cooldown bricht der Hintergrund-Sweep
/// sofort mit 0 erneuerten Tokens ab, ohne fällige Streamer gegen Twitch zu
/// refreshen (Python `if self.is_client_auth_blocked(): return 0`). Trotz einer
/// fälligen (abgelaufenen) Auth-Zeile wird `refresh` nie aufgerufen.
#[tokio::test]
async fn refresh_all_due_kurzschliesst_bei_client_auth_block() {
    let pool = pool_or_skip!("t6a_refresh_client_auth_block");
    let cipher = cipher();
    // Fällige Zeile (abgelaufen) — ohne den Guard würde sie refresht.
    seed_row_expired(&pool, &cipher, "42", "alt-refresh").await;
    let blacklist = Arc::new(StubBlacklist::default());
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        Arc::new(BlockedStubClient),
        blacklist.clone(),
    );

    let refreshed = refresher.refresh_all_due(Utc::now()).await.unwrap();
    assert_eq!(
        refreshed, 0,
        "Sweep muss bei invalid_client-Cooldown 0 liefern"
    );
    // Klartext-Spalte unverändert (kein 'ENC'-Write → kein Refresh passiert).
    let acc_plain: Option<String> =
        sqlx::query_scalar("SELECT access_token FROM twitch_raid_auth WHERE twitch_user_id = '42'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(acc_plain.as_deref(), Some("ENC"));
}

#[tokio::test]
async fn erfolgreicher_refresh_schreibt_neue_verschluesselte_tokens() {
    let pool = pool_or_skip!("t6a_refresh_ok");
    let cipher = cipher();
    seed_row(&pool, &cipher, "42").await;
    let blacklist = Arc::new(StubBlacklist::default());
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        StubClient::ok("neu-access", "neu-refresh"),
        blacklist.clone(),
    );

    let outcome = refresher
        .refresh_and_store("42", "drag", "alt-refresh", Utc::now())
        .await
        .unwrap();
    assert_eq!(outcome, RefreshOutcome::Refreshed);

    // Round-Trip: die neu geschriebenen Blobs sind mit demselben Key/AAD lesbar.
    let store = RaidAuthStore::new(pool.clone(), cipher);
    let tokens = store.load_decrypted("42").await.unwrap().expect("Zeile");
    assert_eq!(tokens.access_token, "neu-access");
    assert_eq!(tokens.refresh_token.as_deref(), Some("neu-refresh"));

    // Klartext-Spalten sind 'ENC'-Platzhalter, last_refreshed_at gesetzt.
    let (acc_plain, last): (String, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT access_token, last_refreshed_at FROM twitch_raid_auth WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(acc_plain, "ENC");
    assert!(last.is_some());
    assert!(blacklist.added.lock().unwrap().is_empty());
}

#[tokio::test]
async fn invalid_grant_blacklistet_und_laesst_tokens_unveraendert() {
    let pool = pool_or_skip!("t6a_refresh_invalid_grant");
    let cipher = cipher();
    seed_row(&pool, &cipher, "42").await;
    let blacklist = Arc::new(StubBlacklist::default());
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        StubClient::err(RefreshError::InvalidGrant),
        blacklist.clone(),
    );

    let outcome = refresher
        .refresh_and_store("42", "drag", "alt-refresh", Utc::now())
        .await
        .unwrap();
    assert_eq!(outcome, RefreshOutcome::Blacklisted);
    assert_eq!(*blacklist.added.lock().unwrap(), vec!["42".to_string()]);

    // Alte Tokens unverändert lesbar.
    let store = RaidAuthStore::new(pool.clone(), cipher);
    let tokens = store.load_decrypted("42").await.unwrap().expect("Zeile");
    assert_eq!(tokens.access_token, "alt-access");
}

#[tokio::test]
async fn invalid_client_und_other_ueberspringen_ohne_blacklist() {
    let pool = pool_or_skip!("t6a_refresh_skip");
    let cipher = cipher();
    seed_row(&pool, &cipher, "42").await;
    let blacklist = Arc::new(StubBlacklist::default());
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        StubClient::err(RefreshError::Other("5xx".into())),
        blacklist.clone(),
    );
    let outcome = refresher
        .refresh_and_store("42", "drag", "alt-refresh", Utc::now())
        .await
        .unwrap();
    assert_eq!(outcome, RefreshOutcome::Skipped);
    assert!(
        blacklist.added.lock().unwrap().is_empty(),
        "kein Blacklist bei Other"
    );
}

#[tokio::test]
async fn geblacklisteter_streamer_wird_vorab_uebersprungen() {
    let pool = pool_or_skip!("t6a_refresh_pre_blacklist");
    let cipher = cipher();
    seed_row(&pool, &cipher, "42").await;
    let blacklist = Arc::new(StubBlacklist::default());
    blacklist.blacklisted.store(true, Ordering::SeqCst);
    // Client-Stub würde panicken (kein Ergebnis gesetzt) → beweist: nie aufgerufen.
    let client = Arc::new(StubClient {
        result: Mutex::new(None),
    });
    let refresher = RaidTokenRefresher::new(pool.clone(), cipher, client, blacklist.clone());
    let outcome = refresher
        .refresh_and_store("42", "drag", "alt-refresh", Utc::now())
        .await
        .unwrap();
    assert_eq!(outcome, RefreshOutcome::Skipped);
}

/// Kerntest: Token mit abgelaufenem `token_expires_at` → Re-Read unterm Lock
/// liefert den DB-seitigen (ggf. vom parallelen Writer rotierten) Refresh-Token,
/// refresht damit, schreibt neuen Token — und gibt `Refreshed` zurück.
///
/// Simuliert den Race: Der Aufrufer übergibt einen "alten" Refresh-Token
/// (`veraltet-refresh`), aber in der DB steht der "frische" (`db-refresh`).
/// Der Fix stellt sicher, dass der HTTP-Call mit dem DB-seitigen Token erfolgt.
#[tokio::test]
async fn re_read_unterm_lock_nutzt_db_refresh_token_nicht_uebergebenen() {
    let pool = pool_or_skip!("t6a_refresh_reread");
    let cipher = cipher();
    // DB enthält `db-refresh` als Refresh-Token; Token ist abgelaufen.
    seed_row_expired(&pool, &cipher, "42", "db-refresh").await;
    let blacklist = Arc::new(StubBlacklist::default());
    let client = CapturingStubClient::new("neu-access", "neu-refresh");

    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        client.clone(),
        blacklist.clone(),
    );

    // Aufrufer übergibt absichtlich einen veralteten Token (simuliert Race).
    let outcome = refresher
        .refresh_and_store("42", "drag", "veraltet-refresh", Utc::now())
        .await
        .unwrap();
    assert_eq!(outcome, RefreshOutcome::Refreshed);

    // Der HTTP-Client wurde mit dem DB-seitigen Token aufgerufen, NICHT dem übergebenen.
    assert_eq!(
        client.received().as_deref(),
        Some("db-refresh"),
        "refresh() muss mit dem frisch aus der DB gelesenen Token aufgerufen werden"
    );

    // Neuer Token ist korrekt verschlüsselt in der DB gelandet.
    let store = RaidAuthStore::new(pool.clone(), cipher);
    let tokens = store.load_decrypted("42").await.unwrap().expect("Zeile");
    assert_eq!(tokens.access_token, "neu-access");
    assert_eq!(tokens.refresh_token.as_deref(), Some("neu-refresh"));
}
