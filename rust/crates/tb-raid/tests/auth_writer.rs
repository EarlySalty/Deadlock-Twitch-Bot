//! Hermetische Tests des Onboarding-/Re-Auth-Writes. Round-Trip-Verifikation
//! über RaidAuthStore; Scope-Validierung + raid_enabled-Erhalt geprüft.

use chrono::Utc;
use std::sync::Arc;
use tb_crypto::{FieldCipher, KID};
use tb_raid::{AuthWriteError, AuthWriter, NewAuth, RaidAuthStore};
#[path = "support/auth_database.rs"]
mod auth_database;
use auth_database::Database;
const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";
struct ValidatedScopes(Vec<String>);
#[async_trait::async_trait]
impl tb_raid::TwitchTokenClient for ValidatedScopes {
    async fn validate_token(
        &self,
        _: &str,
        uid: &str,
    ) -> Result<tb_raid::token_refresher::TokenValidation, tb_raid::RefreshError> {
        Ok(tb_raid::token_refresher::TokenValidation {
            client_id: "test".into(),
            twitch_user_id: uid.into(),
            scopes: self.0.clone(),
            expires_in: 3600,
        })
    }
    async fn refresh(&self, _: &str) -> Result<tb_raid::TokenResponse, tb_raid::RefreshError> {
        unreachable!()
    }
    async fn exchange_code(
        &self,
        _: &str,
    ) -> Result<tb_raid::TokenResponse, tb_raid::RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _: &str) -> Result<tb_raid::TokenOwnerInfo, tb_raid::RefreshError> {
        unreachable!()
    }
}
fn valid_client() -> ValidatedScopes {
    ValidatedScopes(BASE_SCOPES.iter().map(|s| (*s).into()).collect())
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
        state_created_at: Utc::now(),
    }
}

type PartnerPauseRow = (String, Option<String>, Option<i32>, Option<i32>);

#[tokio::test]
async fn neuer_auth_wird_verschluesselt_gespeichert_und_ist_lesbar() {
    let db = Database::new().await;
    let pool = db.pool.clone();
    let cipher = cipher();
    let writer = AuthWriter::new(pool.clone(), cipher.clone());

    writer
        .store_new_auth(&base_auth("42", true), &valid_client(), Utc::now())
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
    let db = Database::new().await;
    let pool = db.pool.clone();
    let writer = AuthWriter::new(pool.clone(), cipher());

    let mut bad = base_auth("42", true);
    bad.granted_scopes = vec!["bits:read".to_string()]; // unvollständig
    let err = writer
        .store_new_auth(
            &bad,
            &ValidatedScopes(bad.granted_scopes.clone()),
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AuthWriteError::ScopeMismatch { .. }));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_auth")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "nichts geschrieben bei Scope-Mismatch");
}

#[tokio::test]
async fn bestehendes_raid_enabled_bleibt_bei_reauth_erhalten() {
    let db = Database::new().await;
    let pool = db.pool.clone();
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
        .store_new_auth(&base_auth("42", false), &valid_client(), Utc::now())
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
    let db = Database::new().await;
    let pool = db.pool.clone();
    let writer = AuthWriter::new(pool.clone(), cipher());

    // Ausgangslage: wegen invalid_grant blacklisteter + technisch pausierter Partner.
    sqlx::query(
        "INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login) VALUES ('42', 'drag')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, technical_pause_reason,
            manual_partner_opt_out, raid_bot_enabled)
         VALUES ('42', 'token_error', 1, 0)",
    )
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
        .store_new_auth(&base_auth("42", true), &valid_client(), Utc::now())
        .await
        .unwrap();

    // Blacklist-Eintrag entfernt.
    let bl: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_token_blacklist WHERE twitch_user_id='42'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(bl, 0, "Blacklist-Eintrag nach Re-Auth gelöscht");

    // technical_pause_reason='token_error' aufgehoben, technischer Opt-out
    // zurückgesetzt und Raid wieder aktiviert.
    let (pause, opt_out, raid_enabled): (Option<String>, Option<i32>, Option<i32>) =
        sqlx::query_as("SELECT technical_pause_reason, manual_partner_opt_out, raid_bot_enabled FROM twitch_partners WHERE twitch_user_id='42'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pause, None, "token_error-Pause aufgehoben");
    assert_eq!(opt_out, Some(0), "technischer Opt-out zurückgesetzt");
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
async fn reauth_loest_token_error_suffix_pause_auch_ohne_aktivierung() {
    let db = Database::new().await;
    let pool = db.pool.clone();
    let writer = AuthWriter::new(pool.clone(), cipher());

    sqlx::query(
        "INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login) VALUES ('43', 'drag')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, technical_pause_reason,
            manual_partner_opt_out, raid_bot_enabled)
         VALUES ('43', 'token_error_expired', 1, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    writer
        .store_new_auth(&base_auth("43", false), &valid_client(), Utc::now())
        .await
        .unwrap();

    let bl: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_token_blacklist WHERE twitch_user_id='43'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(bl, 0, "Blacklist-Eintrag nach Re-Auth gelöscht");

    let (pause, opt_out, raid_enabled): (Option<String>, Option<i32>, Option<i32>) =
        sqlx::query_as("SELECT technical_pause_reason, manual_partner_opt_out, raid_bot_enabled FROM twitch_partners WHERE twitch_user_id='43'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pause, None, "token_error*-Pause aufgehoben");
    assert_eq!(opt_out, Some(0), "technischer Opt-out zurückgesetzt");
    assert_eq!(raid_enabled, Some(1));
}

#[tokio::test]
async fn reauth_reaktiviert_hard_pause_nicht() {
    let db = Database::new().await;
    let pool = db.pool.clone();
    let writer = AuthWriter::new(pool.clone(), cipher());

    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, technical_pause_reason, raid_bot_enabled)
         VALUES ('55', 'bot_banned', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    writer
        .store_new_auth(&base_auth("55", true), &valid_client(), Utc::now())
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

#[tokio::test]
async fn reauth_respektiert_echte_optouts_und_hard_pauses() {
    let db = Database::new().await;
    let pool = db.pool.clone();
    let writer = AuthWriter::new(pool.clone(), cipher());

    sqlx::query(
        "INSERT INTO twitch_partners
            (twitch_user_id, technical_pause_reason, manual_partner_opt_out, raid_bot_enabled)
         VALUES
            ('60', NULL, 1, 0),
            ('61', 'paused_by_admin', 1, 0),
            ('62', 'blocked', 0, 0),
            ('63', 'bot_banned', 0, 0),
            ('64', 'blocked', 1, 0),
            ('65', 'bot_banned', 1, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    for user_id in ["60", "61", "62", "63", "64", "65"] {
        let mut auth = base_auth(user_id, true);
        auth.twitch_login = format!("drag{user_id}");
        writer
            .store_new_auth(&auth, &valid_client(), Utc::now())
            .await
            .unwrap();
    }

    let rows: Vec<PartnerPauseRow> = sqlx::query_as(
        "SELECT twitch_user_id, technical_pause_reason,
                manual_partner_opt_out, raid_bot_enabled
         FROM twitch_partners
         ORDER BY twitch_user_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        rows,
        vec![
            ("60".to_string(), None, Some(1), Some(0)),
            (
                "61".to_string(),
                Some("paused_by_admin".to_string()),
                Some(1),
                Some(0),
            ),
            (
                "62".to_string(),
                Some("blocked".to_string()),
                Some(0),
                Some(0)
            ),
            (
                "63".to_string(),
                Some("bot_banned".to_string()),
                Some(0),
                Some(0),
            ),
            (
                "64".to_string(),
                Some("blocked".to_string()),
                Some(1),
                Some(0)
            ),
            (
                "65".to_string(),
                Some("bot_banned".to_string()),
                Some(1),
                Some(0),
            ),
        ]
    );
}
