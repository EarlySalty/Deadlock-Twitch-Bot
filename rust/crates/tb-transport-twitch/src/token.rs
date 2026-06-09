//! App-Token-Verwaltung für Twitch client_credentials.
//!
//! Expiry-Logik ist pure (keine Uhrzeit-Abhängigkeit im Struct) —
//! testbar ohne Mock-Clock. HTTP-Fetch gegen wiremock testbar.

use reqwest::Client;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

const EXPIRY_MARGIN_SECS: i64 = 60;

/// Fehlertyp für Token-Operationen.
#[derive(Debug, Error)]
pub enum TokenError {
    #[error("HTTP-Fehler beim Token-Abruf: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Token-Response nicht parsebar: {0}")]
    Parse(#[from] serde_json::Error),
}

/// In-Memory-Repräsentation eines gültigen App-Tokens.
#[derive(Debug, Clone)]
pub struct AppToken {
    pub access_token: String,
    /// Unix-Zeitstempel (Sekunden), ab dem der Token spätestens erneuert werden muss.
    pub expiry_unix: i64,
}

impl AppToken {
    /// Erstellt einen neuen AppToken.
    ///
    /// `issued_at_unix` + `expires_in` → `expiry_unix`.
    pub fn new(access_token: String, expires_in: u64, issued_at_unix: i64) -> Self {
        let expiry_unix = issued_at_unix + expires_in as i64;
        Self {
            access_token,
            expiry_unix,
        }
    }

    /// Gibt `true` zurück, wenn der Token erneuert werden sollte.
    ///
    /// Erneuerung nötig, wenn `now_unix >= expiry_unix - EXPIRY_MARGIN_SECS`.
    pub fn needs_refresh(&self, now_unix: i64) -> bool {
        now_unix >= self.expiry_unix - EXPIRY_MARGIN_SECS
    }
}

/// Liefert den aktuellen Unix-Zeitstempel in Sekunden.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Rohe Twitch-Token-Response (nur benötigte Felder).
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// Holt einen neuen App-Token via client_credentials-Grant.
pub async fn fetch_app_token(
    client: &Client,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<AppToken, TokenError> {
    let issued_at = unix_now();
    let resp = client
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await?;

    let body = resp.text().await?;
    let parsed: TokenResponse = serde_json::from_str(&body)?;
    Ok(AppToken::new(
        parsed.access_token,
        parsed.expires_in,
        issued_at,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_refresh_false_wenn_weit_von_ablauf() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_000_000; Delta = 3600 >> 60 → kein Refresh
        assert!(!token.needs_refresh(1_000_000));
    }

    #[test]
    fn needs_refresh_true_bei_genau_60s_vorlauf() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_540; Delta = 60 → Refresh
        assert!(token.needs_refresh(1_003_540));
    }

    #[test]
    fn needs_refresh_true_wenn_abgelaufen() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_700 > expiry → Refresh
        assert!(token.needs_refresh(1_003_700));
    }

    #[test]
    fn needs_refresh_true_wenn_59s_verbleiben() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_541; Delta = 59 < 60 → Refresh nötig (Marge = 60)
        assert!(token.needs_refresh(1_003_541));
    }

    #[test]
    fn needs_refresh_false_wenn_61s_verbleiben() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_539; Delta = 61 > 60 → noch gültig
        assert!(!token.needs_refresh(1_003_539));
    }
}
