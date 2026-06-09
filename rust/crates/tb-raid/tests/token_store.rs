//! Hermetische Tests des verschlüsselten Token-Lesepfads.
//!
//! Kern: ein synthetischer Wegwerf-Key ver- und entschlüsselt round-trip; der
//! Store liest ausschließlich die `_enc`-Spalten und gibt bei Misserfolg `None`
//! (kein Klartext-Fallback). Zusätzlich ein **gated Prod-Interop-Test**
//! (`TWITCH_ANALYTICS_DSN` + `DB_MASTER_KEY_V1`), der beweist, dass Rust echte
//! Prod-Blobs entschlüsselt — **ohne** den Klartext je auszugeben.

use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher, KID};
use tb_raid::RaidAuthStore;

// Synthetischer 32-Byte-Testschlüssel (Hex). NUR Test — niemals der Prod-Key.
const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

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

fn test_cipher() -> Arc<FieldCipher> {
    Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap())
}

/// Verschlüsselt + insertet eine raid_auth-Zeile mit der Test-Chiffre (enc_version 1).
#[allow(clippy::too_many_arguments)] // Test-Fixture, bewusst flach gehalten.
async fn insert_encrypted(
    pool: &PgPool,
    cipher: &FieldCipher,
    user_id: &str,
    login: &str,
    access: &str,
    refresh: Option<&str>,
    raid_enabled: bool,
    needs_reauth: bool,
) {
    let access_enc = cipher
        .encrypt_field(access, &aad::raid_auth("access_token", user_id, 1))
        .unwrap();
    let refresh_enc = refresh.map(|r| {
        cipher
            .encrypt_field(r, &aad::raid_auth("refresh_token", user_id, 1))
            .unwrap()
    });
    sqlx::query(
        "INSERT INTO twitch_raid_auth
            (twitch_user_id, twitch_login, access_token, refresh_token,
             token_expires_at, scopes, raid_enabled, needs_reauth,
             access_token_enc, refresh_token_enc, enc_version, enc_kid)
         VALUES ($1, $2, 'ENC', 'ENC', NOW() + INTERVAL '1 hour',
                 'channel:manage:raids bits:read', $3, $4, $5, $6, 1, 'v1')",
    )
    .bind(user_id)
    .bind(login)
    .bind(raid_enabled)
    .bind(needs_reauth)
    .bind(access_enc)
    .bind(refresh_enc)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn entschluesselt_aus_enc_spalten_und_liest_scopes() {
    let pool = pool_or_skip!("t6a_token_roundtrip");
    let cipher = test_cipher();
    let store = RaidAuthStore::new(pool.clone(), cipher.clone());

    insert_encrypted(&pool, &cipher, "42", "drag", "acc-secret", Some("ref-secret"), true, false).await;

    let tokens = store.load_decrypted("42").await.unwrap().expect("Zeile da");
    assert_eq!(tokens.access_token, "acc-secret");
    assert_eq!(tokens.refresh_token.as_deref(), Some("ref-secret"));
    assert_eq!(tokens.twitch_login, "drag");
    assert!(!tokens.needs_reauth);
    assert!(tokens.token_expires_at.is_some());

    let scopes = store.get_scopes("42").await.unwrap();
    assert_eq!(scopes, vec!["channel:manage:raids", "bits:read"]);
}

#[tokio::test]
async fn raid_disabled_und_fehlende_zeile_ergeben_none() {
    let pool = pool_or_skip!("t6a_token_gating");
    let cipher = test_cipher();
    let store = RaidAuthStore::new(pool.clone(), cipher.clone());

    insert_encrypted(&pool, &cipher, "99", "off", "acc", Some("ref"), false, false).await;
    assert!(store.load_decrypted("99").await.unwrap().is_none(), "raid_enabled=false → None");
    assert!(store.load_decrypted("unbekannt").await.unwrap().is_none(), "keine Zeile → None");
}

#[tokio::test]
async fn unlesbares_oder_null_enc_ergibt_none_kein_fehler() {
    let pool = pool_or_skip!("t6a_token_unreadable");
    let store = RaidAuthStore::new(pool.clone(), test_cipher());

    // NULL access_token_enc → None.
    sqlx::query(
        "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, enc_version)
         VALUES ('1', 'a', TRUE, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(store.load_decrypted("1").await.unwrap().is_none(), "NULL-enc → None");

    // Falscher Key: mit anderem Schlüssel verschlüsselt → Tag-Mismatch → None (kein Panic/Err).
    let other = FieldCipher::from_hex_key(
        "1111111111111111111111111111111111111111111111111111111111111111",
        KID,
    )
    .unwrap();
    let foreign_blob = other
        .encrypt_field("geheim", &aad::raid_auth("access_token", "2", 1))
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, enc_version, access_token_enc)
         VALUES ('2', 'b', TRUE, 1, $1)",
    )
    .bind(foreign_blob)
    .execute(&pool)
    .await
    .unwrap();
    assert!(store.load_decrypted("2").await.unwrap().is_none(), "falscher Key → None statt Err");
}

/// Beweist Interop auf ECHTEN Prod-Daten: entschlüsselt eine reale Zeile und
/// prüft nur, dass es **gelingt** (nicht-leer) — der Klartext wird nie
/// ausgegeben. Gated auf beide Secrets; ohne sie übersprungen.
#[tokio::test]
async fn prod_interop_entschluesselt_echten_blob_ohne_klartext_auszugeben() {
    let (Ok(dsn), Ok(_key)) = (
        std::env::var("TWITCH_ANALYTICS_DSN"),
        std::env::var("DB_MASTER_KEY_V1"),
    ) else {
        eprintln!("SKIP: TWITCH_ANALYTICS_DSN + DB_MASTER_KEY_V1 nicht gesetzt — Prod-Interop übersprungen.");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&dsn)
        .await
        .expect("prod connect (read-only)");
    // Eine raid-aktivierte user_id mit vorhandenem Access-Blob ziehen.
    let user_id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_raid_auth
          WHERE raid_enabled IS TRUE AND access_token_enc IS NOT NULL
          LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .expect("query");
    let Some(user_id) = user_id else {
        eprintln!("SKIP: keine entschlüsselbare Prod-Zeile vorhanden.");
        return;
    };
    let cipher = Arc::new(FieldCipher::from_env().expect("DB_MASTER_KEY_V1 valide"));
    let store = RaidAuthStore::new(pool, cipher);
    let tokens = store
        .load_decrypted(&user_id)
        .await
        .expect("query ok")
        .expect("Zeile entschlüsselbar");
    // NUR Erfolg + Nicht-Leere prüfen — niemals den Token-Wert ausgeben.
    assert!(!tokens.access_token.is_empty(), "Access-Token entschlüsselt + nicht-leer");
    let _ = Utc::now();
    println!("Prod-Interop OK: echter Access-Token entschlüsselt (Wert nicht ausgegeben).");
}
