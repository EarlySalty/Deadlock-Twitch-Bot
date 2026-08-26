//! User-Token-Refresh gegen den Twitch-OAuth-Endpoint. Port von
//! `raid/auth.py` `refresh_token` (Z. 908–970) inkl. der Fehlerklassifikation
//! aus `api/twitch_auth.py` `is_invalid_client_response`:
//!
//! - **InvalidClient**: HTTP 400 + „invalid client" im Body bzw. in den
//!   JSON-Feldern `message`/`error` — Client-Credentials kaputt, NIEMALS den
//!   Streamer dafür sperren.
//! - **InvalidGrant**: HTTP 400 + „invalid refresh token"/`invalid_grant` —
//!   der Streamer muss neu autorisieren (Token-Blacklist-Pfad).
//! - **Other**: alles andere (Netz, 5xx, sonstige 4xx) — kein Blacklisting.

use serde::Deserialize;

use crate::client::HelixClient;

/// Python-Parität (raid/auth.py `save_auth`: `expires_in or 3600`): fehlt
/// `expires_in` in der Twitch-Antwort, gilt 3600 s. Ohne diesen Default würde
/// ein fehlendes Feld zu 0 deserialisiert → `token_expires_at = now` → der
/// frisch persistierte Partner-Token wäre sofort (hinter dem 300-s-Puffer) stale.
fn default_expires_in() -> i64 {
    3600
}

/// Antwort des Token-Endpoints (Refresh-Pfad).
#[derive(Debug, Clone, Deserialize)]
pub struct UserTokenResponse {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default = "default_expires_in")]
    pub expires_in: i64,
    #[serde(default)]
    pub scope: Vec<String>,
}

/// Fehlerklassen des Token-Endpoints (steuern Blacklist vs. nicht).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserTokenError {
    /// `invalid_client` — Client-Credentials abgelehnt. Auch der lokal
    /// kurzgeschlossene Zustand bei aktivem 15-Min-Cooldown (P2.33): Der Aufrufer
    /// behandelt beide identisch (Streamer NICHT sperren, Refresh überspringen),
    /// daher ist hier bewusst KEINE eigene Variante nötig — das hält die
    /// bestehenden `match`-Stellen der Adapter exhaustiv ohne Änderung.
    InvalidClient,
    InvalidGrant,
    Other(String),
}

#[derive(Default, Deserialize)]
struct OAuthErrorBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    error: String,
}

/// Python `is_invalid_client_response`: 400 + „invalid client" im Roh-Body
/// oder in den JSON-Feldern `message`/`error`.
fn is_invalid_client(status: u16, body: &str) -> bool {
    if status != 400 {
        return false;
    }
    if body.to_lowercase().contains("invalid client") {
        return true;
    }
    let parsed: OAuthErrorBody = match serde_json::from_str(body) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(
                %error,
                status,
                "Twitch-User-Token: invalid_client-Body nicht als JSON lesbar"
            );
            OAuthErrorBody::default()
        }
    };
    parsed.message.to_lowercase().contains("invalid client")
        || parsed.error.to_lowercase().contains("invalid client")
}

/// Python Z. 964–970: 400 + „invalid refresh token"/`invalid_grant` in
/// Roh-Body, `message` oder `error`.
fn is_invalid_grant(status: u16, body: &str) -> bool {
    if status != 400 {
        return false;
    }
    let lowered = body.to_lowercase();
    if lowered.contains("invalid refresh token") || lowered.contains("invalid_grant") {
        return true;
    }
    let parsed: OAuthErrorBody = match serde_json::from_str(body) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(
                %error,
                status,
                "Twitch-User-Token: invalid_grant-Body nicht als JSON lesbar"
            );
            OAuthErrorBody::default()
        }
    };
    let message = parsed.message.to_lowercase();
    let error = parsed.error.to_lowercase();
    message.contains("invalid refresh token")
        || message.contains("invalid_grant")
        || error.contains("invalid_grant")
}

/// Inhaber eines User-Access-Tokens (`GET /users` ohne Parameter mit dem
/// User-Bearer liefert genau den Token-Owner).
#[derive(Debug, Clone, Deserialize)]
pub struct TokenOwner {
    pub id: String,
    pub login: String,
    /// Anzeigename (Python `display_name`, Fallback auf `login`). Wird vom
    /// Dashboard-Login für den Session-Display gebraucht.
    #[serde(default)]
    pub display_name: String,
    /// E-Mail aus `user:read:email`-Scopes. Beim normalen Dashboard-Login leer,
    /// beim Affiliate-Login PII-Quelle für das Profil.
    #[serde(default)]
    pub email: String,
}

#[derive(Deserialize)]
struct TokenOwnerResponse {
    #[serde(default)]
    data: Vec<TokenOwner>,
}

impl HelixClient {
    /// Erneuert einen User-Access-Token (grant_type=refresh_token).
    pub async fn refresh_user_token(
        &self,
        refresh_token: &str,
    ) -> Result<UserTokenResponse, UserTokenError> {
        let config = self.helix_config();
        let params = [
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];
        self.request_user_token(&params).await
    }

    /// Tauscht einen Authorization-Code gegen User-Tokens
    /// (grant_type=authorization_code, Python `exchange_code_for_token` in
    /// `raid/auth.py:864`). `redirect_uri` muss exakt der beim Authorize-Link
    /// verwendeten URI entsprechen, sonst lehnt Twitch den Tausch ab.
    pub async fn exchange_user_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<UserTokenResponse, UserTokenError> {
        let config = self.helix_config();
        let params = [
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ];
        self.request_user_token(&params).await
    }

    async fn request_user_token(
        &self,
        params: &[(&str, &str)],
    ) -> Result<UserTokenResponse, UserTokenError> {
        // P2.33: Cooldown vorab prüfen — Python `_raise_if_client_auth_blocked`
        // am Eintritt von exchange_code_for_token/refresh_token. Bei aktivem
        // 15-Min-Block erreicht der Request Twitch gar nicht; der Fall wird als
        // `InvalidClient` gemeldet (Aufrufer-Verhalten identisch: kein Sperren,
        // Refresh überspringen).
        if self.is_client_auth_blocked() {
            return Err(UserTokenError::InvalidClient);
        }

        let config = self.helix_config();
        let response = self
            .http_client()
            .post(&config.token_url)
            .form(params)
            .send()
            .await
            .map_err(|error| UserTokenError::Other(format!("request failed: {error}")))?;

        let status = response.status().as_u16();
        if status != 200 {
            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        status,
                        "Twitch-User-Token: Fehlerbody nicht lesbar"
                    );
                    String::new()
                }
            };
            if is_invalid_client(status, &body) {
                // P2.33: 15-Min-Block setzen (Python `_block_client_auth`), damit
                // Exchange/Refresh/Sweep Twitch während einer Credentials-Panne
                // nicht weiter bombardieren.
                self.block_client_auth();
                return Err(UserTokenError::InvalidClient);
            }
            if is_invalid_grant(status, &body) {
                return Err(UserTokenError::InvalidGrant);
            }
            let snippet: String = body.chars().take(300).collect();
            return Err(UserTokenError::Other(format!("HTTP {status}: {snippet}")));
        }

        let parsed = response
            .json::<UserTokenResponse>()
            .await
            .map_err(|error| UserTokenError::Other(format!("invalid token response: {error}")))?;
        // P2.33: Erfolgreicher Tausch/Refresh hebt einen evtl. Cooldown auf
        // (Python: `_client_auth_blocked_until = 0.0`).
        self.clear_client_auth_block();
        Ok(parsed)
    }

    /// Ermittelt den Inhaber eines frischen User-Access-Tokens
    /// (Python `oauth_callback.py:130`: `GET /helix/users` mit `Client-ID` +
    /// `Bearer <access_token>`, OHNE Query-Parameter — Twitch antwortet dann
    /// mit dem Token-Owner). Bewusst NICHT über den App-Token-Pfad
    /// (`self.get`), denn die Identität steckt im übergebenen Bearer.
    pub async fn fetch_token_owner(
        &self,
        access_token: &str,
    ) -> Result<TokenOwner, UserTokenError> {
        let config = self.helix_config();
        let url = format!("{}/users", config.helix_base.trim_end_matches('/'));
        let response = self
            .http_client()
            .get(&url)
            .header("Client-ID", config.client_id.as_str())
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|error| UserTokenError::Other(format!("request failed: {error}")))?;

        let status = response.status().as_u16();
        if status != 200 {
            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        status,
                        "Twitch-User-Token: Owner-Fehlerbody nicht lesbar"
                    );
                    String::new()
                }
            };
            let snippet: String = body.chars().take(300).collect();
            return Err(UserTokenError::Other(format!(
                "user lookup HTTP {status}: {snippet}"
            )));
        }

        let body: TokenOwnerResponse = response
            .json()
            .await
            .map_err(|error| UserTokenError::Other(format!("invalid users response: {error}")))?;
        let owner =
            body.data.into_iter().next().ok_or_else(|| {
                UserTokenError::Other("missing user data in response".to_string())
            })?;
        if owner.id.trim().is_empty() || owner.login.trim().is_empty() {
            return Err(UserTokenError::Other(
                "invalid user payload in response".to_string(),
            ));
        }
        let display_name = if owner.display_name.trim().is_empty() {
            owner.login.trim().to_string()
        } else {
            owner.display_name.trim().to_string()
        };
        let email = owner.email.trim().to_string();
        Ok(TokenOwner {
            id: owner.id.trim().to_string(),
            login: owner.login.trim().to_lowercase(),
            display_name,
            email,
        })
    }
}

#[derive(Deserialize)]
struct StreamKeyEintrag {
    #[serde(default)]
    stream_key: String,
}

#[derive(Deserialize)]
struct StreamKeyResponse {
    #[serde(default)]
    data: Vec<StreamKeyEintrag>,
}

impl HelixClient {
    /// Stream-Key des Broadcasters (`GET /streams/key?broadcaster_id=`).
    /// Braucht ein Nutzer-Token mit `channel:read:stream_key`. Der Key wird
    /// nie geloggt, auch nicht im Fehlerfall.
    pub async fn fetch_stream_key(
        &self,
        access_token: &str,
        broadcaster_id: &str,
    ) -> Result<String, UserTokenError> {
        let config = self.helix_config();
        let url = format!("{}/streams/key", config.helix_base.trim_end_matches('/'));
        let response = self
            .http_client()
            .get(&url)
            .query(&[("broadcaster_id", broadcaster_id)])
            .header("Client-ID", config.client_id.as_str())
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|error| UserTokenError::Other(format!("request failed: {error}")))?;
        let status = response.status().as_u16();
        if status != 200 {
            // Kein Body-Auszug: bei 200 staende der Key darin, und ein
            // Fehler-Body von Twitch traegt keine Information, die der
            // Statuscode nicht schon hat.
            return Err(UserTokenError::Other(format!("stream key HTTP {status}")));
        }
        let body: StreamKeyResponse = response
            .json()
            .await
            .map_err(|_| UserTokenError::Other("stream key: Antwort nicht lesbar".into()))?;
        let key = body
            .data
            .into_iter()
            .next()
            .map(|e| e.stream_key.trim().to_string())
            .unwrap_or_default();
        if key.is_empty() {
            return Err(UserTokenError::Other("stream key: Antwort ohne Key".into()));
        }
        Ok(key)
    }

    /// Nimmt ein Nutzer-Token bei Twitch zurueck (`POST /oauth2/revoke`).
    /// Ein unbekanntes Token quittiert Twitch mit 400; der Aufrufer loggt
    /// das nur, das Token ist dann ohnehin nicht mehr brauchbar.
    pub async fn revoke_user_token(&self, access_token: &str) -> Result<(), UserTokenError> {
        let config = self.helix_config();
        let url = config
            .token_url
            .replacen("/oauth2/token", "/oauth2/revoke", 1);
        // Ohne den erwarteten Teilstring ginge der POST samt Token an den
        // Token-Endpunkt statt an Revoke. Lieber gar nicht senden.
        if url == config.token_url {
            return Err(UserTokenError::Other(
                "revoke: Token-URL ohne /oauth2/token, Revoke-Adresse unbekannt".into(),
            ));
        }
        let params = [
            ("client_id", config.client_id.as_str()),
            ("token", access_token),
        ];
        let response = self
            .http_client()
            .post(&url)
            .form(&params)
            .send()
            .await
            .map_err(|error| UserTokenError::Other(format!("request failed: {error}")))?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(UserTokenError::Other(format!("revoke HTTP {status}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HelixConfig;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(server: &MockServer) -> HelixClient {
        HelixClient::new(HelixConfig {
            client_id: "cid".to_string(),
            client_secret: "sec".to_string(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn refresh_liefert_neue_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=alt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "neu-acc", "refresh_token": "neu-ref",
                "expires_in": 14000, "scope": ["channel:manage:raids"]
            })))
            .mount(&server)
            .await;

        let result = client_for(&server).refresh_user_token("alt").await.unwrap();
        assert_eq!(result.access_token, "neu-acc");
        assert_eq!(result.refresh_token, "neu-ref");
        assert_eq!(result.expires_in, 14000);
        assert_eq!(result.scope, vec!["channel:manage:raids".to_string()]);
    }

    #[tokio::test]
    async fn stream_key_kommt_aus_helix_mit_nutzer_token() {
        use wiremock::matchers::{header, query_param};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/streams/key"))
            .and(query_param("broadcaster_id", "4242"))
            .and(header("Authorization", "Bearer acc-1"))
            .and(header("Client-ID", "cid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "stream_key": "live_4242_geheim" }]
            })))
            .mount(&server)
            .await;
        let key = client_for(&server)
            .fetch_stream_key("acc-1", "4242")
            .await
            .unwrap();
        assert_eq!(key, "live_4242_geheim");
    }

    #[tokio::test]
    async fn stream_key_fehler_traegt_keinen_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/streams/key"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({ "message": "Missing scope" })),
            )
            .mount(&server)
            .await;
        let fehler = client_for(&server)
            .fetch_stream_key("acc-1", "4242")
            .await
            .unwrap_err();
        assert_eq!(fehler, UserTokenError::Other("stream key HTTP 401".into()));
    }

    #[tokio::test]
    async fn revoke_geht_an_den_revoke_endpunkt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/revoke"))
            .and(body_string_contains("client_id=cid"))
            .and(body_string_contains("token=acc-1"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        client_for(&server)
            .revoke_user_token("acc-1")
            .await
            .unwrap();
    }

    #[test]
    fn expires_in_default_3600_bei_fehlendem_feld() {
        // Python-Parität (save_auth: `expires_in or 3600`): fehlt das Feld, gilt 3600 s
        // statt 0 (sonst wäre der frisch persistierte Token sofort stale).
        let r: UserTokenResponse =
            serde_json::from_str(r#"{"access_token":"a","refresh_token":"r","scope":[]}"#).unwrap();
        assert_eq!(r.expires_in, 3600);
        // Vorhandenes Feld bleibt unverändert.
        let r2: UserTokenResponse =
            serde_json::from_str(r#"{"access_token":"a","expires_in":14000}"#).unwrap();
        assert_eq!(r2.expires_in, 14000);
    }

    #[tokio::test]
    async fn invalid_grant_und_invalid_client_werden_klassifiziert() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "status": 400, "message": "Invalid refresh token"
            })))
            .mount(&server)
            .await;
        assert_eq!(
            client_for(&server)
                .refresh_user_token("x")
                .await
                .unwrap_err(),
            UserTokenError::InvalidGrant
        );

        let server2 = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "status": 400, "message": "invalid client"
            })))
            .mount(&server2)
            .await;
        assert_eq!(
            client_for(&server2)
                .refresh_user_token("x")
                .await
                .unwrap_err(),
            UserTokenError::InvalidClient
        );
    }

    /// P2.33: Erste `invalid_client`-Ablehnung setzt den 15-Min-Cooldown; der
    /// zweite Refresh UND ein Exchange brechen lokal ab, OHNE Twitch erneut zu
    /// kontaktieren (Mock `expect(1)` bewacht das). `is_client_auth_blocked`
    /// meldet danach `true`.
    #[tokio::test]
    async fn invalid_client_setzt_15min_cooldown_und_kurzschliesst_folgecalls() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "status": 400, "message": "invalid client"
            })))
            .expect(1) // nur der erste Versuch erreicht Twitch
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(!client.is_client_auth_blocked());

        assert_eq!(
            client.refresh_user_token("x").await.unwrap_err(),
            UserTokenError::InvalidClient
        );
        assert!(client.is_client_auth_blocked());

        // Folge-Refresh: kein Twitch-Request mehr (expect(1) prüft das).
        assert_eq!(
            client.refresh_user_token("y").await.unwrap_err(),
            UserTokenError::InvalidClient
        );
        // Auch ein Exchange ist im Cooldown gesperrt.
        assert_eq!(
            client
                .exchange_user_code("code", "https://example.test/cb")
                .await
                .unwrap_err(),
            UserTokenError::InvalidClient
        );
        server.verify().await;
    }

    /// P2.33: Ein erfolgreicher Refresh hebt einen abgelaufenen Cooldown auf
    /// (Python: `_client_auth_blocked_until = 0.0`).
    #[tokio::test]
    async fn erfolgreicher_refresh_hebt_cooldown_auf() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "neu", "refresh_token": "neu-ref", "expires_in": 14000, "scope": []
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        // Abgelaufene Cooldown-Deadline setzen (Vergangenheit) → nicht mehr aktiv.
        client.user_auth_blocked_until.store(
            crate::token::unix_now() - 1,
            std::sync::atomic::Ordering::Release,
        );
        assert!(!client.is_client_auth_blocked());

        client.refresh_user_token("alt").await.unwrap();
        // Deadline auf 0 genullt.
        assert_eq!(
            client
                .user_auth_blocked_until
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
    }

    #[tokio::test]
    async fn andere_fehler_bleiben_other() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(500).set_body_string("kaputt"))
            .mount(&server)
            .await;
        match client_for(&server)
            .refresh_user_token("x")
            .await
            .unwrap_err()
        {
            UserTokenError::Other(message) => assert!(message.contains("HTTP 500")),
            other => panic!("unerwartet: {other:?}"),
        }
    }

    #[tokio::test]
    async fn exchange_sendet_authorization_code_grant_mit_redirect_uri() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=abc123"))
            .and(body_string_contains(
                "redirect_uri=https%3A%2F%2Fexample.test%2Fcallback",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "acc", "refresh_token": "ref",
                "expires_in": 14000, "scope": ["channel:manage:raids"]
            })))
            .mount(&server)
            .await;

        let result = client_for(&server)
            .exchange_user_code("abc123", "https://example.test/callback")
            .await
            .unwrap();
        assert_eq!(result.access_token, "acc");
        assert_eq!(result.scope, vec!["channel:manage:raids".to_string()]);
    }

    #[tokio::test]
    async fn token_owner_nutzt_bearer_und_normalisiert_login() {
        use wiremock::matchers::header;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(header("Authorization", "Bearer frisch"))
            .and(header("Client-ID", "cid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "993954638", "login": "DenoShock", "display_name": "DenoShock"}]
            })))
            .mount(&server)
            .await;

        let owner = client_for(&server)
            .fetch_token_owner("frisch")
            .await
            .unwrap();
        assert_eq!(owner.id, "993954638");
        // Login wird lowercase-normalisiert (Python: .strip().lower()).
        assert_eq!(owner.login, "denoshock");
    }

    #[tokio::test]
    async fn token_owner_leere_daten_sind_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .mount(&server)
            .await;

        let err = client_for(&server)
            .fetch_token_owner("frisch")
            .await
            .unwrap_err();
        assert!(matches!(err, UserTokenError::Other(m) if m.contains("missing user data")));
    }
}
