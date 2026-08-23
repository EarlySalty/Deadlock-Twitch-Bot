//! Token-Refresh-Worker (Port von `bot/social_media/token_refresh_worker.py`).
//!
//! Refresht Plattform-Tokens **bevor** sie ablaufen (alle 5 min, Threshold 1h).
//! `run_once` ist ohne Loop testbar. Der periodische [`TokenRefreshWorker::run`]
//! wird vom Pipeline-Cutover gespawnt (noch nicht verdrahtet).
//!
//! Der Admin-Reauth-Hinweis bei nicht-transienten Refresh-Fehlern ist in Python
//! eine **Discord-DM** (B10, von Nani ausgeschlossen) → hier nur geloggt.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

use crate::oauth::OAuthManager;

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
}

impl TokenRefreshWorker {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>, oauth: OAuthManager) -> Self {
        Self {
            pool,
            cipher,
            oauth,
        }
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
        let jetzt = Utc::now();
        let kurz = (jetzt + chrono::Duration::hours(REFRESH_THRESHOLD_HOURS)).to_rfc3339();
        let lang = (jetzt + chrono::Duration::days(INSTAGRAM_THRESHOLD_DAYS)).to_rfc3339();
        // Instagram hat keinen Refresh-Token, das Langzeit-Token verlaengert
        // sich selbst. Die alte Bedingung `refresh_token_enc IS NOT NULL` hat
        // Instagram deshalb nie ausgewaehlt: der Zugang starb nach 60 Tagen,
        // ohne dass irgendwo etwas passierte.
        let rows = sqlx::query!(
            "SELECT platform AS \"platform!\", streamer_login, access_token_enc AS \"access_token_enc!\", \
                    refresh_token_enc, client_id, client_secret_enc, \
                    token_expires_at, enc_version \
             FROM social_media_platform_auth \
             WHERE enabled = 1 AND token_expires_at IS NOT NULL \
               AND ( (platform = 'instagram' AND token_expires_at < $2) \
                     OR (platform <> 'instagram' AND refresh_token_enc IS NOT NULL AND token_expires_at < $1) ) \
             ORDER BY token_expires_at ASC",
            &kurz,
            &lang
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for row in rows {
            self.refresh_one(
                row.platform,
                row.streamer_login,
                row.access_token_enc,
                row.refresh_token_enc,
                row.client_id,
                row.client_secret_enc,
                row.enc_version.unwrap_or(1) as i64,
            )
            .await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn refresh_one(
        &self,
        platform: String,
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
                let text = error.to_string();
                // `invalid_grant` heisst: der Nutzer hat den Zugriff entzogen,
                // das Token ist zu lange ungenutzt oder das Passwort wurde
                // gewechselt. Das heilt kein Retry. Frueher blieb der Eintrag
                // dabei auf "gueltig bis irgendwann", das Dashboard meldete
                // gruen und jeder Upload lief ins Leere. Wir setzen den Ablauf
                // deshalb auf jetzt, damit die Oberflaeche "abgelaufen" zeigt
                // und der Streamer neu verbinden kann.
                //
                // Nur `invalid_grant`. `invalid_request` sagt nichts ueber den
                // Zugang aus, sondern nur, dass diese eine Anfrage falsch
                // geformt war; daran haette ein Streamer einen intakten Zugang
                // neu verbunden, obwohl der Fehler bei uns lag.
                if text.contains("invalid_grant") {
                    self.markiere_abgelaufen(&platform, streamer_ref).await;
                }
                tracing::error!(platform = %platform, error = %text, "Token-Refresh fehlgeschlagen");
                return;
            }
        };

        self.save_refreshed(&platform, streamer_ref, &new_tokens)
            .await;
    }

    /// Setzt den Ablauf auf jetzt, damit der Zustand im Dashboard als
    /// "abgelaufen" sichtbar wird. Der Eintrag bleibt `enabled = 1`, damit ein
    /// erneutes Verbinden dieselbe Zeile aktualisiert.
    async fn markiere_abgelaufen(&self, platform: &str, streamer: Option<&str>) {
        let jetzt = Utc::now().to_rfc3339();
        let result = sqlx::query!(
            "UPDATE social_media_platform_auth SET token_expires_at = $1 \
             WHERE platform = $2 AND (streamer_login = $3 OR (streamer_login IS NULL AND $3 IS NULL))",
            &jetzt,
            platform,
            streamer
        )
        .execute(&self.pool)
        .await;
        if let Err(error) = result {
            tracing::error!(platform = %platform, %error, "Ablauf-Markierung fehlgeschlagen");
        }
    }

    async fn save_refreshed(
        &self,
        platform: &str,
        streamer: Option<&str>,
        new_tokens: &crate::oauth::RefreshedTokens,
    ) {
        let Ok(access_enc) = self.cipher.encrypt_field(
            &new_tokens.access_token,
            &aad::social_media("access_token", platform, streamer, 1),
        ) else {
            tracing::error!(platform = %platform, "Refresh-Persist: encrypt access fehlgeschlagen");
            return;
        };
        let refresh_enc = new_tokens.refresh_token.as_ref().and_then(|t| {
            self.cipher
                .encrypt_field(
                    t,
                    &aad::social_media("refresh_token", platform, streamer, 1),
                )
                .ok()
        });
        let expires_iso = new_tokens.expires_at.to_rfc3339();

        // WHERE platform AND (streamer = $ OR (streamer IS NULL AND $ IS NULL)).
        let result = if let Some(refresh_enc) = refresh_enc {
            sqlx::query!(
                "UPDATE social_media_platform_auth \
                 SET access_token_enc = $1, refresh_token_enc = $2, token_expires_at = $3, \
                     last_refreshed_at = CURRENT_TIMESTAMP \
                 WHERE platform = $4 AND (streamer_login = $5 OR (streamer_login IS NULL AND $5 IS NULL))",
                access_enc,
                refresh_enc,
                &expires_iso,
                platform,
                streamer
            )
            .execute(&self.pool)
            .await
        } else {
            sqlx::query!(
                "UPDATE social_media_platform_auth \
                 SET access_token_enc = $1, token_expires_at = $2, last_refreshed_at = CURRENT_TIMESTAMP \
                 WHERE platform = $3 AND (streamer_login = $4 OR (streamer_login IS NULL AND $4 IS NULL))",
                access_enc,
                &expires_iso,
                platform,
                streamer
            )
            .execute(&self.pool)
            .await
        };
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
                enc_kid TEXT DEFAULT 'v1', last_refreshed_at TEXT, enabled INTEGER DEFAULT 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
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
