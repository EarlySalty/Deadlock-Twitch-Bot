use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::{Client, Method};
use serde_json::Value;

use super::ratelimit::{RateLimiter, RouteKey};
use crate::HubError;

pub const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const MAX_ATTEMPTS: u32 = 3;
pub(crate) const MAX_RETRY_WAIT: Duration = Duration::from_secs(10);
const AGENT: &str = "DiscordBot (https://github.com/EarlySalty/Deadlock-Twitch-Bot, 0.1)";

#[derive(Debug, Clone)]
pub(crate) enum RestError {
    Status {
        status: u16,
        code: Option<i64>,
        message: String,
    },
    RateLimited,
    Transport(String),
    InvalidResponse,
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
            RestError::InvalidResponse => write!(f, "Ungültige Discord-Antwort"),
            RestError::Transport(e) => write!(f, "Discord nicht erreichbar: {e}"),
        }
    }
}

pub(crate) struct DiscordHttp {
    client: Client,
    base_url: String,
    limiter: RateLimiter,
}

impl std::fmt::Debug for DiscordHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        let parsed = reqwest::Url::parse(&base_url)
            .map_err(|_| HubError::Internal("Ungültige Discord-API-Adresse".into()))?;
        let loopback = parsed.host_str().is_some_and(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        });
        if !(parsed.scheme() == "https" || parsed.scheme() == "http" && loopback)
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(HubError::Internal("Unsichere Discord-API-Adresse".into()));
        }
        let builder = Client::builder()
            .default_headers(headers)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        let client = builder
            .build()
            .map_err(|e| HubError::Internal(format!("HTTP-Client nicht baubar: {e}")))?;
        Ok(Self {
            client,
            base_url,
            limiter: RateLimiter::default(),
        })
    }

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
            let waiting_since = std::time::Instant::now();
            let reservation = loop {
                match self.limiter.acquire(&route) {
                    Ok(reservation) => break reservation,
                    Err(wait) => {
                        if waiting_since.elapsed().saturating_add(wait) > MAX_RETRY_WAIT {
                            return Err(RestError::RateLimited);
                        }
                        tokio::time::sleep(wait).await;
                    }
                }
            };

            let mut builder = self.client.request(method.clone(), &url);
            if let Some(reason) = reason {
                builder = builder.header("X-Audit-Log-Reason", audit_reason(reason));
            }
            if let Some(body) = body {
                builder = builder.json(body);
            }
            let writes = method != Method::GET && path != "/users/@me/channels";
            let previous_write = super::WRITE_UNCERTAIN
                .try_with(|state| {
                    if writes {
                        state.swap(true, std::sync::atomic::Ordering::Relaxed)
                    } else {
                        state.load(std::sync::atomic::Ordering::Relaxed)
                    }
                })
                .unwrap_or(false);
            let response = match builder.send().await {
                Ok(response) => response,
                Err(err) => {
                    if writes && err.is_connect() {
                        let _ = super::WRITE_UNCERTAIN.try_with(|state| {
                            state.store(previous_write, std::sync::atomic::Ordering::Relaxed);
                        });
                    }
                    return Err(RestError::Transport(err.without_url().to_string()));
                }
            };
            let status = response.status().as_u16();
            if writes && (400..500).contains(&status) {
                let _ = super::WRITE_UNCERTAIN.try_with(|state| {
                    state.store(previous_write, std::sync::atomic::Ordering::Relaxed);
                });
            }
            let headers = response.headers().clone();
            reservation.update(&headers);
            drop(reservation);
            let bytes = response
                .bytes()
                .await
                .map_err(|e| RestError::Transport(e.without_url().to_string()))?;
            let json: Value = if bytes.is_empty() {
                Value::Null
            } else {
                match serde_json::from_slice(&bytes) {
                    Ok(value) => value,
                    Err(_) if (200..300).contains(&status) => {
                        return Err(RestError::InvalidResponse)
                    }
                    Err(_) => Value::Null,
                }
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
                let retry_after =
                    Duration::try_from_secs_f64(retry_after).map_err(|_| RestError::RateLimited)?;
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
    fn insecure_api_addresses_are_rejected() {
        for url in [
            "http://example.com",
            "http://localhost:9",
            "https://user:password@example.com",
            "https://example.com/?token=synthetic",
        ] {
            assert!(DiscordHttp::new("synthetic", url).is_err());
        }
    }

    #[tokio::test]
    async fn unrepresentable_retry_after_fails_closed() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gateway/bot"))
            .respond_with(
                ResponseTemplate::new(429).set_body_json(serde_json::json!({"retry_after": 1e100})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let http = DiscordHttp::new("synthetic", &server.uri()).expect("client");
        assert!(matches!(
            http.request(Method::GET, "/gateway/bot", None, None).await,
            Err(RestError::RateLimited)
        ));
    }

    #[test]
    fn leeres_token_ist_fehler() {
        assert!(DiscordHttp::new("  ", DISCORD_API_BASE).is_err());
    }
}
