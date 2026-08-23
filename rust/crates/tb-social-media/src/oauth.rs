//! Multi-Plattform-OAuth für die Social-Media-Pipeline (Port von
//! `bot/social_media/oauth_manager.py`).
//!
//! Unterstützt TikTok / YouTube / Instagram. Diese Slice (O1) deckt die
//! Authorize-Seite ab: PKCE-Generierung, plattform-spezifische Authorize-URLs
//! und das Persistieren des CSRF-State in `oauth_state_tokens`. Code-Exchange +
//! Token-Persist (Schreib-Seite zu [`crate::credentials`]) + Refresh folgen.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher};

/// State-Token-Gültigkeit (Python: 10 min).
const STATE_TTL_MINUTES: i64 = 10;
/// HTTP-Timeout der Token-Endpoints (Python total=30s).
const OAUTH_TIMEOUT: StdDuration = StdDuration::from_secs(30);

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
    #[error("State ungültig, abgelaufen oder bereits benutzt")]
    StateInvalid,
    #[error("OAuth redirect URI mismatch")]
    RedirectMismatch,
    #[error("{platform}-Token-Exchange fehlgeschlagen: {detail}")]
    Exchange {
        platform: &'static str,
        detail: String,
    },
}

/// Token-Endpoints je Plattform (Default = Produktiv; Tests injizieren).
struct TokenUrls {
    tiktok: String,
    youtube: String,
    instagram: String,
    /// Basis fuer den Langzeit-Tausch und den Instagram-Refresh. Getrennt vom
    /// Token-Endpoint, weil Instagram beides auf zwei verschiedenen Hosts
    /// anbietet: der Code-Tausch laeuft ueber api.instagram.com, alles danach
    /// ueber graph.instagram.com.
    instagram_graph: String,
}

impl Default for TokenUrls {
    fn default() -> Self {
        Self {
            tiktok: "https://open.tiktokapis.com/v2/oauth/token/".to_string(),
            youtube: "https://oauth2.googleapis.com/token".to_string(),
            instagram: "https://api.instagram.com/oauth/access_token".to_string(),
            instagram_graph: "https://graph.instagram.com".to_string(),
        }
    }
}

/// Aus dem Code-Exchange gewonnene Tokens (Python `tokens`-Dict).
struct ExchangedTokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: DateTime<Utc>,
    scopes: Option<String>,
    user_id: Option<String>,
    username: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
}

/// Ergebnis eines erfolgreichen Callbacks.
#[derive(Debug, PartialEq, Eq)]
pub struct CallbackResult {
    pub platform: String,
    pub streamer_login: Option<String>,
}

/// OAuth-Manager: Authorize-URLs + Callback-Exchange + verschlüsselter Persist.
pub struct OAuthManager {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
    http: reqwest::Client,
    token_urls: TokenUrls,
}

impl OAuthManager {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(OAUTH_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            pool,
            cipher,
            http,
            token_urls: TokenUrls::default(),
        }
    }

    /// Überschreibt die Token-Endpoints (Tests).
    pub fn with_token_urls(mut self, tiktok: String, youtube: String, instagram: String) -> Self {
        self.token_urls = TokenUrls {
            tiktok,
            youtube,
            instagram,
            instagram_graph: self.token_urls.instagram_graph,
        };
        self
    }

    /// Ueberschreibt die Instagram-Graph-Basis (Tests).
    pub fn with_instagram_graph(mut self, base: String) -> Self {
        self.token_urls.instagram_graph = base;
        self
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
        sqlx::query!(
            "INSERT INTO oauth_state_tokens \
                (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at, consumed_at) \
             VALUES ($1, $2, $3, $4, $5, $6, NULL)",
            &state,
            platform,
            streamer_login,
            redirect_uri,
            pkce_verifier.as_deref(),
            expires_at
        )
        .execute(&self.pool)
        .await?;

        Ok(auth_url)
    }

    /// Verarbeitet den OAuth-Callback: State verbrauchen → Code tauschen →
    /// verschlüsselt persistieren (Python `handle_callback`).
    pub async fn handle_callback(
        &self,
        code: &str,
        state: &str,
        expected_platform: Option<&str>,
        expected_redirect_uri: Option<&str>,
    ) -> Result<CallbackResult, OAuthError> {
        let (platform, streamer_login, redirect_uri, verifier) = self
            .consume_state_token(state, expected_platform, expected_redirect_uri)
            .await?;

        let tokens = match platform.as_str() {
            "tiktok" => {
                self.tiktok_exchange_code(code, &redirect_uri, verifier.as_deref().unwrap_or(""))
                    .await?
            }
            "youtube" => {
                self.youtube_exchange_code(code, &redirect_uri, verifier.as_deref().unwrap_or(""))
                    .await?
            }
            "instagram" => self.instagram_exchange_code(code, &redirect_uri).await?,
            other => return Err(OAuthError::UnknownPlatform(other.to_string())),
        };

        self.save_encrypted_tokens(&platform, streamer_login.as_deref(), &tokens)
            .await?;
        Ok(CallbackResult {
            platform,
            streamer_login,
        })
    }

    /// Verbraucht den State atomar (single-use; Python `_consume_state_token`):
    /// gültig + nicht abgelaufen + nicht benutzt → `consumed_at` setzen + Zeile
    /// zurückgeben. Optionaler Plattform-/Redirect-Abgleich.
    async fn consume_state_token(
        &self,
        state: &str,
        expected_platform: Option<&str>,
        expected_redirect_uri: Option<&str>,
    ) -> Result<(String, Option<String>, String, Option<String>), OAuthError> {
        let expected_platform = expected_platform
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty());
        let now = Utc::now();
        // Plattform-Filter optional über $3 (NULL = kein Filter).
        let row = sqlx::query!(
            "UPDATE oauth_state_tokens SET consumed_at = $1 \
             WHERE state_token = $2 AND expires_at > $1 AND consumed_at IS NULL \
               AND ($3::text IS NULL OR platform = $3) \
             RETURNING platform, streamer_login, redirect_uri, pkce_verifier",
            now,
            state,
            expected_platform.as_deref()
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(OAuthError::StateInvalid)?;

        let platform = row.platform;
        let streamer_login = row.streamer_login;
        let redirect_uri = row.redirect_uri.unwrap_or_default();
        let verifier = row.pkce_verifier;

        if let Some(expected) = expected_redirect_uri
            .map(normalize_redirect)
            .filter(|s| !s.is_empty())
        {
            if normalize_redirect(&redirect_uri) != expected {
                return Err(OAuthError::RedirectMismatch);
            }
        }
        Ok((platform, streamer_login, redirect_uri, verifier))
    }

    async fn tiktok_exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<ExchangedTokens, OAuthError> {
        let client_key = env_nonempty("TIKTOK_CLIENT_KEY").unwrap_or_default();
        let client_secret = env_nonempty("TIKTOK_CLIENT_SECRET").unwrap_or_default();
        let data = self
            .post_token(
                "TikTok",
                &self.token_urls.tiktok,
                &[
                    ("client_key", &client_key),
                    ("client_secret", &client_secret),
                    ("code", code),
                    ("grant_type", "authorization_code"),
                    ("redirect_uri", redirect_uri),
                    ("code_verifier", verifier),
                ],
            )
            .await?;
        if data.get("error").is_some() {
            return Err(OAuthError::Exchange {
                platform: "TikTok",
                detail: error_detail(&data),
            });
        }
        let d = tiktok_payload(&data);
        let access_token = str_field(&d, "access_token");
        let expires_in = d
            .get("expires_in")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        // Ein leerer Token oder eine Laufzeit von null ist kein Erfolg, sondern
        // eine Antwort, die wir nicht verstanden haben. Frueher landete genau
        // das verschluesselt in der Datenbank: der Kanal galt als verbunden und
        // konnte nie posten.
        if access_token.is_empty() || expires_in <= 0 {
            return Err(OAuthError::Exchange {
                platform: "TikTok",
                detail: error_detail(&data),
            });
        }
        Ok(ExchangedTokens {
            access_token,
            refresh_token: opt_field(&d, "refresh_token"),
            expires_at: Utc::now() + Duration::seconds(expires_in),
            scopes: opt_field(&d, "scope"),
            user_id: opt_field(&d, "open_id"),
            username: None,
            client_id: Some(client_key),
            client_secret: Some(client_secret),
        })
    }

    async fn youtube_exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<ExchangedTokens, OAuthError> {
        let client_id = youtube_client_id().unwrap_or_default();
        let client_secret = youtube_client_secret().unwrap_or_default();
        let data = self
            .post_token(
                "YouTube",
                &self.token_urls.youtube,
                &[
                    ("client_id", &client_id),
                    ("client_secret", &client_secret),
                    ("code", code),
                    ("code_verifier", verifier),
                    ("grant_type", "authorization_code"),
                    ("redirect_uri", redirect_uri),
                ],
            )
            .await?;
        if data.get("error").is_some() {
            return Err(OAuthError::Exchange {
                platform: "YouTube",
                detail: error_detail(&data),
            });
        }
        Ok(ExchangedTokens {
            access_token: str_field(&data, "access_token"),
            refresh_token: opt_field(&data, "refresh_token"), // nur bei Erst-Auth
            expires_at: Utc::now()
                + Duration::seconds(
                    data.get("expires_in")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                ),
            scopes: opt_field(&data, "scope"),
            user_id: None,
            username: None,
            client_id: Some(client_id),
            client_secret: Some(client_secret),
        })
    }

    async fn instagram_exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<ExchangedTokens, OAuthError> {
        let client_id = env_nonempty("INSTAGRAM_CLIENT_ID").unwrap_or_default();
        let client_secret = env_nonempty("INSTAGRAM_CLIENT_SECRET").unwrap_or_default();
        let data = self
            .post_token(
                "Instagram",
                &self.token_urls.instagram,
                &[
                    ("client_id", &client_id),
                    ("client_secret", &client_secret),
                    ("grant_type", "authorization_code"),
                    ("redirect_uri", redirect_uri),
                    ("code", code),
                ],
            )
            .await?;
        if data.get("error_message").is_some() || data.get("error").is_some() {
            return Err(OAuthError::Exchange {
                platform: "Instagram",
                detail: error_detail(&data),
            });
        }
        let short_lived = str_field(&data, "access_token");
        if short_lived.is_empty() {
            return Err(OAuthError::Exchange {
                platform: "Instagram",
                detail: error_detail(&data),
            });
        }
        // `user_id` kommt hier je nach Antwort als Zahl oder als String.
        let user_id = opt_field(&data, "user_id").or_else(|| {
            data.get("user_id")
                .and_then(serde_json::Value::as_i64)
                .map(|v| v.to_string())
        });
        // Der Code-Tausch liefert ein Token mit EINER Stunde Laufzeit, nicht mit
        // 60 Tagen. Ohne den zweiten Tausch stand in der Datenbank ein Ablauf in
        // 60 Tagen ueber einem Token, das nach 60 Minuten tot war: das Dashboard
        // meldete zwei Monate lang gruen, waehrend jeder Upload scheiterte.
        let (access_token, expires_at) = self
            .instagram_exchange_long_lived(&short_lived, &client_secret)
            .await?;
        Ok(ExchangedTokens {
            access_token,
            refresh_token: None,
            expires_at,
            scopes: opt_field(&data, "permissions"),
            user_id,
            username: None,
            client_id: Some(client_id),
            client_secret: Some(client_secret),
        })
    }

    /// Tauscht das kurzlebige Token gegen ein 60-Tage-Token
    /// (`grant_type=ig_exchange_token`).
    async fn instagram_exchange_long_lived(
        &self,
        short_lived: &str,
        client_secret: &str,
    ) -> Result<(String, DateTime<Utc>), OAuthError> {
        let url = format!("{}/access_token", self.token_urls.instagram_graph);
        let data = self
            .get_json(
                "Instagram",
                &url,
                &[
                    ("grant_type", "ig_exchange_token"),
                    ("client_secret", client_secret),
                    ("access_token", short_lived),
                ],
            )
            .await?;
        let access_token = str_field(&data, "access_token");
        let expires_in = data
            .get("expires_in")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        if access_token.is_empty() || expires_in <= 0 {
            return Err(OAuthError::Exchange {
                platform: "Instagram",
                detail: error_detail(&data),
            });
        }
        Ok((access_token, Utc::now() + Duration::seconds(expires_in)))
    }

    /// Instagram kennt keinen Refresh-Token: das Langzeit-Token verlaengert sich
    /// selbst (`grant_type=ig_refresh_token`). Voraussetzung ist ein Token, das
    /// mindestens 24 Stunden alt und noch nicht abgelaufen ist.
    async fn refresh_instagram(&self, access_token: &str) -> Result<RefreshedTokens, OAuthError> {
        let url = format!("{}/refresh_access_token", self.token_urls.instagram_graph);
        let data = self
            .get_json(
                "instagram-refresh",
                &url,
                &[
                    ("grant_type", "ig_refresh_token"),
                    ("access_token", access_token),
                ],
            )
            .await?;
        let new_token = str_field(&data, "access_token");
        let expires_in = data
            .get("expires_in")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        if new_token.is_empty() || expires_in <= 0 {
            return Err(OAuthError::Exchange {
                platform: "instagram-refresh",
                detail: error_detail(&data),
            });
        }
        Ok(RefreshedTokens {
            access_token: new_token,
            refresh_token: None,
            expires_at: Utc::now() + Duration::seconds(expires_in),
        })
    }

    /// GET mit Query-Parametern auf einen JSON-Endpunkt. Nicht-2xx wird wie beim
    /// Form-POST als Body zurueckgegeben, damit der Aufrufer den Fehlertext der
    /// Plattform sieht statt nur eines Statuscodes.
    async fn get_json(
        &self,
        platform: &'static str,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value, OAuthError> {
        let resp =
            self.http
                .get(url)
                .query(params)
                .send()
                .await
                .map_err(|e| OAuthError::Exchange {
                    platform,
                    detail: e.to_string(),
                })?;
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| OAuthError::Exchange {
                platform,
                detail: e.to_string(),
            })
    }

    /// Form-POST an einen Token-Endpoint → JSON-Body. Nicht-2xx wird trotzdem als
    /// JSON geparst (Plattformen liefern Fehler im Body, wie Python).
    async fn post_token(
        &self,
        platform: &'static str,
        url: &str,
        form: &[(&str, &str)],
    ) -> Result<serde_json::Value, OAuthError> {
        let resp =
            self.http
                .post(url)
                .form(form)
                .send()
                .await
                .map_err(|e| OAuthError::Exchange {
                    platform,
                    detail: e.to_string(),
                })?;
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| OAuthError::Exchange {
                platform,
                detail: e.to_string(),
            })
    }

    /// Verschlüsselt + persistiert die Tokens (Python `save_encrypted_tokens`).
    /// UPSERT auf die partiellen Unique-Indizes (global vs. streamer-spezifisch),
    /// COALESCE behält bestehende Werte bei NULL-Neuwert.
    async fn save_encrypted_tokens(
        &self,
        platform: &str,
        streamer_login: Option<&str>,
        tokens: &ExchangedTokens,
    ) -> Result<(), OAuthError> {
        let access_enc = self
            .cipher
            .encrypt_field(
                &tokens.access_token,
                &aad::social_media("access_token", platform, streamer_login, 1),
            )
            .map_err(|_| OAuthError::Exchange {
                platform: "persist",
                detail: "encrypt access".to_string(),
            })?;
        let refresh_enc = tokens.refresh_token.as_ref().and_then(|t| {
            self.cipher
                .encrypt_field(
                    t,
                    &aad::social_media("refresh_token", platform, streamer_login, 1),
                )
                .ok()
        });
        let secret_enc = tokens
            .client_secret
            .as_ref()
            .filter(|s| !s.is_empty())
            .and_then(|s| {
                self.cipher
                    .encrypt_field(
                        s,
                        &aad::social_media("client_secret", platform, streamer_login, 1),
                    )
                    .ok()
            });
        let expires_at_iso = tokens.expires_at.to_rfc3339();

        let conflict = if streamer_login.is_none() {
            "ON CONFLICT (platform) WHERE streamer_login IS NULL"
        } else {
            "ON CONFLICT (platform, streamer_login) WHERE streamer_login IS NOT NULL"
        };
        let sql = format!(
            "INSERT INTO social_media_platform_auth \
                (platform, streamer_login, access_token_enc, refresh_token_enc, client_id, \
                 client_secret_enc, token_expires_at, scopes, platform_user_id, platform_username, \
                 enc_version, enc_kid) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 1, 'v1') \
             {conflict} DO UPDATE SET \
                access_token_enc = EXCLUDED.access_token_enc, \
                refresh_token_enc = COALESCE(EXCLUDED.refresh_token_enc, social_media_platform_auth.refresh_token_enc), \
                client_id = COALESCE(EXCLUDED.client_id, social_media_platform_auth.client_id), \
                client_secret_enc = COALESCE(EXCLUDED.client_secret_enc, social_media_platform_auth.client_secret_enc), \
                token_expires_at = COALESCE(EXCLUDED.token_expires_at, social_media_platform_auth.token_expires_at), \
                scopes = COALESCE(EXCLUDED.scopes, social_media_platform_auth.scopes), \
                platform_user_id = COALESCE(EXCLUDED.platform_user_id, social_media_platform_auth.platform_user_id), \
                platform_username = COALESCE(EXCLUDED.platform_username, social_media_platform_auth.platform_username), \
                enc_version = EXCLUDED.enc_version, enc_kid = EXCLUDED.enc_kid, \
                enabled = 1, last_refreshed_at = CURRENT_TIMESTAMP"
        );
        sqlx::query(&sql)
            .bind(platform)
            .bind(streamer_login)
            .bind(access_enc)
            .bind(refresh_enc)
            .bind(tokens.client_id.as_deref())
            .bind(secret_enc)
            .bind(expires_at_iso)
            .bind(tokens.scopes.as_deref())
            .bind(tokens.user_id.as_deref())
            .bind(tokens.username.as_deref())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Erneuert einen Access-Token (Python `refresh_token`). Nur TikTok/YouTube
    /// (Instagram-Token sind langlebig, kein Refresh).
    pub async fn refresh_token(
        &self,
        platform: &str,
        refresh_token: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<RefreshedTokens, OAuthError> {
        match platform {
            "tiktok" => {
                self.refresh_tiktok(refresh_token, client_id, client_secret)
                    .await
            }
            "youtube" => {
                self.refresh_youtube(refresh_token, client_id, client_secret)
                    .await
            }
            // Instagram hat keinen Refresh-Token. Der Aufrufer uebergibt hier
            // deshalb das aktuelle Access-Token, das sich selbst verlaengert.
            "instagram" => self.refresh_instagram(refresh_token).await,
            other => Err(OAuthError::UnknownPlatform(other.to_string())),
        }
    }

    async fn refresh_tiktok(
        &self,
        refresh_token: &str,
        client_key: &str,
        client_secret: &str,
    ) -> Result<RefreshedTokens, OAuthError> {
        // Beide TikTok-Grants laufen ueber denselben Endpunkt und beide
        // erwarten application/x-www-form-urlencoded. Der frueher hier
        // verwendete JSON-Body wurde von TikTok abgelehnt, womit sich jeder
        // Kanal nach 24 Stunden stillschweigend selbst abschaltete.
        let data = self
            .post_token(
                "tiktok-refresh",
                &self.token_urls.tiktok,
                &[
                    ("client_key", client_key),
                    ("client_secret", client_secret),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                ],
            )
            .await?;
        if data.get("error").is_some() {
            return Err(OAuthError::Exchange {
                platform: "tiktok-refresh",
                detail: error_detail(&data),
            });
        }
        let d = tiktok_payload(&data);
        let access_token = str_field(&d, "access_token");
        let expires_in = d
            .get("expires_in")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        if access_token.is_empty() || expires_in <= 0 {
            return Err(OAuthError::Exchange {
                platform: "tiktok-refresh",
                detail: error_detail(&data),
            });
        }
        Ok(RefreshedTokens {
            access_token,
            refresh_token: opt_field(&d, "refresh_token"),
            expires_at: Utc::now() + Duration::seconds(expires_in),
        })
    }

    async fn refresh_youtube(
        &self,
        refresh_token: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<RefreshedTokens, OAuthError> {
        let data = self
            .post_token(
                "youtube-refresh",
                &self.token_urls.youtube,
                &[
                    ("client_id", client_id),
                    ("client_secret", client_secret),
                    ("refresh_token", refresh_token),
                    ("grant_type", "refresh_token"),
                ],
            )
            .await?;
        if data.get("error").is_some() {
            return Err(OAuthError::Exchange {
                platform: "youtube-refresh",
                detail: error_detail(&data),
            });
        }
        Ok(RefreshedTokens {
            access_token: str_field(&data, "access_token"),
            // Meist liefert Google keinen neuen Refresh-Token. In den Faellen,
            // in denen er rotiert wird, ginge er hier sonst verloren; das
            // COALESCE beim Persistieren behaelt den alten, wenn None kommt.
            refresh_token: opt_field(&data, "refresh_token"),
            expires_at: Utc::now()
                + Duration::seconds(
                    data.get("expires_in")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                ),
        })
    }
}

/// Neue Tokens aus einem Refresh (Python `new_tokens`).
pub struct RefreshedTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
}

/// Normalisiert eine Redirect-URI für den Vergleich (trim + lowercase Schema/Host).
fn normalize_redirect(value: &str) -> String {
    value.trim().trim_end_matches('/').to_lowercase()
}

/// TikTok v2 antwortet flach; nur die alte v1-Fassung packte alles in `data`.
/// Wir lesen deshalb `data` nur dann, wenn es tatsaechlich ein Objekt ist.
fn tiktok_payload(value: &serde_json::Value) -> serde_json::Value {
    match value.get("data") {
        Some(inner) if inner.is_object() => inner.clone(),
        _ => value.clone(),
    }
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn opt_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn error_detail(v: &serde_json::Value) -> String {
    v.to_string().chars().take(200).collect()
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

/// Google-Zugangsdaten. Historisch heissen sie hier `YOUTUBE_*`, gepflegt werden
/// sie im Secret-Store aber als `GOOGLE_*`. Die `GOOGLE_*`-Namen haben deshalb
/// Vorrang: unter `YOUTUBE_CLIENT_ID` liegt teils noch ein alter Client, der
/// sonst den aktuellen ueberstimmen wuerde.
fn youtube_client_id() -> Option<String> {
    env_nonempty("GOOGLE_OAUTH_ID")
        .or_else(|| env_nonempty("GOOGLE_CLIENT_ID"))
        .or_else(|| env_nonempty("YOUTUBE_CLIENT_ID"))
}

fn youtube_client_secret() -> Option<String> {
    env_nonempty("GOOGLE_CLIENT_SECRET").or_else(|| env_nonempty("YOUTUBE_CLIENT_SECRET"))
}

fn build_url(base: &str, params: &[(&str, &str)]) -> Result<String, OAuthError> {
    url::Url::parse_with_params(base, params)
        .map(|u| u.to_string())
        .map_err(|e| OAuthError::Url(e.to_string()))
}

fn tiktok_auth_url(state: &str, redirect_uri: &str, verifier: &str) -> Result<String, OAuthError> {
    let client_key =
        env_nonempty("TIKTOK_CLIENT_KEY").ok_or(OAuthError::MissingConfig("TIKTOK_CLIENT_KEY"))?;
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
    let client_id = youtube_client_id().ok_or(OAuthError::MissingConfig("GOOGLE_OAUTH_ID"))?;
    let challenge = pkce_challenge(verifier);
    build_url(
        "https://accounts.google.com/o/oauth2/v2/auth",
        &[
            ("client_id", &client_id),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            // Nur was die Anwendung wirklich tut: hochladen und den verbundenen
            // Kanal lesen. Google verlangt im Audit, dass jeder angefragte Bereich
            // im Demo-Video in Benutzung zu sehen ist; yt-analytics.readonly kommt
            // erst dazu, wenn das Dashboard die Zahlen tatsaechlich abruft.
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
    let client_id = env_nonempty("INSTAGRAM_CLIENT_ID")
        .ok_or(OAuthError::MissingConfig("INSTAGRAM_CLIENT_ID"))?;
    build_url(
        "https://api.instagram.com/oauth/authorize",
        &[
            ("client_id", &client_id),
            ("redirect_uri", redirect_uri),
            // Instagram Login kennt nur die instagram_business_*-Bereiche.
            // instagram_basic/instagram_content_publish gehoeren zum
            // Facebook-Login-Weg und werden hier mit "Invalid scope"
            // abgewiesen, bevor der Nutzer den Zustimmungsdialog sieht.
            (
                "scope",
                "instagram_business_basic,instagram_business_content_publish,instagram_business_manage_insights",
            ),
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap())
    }

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

    /// Pool, der nie verbunden wird. Die hier geprueften Pfade sprechen nur
    /// HTTP, deshalb braucht der Test keine Datenbank.
    fn lazy_pool() -> PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://ungenutzt@127.0.0.1:1/ungenutzt")
            .unwrap()
    }

    #[tokio::test]
    async fn tiktok_exchange_liest_die_flache_v2_antwort() {
        let server = MockServer::start().await;
        // v2 antwortet flach. Frueher las der Code data["data"], bekam Null und
        // schrieb einen leeren Token mit sofortigem Ablauf in die Datenbank,
        // ohne dass irgendwo ein Fehler auftauchte.
        Mock::given(method("POST"))
            .and(path("/tt/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "act-1",
                "expires_in": 86400,
                "open_id": "open-1",
                "refresh_token": "rft-1",
                "refresh_expires_in": 31536000,
                "scope": "video.publish",
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        let mgr = OAuthManager::new(lazy_pool(), test_cipher()).with_token_urls(
            format!("{}/tt/token", server.uri()),
            "http://127.0.0.1:1".into(),
            "http://127.0.0.1:1".into(),
        );
        let tokens = with_env_async(
            &[("TIKTOK_CLIENT_KEY", "ck"), ("TIKTOK_CLIENT_SECRET", "cs")],
            mgr.tiktok_exchange_code("code", "https://cb", "verifier"),
        )
        .await
        .expect("Exchange muss gelingen");
        assert_eq!(tokens.access_token, "act-1");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rft-1"));
        assert_eq!(tokens.user_id.as_deref(), Some("open-1"));
        assert!(tokens.expires_at > Utc::now() + Duration::hours(23));
    }

    #[tokio::test]
    async fn tiktok_exchange_ohne_token_ist_ein_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/tt/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "expires_in": 0 })),
            )
            .mount(&server)
            .await;
        let mgr = OAuthManager::new(lazy_pool(), test_cipher()).with_token_urls(
            format!("{}/tt/token", server.uri()),
            "http://127.0.0.1:1".into(),
            "http://127.0.0.1:1".into(),
        );
        let result = with_env_async(
            &[("TIKTOK_CLIENT_KEY", "ck"), ("TIKTOK_CLIENT_SECRET", "cs")],
            mgr.tiktok_exchange_code("code", "https://cb", "verifier"),
        )
        .await;
        assert!(matches!(result, Err(OAuthError::Exchange { .. })));
    }

    #[tokio::test]
    async fn tiktok_refresh_schickt_ein_formular() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/tt/token"))
            .and(wiremock::matchers::header(
                "content-type",
                "application/x-www-form-urlencoded",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "act-neu",
                "expires_in": 86400,
                "refresh_token": "rft-neu"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mgr = OAuthManager::new(lazy_pool(), test_cipher()).with_token_urls(
            format!("{}/tt/token", server.uri()),
            "http://127.0.0.1:1".into(),
            "http://127.0.0.1:1".into(),
        );
        let neu = mgr
            .refresh_token("tiktok", "rft-alt", "ck", "cs")
            .await
            .expect("Refresh muss gelingen");
        assert_eq!(neu.access_token, "act-neu");
        assert_eq!(neu.refresh_token.as_deref(), Some("rft-neu"));
    }

    #[tokio::test]
    async fn instagram_exchange_tauscht_auf_das_langzeit_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ig/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "kurz-1",
                "user_id": 17841400000000000i64,
                "permissions": "instagram_business_basic"
            })))
            .mount(&server)
            .await;
        // Ohne diesen zweiten Tausch stand in der Datenbank ein Ablauf in 60
        // Tagen ueber einem Token, das nach 60 Minuten tot war.
        Mock::given(method("GET"))
            .and(path("/graph/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "lang-1",
                "token_type": "bearer",
                "expires_in": 5_183_944
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mgr = OAuthManager::new(lazy_pool(), test_cipher())
            .with_token_urls(
                "http://127.0.0.1:1".into(),
                "http://127.0.0.1:1".into(),
                format!("{}/ig/token", server.uri()),
            )
            .with_instagram_graph(format!("{}/graph", server.uri()));
        let tokens = with_env_async(
            &[
                ("INSTAGRAM_CLIENT_ID", "cid"),
                ("INSTAGRAM_CLIENT_SECRET", "csec"),
            ],
            mgr.instagram_exchange_code("code", "https://cb"),
        )
        .await
        .expect("Exchange muss gelingen");
        assert_eq!(tokens.access_token, "lang-1");
        assert_eq!(tokens.user_id.as_deref(), Some("17841400000000000"));
        assert!(tokens.expires_at > Utc::now() + Duration::days(59));
    }

    #[tokio::test]
    async fn instagram_refresh_verlaengert_das_eigene_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/graph/refresh_access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "lang-2",
                "expires_in": 5_183_944
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mgr = OAuthManager::new(lazy_pool(), test_cipher())
            .with_instagram_graph(format!("{}/graph", server.uri()));
        let neu = mgr
            .refresh_token("instagram", "lang-1", "cid", "csec")
            .await
            .expect("Refresh muss gelingen");
        assert_eq!(neu.access_token, "lang-2");
        assert!(neu.expires_at > Utc::now() + Duration::days(59));
    }

    /// Wie `with_env`, aber fuer einen await-Punkt zwischen Setzen und Aufraeumen.
    async fn with_env_async<T>(
        vars: &[(&str, &str)],
        future: impl std::future::Future<Output = T>,
    ) -> T {
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = future.await;
        for (k, _) in vars {
            std::env::remove_var(k);
        }
        out
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
            "CREATE TABLE oauth_state_tokens (state_token TEXT PRIMARY KEY, platform TEXT, \
             streamer_login TEXT, redirect_uri TEXT, pkce_verifier TEXT, expires_at TIMESTAMPTZ, \
             consumed_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Auth-Tabelle + partielle Unique-Indizes (storage/pg.py).
        sqlx::query(
            "CREATE TABLE social_media_platform_auth (\
                id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, platform TEXT NOT NULL, \
                streamer_login TEXT, access_token_enc BYTEA NOT NULL, refresh_token_enc BYTEA, \
                client_id TEXT, client_secret_enc BYTEA, token_expires_at TEXT, scopes TEXT, \
                platform_user_id TEXT, platform_username TEXT, enc_version INTEGER DEFAULT 1, \
                enc_kid TEXT DEFAULT 'v1', authorized_at TEXT DEFAULT CURRENT_TIMESTAMP, \
                last_refreshed_at TEXT, enabled INTEGER DEFAULT 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE UNIQUE INDEX idx_spa_streamer ON social_media_platform_auth(platform, streamer_login) WHERE streamer_login IS NOT NULL")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE UNIQUE INDEX idx_spa_global ON social_media_platform_auth(platform) WHERE streamer_login IS NULL")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    /// Seedet einen gültigen State-Token (wie generate_auth_url).
    async fn seed_state(
        pool: &PgPool,
        state: &str,
        platform: &str,
        streamer: Option<&str>,
        redirect: &str,
        verifier: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO oauth_state_tokens (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at, consumed_at) \
             VALUES ($1, $2, $3, $4, $5, NOW() + INTERVAL '5 minutes', NULL)",
        )
        .bind(state).bind(platform).bind(streamer).bind(redirect).bind(verifier)
        .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn generate_auth_url_persistiert_state() {
        let Some(pool) = make_pool("t_sm_oauth_state").await else {
            return;
        };
        std::env::set_var("YOUTUBE_CLIENT_ID", "yt-cid");
        let mgr = OAuthManager::new(pool.clone(), test_cipher());
        let url = mgr
            .generate_auth_url("youtube", Some("nani"), "https://cb/yt")
            .await
            .unwrap();
        assert!(url.contains("client_id=yt-cid"));
        assert!(url.contains("access_type=offline"));

        // State persistiert mit PKCE-Verifier + 10min-Ablauf.
        let (platform, streamer, verifier, in_future): (
            String,
            Option<String>,
            Option<String>,
            bool,
        ) = sqlx::query_as(
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

    #[tokio::test]
    async fn consume_state_single_use_und_redirect() {
        let Some(pool) = make_pool("t_sm_oauth_consume").await else {
            return;
        };
        let mgr = OAuthManager::new(pool.clone(), test_cipher());
        seed_state(
            &pool,
            "st1",
            "tiktok",
            Some("nani"),
            "https://cb/x",
            Some("verif"),
        )
        .await;
        // Erster Consume liefert die Zeile.
        let (platform, streamer, redirect, verifier) =
            mgr.consume_state_token("st1", None, None).await.unwrap();
        assert_eq!(platform, "tiktok");
        assert_eq!(streamer.as_deref(), Some("nani"));
        assert_eq!(redirect, "https://cb/x");
        assert_eq!(verifier.as_deref(), Some("verif"));
        // Zweiter Consume → bereits benutzt.
        assert!(matches!(
            mgr.consume_state_token("st1", None, None).await,
            Err(OAuthError::StateInvalid)
        ));
        // Redirect-Mismatch.
        seed_state(&pool, "st2", "tiktok", None, "https://cb/x", None).await;
        assert!(matches!(
            mgr.consume_state_token("st2", None, Some("https://andere"))
                .await,
            Err(OAuthError::RedirectMismatch)
        ));
        // Plattform-Filter: falsche erwartete Plattform → kein Treffer.
        seed_state(&pool, "st3", "tiktok", None, "https://cb/x", None).await;
        assert!(matches!(
            mgr.consume_state_token("st3", Some("youtube"), None).await,
            Err(OAuthError::StateInvalid)
        ));
    }

    #[tokio::test]
    async fn handle_callback_youtube_persistiert_verschluesselt() {
        let Some(pool) = make_pool("t_sm_oauth_cb").await else {
            return;
        };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "yt-access", "refresh_token": "yt-refresh",
                "expires_in": 3600, "scope": "youtube.upload"
            })))
            .mount(&server)
            .await;
        let mgr = OAuthManager::new(pool.clone(), test_cipher()).with_token_urls(
            "http://127.0.0.1:1".into(),
            format!("{}/token", server.uri()),
            "http://127.0.0.1:1".into(),
        );
        seed_state(
            &pool,
            "stc",
            "youtube",
            Some("nani"),
            "https://cb/yt",
            Some("verif"),
        )
        .await;

        let result = mgr
            .handle_callback("the-code", "stc", None, None)
            .await
            .unwrap();
        assert_eq!(
            result,
            CallbackResult {
                platform: "youtube".to_string(),
                streamer_login: Some("nani".to_string())
            }
        );

        // Access-Token verschlüsselt persistiert + entschlüsselbar.
        let enc: Vec<u8> = sqlx::query_scalar(
            "SELECT access_token_enc FROM social_media_platform_auth WHERE platform='youtube' AND streamer_login='nani'",
        )
        .fetch_one(&pool).await.unwrap();
        let dec = test_cipher()
            .decrypt_field(
                &enc,
                &aad::social_media("access_token", "youtube", Some("nani"), 1),
            )
            .unwrap();
        assert_eq!(dec, "yt-access");
        let scopes: Option<String> = sqlx::query_scalar(
            "SELECT scopes FROM social_media_platform_auth WHERE platform='youtube'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(scopes.as_deref(), Some("youtube.upload"));

        // Zweiter Callback (neuer State, kein refresh_token in Antwort) → UPSERT,
        // refresh_token bleibt via COALESCE erhalten.
        Mock::given(method("POST"))
            .and(path("/token2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "yt-access-2", "expires_in": 3600, "scope": "youtube.upload"
            })))
            .mount(&server)
            .await;
        let mgr2 = OAuthManager::new(pool.clone(), test_cipher()).with_token_urls(
            "http://127.0.0.1:1".into(),
            format!("{}/token2", server.uri()),
            "http://127.0.0.1:1".into(),
        );
        seed_state(
            &pool,
            "stc2",
            "youtube",
            Some("nani"),
            "https://cb/yt",
            Some("v2"),
        )
        .await;
        mgr2.handle_callback("code2", "stc2", None, None)
            .await
            .unwrap();
        let refresh_present: bool = sqlx::query_scalar(
            "SELECT refresh_token_enc IS NOT NULL FROM social_media_platform_auth WHERE platform='youtube'",
        )
        .fetch_one(&pool).await.unwrap();
        assert!(refresh_present, "refresh_token via COALESCE erhalten");
    }
}
