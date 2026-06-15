//! Verschlüsselter Token-Store für den Engagement-Sende-Account (Port von
//! `bot/engagement/sender_auth.py`, Lese-/Refresh-Seite).
//!
//! Der Engagement-Layer spricht im Chat NICHT über die zentrale Bot-Identität,
//! sondern über einen separaten, unauffälligen Twitch-Account (den „Stammgast"/
//! Smoke-Account). Dessen Token liegt verschlüsselt in
//! `twitch_engagement_sender_auth` (Field-Crypto, AES-256-GCM, AAD-gebunden) und
//! wird hier bei Ablauf via Twitch-Token-Endpoint refreshed.
//!
//! Diese Slice deckt den Cutover-kritischen Pfad ab — Token lesen, refreshen,
//! liefern. Der Onboarding-Flow (Authorize-Link + OAuth-Callback) ist eine
//! getrennte Admin-Strecke (Web-Routing) und folgt separat; der Account ist in
//! der Live-DB bereits autorisiert.

use std::sync::Arc;

use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

/// Twitch-Token-Endpoint (Refresh-Grant). Per `with_token_url` injizierbar.
const DEFAULT_TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
/// 5 min vor Ablauf proaktiv refreshen (Python `_REFRESH_SKEW_SECONDS`).
const REFRESH_SKEW_SECONDS: i64 = 300;

/// Liest + refresht den Token des Engagement-Sende-Accounts.
pub struct SenderAuthStore {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    token_url: String,
}

/// Eine entschlüsselbare Zeile aus `twitch_engagement_sender_auth`.
struct SenderRow {
    user_id: String,
    login: String,
    access_enc: Vec<u8>,
    refresh_enc: Vec<u8>,
    scopes: Option<String>,
    expires_at: i64,
}

impl SenderAuthStore {
    pub fn new(
        pool: PgPool,
        cipher: Arc<FieldCipher>,
        client_id: String,
        client_secret: String,
    ) -> Self {
        Self {
            pool,
            cipher,
            http: reqwest::Client::new(),
            client_id,
            client_secret,
            token_url: DEFAULT_TOKEN_URL.to_string(),
        }
    }

    /// Baut den Store aus den Bot-App-Credentials in der Env
    /// (`TWITCH_CLIENT_ID`/`TWITCH_CLIENT_SECRET`). Fehlt eine, gibt es kein
    /// Onboarding/Refresh → `None` (Aufrufer fällt auf Default zurück, mirror von
    /// Pythons `_client_credentials`-Raise).
    pub fn from_env(pool: PgPool, cipher: Arc<FieldCipher>) -> Option<Self> {
        let client_id = nonempty_env("TWITCH_CLIENT_ID")?;
        let client_secret = nonempty_env("TWITCH_CLIENT_SECRET")?;
        Some(Self::new(pool, cipher, client_id, client_secret))
    }

    /// Setzt den Token-Endpoint (für Tests; produktiv bleibt der Default).
    pub fn with_token_url(mut self, url: impl Into<String>) -> Self {
        self.token_url = url.into();
        self
    }

    /// Legt die Token-Tabelle an, falls sie fehlt (idempotent, self-contained;
    /// bewusst getrennt von `twitch_raid_auth`). Byte-genau zu Pythons Schema.
    pub async fn ensure_table(&self) {
        let _ = sqlx::query(
            "CREATE TABLE IF NOT EXISTS twitch_engagement_sender_auth (\
                twitch_user_id     TEXT PRIMARY KEY,\
                twitch_login       TEXT NOT NULL,\
                access_token_enc   BYTEA NOT NULL,\
                refresh_token_enc  BYTEA NOT NULL,\
                scopes             TEXT,\
                token_expires_at   BIGINT NOT NULL,\
                updated_at         TIMESTAMPTZ NOT NULL DEFAULT now())",
        )
        .execute(&self.pool)
        .await;
    }

    /// Liefert `(access_token, sender_user_id)` für den Sende-Account; refresht
    /// bei (nahem) Ablauf. `None`, wenn kein Account onboarded ist oder der
    /// Refresh scheitert (Aufrufer verwirft die AI-Antwort).
    pub async fn get_valid_access_token(&self) -> Option<(String, String)> {
        let row = self.load_row().await?;
        let now = chrono::Utc::now().timestamp();

        // Noch frisch genug → Access-Token direkt entschlüsseln.
        if now < row.expires_at - REFRESH_SKEW_SECONDS {
            match self
                .cipher
                .decrypt_field(&row.access_enc, &aad::engagement_sender("access_token", &row.user_id))
            {
                Ok(access) => return Some((access, row.user_id)),
                Err(_) => tracing::warn!(
                    "Engagement-Sender: Access-Token-Decrypt fehlgeschlagen, versuche Refresh"
                ),
            }
        }

        // Refresh.
        let refresh_token = self
            .cipher
            .decrypt_field(&row.refresh_enc, &aad::engagement_sender("refresh_token", &row.user_id))
            .ok()?;
        let token = match self.post_refresh(&refresh_token).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "Engagement-Sender: Token-Refresh fehlgeschlagen");
                return None;
            }
        };
        if token.access_token.is_empty() {
            return None;
        }
        let new_refresh = if token.refresh_token.is_empty() {
            refresh_token
        } else {
            token.refresh_token
        };
        let scopes = if token.scope.is_empty() {
            row.scopes.unwrap_or_default()
        } else {
            token.scope
        };
        self.store_tokens(
            &row.user_id,
            &row.login,
            &token.access_token,
            &new_refresh,
            token.expires_in,
            &scopes,
        )
        .await;
        Some((token.access_token, row.user_id))
    }

    /// Jüngste onboardete Zeile (Python `_load_row`, `ORDER BY updated_at DESC`).
    async fn load_row(&self) -> Option<SenderRow> {
        let row: (String, String, Vec<u8>, Vec<u8>, Option<String>, i64) = sqlx::query_as(
            "SELECT twitch_user_id, twitch_login, access_token_enc, refresh_token_enc, \
                    scopes, token_expires_at \
             FROM twitch_engagement_sender_auth ORDER BY updated_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .ok()??;
        if row.2.is_empty() || row.3.is_empty() {
            return None;
        }
        Some(SenderRow {
            user_id: row.0,
            login: row.1,
            access_enc: row.2,
            refresh_enc: row.3,
            scopes: row.4,
            expires_at: row.5,
        })
    }

    /// Verschlüsselt + persistiert das Token-Paar (Python `_store_tokens`).
    /// Schlägt das Verschlüsseln fehl, wird NICHTS geschrieben (Lockout-Schutz).
    async fn store_tokens(
        &self,
        user_id: &str,
        login: &str,
        access_token: &str,
        refresh_token: &str,
        expires_in: i64,
        scopes: &str,
    ) {
        let (Ok(access_enc), Ok(refresh_enc)) = (
            self.cipher
                .encrypt_field(access_token, &aad::engagement_sender("access_token", user_id)),
            self.cipher
                .encrypt_field(refresh_token, &aad::engagement_sender("refresh_token", user_id)),
        ) else {
            tracing::error!("Engagement-Sender: Verschlüsseln fehlgeschlagen — Tokens NICHT geschrieben");
            return;
        };
        let expires_at = chrono::Utc::now().timestamp() + expires_in.max(0);
        let _ = sqlx::query(
            "INSERT INTO twitch_engagement_sender_auth \
                (twitch_user_id, twitch_login, access_token_enc, refresh_token_enc, \
                 scopes, token_expires_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, now()) \
             ON CONFLICT (twitch_user_id) DO UPDATE SET \
                twitch_login      = EXCLUDED.twitch_login, \
                access_token_enc  = EXCLUDED.access_token_enc, \
                refresh_token_enc = EXCLUDED.refresh_token_enc, \
                scopes            = EXCLUDED.scopes, \
                token_expires_at  = EXCLUDED.token_expires_at, \
                updated_at        = now()",
        )
        .bind(user_id)
        .bind(login)
        .bind(access_enc)
        .bind(refresh_enc)
        .bind(scopes)
        .bind(expires_at)
        .execute(&self.pool)
        .await;
    }

    /// Refresh-Grant gegen den Twitch-Token-Endpoint (Python `_post_token`).
    async fn post_refresh(&self, refresh_token: &str) -> Result<TokenResponse, String> {
        let resp = self
            .http
            .post(&self.token_url)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let payload: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(TokenResponse::from_value(&payload))
    }
}

/// Geparste Twitch-Token-Antwort (manuell aus `Value`, kein serde-Derive — wie
/// der Rest des Crates). `scope` kommt als Array → mit Leerzeichen gejoint.
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    scope: String,
}

impl TokenResponse {
    fn from_value(v: &serde_json::Value) -> Self {
        let str_field = |k: &str| v.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
        let scope = match v.get("scope") {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        Self {
            access_token: str_field("access_token"),
            refresh_token: str_field("refresh_token"),
            expires_in: v.get("expires_in").and_then(serde_json::Value::as_i64).unwrap_or(0),
            scope,
        }
    }
}

/// Env-Var nur wenn gesetzt UND nicht leer.
fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cipher() -> Arc<FieldCipher> {
        // Fester 32-Byte-Hex-Key für deterministische Tests.
        Arc::new(FieldCipher::from_hex_key(&"ab".repeat(32), "k1").unwrap())
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        Some(pool)
    }

    fn store(pool: PgPool) -> SenderAuthStore {
        SenderAuthStore::new(pool, cipher(), "cid".into(), "csec".into())
    }

    /// Helper: legt eine Zeile mit verschlüsseltem Token-Paar + Ablauf an.
    async fn seed(s: &SenderAuthStore, user_id: &str, access: &str, refresh: &str, expires_at: i64) {
        let access_enc = s
            .cipher
            .encrypt_field(access, &aad::engagement_sender("access_token", user_id))
            .unwrap();
        let refresh_enc = s
            .cipher
            .encrypt_field(refresh, &aad::engagement_sender("refresh_token", user_id))
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_engagement_sender_auth (twitch_user_id, twitch_login, \
             access_token_enc, refresh_token_enc, scopes, token_expires_at) \
             VALUES ($1, 'smoke', $2, $3, 'user:write:chat user:bot', $4)",
        )
        .bind(user_id)
        .bind(access_enc)
        .bind(refresh_enc)
        .bind(expires_at)
        .execute(&s.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn none_wenn_kein_account() {
        let Some(pool) = make_pool("t_eng_sender_none").await else { return };
        let s = store(pool);
        s.ensure_table().await;
        assert_eq!(s.get_valid_access_token().await, None);
    }

    #[tokio::test]
    async fn frischer_token_wird_direkt_entschluesselt() {
        let Some(pool) = make_pool("t_eng_sender_fresh").await else { return };
        let s = store(pool);
        s.ensure_table().await;
        // Ablauf weit in der Zukunft → kein Refresh, direkter Decrypt.
        let future = chrono::Utc::now().timestamp() + 3600;
        seed(&s, "42", "fresh-access", "the-refresh", future).await;
        let got = s.get_valid_access_token().await;
        assert_eq!(got, Some(("fresh-access".to_string(), "42".to_string())));
    }

    #[tokio::test]
    async fn abgelaufener_token_wird_refreshed() {
        let Some(pool) = make_pool("t_eng_sender_refresh").await else { return };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-access",
                "refresh_token": "new-refresh",
                "expires_in": 14400,
                "scope": ["user:write:chat", "user:bot"]
            })))
            .mount(&server)
            .await;

        let s = store(pool).with_token_url(format!("{}/oauth2/token", server.uri()));
        s.ensure_table().await;
        // Ablauf in der Vergangenheit → Refresh-Pfad.
        let past = chrono::Utc::now().timestamp() - 10;
        seed(&s, "42", "old-access", "old-refresh", past).await;

        let got = s.get_valid_access_token().await;
        assert_eq!(got, Some(("new-access".to_string(), "42".to_string())));
        // Neuer Token persistiert + entschlüsselbar, Ablauf jetzt in der Zukunft.
        let row = s.load_row().await.unwrap();
        assert!(row.expires_at > chrono::Utc::now().timestamp());
        let access = s
            .cipher
            .decrypt_field(&row.access_enc, &aad::engagement_sender("access_token", "42"))
            .unwrap();
        assert_eq!(access, "new-access");
        let refresh = s
            .cipher
            .decrypt_field(&row.refresh_enc, &aad::engagement_sender("refresh_token", "42"))
            .unwrap();
        assert_eq!(refresh, "new-refresh");
    }
}
