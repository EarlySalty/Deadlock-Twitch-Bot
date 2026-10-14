//! Schlanker Discord-REST-v10-Client mit Rate-Limit-Behandlung.
//!
//! - Header `Authorization: Bot <token>` (als sensibel markiert, nie geloggt).
//! - Keine Redirects, Timeout 10 s.
//! - 429: `retry_after` abwarten (max. [`MAX_ATTEMPTS`] Versuche, Wartezeit
//!   höchstens [`MAX_RETRY_WAIT`]); globales Limit wird respektiert.
//! - Sonstige Fehlerstatus: kein erneuter Versuch.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::{Client, Method};
use serde_json::Value;

use super::ratelimit::{RateLimiter, RouteKey};
use crate::HubError;

/// Produktive Discord-API-Basis.
pub const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const TIMEOUT: Duration = Duration::from_secs(10);
/// Maximale Versuche bei 429.
pub(crate) const MAX_ATTEMPTS: u32 = 3;
/// Längste Wartezeit, die wir für ein Rate-Limit in Kauf nehmen.
pub(crate) const MAX_RETRY_WAIT: Duration = Duration::from_secs(10);
const AGENT: &str = "DiscordBot (https://github.com/EarlySalty/Deadlock-Twitch-Bot, 0.1)";

/// Fehler eines Discord-Aufrufs.
#[derive(Debug, Clone)]
pub(crate) enum RestError {
    /// Discord hat mit Fehlerstatus geantwortet.
    Status {
        status: u16,
        /// Discord-JSON-Fehlercode (z. B. 10003 Unknown Channel).
        code: Option<i64>,
        message: String,
    },
    /// Rate-Limit nicht innerhalb der Obergrenze aufgelöst.
    RateLimited,
    /// Netzwerk/Timeout.
    Transport(String),
}

impl RestError {
    pub(crate) fn is_status(&self, status: u16) -> bool {
        matches!(self, RestError::Status { status: s, .. } if *s == status)
    }

    pub(crate) fn code(&self) -> Option<i64> {
        match self {
            RestError::Status { code, .. } => *code,
            _ => None,
        }
    }
}

impl std::fmt::Display for RestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestError::Status {
                status,
                code,
                message,
            } => write!(f, "Discord-Status {status} (Code {code:?}): {message}"),
            RestError::RateLimited => write!(f, "Discord-Rate-Limit nicht aufgelöst"),
            RestError::Transport(e) => write!(f, "Discord nicht erreichbar: {e}"),
        }
    }
}

/// Discord-REST-Client eines Bot-Tokens.
pub(crate) struct DiscordHttp {
    client: Client,
    base_url: String,
    limiter: RateLimiter,
}

impl std::fmt::Debug for DiscordHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Bewusst ohne Client/Header: dort steckt das Bot-Token.
        f.debug_struct("DiscordHttp")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl DiscordHttp {
    pub(crate) fn new(bot_token: &str, base_url: &str) -> Result<Self, HubError> {
        let token = bot_token.trim();
        if token.is_empty() {
            return Err(HubError::Internal("Bot-Token fehlt".to_string()));
        }
        let mut auth = HeaderValue::from_str(&format!("Bot {token}"))
            .map_err(|_| HubError::Internal("Bot-Token enthält ungültige Zeichen".to_string()))?;
        auth.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth);
        headers.insert(USER_AGENT, HeaderValue::from_static(AGENT));

        let base_url = base_url.trim_end_matches('/').to_string();
        let mut builder = Client::builder()
            .default_headers(headers)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none());
        let loopback = reqwest::Url::parse(&base_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .is_some_and(|h| {
                h == "localhost"
                    || h.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            });
        if loopback {
            // Test-/Lokalbetrieb: keinen Umgebungs-Proxy für Loopback verwenden.
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|e| HubError::Internal(format!("HTTP-Client nicht baubar: {e}")))?;
        Ok(Self {
            client,
            base_url,
            limiter: RateLimiter::default(),
        })
    }

    /// Führt einen Discord-Aufruf aus. `path` beginnt mit `/` (ohne API-Basis).
    /// 204 → `Value::Null`.
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        reason: Option<&str>,
    ) -> Result<Value, RestError> {
        let route = RouteKey::new(method.as_str(), path);
        let url = format!("{}{}", self.base_url, path);

        for attempt in 1..=MAX_ATTEMPTS {
            let wait = self.limiter.acquire(&route);
            if wait > MAX_RETRY_WAIT {
                tracing::warn!(route = %route.template, ?wait, "Discord-Bucket zu lange gesperrt");
                return Err(RestError::RateLimited);
            }
            if !wait.is_zero() {
                tracing::debug!(route = %route.template, ?wait, "Warte auf Discord-Rate-Limit");
                tokio::time::sleep(wait).await;
            }

            let mut builder = self.client.request(method.clone(), &url);
            if let Some(reason) = reason {
                builder = builder.header("X-Audit-Log-Reason", audit_reason(reason));
            }
            if let Some(body) = body {
                builder = builder.json(body);
            }
            let response = builder
                .send()
                .await
                .map_err(|e| RestError::Transport(e.without_url().to_string()))?;
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            self.limiter.update(&route, &headers);
            let bytes = response
                .bytes()
                .await
                .map_err(|e| RestError::Transport(e.without_url().to_string()))?;
            let json: Value = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap_or(Value::Null)
            };

            if (200..300).contains(&status) {
                return Ok(json);
            }
            if status == 429 {
                let retry_after = json
                    .get("retry_after")
                    .and_then(Value::as_f64)
                    .or_else(|| {
                        headers
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.trim().parse::<f64>().ok())
                    })
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(1.0);
                let global = json.get("global").and_then(Value::as_bool).unwrap_or(false)
                    || headers
                        .get("x-ratelimit-global")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
                let retry_after = Duration::from_secs_f64(retry_after);
                self.limiter.block(&route, retry_after, global);
                tracing::warn!(
                    route = %route.template,
                    attempt,
                    ?retry_after,
                    global,
                    "Discord-Rate-Limit (429)"
                );
                if retry_after > MAX_RETRY_WAIT || attempt == MAX_ATTEMPTS {
                    return Err(RestError::RateLimited);
                }
                tokio::time::sleep(retry_after).await;
                continue;
            }
            let code = json.get("code").and_then(Value::as_i64);
            let message = json
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();
            return Err(RestError::Status {
                status,
                code,
                message,
            });
        }
        Err(RestError::RateLimited)
    }
}

/// `X-Audit-Log-Reason` muss URL-kodiert sein und ist auf 512 Zeichen begrenzt.
fn audit_reason(reason: &str) -> String {
    let short: String = reason.chars().take(512).collect();
    let mut out = String::with_capacity(short.len());
    for byte in short.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_reason_kodiert_sonderzeichen() {
        assert_eq!(audit_reason("a b:ü"), "a%20b:%C3%BC");
    }

    #[test]
    fn debug_enthaelt_kein_token() {
        let http = DiscordHttp::new("geheimes-token", "http://127.0.0.1:1").expect("client");
        let rendered = format!("{http:?}");
        assert!(!rendered.contains("geheimes-token"));
    }

    #[test]
    fn leeres_token_ist_fehler() {
        assert!(DiscordHttp::new("  ", DISCORD_API_BASE).is_err());
    }
}
