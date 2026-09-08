//! Echte isolierte PostgreSQL-Regressionen, ohne ENV/Produktivzugang.
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tb_crypto::{FieldCipher, KID};
use tb_raid::token_refresher::{
    RefreshError, TokenOwnerInfo, TokenResponse, TokenValidation, TwitchTokenClient,
};
use tb_raid::{AuthWriter, NewAuth, RaidAuthStore};
struct VerifiedClient;
#[async_trait::async_trait]
impl TwitchTokenClient for VerifiedClient {
    async fn validate_token(
        &self,
        token: &str,
        uid: &str,
    ) -> Result<TokenValidation, RefreshError> {
        let profile = if token.contains("uplink") {
            "uplink"
        } else {
            "base"
        };
        Ok(TokenValidation {
            client_id: "test-client".into(),
            twitch_user_id: uid.into(),
            scopes: tb_raid::scope_profiles::scopes_for_profile(profile)
                .iter()
                .map(|s| (*s).into())
                .collect(),
            expires_in: 7200,
        })
    }
    async fn refresh(&self, _: &str) -> Result<TokenResponse, RefreshError> {
        Err(RefreshError::Other("unerwarteter Refresh".into()))
    }
    async fn exchange_code(&self, _: &str) -> Result<TokenResponse, RefreshError> {
        Err(RefreshError::Other("unerwarteter Austausch".into()))
    }
    async fn token_owner(&self, _: &str) -> Result<TokenOwnerInfo, RefreshError> {
        Err(RefreshError::Other("unerwarteter Nachschlag".into()))
    }
}

#[path = "support/auth_database.rs"]
mod auth_database;
use auth_database::Database;
fn cipher() -> Arc<FieldCipher> {
    Arc::new(
        FieldCipher::from_hex_key(
            "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100",
            KID,
        )
        .unwrap(),
    )
}
fn grant(uid: &str, profile: &str, token: &str) -> NewAuth {
    NewAuth {
        twitch_user_id: uid.into(),
        twitch_login: "same_login".into(),
        access_token: token.into(),
        refresh_token: format!("refresh-{token}"),
        expires_in: 7200,
        granted_scopes: tb_raid::scope_profiles::scopes_for_profile(profile)
            .iter()
            .map(|s| (*s).into())
            .collect(),
        resolved_scope_profile: profile.into(),
        activate_raid_features: false,
        state_created_at: Utc::now(),
    }
}

#[tokio::test]
async fn smaller_grant_must_not_unconditionally_replace_the_uplink_grant() {
    let db = Database::new().await;
    let cipher = cipher();
    let writer = AuthWriter::new(db.pool.clone(), cipher.clone());
    writer
        .store_new_auth(
            &grant("42", "uplink", "synthetic-uplink"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    writer
        .store_new_auth(
            &grant("42", "base", "synthetic-base"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    let tokens = RaidAuthStore::new(db.pool.clone(), cipher)
        .load_decrypted_unrestricted("42")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tokens.access_token, "synthetic-uplink",
        "Kleinerer Grant hat den einzigen umfangreichen Zugang überschrieben"
    );
    db.close().await;
}

#[tokio::test]
async fn another_platform_id_with_the_same_login_cannot_enable_raids() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    let mut other = grant("99", "base", "synthetic-other");
    other.activate_raid_features = true;
    writer
        .store_new_auth(&other, &VerifiedClient, Utc::now())
        .await
        .unwrap();
    writer
        .store_new_auth(
            &grant("42", "base", "synthetic-new"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    let enabled: bool =
        sqlx::query_scalar("SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id='42'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        !enabled,
        "Login hat fremde Berechtigungen auf eine andere Plattform-ID übertragen"
    );
    db.close().await;
}

async fn snapshot(db: &Database) -> (tb_raid::RaidTokens, Vec<String>) {
    RaidAuthStore::new(db.pool.clone(), cipher())
        .load_decrypted_with_scopes("42")
        .await
        .unwrap()
        .unwrap()
}
async fn database_time(db: &Database) -> chrono::DateTime<Utc> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn disconnect_preserves_shared_raid_grant_and_only_fresh_uplink_state_reenables() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    let mut original = grant("42", "uplink", "synthetic-uplink");
    original.activate_raid_features = true;
    writer
        .store_new_auth(&original, &VerifiedClient, Utc::now())
        .await
        .unwrap();
    let mut stale = grant("42", "uplink", "synthetic-uplink-stale");
    stale.state_created_at = database_time(&db).await;
    writer
        .disconnect_uplink("42", Utc::now() + chrono::Duration::days(100))
        .await
        .unwrap();
    let (tokens, scopes) = snapshot(&db).await;
    assert!(tokens.uplink_disconnected);
    assert!(!tokens.needs_reauth);
    assert_eq!(tokens.access_token, "synthetic-uplink");
    assert!(scopes.iter().any(|s| s == "user:read:chat"));
    let raid: bool =
        sqlx::query_scalar("SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id='42'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(raid);
    writer
        .store_new_auth(
            &grant("42", "base", "synthetic-base"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    assert!(snapshot(&db).await.0.uplink_disconnected);
    assert!(matches!(
        writer
            .store_new_auth(&stale, &VerifiedClient, Utc::now())
            .await,
        Err(tb_raid::AuthWriteError::DisconnectedSinceAuthorization)
    ));
    let mut fresh = grant("42", "uplink", "synthetic-uplink-fresh");
    fresh.state_created_at = database_time(&db).await;
    writer
        .store_new_auth(&fresh, &VerifiedClient, Utc::now())
        .await
        .unwrap();
    assert!(!snapshot(&db).await.0.uplink_disconnected);
    assert!(matches!(
        writer
            .store_new_auth(&stale, &VerifiedClient, Utc::now())
            .await,
        Err(tb_raid::AuthWriteError::DisconnectedSinceAuthorization)
    ));
    assert_eq!(snapshot(&db).await.0.access_token, "synthetic-uplink-fresh");
}

#[tokio::test]
async fn disconnect_before_first_grant_persists_and_state_uses_database_creation_time() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    let states = tb_raid::StateStore::new(db.pool.clone(), "https://example.test/callback");
    let state = tb_raid::RaidOAuthState {
        requested_login: "synthetic".into(),
        scope_profile: "uplink".into(),
        expected_twitch_login: None,
        expected_twitch_user_id: Some("42".into()),
        discord_user_id: None,
    };
    states
        .persist(
            "synthetic-state",
            &state,
            Utc::now() + chrono::Duration::days(1),
        )
        .await
        .unwrap();
    let created: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT created_at FROM oauth_state_tokens")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    writer.disconnect_uplink("42", Utc::now()).await.unwrap();
    let (_, observed) = states
        .consume_with_created_at("synthetic-state", Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created, observed);
    assert!(observed < database_time(&db).await);
    assert!(states
        .consume_with_created_at("synthetic-state", Utc::now())
        .await
        .unwrap()
        .is_none());
    let mut old = grant("42", "uplink", "synthetic-uplink");
    old.state_created_at = observed;
    assert!(matches!(
        writer
            .store_new_auth(&old, &VerifiedClient, Utc::now())
            .await,
        Err(tb_raid::AuthWriteError::DisconnectedSinceAuthorization)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_raid_auth")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

struct OldValidation {
    failure: &'static str,
}
#[async_trait::async_trait]
impl TwitchTokenClient for OldValidation {
    async fn validate_token(
        &self,
        token: &str,
        uid: &str,
    ) -> Result<TokenValidation, RefreshError> {
        if token == "synthetic-uplink" {
            match self.failure {
                "revoked" => return Err(RefreshError::InvalidGrant),
                "network" => return Err(RefreshError::Other("private upstream detail".into())),
                "expired" => {
                    let mut v = VerifiedClient.validate_token(token, uid).await?;
                    v.expires_in = 0;
                    return Ok(v);
                }
                "foreign" => {
                    let mut v = VerifiedClient.validate_token(token, uid).await?;
                    v.twitch_user_id = "99".into();
                    return Ok(v);
                }
                "extra" => {
                    let mut v = VerifiedClient.validate_token(token, uid).await?;
                    v.scopes.push("old:right".into());
                    return Ok(v);
                }
                _ => unreachable!(),
            }
        }
        let mut v = VerifiedClient.validate_token(token, uid).await?;
        if self.failure == "extra" {
            v.scopes.push("new:right".into());
        }
        Ok(v)
    }
    async fn refresh(&self, _: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn exchange_code(&self, _: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!()
    }
}
#[tokio::test]
async fn revoked_expired_or_foreign_old_grants_cannot_be_preserved() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    for failure in ["revoked", "expired", "foreign"] {
        writer
            .store_new_auth(
                &grant("42", "uplink", "synthetic-uplink"),
                &VerifiedClient,
                Utc::now(),
            )
            .await
            .unwrap();
        writer
            .store_new_auth(
                &grant("42", "base", "synthetic-base"),
                &OldValidation { failure },
                Utc::now(),
            )
            .await
            .unwrap();
        let (tokens, scopes) = snapshot(&db).await;
        assert_eq!(tokens.access_token, "synthetic-base");
        assert!(!scopes.iter().any(|s| s == "user:read:chat"));
    }
}
#[tokio::test]
async fn unknown_old_validity_or_incomparable_rights_do_not_silently_overwrite() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    writer
        .store_new_auth(
            &grant("42", "uplink", "synthetic-uplink"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    for failure in ["network", "extra"] {
        assert!(writer
            .store_new_auth(
                &grant("42", "base", "synthetic-base"),
                &OldValidation { failure },
                Utc::now()
            )
            .await
            .is_err());
        assert_eq!(snapshot(&db).await.0.access_token, "synthetic-uplink");
    }
}

struct PausedClient {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    invalid: bool,
    pause_validation: bool,
    once: std::sync::atomic::AtomicBool,
}
impl PausedClient {
    fn new(invalid: bool, pause_validation: bool) -> Arc<Self> {
        Arc::new(Self {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            invalid,
            pause_validation,
            once: std::sync::atomic::AtomicBool::new(false),
        })
    }
}
#[async_trait::async_trait]
impl TwitchTokenClient for PausedClient {
    async fn validate_token(
        &self,
        token: &str,
        uid: &str,
    ) -> Result<TokenValidation, RefreshError> {
        if self.pause_validation && !self.once.swap(true, std::sync::atomic::Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        VerifiedClient.validate_token(token, uid).await
    }
    async fn refresh(&self, _: &str) -> Result<TokenResponse, RefreshError> {
        self.entered.notify_one();
        self.release.notified().await;
        if self.invalid {
            return Err(RefreshError::InvalidGrant);
        }
        Ok(TokenResponse {
            access_token: "synthetic-uplink-refreshed".into(),
            refresh_token: "synthetic-refresh-rotated".into(),
            expires_in: 7200,
            scopes: vec!["untrusted:exchange-scope".into()],
        })
    }
    async fn exchange_code(&self, _: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!()
    }
}
async fn wait_for_waiting_writer(db: &Database) {
    tokio::time::timeout(Duration::from_secs(2),async{
    loop{let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname='postgres' AND wait_event='advisory'").fetch_one(&db.pool).await.unwrap();if waiting>0{break}tokio::time::sleep(Duration::from_millis(10)).await;}
 }).await.expect("Konkurrierender Writer hat keinen echten UID-Lock erreicht");
}
async fn expire(db: &Database) {
    sqlx::query("UPDATE twitch_raid_auth SET token_expires_at=clock_timestamp()-interval '1 hour'")
        .execute(&db.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn refresh_and_uplink_disconnect_serialize_without_revoking_shared_grant() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    writer
        .store_new_auth(
            &grant("42", "uplink", "synthetic-uplink"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    expire(&db).await;
    let client = PausedClient::new(false, false);
    let refresher = tb_raid::RaidTokenRefresher::new(
        db.pool.clone(),
        cipher(),
        client.clone(),
        Arc::new(tb_raid::TokenBlacklistStore::new(db.pool.clone())),
    );
    let refresh = tokio::spawn(async move {
        refresher
            .refresh_and_store("42", "synthetic", "stale snapshot", Utc::now())
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), client.entered.notified())
        .await
        .unwrap();
    let disconnect = tokio::spawn(async move { writer.disconnect_uplink("42", Utc::now()).await });
    wait_for_waiting_writer(&db).await;
    assert!(!disconnect.is_finished());
    client.release.notify_one();
    assert_eq!(
        refresh.await.unwrap().unwrap(),
        tb_raid::RefreshOutcome::Refreshed
    );
    disconnect.await.unwrap().unwrap();
    let (tokens, scopes) = snapshot(&db).await;
    assert!(tokens.uplink_disconnected);
    assert!(!tokens.needs_reauth);
    assert_eq!(tokens.access_token, "synthetic-uplink-refreshed");
    assert!(!scopes.iter().any(|s| s == "untrusted:exchange-scope"));
}

#[tokio::test]
async fn invalid_refresh_blacklist_cannot_land_after_successful_callback() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    writer
        .store_new_auth(
            &grant("42", "uplink", "synthetic-uplink"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    expire(&db).await;
    let client = PausedClient::new(true, false);
    let refresher = tb_raid::RaidTokenRefresher::new(
        db.pool.clone(),
        cipher(),
        client.clone(),
        Arc::new(tb_raid::TokenBlacklistStore::new(db.pool.clone())),
    );
    let refresh = tokio::spawn(async move {
        refresher
            .refresh_and_store("42", "synthetic", "unused", Utc::now())
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), client.entered.notified())
        .await
        .unwrap();
    let callback = tokio::spawn(async move {
        writer
            .store_new_auth(
                &grant("42", "uplink", "synthetic-uplink-new"),
                &VerifiedClient,
                Utc::now(),
            )
            .await
    });
    wait_for_waiting_writer(&db).await;
    client.release.notify_one();
    assert_eq!(
        refresh.await.unwrap().unwrap(),
        tb_raid::RefreshOutcome::Blacklisted
    );
    callback.await.unwrap().unwrap();
    let (tokens, _) = snapshot(&db).await;
    assert!(!tokens.needs_reauth);
    assert_eq!(tokens.access_token, "synthetic-uplink-new");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_token_blacklist")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn disconnect_waits_for_callback_and_canceled_callback_releases_the_lock() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    let client = PausedClient::new(false, true);
    let c = client.clone();
    let w = writer.clone();
    let callback = tokio::spawn(async move {
        w.store_new_auth(
            &grant("42", "uplink", "synthetic-uplink"),
            c.as_ref(),
            Utc::now(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), client.entered.notified())
        .await
        .unwrap();
    let disconnect = tokio::spawn(async move { writer.disconnect_uplink("42", Utc::now()).await });
    wait_for_waiting_writer(&db).await;
    callback.abort();
    assert!(callback.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(2), disconnect)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let enabled: bool = sqlx::query_scalar(
        "SELECT enabled FROM twitch_uplink_auth_intent WHERE twitch_user_id='42'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(!enabled);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_raid_auth")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn concurrent_reader_never_combines_a_token_with_another_grants_scopes() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    writer
        .store_new_auth(
            &grant("42", "uplink", "synthetic-uplink"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    let store = RaidAuthStore::new(db.pool.clone(), cipher());
    let writer_task = tokio::spawn(async move {
        for _ in 0..20 {
            writer
                .store_new_auth(
                    &grant("42", "base", "synthetic-base"),
                    &OldValidation { failure: "revoked" },
                    Utc::now(),
                )
                .await
                .unwrap();
            writer
                .store_new_auth(
                    &grant("42", "uplink", "synthetic-uplink"),
                    &VerifiedClient,
                    Utc::now(),
                )
                .await
                .unwrap();
        }
    });
    for _ in 0..100 {
        let (tokens, scopes) = store
            .load_decrypted_with_scopes("42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            tokens.access_token.contains("uplink"),
            scopes.iter().any(|s| s == "user:read:chat")
        );
    }
    writer_task.await.unwrap();
}

#[tokio::test]
async fn invalid_new_token_and_missing_refresh_leave_no_auth_or_intent() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    for failure in ["revoked", "expired", "foreign"] {
        assert!(writer
            .store_new_auth(
                &grant("42", "uplink", "synthetic-uplink"),
                &OldValidation { failure },
                Utc::now()
            )
            .await
            .is_err());
    }
    let mut missing = grant("42", "uplink", "synthetic-uplink");
    missing.refresh_token.clear();
    assert!(writer
        .store_new_auth(&missing, &VerifiedClient, Utc::now())
        .await
        .is_err());
    let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM twitch_raid_auth)+(SELECT count(*) FROM twitch_uplink_auth_intent)").fetch_one(&db.pool).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn refresh_blacklist_uses_the_held_transaction_with_a_single_connection() {
    let db = Database::new().await;
    let writer = AuthWriter::new(db.pool.clone(), cipher());
    writer
        .store_new_auth(
            &grant("42", "uplink", "synthetic-uplink"),
            &VerifiedClient,
            Utc::now(),
        )
        .await
        .unwrap();
    expire(&db).await;
    let single = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(300))
        .connect_lazy_with((*db.pool.connect_options()).clone());
    let client = PausedClient::new(true, false);
    client.release.notify_one();
    let refresher = tb_raid::RaidTokenRefresher::new(
        single.clone(),
        cipher(),
        client,
        Arc::new(tb_raid::TokenBlacklistStore::new(single.clone())),
    );
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        refresher.refresh_and_store("42", "synthetic", "unused", Utc::now()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result, tb_raid::RefreshOutcome::Blacklisted);
    assert!(snapshot(&db).await.0.needs_reauth);
    let count: i32 = sqlx::query_scalar(
        "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id='42'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    single.close().await;
}

#[tokio::test]
async fn sensitive_auth_debug_output_is_redacted() {
    let auth = grant("42", "uplink", "synthetic-private-access");
    assert!(!format!("{auth:?}").contains("synthetic-private-access"));
    let response = TokenResponse {
        access_token: "synthetic-private-access".into(),
        refresh_token: "synthetic-private-refresh".into(),
        expires_in: 100,
        scopes: vec![],
    };
    assert!(!format!("{response:?}").contains("synthetic-private"));
}
