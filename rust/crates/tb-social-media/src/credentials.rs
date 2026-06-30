//! Lese-Pfad für verschlüsselte Plattform-OAuth-Credentials (Port von
//! `bot/social_media/credential_manager.py`).
//!
//! Lädt Tokens für TikTok/YouTube/Instagram aus `social_media_platform_auth`,
//! entschlüsselt sie (AES-256-GCM, AAD-gebunden) und unterstützt per-Streamer-
//! Credentials mit Fallback auf globale. AAD-Format identisch zu Python:
//! `social_media_platform_auth|<column>|<platform>|<streamer|global>|<enc_version>`
//! (= [`tb_crypto::aad::social_media`]).
//!
//! Secret-Disziplin: Klartext-Tokens werden NIE geloggt — nur die Tatsache eines
//! Decrypt-Fehlers (mit sanitisierten platform/streamer-Werten).

use std::sync::Arc;

use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

/// Plattformen mit Social-Media-Anbindung (Python-Reihenfolge).
pub const PLATFORMS: [&str; 3] = ["tiktok", "youtube", "instagram"];

/// Entschlüsselte Plattform-Credentials.
#[derive(Debug, Clone)]
pub struct SocialMediaCredentials {
    pub id: i32,
    pub platform: String,
    pub streamer_login: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub expires_at: Option<String>,
    pub scopes: Option<String>,
    pub platform_user_id: Option<String>,
    pub platform_username: Option<String>,
}

/// Verbindungs-Status einer Plattform (für das Dashboard).
#[derive(Debug, Clone)]
pub struct PlatformStatus {
    pub platform: String,
    pub connected: bool,
    pub username: Option<String>,
    pub user_id: Option<String>,
    pub expires_at: Option<String>,
    pub expired: bool,
    pub scopes: Option<String>,
    pub uses_global_fallback: bool,
}

/// Lädt + entschlüsselt Plattform-Credentials.
pub struct CredentialManager {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
}

impl CredentialManager {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Holt + entschlüsselt Credentials für eine Plattform. Bevorzugt einen
    /// exakten Streamer-Treffer, sonst den globalen Eintrag (Python
    /// `get_credentials`). `None` bei fehlendem Eintrag oder Decrypt-Fehler.
    pub async fn get_credentials(
        &self,
        platform: &str,
        streamer_login: Option<&str>,
    ) -> Option<SocialMediaCredentials> {
        let row = sqlx::query!(
            "SELECT id, platform, streamer_login, access_token_enc, refresh_token_enc, \
                    client_id, client_secret_enc, token_expires_at, scopes, \
                    platform_user_id, platform_username, enc_version \
             FROM social_media_platform_auth \
             WHERE platform = $1 AND enabled = 1 AND ( \
                   streamer_login = $2 \
                   OR ($2 IS NOT NULL AND streamer_login IS NULL) \
                   OR ($2 IS NULL AND streamer_login IS NULL)) \
             ORDER BY CASE WHEN streamer_login = $2 THEN 1 ELSE 0 END DESC, \
                      authorized_at DESC, id DESC \
             LIMIT 1",
            platform,
            streamer_login
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;

        let id = row.id;
        let row_platform = row.platform;
        let row_streamer = row.streamer_login;
        let access_enc = row.access_token_enc;
        let refresh_enc = row.refresh_token_enc;
        let client_secret_enc = row.client_secret_enc;
        let enc_version = row.enc_version.unwrap_or(1) as i64;
        let streamer_ref = row_streamer.as_deref();

        let access_token = match self.cipher.decrypt_field(
            &access_enc,
            &aad::social_media("access_token", &row_platform, streamer_ref, enc_version),
        ) {
            Ok(t) => t,
            Err(_) => {
                tracing::error!(
                    platform = %sanitize(platform),
                    streamer = %sanitize(streamer_login.unwrap_or("<none>")),
                    "Decrypt des Auth-Records fehlgeschlagen"
                );
                return None;
            }
        };
        let refresh_token = refresh_enc.and_then(|b| {
            self.cipher
                .decrypt_field(
                    &b,
                    &aad::social_media("refresh_token", &row_platform, streamer_ref, enc_version),
                )
                .ok()
        });
        let client_secret = client_secret_enc.and_then(|b| {
            self.cipher
                .decrypt_field(
                    &b,
                    &aad::social_media("client_secret", &row_platform, streamer_ref, enc_version),
                )
                .ok()
        });

        Some(SocialMediaCredentials {
            id,
            platform: row_platform,
            streamer_login: row_streamer,
            access_token,
            refresh_token,
            client_id: row.client_id,
            client_secret,
            expires_at: row.token_expires_at,
            scopes: row.scopes,
            platform_user_id: row.platform_user_id,
            platform_username: row.platform_username,
        })
    }

    /// Verbindungs-Status aller drei Plattformen (Python
    /// `get_all_platforms_status`).
    pub async fn get_all_platforms_status(
        &self,
        streamer_login: Option<&str>,
    ) -> Vec<PlatformStatus> {
        let mut out = Vec::with_capacity(PLATFORMS.len());
        for platform in PLATFORMS {
            let status = match self.get_credentials(platform, streamer_login).await {
                Some(creds) => {
                    let uses_global_fallback =
                        streamer_login.is_some() && creds.streamer_login.is_none();
                    PlatformStatus {
                        platform: platform.to_string(),
                        connected: true,
                        username: creds.platform_username.clone(),
                        user_id: creds.platform_user_id.clone(),
                        expires_at: creds.expires_at.clone(),
                        expired: token_expired(creds.expires_at.as_deref(), now_ts()),
                        scopes: creds.scopes.clone(),
                        uses_global_fallback,
                    }
                }
                None => PlatformStatus {
                    platform: platform.to_string(),
                    connected: false,
                    username: None,
                    user_id: None,
                    expires_at: None,
                    expired: false,
                    scopes: None,
                    uses_global_fallback: false,
                },
            };
            out.push(status);
        }
        out
    }

    /// True, wenn der Token abgelaufen ist oder binnen 1h abläuft.
    pub fn is_token_expired(&self, credentials: &SocialMediaCredentials) -> bool {
        token_expired(credentials.expires_at.as_deref(), now_ts())
    }
}

fn now_ts() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

/// Pure Ablauf-Prüfung (Python `is_token_expired`): kein/leerer/unparsebarer Wert
/// → abgelaufen; sonst Restzeit < 3600s → abgelaufen.
fn token_expired(expires_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> bool {
    let Some(raw) = expires_at.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    let normalized = raw.replace('Z', "+00:00");
    match chrono::DateTime::parse_from_rfc3339(&normalized) {
        Ok(exp) => (exp.with_timezone(&chrono::Utc) - now).num_seconds() < 3600,
        Err(_) => true,
    }
}

/// Verhindert CRLF-Log-Forging (Python `_sanitize_log_value`).
fn sanitize(value: &str) -> String {
    value.replace('\r', "\\r").replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap())
    }

    #[test]
    fn token_expired_faelle() {
        let now = chrono::Utc::now();
        assert!(token_expired(None, now)); // fehlend
        assert!(token_expired(Some("   "), now)); // leer
        assert!(token_expired(Some("kaputt"), now)); // unparsebar
                                                     // In 30min → < 1h → abgelaufen.
        assert!(token_expired(
            Some(&(now + Duration::minutes(30)).to_rfc3339()),
            now
        ));
        // In 2h → frisch.
        assert!(!token_expired(
            Some(&(now + Duration::hours(2)).to_rfc3339()),
            now
        ));
        // Z-Suffix wird normalisiert.
        let z = (now + Duration::hours(2))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        assert!(!token_expired(Some(&z), now));
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
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
                enc_kid TEXT DEFAULT 'v1', authorized_at TEXT DEFAULT CURRENT_TIMESTAMP, \
                enabled INTEGER DEFAULT 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    /// Fügt einen verschlüsselten Auth-Record ein.
    async fn seed(
        pool: &PgPool,
        c: &FieldCipher,
        platform: &str,
        streamer: Option<&str>,
        access: &str,
        refresh: Option<&str>,
    ) {
        let access_enc = c
            .encrypt_field(
                access,
                &aad::social_media("access_token", platform, streamer, 1),
            )
            .unwrap();
        let refresh_enc = refresh.map(|r| {
            c.encrypt_field(
                r,
                &aad::social_media("refresh_token", platform, streamer, 1),
            )
            .unwrap()
        });
        sqlx::query(
            "INSERT INTO social_media_platform_auth (platform, streamer_login, access_token_enc, refresh_token_enc, platform_username, enabled) \
             VALUES ($1, $2, $3, $4, 'theuser', 1)",
        )
        .bind(platform)
        .bind(streamer)
        .bind(access_enc)
        .bind(refresh_enc)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn get_credentials_entschluesselt_und_fallback() {
        let Some(pool) = make_pool("t_sm_creds").await else {
            return;
        };
        let c = cipher();
        // Globaler TikTok-Eintrag + streamer-spezifischer für 'nani'.
        seed(
            &pool,
            &c,
            "tiktok",
            None,
            "global-access",
            Some("global-refresh"),
        )
        .await;
        seed(&pool, &c, "tiktok", Some("nani"), "nani-access", None).await;
        let mgr = CredentialManager::new(pool.clone(), c);

        // Exakter Streamer-Treffer bevorzugt.
        let creds = mgr.get_credentials("tiktok", Some("nani")).await.unwrap();
        assert_eq!(creds.access_token, "nani-access");
        assert_eq!(creds.streamer_login.as_deref(), Some("nani"));
        assert_eq!(creds.platform_username.as_deref(), Some("theuser"));

        // Unbekannter Streamer → globaler Fallback.
        let creds = mgr.get_credentials("tiktok", Some("wer")).await.unwrap();
        assert_eq!(creds.access_token, "global-access");
        assert!(creds.streamer_login.is_none());
        assert_eq!(creds.refresh_token.as_deref(), Some("global-refresh"));

        // Plattform ohne Eintrag → None.
        assert!(mgr.get_credentials("youtube", None).await.is_none());
    }

    #[tokio::test]
    async fn all_platforms_status() {
        let Some(pool) = make_pool("t_sm_creds_status").await else {
            return;
        };
        let c = cipher();
        seed(&pool, &c, "youtube", None, "yt-access", None).await;
        let mgr = CredentialManager::new(pool, c);
        let status = mgr.get_all_platforms_status(None).await;
        assert_eq!(status.len(), 3);
        let yt = status.iter().find(|s| s.platform == "youtube").unwrap();
        assert!(yt.connected);
        let tk = status.iter().find(|s| s.platform == "tiktok").unwrap();
        assert!(!tk.connected);
    }
}
