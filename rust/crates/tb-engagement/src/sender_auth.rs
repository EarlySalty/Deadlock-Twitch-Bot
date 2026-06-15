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

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

/// Login des Engagement-Sende-Accounts (Smoke-Account; per Code, keine Env —
/// mirror von `sender_auth.SENDER_LOGIN`). Der IRC-Reader nutzt ihn als
/// Echo-Guard (eigene Nachrichten überspringen).
pub const SENDER_LOGIN: &str = "iamspyingthroughtyourcam";

/// Twitch-Token-Endpoint (Refresh- + Code-Grant). Per `with_token_url` injizierbar.
const DEFAULT_TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
/// Helix-Users-Endpoint (Onboarding-User-Lookup). Per `with_users_url` injizierbar.
const DEFAULT_USERS_URL: &str = "https://api.twitch.tv/helix/users";
/// 5 min vor Ablauf proaktiv refreshen (Python `_REFRESH_SKEW_SECONDS`).
const REFRESH_SKEW_SECONDS: i64 = 300;

// === Onboarding (Authorize-Link + Callback) ===
/// Twitch-Authorize-Endpoint.
const AUTHORIZE_URL: &str = "https://id.twitch.tv/oauth2/authorize";
/// Registrierte Redirect-URI des Sende-Accounts (Caddy → Dashboard).
const REDIRECT_URI: &str = "https://deutsche-deadlock-community.de/callback/engagement-sender";
/// Plattform-Discriminator in der geteilten `oauth_state_tokens`-Tabelle.
const PLATFORM: &str = "engagement_sender";
/// Scopes des Sende-Accounts (chatten als unauffälliger Account).
const SCOPES: [&str; 2] = ["user:write:chat", "user:bot"];
/// Authorize-Link gültig 10 min (Python `_STATE_TTL_SECONDS`).
const STATE_TTL_SECONDS: i64 = 600;

/// Ergebnis eines erfolgreichen OAuth-Callbacks.
#[derive(Debug)]
pub struct CallbackResult {
    pub login: String,
    pub user_id: String,
}

/// Liest + refresht den Token des Engagement-Sende-Accounts.
pub struct SenderAuthStore {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    token_url: String,
    users_url: String,
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
            users_url: DEFAULT_USERS_URL.to_string(),
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

    /// Setzt den Helix-Users-Endpoint (für Tests; produktiv bleibt der Default).
    pub fn with_users_url(mut self, url: impl Into<String>) -> Self {
        self.users_url = url.into();
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

    /// Generischer Token-Endpoint-POST (Python `_post_token`); Grant-spezifische
    /// Parameter kommen vom Aufrufer.
    async fn post_token(&self, params: &[(&str, &str)]) -> Result<TokenResponse, String> {
        let resp = self
            .http
            .post(&self.token_url)
            .form(params)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let payload: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(TokenResponse::from_value(&payload))
    }

    /// Refresh-Grant.
    async fn post_refresh(&self, refresh_token: &str) -> Result<TokenResponse, String> {
        self.post_token(&[
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .await
    }

    /// Authorization-Code-Grant (Onboarding).
    async fn exchange_code(&self, code: &str) -> Result<TokenResponse, String> {
        self.post_token(&[
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", REDIRECT_URI),
        ])
        .await
    }

    /// Holt `(user_id, login)` des frisch autorisierten Accounts via Helix.
    async fn fetch_user(&self, access_token: &str) -> Result<(String, String), String> {
        let resp = self
            .http
            .get(&self.users_url)
            .header("Client-ID", &self.client_id)
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let payload: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let first = payload.get("data").and_then(serde_json::Value::as_array).and_then(|a| a.first());
        let id = first.and_then(|d| d.get("id")).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
        let login = first.and_then(|d| d.get("login")).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
        if id.is_empty() {
            return Err("helix/users lieferte keine Daten".to_string());
        }
        Ok((id, login))
    }

    /// Persistiert einen State-Token (Upsert) in der geteilten
    /// `oauth_state_tokens`-Tabelle, plattform-gated auf `engagement_sender`.
    async fn persist_state(&self, state: &str, expires_at: DateTime<Utc>) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO oauth_state_tokens \
                (state_token, platform, streamer_login, redirect_uri, expires_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (state_token) DO UPDATE SET expires_at = EXCLUDED.expires_at",
        )
        .bind(state)
        .bind(PLATFORM)
        .bind(SENDER_LOGIN)
        .bind(REDIRECT_URI)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Verbraucht einen State-Token atomar (Python `_consume_state`): existiert +
    /// nicht abgelaufen → true. `expires_at` NULL/unbestimmt → nicht künstlich
    /// abweisen (Python-Toleranz).
    async fn consume_state(&self, state: &str) -> bool {
        if state.is_empty() {
            return false;
        }
        let row: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
            "DELETE FROM oauth_state_tokens WHERE state_token = $1 AND platform = $2 \
             RETURNING expires_at",
        )
        .bind(state)
        .bind(PLATFORM)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);
        match row {
            Some((Some(exp),)) => Utc::now() <= exp,
            Some((None,)) => true,
            None => false,
        }
    }

    /// Erzeugt einen Twitch-Authorize-Link für den Sende-Account und legt den
    /// getrackten State ab (Python `build_authorize_url`).
    pub async fn build_authorize_url(&self) -> Result<String, String> {
        let state = format!("engsender-{}", tb_crypto::random_hex_token(18));
        let expires_at = Utc::now() + Duration::seconds(STATE_TTL_SECONDS);
        self.persist_state(&state, expires_at).await.map_err(|e| e.to_string())?;
        let scope = SCOPES.join(" ");
        let url = reqwest::Url::parse_with_params(
            AUTHORIZE_URL,
            &[
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", REDIRECT_URI),
                ("response_type", "code"),
                ("scope", scope.as_str()),
                ("state", state.as_str()),
                ("force_verify", "true"),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(url.to_string())
    }

    /// Tauscht den Authorize-Code gegen Token und legt ihn verschlüsselt ab
    /// (Python `handle_callback`). Liefert Login + User-ID des Accounts.
    pub async fn handle_callback(&self, code: &str, state: &str) -> Result<CallbackResult, String> {
        let code = code.trim();
        if code.is_empty() {
            return Err("Kein Code im Callback".to_string());
        }
        if !self.consume_state(state).await {
            return Err("State ungültig oder abgelaufen".to_string());
        }
        let token = self.exchange_code(code).await?;
        if token.access_token.is_empty() || token.refresh_token.is_empty() {
            return Err("Token-Response unvollständig".to_string());
        }
        let (user_id, login) = self.fetch_user(&token.access_token).await?;
        let login = if login.is_empty() { SENDER_LOGIN.to_string() } else { login };
        let scopes = if token.scope.is_empty() { SCOPES.join(" ") } else { token.scope.clone() };
        self.store_tokens(&user_id, &login, &token.access_token, &token.refresh_token, token.expires_in, &scopes)
            .await;
        Ok(CallbackResult { login, user_id })
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
        sqlx::query(
            "CREATE TABLE oauth_state_tokens (state_token TEXT PRIMARY KEY, platform TEXT, \
             streamer_login TEXT, redirect_uri TEXT, expires_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
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

    #[tokio::test]
    async fn authorize_url_legt_state_an() {
        let Some(pool) = make_pool("t_eng_sender_authurl").await else { return };
        let s = store(pool.clone());
        let url = s.build_authorize_url().await.unwrap();
        assert!(url.starts_with("https://id.twitch.tv/oauth2/authorize?"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("force_verify=true"));
        assert!(url.contains("response_type=code"));
        // Scope url-encoded (Leerzeichen → +/%20), beide Scopes enthalten.
        assert!(url.contains("user%3Awrite%3Achat"));
        // Genau ein State-Token persistiert, plattform-gated.
        let (token, platform): (String, String) = sqlx::query_as(
            "SELECT state_token, platform FROM oauth_state_tokens LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(token.starts_with("engsender-"));
        assert_eq!(platform, "engagement_sender");
        assert!(url.contains(&format!("state={token}")));
    }

    #[tokio::test]
    async fn callback_speichert_token() {
        let Some(pool) = make_pool("t_eng_sender_cb").await else { return };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "onb-access",
                "refresh_token": "onb-refresh",
                "expires_in": 14400,
                "scope": ["user:write:chat", "user:bot"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "555", "login": "iamspyingthroughtyourcam"}]
            })))
            .mount(&server)
            .await;

        let s = store(pool.clone())
            .with_token_url(format!("{}/oauth2/token", server.uri()))
            .with_users_url(format!("{}/helix/users", server.uri()));
        s.ensure_table().await;
        // Gültigen State seeden (wie build_authorize_url ihn anlegen würde).
        let future = chrono::Utc::now() + chrono::Duration::seconds(600);
        sqlx::query("INSERT INTO oauth_state_tokens (state_token, platform, expires_at) VALUES ('st1', 'engagement_sender', $1)")
            .bind(future).execute(&pool).await.unwrap();

        let result = s.handle_callback("the-code", "st1").await.unwrap();
        assert_eq!(result.user_id, "555");
        assert_eq!(result.login, "iamspyingthroughtyourcam");
        // State verbraucht (gelöscht).
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM oauth_state_tokens")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(remaining, 0);
        // Token gespeichert + via get_valid_access_token lesbar.
        assert_eq!(s.get_valid_access_token().await, Some(("onb-access".to_string(), "555".to_string())));

        // Zweiter Callback mit demselben (verbrauchten) State → State-Fehler.
        let err = s.handle_callback("the-code", "st1").await.unwrap_err();
        assert!(err.contains("State"));
    }
}
