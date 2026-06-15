//! Multi-Plattform-OAuth für die Social-Media-Pipeline (Port von
//! `bot/social_media/oauth_manager.py`).
//!
//! Unterstützt TikTok / YouTube / Instagram. Diese Slice (O1) deckt die
//! Authorize-Seite ab: PKCE-Generierung, plattform-spezifische Authorize-URLs
//! und das Persistieren des CSRF-State in `oauth_state_tokens`. Code-Exchange +
//! Token-Persist (Schreib-Seite zu [`crate::credentials`]) + Refresh folgen.

use base64::Engine;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// State-Token-Gültigkeit (Python: 10 min).
const STATE_TTL_MINUTES: i64 = 10;

/// Fehler des OAuth-Flows.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("{0} nicht konfiguriert (Env fehlt)")]
    MissingConfig(&'static str),
    #[error("unbekannte Plattform: {0}")]
    UnknownPlatform(String),
    #[error("DB-Fehler: {0}")]
    Db(#[from] sqlx::Error),
    #[error("URL-Bau fehlgeschlagen: {0}")]
    Url(String),
}

/// OAuth-Manager (Authorize-Seite; Pool für die State-Persistenz).
pub struct OAuthManager {
    pool: PgPool,
}

impl OAuthManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Erzeugt die Authorize-URL einer Plattform und persistiert den CSRF-State
    /// (+ PKCE-Verifier) in `oauth_state_tokens` (Python `generate_auth_url`).
    pub async fn generate_auth_url(
        &self,
        platform: &str,
        streamer_login: Option<&str>,
        redirect_uri: &str,
    ) -> Result<String, OAuthError> {
        let state = tb_crypto::random_hex_token(24);
        let pkce_verifier = match platform {
            "tiktok" | "youtube" => Some(tb_crypto::random_hex_token(48)),
            _ => None,
        };

        let auth_url = match platform {
            "tiktok" => tiktok_auth_url(&state, redirect_uri, pkce_verifier.as_deref().unwrap())?,
            "youtube" => youtube_auth_url(&state, redirect_uri, pkce_verifier.as_deref().unwrap())?,
            "instagram" => instagram_auth_url(&state, redirect_uri)?,
            other => return Err(OAuthError::UnknownPlatform(other.to_string())),
        };

        let expires_at = Utc::now() + Duration::minutes(STATE_TTL_MINUTES);
        sqlx::query(
            "INSERT INTO oauth_state_tokens \
                (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at, consumed_at) \
             VALUES ($1, $2, $3, $4, $5, $6, NULL)",
        )
        .bind(&state)
        .bind(platform)
        .bind(streamer_login)
        .bind(redirect_uri)
        .bind(pkce_verifier)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok(auth_url)
    }
}

/// PKCE-S256-Challenge: `base64url(sha256(verifier))` ohne Padding.
fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Env-Var, leer → `None`.
fn env_nonempty(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

fn build_url(base: &str, params: &[(&str, &str)]) -> Result<String, OAuthError> {
    url::Url::parse_with_params(base, params)
        .map(|u| u.to_string())
        .map_err(|e| OAuthError::Url(e.to_string()))
}

fn tiktok_auth_url(state: &str, redirect_uri: &str, verifier: &str) -> Result<String, OAuthError> {
    let client_key = env_nonempty("TIKTOK_CLIENT_KEY").ok_or(OAuthError::MissingConfig("TIKTOK_CLIENT_KEY"))?;
    let challenge = pkce_challenge(verifier);
    build_url(
        "https://www.tiktok.com/v2/auth/authorize/",
        &[
            ("client_key", &client_key),
            ("scope", "user.info.basic,video.upload,video.publish"),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri),
            ("state", state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ],
    )
}

fn youtube_auth_url(state: &str, redirect_uri: &str, verifier: &str) -> Result<String, OAuthError> {
    let client_id = env_nonempty("YOUTUBE_CLIENT_ID").ok_or(OAuthError::MissingConfig("YOUTUBE_CLIENT_ID"))?;
    let challenge = pkce_challenge(verifier);
    build_url(
        "https://accounts.google.com/o/oauth2/v2/auth",
        &[
            ("client_id", &client_id),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", "https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly"),
            ("state", state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
}

fn instagram_auth_url(state: &str, redirect_uri: &str) -> Result<String, OAuthError> {
    let client_id = env_nonempty("INSTAGRAM_CLIENT_ID").ok_or(OAuthError::MissingConfig("INSTAGRAM_CLIENT_ID"))?;
    build_url(
        "https://api.instagram.com/oauth/authorize",
        &[
            ("client_id", &client_id),
            ("redirect_uri", redirect_uri),
            ("scope", "instagram_basic,instagram_content_publish"),
            ("response_type", "code"),
            ("state", state),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn pkce_challenge_ist_base64url_nopad() {
        // RFC 7636 Beispiel-Verifier → bekannter Challenge.
        let c = pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(c, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        // Kein Padding, url-safe.
        assert!(!c.contains('='));
        assert!(!c.contains('+') && !c.contains('/'));
    }

    fn with_env<F: FnOnce()>(vars: &[(&str, &str)], f: F) {
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        f();
        for (k, _) in vars {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn instagram_url_ohne_config_ist_fehler() {
        std::env::remove_var("INSTAGRAM_CLIENT_ID");
        assert!(matches!(
            instagram_auth_url("st", "https://cb"),
            Err(OAuthError::MissingConfig("INSTAGRAM_CLIENT_ID"))
        ));
    }

    #[test]
    fn tiktok_url_enthaelt_pflichtparameter() {
        with_env(&[("TIKTOK_CLIENT_KEY", "ck123")], || {
            let url = tiktok_auth_url("st-1", "https://cb/x", "verifier123").unwrap();
            assert!(url.starts_with("https://www.tiktok.com/v2/auth/authorize/?"));
            assert!(url.contains("client_key=ck123"));
            assert!(url.contains("code_challenge_method=S256"));
            assert!(url.contains("state=st-1"));
            // scope url-encoded.
            assert!(url.contains("video.upload"));
            // PKCE-Challenge entspricht dem Verifier.
            assert!(url.contains(&format!("code_challenge={}", pkce_challenge("verifier123"))));
        });
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE oauth_state_tokens (state_token TEXT PRIMARY KEY, platform TEXT, \
             streamer_login TEXT, redirect_uri TEXT, pkce_verifier TEXT, expires_at TIMESTAMPTZ, \
             consumed_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn generate_auth_url_persistiert_state() {
        let Some(pool) = make_pool("t_sm_oauth_state").await else { return };
        std::env::set_var("YOUTUBE_CLIENT_ID", "yt-cid");
        let mgr = OAuthManager::new(pool.clone());
        let url = mgr.generate_auth_url("youtube", Some("nani"), "https://cb/yt").await.unwrap();
        assert!(url.contains("client_id=yt-cid"));
        assert!(url.contains("access_type=offline"));

        // State persistiert mit PKCE-Verifier + 10min-Ablauf.
        let (platform, streamer, verifier, in_future): (String, Option<String>, Option<String>, bool) =
            sqlx::query_as(
                "SELECT platform, streamer_login, pkce_verifier, \
                        expires_at > NOW() + INTERVAL '9 minutes' \
                 FROM oauth_state_tokens LIMIT 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(platform, "youtube");
        assert_eq!(streamer.as_deref(), Some("nani"));
        assert!(verifier.is_some()); // youtube → PKCE
        assert!(in_future);
        std::env::remove_var("YOUTUBE_CLIENT_ID");
    }
}
