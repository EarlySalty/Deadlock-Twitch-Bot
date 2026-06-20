//! Bot-Token-Write-Back nach Infisical — siehe `rust/docs/adr/0005-bot-token-infisical-writeback.md`.
//!
//! Der [`BotTokenManager`](crate::token::BotTokenManager) hält seinen Token nach
//! einem Refresh nur in-memory; der Infisical-Snapshot (`TWITCH_BOT_TOKEN` /
//! `TWITCH_BOT_REFRESH_TOKEN`) veraltet dadurch. Dieses Modul schreibt den
//! frischen Token zurück, damit der nächste Boot keinen 401 mehr sieht und
//! Refresh-Token-Rotationen nicht verloren gehen.
//!
//! [`SecretSink`] ist die Schnittstelle (der Manager kennt nur sie, kein HTTP).
//! [`InfisicalWriter`] ist die produktive Implementierung, 1:1 gespiegelt von
//! `scripts/reauth_bot_token.py::_infisical_set` (`PATCH`→`POST`-Fallback auf
//! `/api/v3/secrets/raw/{name}`). Fehlt die Konfiguration, liefert
//! [`InfisicalWriter::from_env`] `None` → der Manager läuft ohne Write-Back wie
//! bisher (Graceful Degradation).
//!
//! **Sicherheit:** Es werden niemals Token-Werte geloggt, nur Secret-Namen und
//! HTTP-Status.

use std::time::Duration;

use serde_json::json;

/// Feste Infisical-Secret-Namen des Bot-Tokens.
const SECRET_BOT_TOKEN: &str = "TWITCH_BOT_TOKEN";
const SECRET_BOT_REFRESH: &str = "TWITCH_BOT_REFRESH_TOKEN";

/// Persistenz-Senke für die Bot-Tokens. Best-effort: die Implementierung loggt
/// eigene Fehler und gibt nichts zurück, damit ein Schreibfehler den Refresh
/// (und damit den Chat) nie kippt.
#[async_trait::async_trait]
pub trait SecretSink: Send + Sync {
    /// Schreibt den Access-Token immer; den Refresh-Token nur wenn `Some`
    /// (der Aufrufer übergibt ihn nur bei tatsächlicher Rotation).
    async fn persist_bot_tokens(&self, access_token: &str, refresh_token: Option<&str>);
}

/// Fehler eines einzelnen Secret-Writes (nie mit Token-Wert).
#[derive(Debug)]
pub enum SecretWriteError {
    Http(reqwest::Error),
    Rejected { status: u16 },
}

impl std::fmt::Display for SecretWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "infisical http error: {e}"),
            Self::Rejected { status } => write!(f, "infisical rejected: HTTP {status}"),
        }
    }
}

impl std::error::Error for SecretWriteError {}

/// Schreibt Secrets über die Infisical-Raw-API zurück.
pub struct InfisicalWriter {
    client: reqwest::Client,
    /// Basis-URL ohne abschließenden Slash.
    base_url: String,
    project_id: String,
    environment: String,
    secret_path: String,
    write_token: String,
}

impl InfisicalWriter {
    /// Baut den Writer aus der Env. Fehlt eine Pflicht-Variable (inkl. des
    /// dedizierten `INFISICAL_WRITE_TOKEN`), wird `None` geliefert → Write-Back
    /// bleibt deaktiviert.
    pub fn from_env() -> Option<Self> {
        let base_url = non_empty_env("INFISICAL_API_URL")?;
        let project_id = non_empty_env("INFISICAL_PROJECT_ID")?;
        let environment = non_empty_env("INFISICAL_ENV")?;
        let write_token = non_empty_env("INFISICAL_WRITE_TOKEN")?;
        let secret_path = non_empty_env("INFISICAL_SECRET_PATH").unwrap_or_else(|| "/".to_string());
        Some(Self::new(
            base_url,
            project_id,
            environment,
            secret_path,
            write_token,
        ))
    }

    /// Direkter Konstruktor (auch von Tests genutzt, dort mit Mock-Base-URL).
    pub fn new(
        base_url: String,
        project_id: String,
        environment: String,
        secret_path: String,
        write_token: String,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            project_id,
            environment,
            secret_path,
            write_token,
        }
    }

    /// Schreibt ein einzelnes Secret: erst `PATCH` (Update), bei `404` `POST`
    /// (Create) — wie das Python-Reauth-Skript.
    async fn set_secret(&self, name: &str, value: &str) -> Result<(), SecretWriteError> {
        let url = format!("{}/api/v3/secrets/raw/{}", self.base_url, name);
        let payload = json!({
            "workspaceId": self.project_id,
            "environment": self.environment,
            "secretPath": self.secret_path,
            "secretValue": value,
        });

        for method in [reqwest::Method::PATCH, reqwest::Method::POST] {
            let is_patch = method == reqwest::Method::PATCH;
            let resp = self
                .client
                .request(method, &url)
                .bearer_auth(&self.write_token)
                .json(&payload)
                .send()
                .await
                .map_err(SecretWriteError::Http)?;
            let status = resp.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(());
            }
            // Secret existiert noch nicht → mit POST anlegen.
            if is_patch && status == 404 {
                continue;
            }
            return Err(SecretWriteError::Rejected { status });
        }
        Err(SecretWriteError::Rejected { status: 0 })
    }
}

#[async_trait::async_trait]
impl SecretSink for InfisicalWriter {
    async fn persist_bot_tokens(&self, access_token: &str, refresh_token: Option<&str>) {
        match self.set_secret(SECRET_BOT_TOKEN, access_token).await {
            Ok(()) => tracing::info!(secret = SECRET_BOT_TOKEN, "Bot-Token nach Infisical geschrieben"),
            Err(e) => tracing::error!(
                secret = SECRET_BOT_TOKEN,
                error = %e,
                "Bot-Token-Write-Back fehlgeschlagen"
            ),
        }
        if let Some(refresh) = refresh_token {
            match self.set_secret(SECRET_BOT_REFRESH, refresh).await {
                Ok(()) => {
                    tracing::info!(secret = SECRET_BOT_REFRESH, "Bot-Refresh-Token nach Infisical geschrieben")
                }
                Err(e) => tracing::error!(
                    secret = SECRET_BOT_REFRESH,
                    error = %e,
                    "Bot-Refresh-Token-Write-Back fehlgeschlagen"
                ),
            }
        }
    }
}

/// Liest eine Env-Variable, trimmt sie und liefert `None` bei leer/fehlend.
fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn writer(base_url: String) -> InfisicalWriter {
        InfisicalWriter::new(
            base_url,
            "proj-1".into(),
            "prod".into(),
            "/".into(),
            "write-tok".into(),
        )
    }

    #[tokio::test]
    async fn patch_erfolg_schreibt_wert() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/api/v3/secrets/raw/TWITCH_BOT_TOKEN"))
            .and(header("authorization", "Bearer write-tok"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        writer(server.uri())
            .set_secret("TWITCH_BOT_TOKEN", "access-xyz")
            .await
            .unwrap();
        // Drop des Servers verifiziert die .expect(1)-Erwartung.
    }

    #[tokio::test]
    async fn patch_404_faellt_auf_post_zurueck() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/api/v3/secrets/raw/TWITCH_BOT_REFRESH_TOKEN"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v3/secrets/raw/TWITCH_BOT_REFRESH_TOKEN"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        writer(server.uri())
            .set_secret("TWITCH_BOT_REFRESH_TOKEN", "refresh-xyz")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ablehnung_liefert_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let err = writer(server.uri())
            .set_secret("TWITCH_BOT_TOKEN", "x")
            .await
            .unwrap_err();
        assert!(matches!(err, SecretWriteError::Rejected { status: 403 }));
    }

    #[tokio::test]
    async fn persist_schreibt_beide_wenn_refresh_some() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/api/v3/secrets/raw/TWITCH_BOT_TOKEN"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/api/v3/secrets/raw/TWITCH_BOT_REFRESH_TOKEN"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        writer(server.uri())
            .persist_bot_tokens("access", Some("refresh"))
            .await;
    }

    #[tokio::test]
    async fn persist_schreibt_nur_access_wenn_refresh_none() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/api/v3/secrets/raw/TWITCH_BOT_TOKEN"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        // Kein Mock für REFRESH_TOKEN → ein Aufruf dorthin würde den Test
        // (über unmatched request) auffällig machen.
        Mock::given(method("PATCH"))
            .and(path("/api/v3/secrets/raw/TWITCH_BOT_REFRESH_TOKEN"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        writer(server.uri())
            .persist_bot_tokens("access", None)
            .await;
    }
}
