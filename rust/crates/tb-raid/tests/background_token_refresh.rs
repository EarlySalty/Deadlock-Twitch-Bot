//! DB-Tests des proaktiven Hintergrund-Token-Refresh (`refresh_all_due`,
//! Port von Python `RaidAuthManager.refresh_all_tokens`). Echter Postgres-
//! Test-Container, Stub-Token-Client mit erfolgreichem Refresh.
//!
//! Jeder Test startet einen isolierten PostgreSQL-Prozess über Unixsocket.

use std::sync::Arc;

use chrono::{Duration, DurationRound, TimeDelta, Utc};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher, KID};
use tb_raid::{
    RaidTokenRefresher, RefreshError, TokenBlacklist, TokenOwnerInfo, TokenResponse,
    TwitchTokenClient,
};

const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

#[path = "support/auth_database.rs"]
mod auth_database;
use auth_database::Database;

/// Token-Client-Stub: liefert immer einen frischen Token (1 h gültig).
struct OkTokenClient;
#[async_trait::async_trait]
impl TwitchTokenClient for OkTokenClient {
    async fn validate_token(
        &self,
        _: &str,
        uid: &str,
    ) -> Result<tb_raid::token_refresher::TokenValidation, RefreshError> {
        Ok(tb_raid::token_refresher::TokenValidation {
            client_id: "test".into(),
            twitch_user_id: uid.into(),
            scopes: vec!["channel:manage:raids".into()],
            expires_in: 3600,
        })
    }

    async fn refresh(&self, _t: &str) -> Result<TokenResponse, RefreshError> {
        Ok(TokenResponse {
            access_token: "neuer-access".to_string(),
            refresh_token: "neuer-refresh".to_string(),
            expires_in: 3600,
            scopes: vec![],
        })
    }
    async fn exchange_code(&self, _c: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _a: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!()
    }
}

/// Blacklist-Stub: nie geblacklistet, nie Cooldown.
struct NoBlacklist;
#[async_trait::async_trait]
impl TokenBlacklist for NoBlacklist {
    async fn add_in_transaction(
        &self,
        _: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        uid: &str,
        login: &str,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        self.add_to_blacklist(uid, login, error).await;
        Ok(())
    }
    async fn clear_in_transaction(
        &self,
        _: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        uid: &str,
    ) -> Result<(), sqlx::Error> {
        self.clear_failure_count(uid).await;
        Ok(())
    }

    async fn is_blacklisted(&self, _id: &str) -> bool {
        false
    }
    async fn has_recent_failure(&self, _id: &str) -> bool {
        false
    }
    async fn add_to_blacklist(&self, _id: &str, _login: &str, _msg: &str) {}
    async fn clear_failure_count(&self, _id: &str) {}
}

async fn seed_auth(
    pool: &PgPool,
    user_id: &str,
    login: &str,
    expires_at: chrono::DateTime<Utc>,
    raid_enabled: bool,
    needs_reauth: bool,
) {
    let cipher = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
    let acc = cipher
        .encrypt_field("alt-access", &aad::raid_auth("access_token", user_id, 1))
        .unwrap();
    let refr = cipher
        .encrypt_field("alt-refresh", &aad::raid_auth("refresh_token", user_id, 1))
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth
            (twitch_user_id, twitch_login, raid_enabled, needs_reauth, enc_version, enc_kid,
             access_token_enc, refresh_token_enc, token_expires_at)
         VALUES ($1, $2, $3, $4, 1, 'v1', $5, $6, $7)",
    )
    .bind(user_id)
    .bind(login)
    .bind(raid_enabled)
    .bind(needs_reauth)
    .bind(acc)
    .bind(refr)
    .bind(expires_at)
    .execute(pool)
    .await
    .unwrap();
}

fn refresher(pool: &PgPool) -> RaidTokenRefresher {
    let cipher = Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap());
    RaidTokenRefresher::new(
        pool.clone(),
        cipher,
        Arc::new(OkTokenClient),
        Arc::new(NoBlacklist),
    )
}

async fn stored_expiry(pool: &PgPool, user_id: &str) -> chrono::DateTime<Utc> {
    sqlx::query_scalar("SELECT token_expires_at FROM twitch_raid_auth WHERE twitch_user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn refresht_nur_faellige_tokens() {
    let db = Database::new().await;
    let pool = db.pool.clone();
    let now = Utc::now();

    // Fällig: läuft in 1 h ab (< 2 h Puffer).
    seed_auth(
        &pool,
        "100",
        "faellig",
        now + Duration::minutes(60),
        true,
        false,
    )
    .await;
    // Nicht fällig: läuft in 5 h ab.
    seed_auth(
        &pool,
        "200",
        "frisch",
        now + Duration::hours(5),
        true,
        false,
    )
    .await;

    let count = refresher(&pool).refresh_all_due(now).await.unwrap();
    assert_eq!(count, 1, "nur der fällige Token wird refresht");

    // Fälliger Token: Ablauf nach vorne verschoben (neuer 1-h-Token).
    let new_expiry = stored_expiry(&pool, "100").await;
    assert!(
        new_expiry > now + Duration::minutes(50) && new_expiry < now + Duration::minutes(70),
        "fälliger Token auf ~1 h erneuert"
    );
    // Nicht-fälliger Token: unverändert (~5 h).
    let untouched = stored_expiry(&pool, "200").await;
    assert!(
        untouched > now + Duration::hours(4),
        "frischer Token unangetastet"
    );
}

#[tokio::test]
async fn ueberspringt_raid_disabled_und_needs_reauth() {
    let db = Database::new().await;
    let pool = db.pool.clone();
    // Auf Mikrosekunden trunkieren: Postgres `TIMESTAMPTZ` speichert nur µs,
    // chrono::Utc::now() liefert ns. Ohne Trunkierung scheitert der exakte
    // `assert_eq!`-Vergleich unten an den verlorenen Nanosekunden.
    let now = Utc::now()
        .duration_trunc(TimeDelta::microseconds(1))
        .unwrap();
    let due = now + Duration::minutes(30);

    // Beide fällig, aber je ein Ausschlusskriterium.
    seed_auth(&pool, "300", "disabled", due, false, false).await; // raid_enabled = false
    seed_auth(&pool, "400", "reauth", due, true, true).await; // needs_reauth = true
                                                              // Ein echter Kandidat, damit der Sweep überhaupt etwas tut.
    seed_auth(&pool, "500", "ok", due, true, false).await;

    let count = refresher(&pool).refresh_all_due(now).await.unwrap();
    assert_eq!(count, 1, "nur der eligible Kandidat wird refresht");

    // Ausgeschlossene Zeilen unverändert.
    assert_eq!(stored_expiry(&pool, "300").await, due);
    assert_eq!(stored_expiry(&pool, "400").await, due);
}
