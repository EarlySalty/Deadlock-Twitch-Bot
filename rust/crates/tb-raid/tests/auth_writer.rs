//! Hermetische Tests des Onboarding-/Re-Auth-Writes. Round-Trip-Verifikation
//! über RaidAuthStore; Scope-Validierung + raid_enabled-Erhalt geprüft.

use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_crypto::{FieldCipher, KID};
use tb_raid::{AuthWriteError, AuthWriter, NewAuth, RaidAuthStore};

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
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
            access_token TEXT, refresh_token TEXT,
            token_expires_at TIMESTAMPTZ, scopes TEXT, authorized_at TIMESTAMPTZ,
            raid_enabled BOOLEAN DEFAULT TRUE, needs_reauth BOOLEAN DEFAULT FALSE,
            reauth_notified_at TIMESTAMPTZ,
            access_token_enc BYTEA, refresh_token_enc BYTEA,
            enc_version INTEGER, enc_kid TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    // remove_from_blacklist (in store_new_auth) räumt diese beiden Tabellen mit auf.
    sqlx::query(
        "CREATE TABLE twitch_token_blacklist (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, error_message TEXT,
            error_count INTEGER DEFAULT 1, first_error_at TEXT, last_error_at TEXT,
            grace_expires_at TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE twitch_partners (
            twitch_user_id TEXT, technical_pause_reason TEXT,
            raid_bot_enabled INTEGER DEFAULT 0
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

/// Die exakten BASE-Profil-Scopes (Reihenfolge egal — store validiert per Set).
const BASE_SCOPES: &[&str] = &[
    "channel:manage:raids",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:redemptions",
];

fn base_auth(user_id: &str, activate: bool) -> NewAuth {
    NewAuth {
        twitch_user_id: user_id.to_string(),
        twitch_login: "drag".to_string(),
        access_token: "acc".to_string(),
        refresh_token: "ref".to_string(),
        expires_in: 3600,
        granted_scopes: BASE_SCOPES.iter().map(|s| s.to_string()).collect(),
        resolved_scope_profile: "base".to_string(),
        activate_raid_features: activate,
    }
}

#[tokio::test]
async fn neuer_auth_wird_verschluesselt_gespeichert_und_ist_lesbar() {
    let pool = pool_or_skip!("t6a_authwrite_new");
    let cipher = cipher();
    let writer = AuthWriter::new(pool.clone(), cipher.clone());

    writer
        .store_new_auth(&base_auth("42", true), Utc::now())
        .await
        .unwrap();

    let store = RaidAuthStore::new(pool.clone(), cipher);
    let tokens = store.load_decrypted("42").await.unwrap().expect("Zeile");
    assert_eq!(tokens.access_token, "acc");
    assert_eq!(tokens.refresh_token.as_deref(), Some("ref"));

    let (plain, scopes, enabled): (String, String, bool) = sqlx::query_as(
        "SELECT access_token, scopes, raid_enabled FROM twitch_raid_auth WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(plain, "ENC");
    assert!(scopes.contains("channel:manage:raids"));
    assert!(enabled);
}

#[tokio::test]
async fn falsche_scopes_werden_abgelehnt_ohne_zu_schreiben() {
    let pool = pool_or_skip!("t6a_authwrite_scopes");
    let writer = AuthWriter::new(pool.clone(), cipher());

    let mut bad = base_auth("42", true);
    bad.granted_scopes = vec!["bits:read".to_string()]; // unvollständig
    let err = writer.store_new_auth(&bad, Utc::now()).await.unwrap_err();
    assert!(matches!(err, AuthWriteError::ScopeMismatch { .. }));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_auth")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "nichts geschrieben bei Scope-Mismatch");
}

#[tokio::test]
async fn bestehendes_raid_enabled_bleibt_bei_reauth_erhalten() {
    let pool = pool_or_skip!("t6a_authwrite_preserve");
    let writer = AuthWriter::new(pool.clone(), cipher());

    // Bestehende Zeile: raid_enabled=true, needs_reauth=true.
    sqlx::query(
        "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
         VALUES ('42', 'drag', TRUE, TRUE)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Re-Auth OHNE activate_raid_features → raid_enabled bleibt true (Erhalt).
    writer
        .store_new_auth(&base_auth("42", false), Utc::now())
        .await
        .unwrap();

    let (enabled, needs): (bool, bool) = sqlx::query_as(
        "SELECT raid_enabled, needs_reauth FROM twitch_raid_auth WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(enabled, "bestehendes raid_enabled erhalten");
    assert!(!needs, "needs_reauth nach Re-Auth zurückgesetzt");
}

#[tokio::test]
async fn reauth_entfernt_blacklist_und_loest_token_error_pause() {
    let pool = pool_or_skip!("t6a_authwrite_unblock");
    let writer = AuthWriter::new(pool.clone(), cipher());

    // Ausgangslage: wegen invalid_grant blacklisteter + technisch pausierter Partner.
    sqlx::query(
        "INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login) VALUES ('42', 'drag')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let legacy_token_error_reason = format!("{}_expired", "token_error");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, technical_pause_reason, raid_bot_enabled)
         VALUES ('42', $1, 0)",
    )
    .bind(legacy_token_error_reason)
    .execute(&pool)
    .await
    .unwrap();
    // Fremder Partner mit anderem Pause-Grund bleibt unangetastet.
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, technical_pause_reason, raid_bot_enabled)
         VALUES ('99', 'bot_banned', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Erfolgreiche Re-Autorisierung.
    writer
        .store_new_auth(&base_auth("42", true), Utc::now())
        .await
        .unwrap();

    // Blacklist-Eintrag entfernt.
    let bl: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_token_blacklist WHERE twitch_user_id='42'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(bl, 0, "Blacklist-Eintrag nach Re-Auth gelöscht");

    // technical_pause_reason='token_error*' aufgehoben und Raid wieder aktiviert.
    let (pause, raid_enabled): (Option<String>, Option<i32>) =
        sqlx::query_as("SELECT technical_pause_reason, raid_bot_enabled FROM twitch_partners WHERE twitch_user_id='42'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pause, None, "token_error-Pause aufgehoben");
    assert_eq!(
        raid_enabled,
        Some(1),
        "raid_bot_enabled nach Re-Auth geheilt"
    );
    let other: Option<String> = sqlx::query_scalar(
        "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id='99'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        other.as_deref(),
        Some("bot_banned"),
        "fremder Pause-Grund unangetastet"
    );
}

#[tokio::test]
async fn reauth_reaktiviert_hard_pause_nicht() {
    let pool = pool_or_skip!("t6a_authwrite_hardpause");
    let writer = AuthWriter::new(pool.clone(), cipher());

    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, technical_pause_reason, raid_bot_enabled)
         VALUES ('55', 'bot_banned', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    writer
        .store_new_auth(&base_auth("55", true), Utc::now())
        .await
        .unwrap();

    let (pause, raid_enabled): (Option<String>, Option<i32>) = sqlx::query_as(
        "SELECT technical_pause_reason, raid_bot_enabled FROM twitch_partners WHERE twitch_user_id='55'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pause.as_deref(), Some("bot_banned"));
    assert_eq!(
        raid_enabled,
        Some(0),
        "Hard-Pause darf Reauth nicht aktivieren"
    );
}
