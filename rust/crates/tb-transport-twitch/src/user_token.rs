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

/// Antwort des Token-Endpoints (Refresh-Pfad).
#[derive(Debug, Clone, Deserialize)]
pub struct UserTokenResponse {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub scope: Vec<String>,
}

/// Fehlerklassen des Token-Endpoints (steuern Blacklist vs. nicht).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserTokenError {
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
    let parsed: OAuthErrorBody = serde_json::from_str(body).unwrap_or_default();
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
    let parsed: OAuthErrorBody = serde_json::from_str(body).unwrap_or_default();
    let message = parsed.message.to_lowercase();
    let error = parsed.error.to_lowercase();
    message.contains("invalid refresh token")
        || message.contains("invalid_grant")
        || error.contains("invalid_grant")
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
        let response = self
            .http_client()
            .post(&config.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|error| UserTokenError::Other(format!("request failed: {error}")))?;

        let status = response.status().as_u16();
        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            if is_invalid_client(status, &body) {
                return Err(UserTokenError::InvalidClient);
            }
            if is_invalid_grant(status, &body) {
                return Err(UserTokenError::InvalidGrant);
            }
            let snippet: String = body.chars().take(300).collect();
            return Err(UserTokenError::Other(format!("HTTP {status}: {snippet}")));
        }

        response
            .json::<UserTokenResponse>()
            .await
            .map_err(|error| UserTokenError::Other(format!("invalid token response: {error}")))
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
}
