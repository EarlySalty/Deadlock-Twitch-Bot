use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

use crate::oauth::{OAuthError, OAuthManager};
use crate::reauth::{ReauthState, SocialReauthNotifier};

const INTERVAL_SECONDS: u64 = 5 * 60;
const INITIAL_DELAY_SECONDS: u64 = 60;
const REFRESH_THRESHOLD_HOURS: i64 = 1;
/// Instagram-Token laufen 60 Tage und lassen sich nur verlaengern, solange sie
/// noch gueltig sind. Eine Stunde Vorlauf wie bei den Stundentoken der anderen
/// Plattformen waere hier zu knapp: faellt der Worker ueber ein Wochenende aus,
/// ist der Zugang unwiederbringlich weg und der Streamer muss neu verbinden.
const INSTAGRAM_THRESHOLD_DAYS: i64 = 7;

/// Periodischer Auto-Refresh ablaufender Plattform-Tokens.
pub struct TokenRefreshWorker {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
    oauth: OAuthManager,
    reauth: ReauthState,
}

impl TokenRefreshWorker {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>, oauth: OAuthManager) -> Self {
        Self {
            reauth: ReauthState::new(pool.clone()),
            pool,
            cipher,
            oauth,
        }
    }

    pub fn with_notifier(mut self, notifier: Arc<dyn SocialReauthNotifier>) -> Self {
        self.reauth = self.reauth.with_notifier(notifier);
        self
    }

    /// Loop: 60s Initial-Delay, dann alle 5 min. Best-effort.
    pub async fn run(self) {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECONDS)).await;
        loop {
            self.run_once().await;
            tokio::time::sleep(Duration::from_secs(INTERVAL_SECONDS)).await;
        }
    }

    /// Eine Refresh-Runde: alle binnen 1h ablaufenden Tokens mit Refresh-Token
    /// erneuern (Python `_refresh_expiring_tokens`).
    pub async fn run_once(&self) {
        self.run_once_at(Utc::now()).await;
    }

    async fn run_once_at(&self, jetzt: chrono::DateTime<Utc>) {
        let kurz = (jetzt + chrono::Duration::hours(REFRESH_THRESHOLD_HOURS)).to_rfc3339();
        let lang = (jetzt + chrono::Duration::days(INSTAGRAM_THRESHOLD_DAYS)).to_rfc3339();
        // Instagram hat keinen Refresh-Token, das Langzeit-Token verlaengert
        // sich selbst. Die alte Bedingung `refresh_token_enc IS NOT NULL` hat
        // Instagram deshalb nie ausgewaehlt: der Zugang starb nach 60 Tagen,
        // ohne dass irgendwo etwas passierte.
        let rows = match sqlx::query!(
            "SELECT id AS \"id!\", platform AS \"platform!\", streamer_login, access_token_enc AS \"access_token_enc!\", \
                    refresh_token_enc, client_id, client_secret_enc, \
                    token_expires_at, refresh_expires_at, enc_version \
             FROM social_media_platform_auth \
             WHERE enabled = 1 AND NOT needs_reauth AND token_expires_at IS NOT NULL \
               AND ( refresh_expires_at <= $3 \
                     OR (platform = 'tiktok' AND refresh_token_enc IS NOT NULL AND refresh_expires_at IS NULL) \
                     OR (platform = 'instagram' AND token_expires_at < $2) \
                     OR (platform <> 'instagram' AND refresh_token_enc IS NOT NULL AND token_expires_at < $1) ) \
             ORDER BY token_expires_at ASC",
            &kurz,
            &lang,
            jetzt,
        )
        .fetch_all(&self.pool)
        .await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "Social refresh selection failed");
                return;
            }
        };

        for row in rows {
            let instagram_expired = row.platform == "instagram"
                && row
                    .token_expires_at
                    .as_deref()
                    .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
                    .is_some_and(|expiry| expiry <= jetzt);
            if instagram_expired || row.refresh_expires_at.is_some_and(|expiry| expiry <= jetzt) {
                if let Err(error) = self
                    .reauth
                    .mark_required(row.id, &row.access_token_enc)
                    .await
                {
                    tracing::error!(%error, "Social reauth state failed");
                }
                continue;
            }
            self.refresh_one(
                row.platform,
                row.id,
                row.streamer_login,
                row.access_token_enc,
                row.refresh_token_enc,
                row.client_id,
                row.client_secret_enc,
                row.enc_version.unwrap_or(1) as i64,
            )
            .await;
        }
        if let Err(error) = self.reauth.sweep(jetzt).await {
            tracing::error!(%error, "Social reauth sweep failed");
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn refresh_one(
        &self,
        platform: String,
        auth_id: i32,
        streamer: Option<String>,
        access_enc: Vec<u8>,
        refresh_enc: Option<Vec<u8>>,
        client_id: Option<String>,
        client_secret_enc: Option<Vec<u8>>,
        enc_version: i64,
    ) {
        let streamer_ref = streamer.as_deref();

        // Instagram verlaengert sein eigenes Access-Token; alle anderen
        // Plattformen brauchen den gespeicherten Refresh-Token.
        let geheimnis = if platform == "instagram" {
            self.cipher.decrypt_field(
                &access_enc,
                &aad::social_media("access_token", &platform, streamer_ref, enc_version),
            )
        } else {
            let Some(refresh_enc) = refresh_enc else {
                tracing::error!(platform = %platform, "Refresh ohne gespeicherten Refresh-Token");
                return;
            };
            self.cipher.decrypt_field(
                &refresh_enc,
                &aad::social_media("refresh_token", &platform, streamer_ref, enc_version),
            )
        };
        let Ok(refresh_token) = geheimnis else {
            tracing::error!(platform = %platform, "Token-Decrypt fuer den Refresh fehlgeschlagen");
            return;
        };
        let client_secret = client_secret_enc.and_then(|b| {
            self.cipher
                .decrypt_field(
                    &b,
                    &aad::social_media("client_secret", &platform, streamer_ref, enc_version),
                )
                .ok()
        });

        let new_tokens = match self
            .oauth
            .refresh_token(
                &platform,
                &refresh_token,
                client_id.as_deref().unwrap_or(""),
                client_secret.as_deref().unwrap_or(""),
            )
            .await
        {
            Ok(t) => t,
            Err(error) => {
                if matches!(error, OAuthError::ReauthRequired { .. }) {
                    if let Err(error) = self.reauth.mark_required(auth_id, &access_enc).await {
                        tracing::error!(%error, "Social reauth state failed");
                    }
                }
                tracing::error!(platform = %platform, %error, "Token-Refresh fehlgeschlagen");
                return;
            }
        };

        self.save_refreshed(
            auth_id,
            &platform,
            streamer_ref,
            enc_version,
            &access_enc,
            &new_tokens,
        )
        .await;
    }

    async fn save_refreshed(
        &self,
        auth_id: i32,
        platform: &str,
        streamer: Option<&str>,
        enc_version: i64,
        previous_access_enc: &[u8],
        new_tokens: &crate::oauth::RefreshedTokens,
    ) {
        let Ok(access_enc) = self.cipher.encrypt_field(
            &new_tokens.access_token,
            &aad::social_media("access_token", platform, streamer, enc_version),
        ) else {
            tracing::error!(platform = %platform, "Refresh-Persist: encrypt access fehlgeschlagen");
            return;
        };
        let refresh_enc = match new_tokens
            .refresh_token
            .as_ref()
            .map(|token| {
                self.cipher.encrypt_field(
                    token,
                    &aad::social_media("refresh_token", platform, streamer, enc_version),
                )
            })
            .transpose()
        {
            Ok(value) => value,
            Err(_) => {
                tracing::error!(platform = %platform, "Refresh-Persist: encrypt refresh failed");
                return;
            }
        };
        let expires_iso = new_tokens.expires_at.to_rfc3339();

        let result = sqlx::query!(
            "UPDATE social_media_platform_auth \
             SET access_token_enc = $1, refresh_token_enc = COALESCE($2, refresh_token_enc), token_expires_at = $3, \
                 refresh_expires_at = CASE WHEN platform = 'tiktok' THEN COALESCE($4, refresh_expires_at) ELSE NULL END, \
                 last_refreshed_at = CURRENT_TIMESTAMP \
             WHERE id = $5 AND access_token_enc = $6 AND enabled = 1 AND NOT needs_reauth",
            access_enc,
            refresh_enc,
            &expires_iso,
            new_tokens.refresh_expires_at,
            auth_id,
            previous_access_enc,
        )
        .execute(&self.pool)
        .await;
        if let Err(error) = result {
            tracing::error!(platform = %platform, %error, "Refresh-Persist fehlgeschlagen");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap())
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE social_media_platform_auth (\
                id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, platform TEXT NOT NULL, \
                streamer_login TEXT, access_token_enc BYTEA NOT NULL, refresh_token_enc BYTEA, \
                client_id TEXT, client_secret_enc BYTEA, token_expires_at TEXT, scopes TEXT, \
                platform_user_id TEXT, platform_username TEXT, enc_version INTEGER DEFAULT 1, \
                enc_kid TEXT DEFAULT 'v1', last_refreshed_at TEXT, enabled INTEGER DEFAULT 1, \
                twitch_user_id TEXT, refresh_expires_at TIMESTAMPTZ, needs_reauth BOOLEAN NOT NULL DEFAULT FALSE, \
                reauth_required_at TIMESTAMPTZ, reauth_notified_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn fixed_now() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-10-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn seed_refresh(pool: &PgPool, c: &FieldCipher, platform: &str) {
        let access = c
            .encrypt_field(
                "local-access",
                &aad::social_media("access_token", platform, None, 1),
            )
            .unwrap();
        let refresh = c
            .encrypt_field(
                "local-refresh",
                &aad::social_media("refresh_token", platform, None, 1),
            )
            .unwrap();
        sqlx::query("INSERT INTO social_media_platform_auth (platform, access_token_enc, refresh_token_enc, token_expires_at, twitch_user_id) VALUES ($1, $2, $3, $4, '42')")
            .bind(platform).bind(access).bind(refresh).bind((fixed_now() + chrono::Duration::minutes(10)).to_rfc3339())
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn permanent_refresh_error_marks_once_while_transient_errors_keep_access() {
        for (status, required) in [(400, true), (429, false), (503, false)] {
            let pool = make_pool(&format!("t_sm_worker_error_{status}"))
                .await
                .unwrap();
            let cipher = cipher();
            seed_refresh(&pool, &cipher, "youtube").await;
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_json(serde_json::json!({"error": "invalid_grant"})),
                )
                .expect(if required { 1 } else { 2 })
                .mount(&server)
                .await;
            let oauth = OAuthManager::new(pool.clone(), cipher.clone()).with_token_urls(
                "http://127.0.0.1:1".into(),
                format!("{}/token", server.uri()),
                "http://127.0.0.1:1".into(),
            );
            let worker = TokenRefreshWorker::new(pool.clone(), cipher, oauth);
            worker.run_once_at(fixed_now()).await;
            worker.run_once_at(fixed_now()).await;
            let state: (bool, bool, String) = sqlx::query_as(
                "SELECT needs_reauth, reauth_required_at IS NOT NULL, token_expires_at FROM social_media_platform_auth",
            ).fetch_one(&pool).await.unwrap();
            assert_eq!(state.0, required);
            assert_eq!(state.1, required);
            assert_eq!(
                state.2,
                (fixed_now() + chrono::Duration::minutes(10)).to_rfc3339()
            );
        }
    }

    #[tokio::test]
    async fn first_tiktok_refresh_backfills_real_refresh_expiry() {
        let pool = make_pool("t_sm_worker_tiktok_expiry").await.unwrap();
        let cipher = cipher();
        seed_refresh(&pool, &cipher, "tiktok").await;
        sqlx::query("UPDATE social_media_platform_auth SET token_expires_at = $1")
            .bind((fixed_now() + chrono::Duration::days(1)).to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-local-access", "refresh_token": "new-local-refresh",
                "expires_in": 86400, "refresh_expires_in": 31536000
            })))
            .expect(1)
            .mount(&server)
            .await;
        let oauth = OAuthManager::new(pool.clone(), cipher.clone()).with_token_urls(
            format!("{}/token", server.uri()),
            "http://127.0.0.1:1".into(),
            "http://127.0.0.1:1".into(),
        );
        let worker = TokenRefreshWorker::new(pool.clone(), cipher, oauth);
        worker.run_once_at(fixed_now()).await;
        let stored: bool = sqlx::query_scalar(
            "SELECT refresh_expires_at > token_expires_at::timestamptz AND NOT needs_reauth AND reauth_required_at IS NULL FROM social_media_platform_auth",
        ).fetch_one(&pool).await.unwrap();
        assert!(stored);
    }

    #[tokio::test]
    async fn expired_instagram_does_not_attempt_impossible_refresh() {
        let pool = make_pool("t_sm_worker_instagram_expired").await.unwrap();
        let cipher = cipher();
        seed_refresh(&pool, &cipher, "instagram").await;
        sqlx::query(
            "UPDATE social_media_platform_auth SET token_expires_at = $1, refresh_token_enc = NULL",
        )
        .bind((fixed_now() - chrono::Duration::days(1)).to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/refresh_access_token"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let oauth =
            OAuthManager::new(pool.clone(), cipher.clone()).with_instagram_graph(server.uri());
        TokenRefreshWorker::new(pool.clone(), cipher, oauth)
            .run_once_at(fixed_now())
            .await;
        let required: bool =
            sqlx::query_scalar("SELECT needs_reauth FROM social_media_platform_auth")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(required);
    }

    #[tokio::test]
    async fn refresh_youtube_aktualisiert_access_behaelt_refresh() {
        let Some(pool) = make_pool("t_sm_refresh").await else {
            return;
        };
        let c = cipher();
        // Ablaufender YouTube-Token (in 10min < 1h Threshold) mit Refresh.
        let soon = (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
        let access_enc = c
            .encrypt_field(
                "old-access",
                &aad::social_media("access_token", "youtube", None, 1),
            )
            .unwrap();
        let refresh_enc = c
            .encrypt_field(
                "the-refresh",
                &aad::social_media("refresh_token", "youtube", None, 1),
            )
            .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_auth (platform, streamer_login, access_token_enc, refresh_token_enc, client_id, token_expires_at, enabled) \
             VALUES ('youtube', NULL, $1, $2, 'cid', $3, 1)",
        )
        .bind(access_enc).bind(refresh_enc).bind(&soon)
        .execute(&pool).await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fresh-access", "expires_in": 3600
            })))
            .mount(&server)
            .await;
        let oauth = OAuthManager::new(pool.clone(), c.clone()).with_token_urls(
            "http://127.0.0.1:1".into(),
            format!("{}/token", server.uri()),
            "http://127.0.0.1:1".into(),
        );
        let worker = TokenRefreshWorker::new(pool.clone(), c.clone(), oauth);
        worker.run_once().await;

        // Access neu + entschlüsselbar, Refresh erhalten, Ablauf in der Zukunft.
        let (access_enc, refresh_present, expires): (Vec<u8>, bool, String) = sqlx::query_as(
            "SELECT access_token_enc, refresh_token_enc IS NOT NULL, token_expires_at FROM social_media_platform_auth WHERE platform='youtube'",
        )
        .fetch_one(&pool).await.unwrap();
        let dec = c
            .decrypt_field(
                &access_enc,
                &aad::social_media("access_token", "youtube", None, 1),
            )
            .unwrap();
        assert_eq!(dec, "fresh-access");
        assert!(refresh_present); // YouTube liefert keinen neuen → alter bleibt
        assert!(expires > Utc::now().to_rfc3339());
    }

    #[tokio::test]
    async fn nicht_ablaufende_werden_uebersprungen() {
        let Some(pool) = make_pool("t_sm_refresh_skip").await else {
            return;
        };
        let c = cipher();
        // Token läuft erst in 3h ab → außerhalb des 1h-Thresholds.
        let later = (Utc::now() + chrono::Duration::hours(3)).to_rfc3339();
        let access_enc = c
            .encrypt_field("a", &aad::social_media("access_token", "youtube", None, 1))
            .unwrap();
        let refresh_enc = c
            .encrypt_field("r", &aad::social_media("refresh_token", "youtube", None, 1))
            .unwrap();
        sqlx::query("INSERT INTO social_media_platform_auth (platform, access_token_enc, refresh_token_enc, token_expires_at, enabled) VALUES ('youtube', $1, $2, $3, 1)")
            .bind(access_enc).bind(refresh_enc).bind(&later).execute(&pool).await.unwrap();
        // Kein Mock nötig — run_once darf gar nicht refreshen.
        let oauth = OAuthManager::new(pool.clone(), c.clone());
        TokenRefreshWorker::new(pool.clone(), c.clone(), oauth)
            .run_once()
            .await;
        let access: Vec<u8> = sqlx::query_scalar(
            "SELECT access_token_enc FROM social_media_platform_auth WHERE platform='youtube'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            c.decrypt_field(
                &access,
                &aad::social_media("access_token", "youtube", None, 1)
            )
            .unwrap(),
            "a"
        );
    }
}
