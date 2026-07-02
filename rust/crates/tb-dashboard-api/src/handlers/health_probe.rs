use std::{net::IpAddr, time::Duration};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use pbkdf2::pbkdf2_hmac;
use serde_json::{json, Value};
use sha2::Sha256;
use sqlx::PgPool;
use tb_http_core::{INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER};
use url::Url;

const SERVICE_NAME: &str = "twitch-dashboard-service";
const DEFAULT_INTERNAL_API_HOST: &str = "127.0.0.1";
const DEFAULT_INTERNAL_API_PORT: &str = "8776";
const INTERNAL_HEALTH_TIMEOUT: Duration = Duration::from_secs(10);
const FINGERPRINT_SALT: &[u8] = b"deadlock.analytics-db-fingerprint.v1";
const FINGERPRINT_ITERATIONS: u32 = 100_000;

#[derive(Clone, Debug)]
struct InternalApiConfig {
    base_url: String,
    token: String,
}

#[derive(Debug)]
struct InternalApiError {
    status: u16,
    code: String,
    message: String,
}

#[derive(Clone, Debug, Default)]
pub struct AnalyticsDbFingerprintStartup {
    pub analytics_db_fingerprint: Option<String>,
    pub internal_api_analytics_db_fingerprint: Option<String>,
    pub analytics_db_fingerprint_mismatch: bool,
}

pub async fn analytics_db_fingerprint_startup_check() -> AnalyticsDbFingerprintStartup {
    let mut startup = AnalyticsDbFingerprintStartup {
        analytics_db_fingerprint: local_analytics_fingerprint(),
        internal_api_analytics_db_fingerprint: None,
        analytics_db_fingerprint_mismatch: false,
    };
    let Some(config) = internal_api_config() else {
        return startup;
    };
    match fetch_internal_health(&config).await {
        Ok(payload) => {
            let upstream_fp = payload
                .get("analyticsDbFingerprint")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
            startup.internal_api_analytics_db_fingerprint = upstream_fp;
            if let (Some(local), Some(upstream)) = (
                startup.analytics_db_fingerprint.as_deref(),
                startup.internal_api_analytics_db_fingerprint.as_deref(),
            ) {
                if local != upstream {
                    startup.analytics_db_fingerprint_mismatch = true;
                    tracing::error!(
                        dashboard = local,
                        internal_api = upstream,
                        "Analytics DB fingerprint mismatch"
                    );
                }
            }
        }
        Err(error) => {
            tracing::warn!(
                status = error.status,
                code = %error.code,
                "Dashboard fingerprint check against internal API failed"
            );
        }
    }
    startup
}

fn startup_fingerprint(
    cache: Option<Extension<AnalyticsDbFingerprintStartup>>,
) -> AnalyticsDbFingerprintStartup {
    cache
        .map(|Extension(cache)| cache)
        .unwrap_or_else(|| AnalyticsDbFingerprintStartup {
            analytics_db_fingerprint: local_analytics_fingerprint(),
            internal_api_analytics_db_fingerprint: None,
            analytics_db_fingerprint_mismatch: false,
        })
}

pub async fn healthz_handler(
    cache: Option<Extension<AnalyticsDbFingerprintStartup>>,
) -> Json<Value> {
    let startup = startup_fingerprint(cache);
    Json(json!({
        "ok": true,
        "service": SERVICE_NAME,
        "status": "alive",
        "internalApiConfigured": internal_api_config().is_some(),
        "oauthConfigured": oauth_configured(),
        "analyticsDbFingerprint": startup.analytics_db_fingerprint,
        "internalApiAnalyticsDbFingerprint": startup.internal_api_analytics_db_fingerprint,
    }))
}

pub async fn readyz_handler(
    State(pool): State<PgPool>,
    cache: Option<Extension<AnalyticsDbFingerprintStartup>>,
) -> Response {
    let database_error = match sqlx::query_scalar!("SELECT 1 AS \"one!\"")
        .fetch_one(&pool)
        .await
    {
        Ok(_) => None,
        Err(error) => Some(error.to_string()),
    };
    readyz_response(
        startup_fingerprint(cache),
        internal_api_config(),
        oauth_configured(),
        database_error,
    )
    .await
}

async fn readyz_response(
    startup: AnalyticsDbFingerprintStartup,
    internal_config: Option<InternalApiConfig>,
    oauth_ready: bool,
    database_error: Option<String>,
) -> Response {
    let mut reasons: Vec<String> = Vec::new();
    let mut details = json!({
        "database": if database_error.is_some() { "error" } else { "ok" },
        "internalApiConfigured": internal_config.is_some(),
        "oauthConfigured": oauth_ready,
        "analyticsDbFingerprint": startup.analytics_db_fingerprint,
        "internalApiAnalyticsDbFingerprint": startup.internal_api_analytics_db_fingerprint,
        "analyticsDbFingerprintMismatch": startup.analytics_db_fingerprint_mismatch,
    });

    if let Some(error) = database_error {
        reasons.push("database_unavailable".to_string());
        details["databaseError"] = json!(error);
    }

    if let Some(config) = internal_config {
        match fetch_internal_health(&config).await {
            Ok(payload) => {
                details["internalApiHealth"] = payload;
            }
            Err(error) => {
                reasons.push(if error.code.is_empty() {
                    "internal_api_unavailable".to_string()
                } else {
                    error.code.clone()
                });
                details["internalApiError"] = json!({
                    "status": error.status,
                    "code": error.code,
                    "message": error.message,
                });
            }
        }
    } else {
        reasons.push("internal_api_not_configured".to_string());
    }

    if !oauth_ready {
        reasons.push("oauth_not_configured".to_string());
    }

    if startup.analytics_db_fingerprint_mismatch {
        reasons.push("analytics_db_fingerprint_mismatch".to_string());
    }

    let ok = reasons.is_empty();
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "ok": ok,
            "service": SERVICE_NAME,
            "status": if ok { "ready" } else { "degraded" },
            "reasons": reasons,
            "details": details,
        })),
    )
        .into_response()
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_env_bool(name: &str, default: bool) -> bool {
    match non_empty_env(name)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn oauth_configured() -> bool {
    parse_env_bool("TWITCH_DASHBOARD_NOAUTH", false)
        || (non_empty_env("TWITCH_CLIENT_ID").is_some()
            && non_empty_env("TWITCH_CLIENT_SECRET").is_some())
}

fn internal_api_config() -> Option<InternalApiConfig> {
    let token = non_empty_env("TWITCH_INTERNAL_API_TOKEN")?;
    let raw_base = non_empty_env("TWITCH_INTERNAL_API_BASE_URL").unwrap_or_else(|| {
        let host = non_empty_env("TWITCH_INTERNAL_API_HOST")
            .unwrap_or_else(|| DEFAULT_INTERNAL_API_HOST.to_string());
        let port = non_empty_env("TWITCH_INTERNAL_API_PORT")
            .unwrap_or_else(|| DEFAULT_INTERNAL_API_PORT.to_string());
        format!("http://{host}:{port}")
    });
    let allow_non_loopback = parse_env_bool("TWITCH_INTERNAL_API_ALLOW_NON_LOOPBACK", false);
    let base_url = normalize_internal_base_url(&raw_base, allow_non_loopback)?;
    Some(InternalApiConfig { base_url, token })
}

fn normalize_internal_base_url(raw: &str, allow_non_loopback: bool) -> Option<String> {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return None;
    }
    if !value.contains("://") {
        value = format!("http://{value}");
    }
    let mut url = Url::parse(&value).ok()?;
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = url.host_str()?.trim().to_string();
    if host.is_empty() {
        return None;
    }
    let is_loopback = is_loopback_host(&host);
    if !allow_non_loopback && !is_loopback {
        return None;
    }
    if !is_loopback && scheme != "https" {
        return None;
    }
    let _ = url.port_or_known_default()?;
    let mut path = url.path().trim_end_matches('/').to_string();
    let internal_base = INTERNAL_API_BASE_PATH.trim_end_matches('/');
    if path == internal_base {
        path.clear();
    } else if path.ends_with(internal_base) {
        path.truncate(path.len().saturating_sub(internal_base.len()));
        path = path.trim_end_matches('/').to_string();
    }
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_string())
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized == "localhost" {
        return true;
    }
    normalized
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

async fn fetch_internal_health(config: &InternalApiConfig) -> Result<Value, InternalApiError> {
    let client = reqwest::Client::builder()
        .timeout(INTERNAL_HEALTH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| InternalApiError {
            status: 502,
            code: "upstream_connection_failed".to_string(),
            message: "Bot internal API is unreachable.".to_string(),
        })?;
    let url = format!(
        "{}{}/healthz",
        config.base_url,
        INTERNAL_API_BASE_PATH.trim_end_matches('/')
    );
    let response = client
        .get(url)
        .header(INTERNAL_TOKEN_HEADER, &config.token)
        .send()
        .await
        .map_err(map_reqwest_error)?;
    let status = response.status().as_u16();
    let raw_text = response.text().await.unwrap_or_default();
    let parsed = parse_json(&raw_text);
    if status >= 400 {
        return Err(map_http_error(status, parsed.as_ref()));
    }
    let Some(body) = parsed else {
        return Err(InternalApiError {
            status: 502,
            code: "upstream_invalid_json".to_string(),
            message: "Bot internal API returned invalid JSON.".to_string(),
        });
    };
    if !body.is_object() {
        return Err(InternalApiError {
            status: 502,
            code: "upstream_invalid_shape".to_string(),
            message: "Bot internal API returned an invalid health payload.".to_string(),
        });
    }
    Ok(body)
}

fn map_reqwest_error(error: reqwest::Error) -> InternalApiError {
    if error.is_timeout() {
        return InternalApiError {
            status: 504,
            code: "upstream_timeout".to_string(),
            message: "Bot internal API request timed out.".to_string(),
        };
    }
    InternalApiError {
        status: 502,
        code: "upstream_connection_failed".to_string(),
        message: "Bot internal API is unreachable.".to_string(),
    }
}

fn parse_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(json!({}));
    }
    serde_json::from_str(trimmed).ok()
}

fn extract_error_text(payload: Option<&Value>) -> String {
    payload
        .and_then(Value::as_object)
        .and_then(|obj| {
            ["message", "error", "detail", "reason"]
                .iter()
                .find_map(|key| obj.get(*key).and_then(Value::as_str))
        })
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn upstream_error_code(payload: Option<&Value>) -> String {
    payload
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("error").and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn sanitize_message(value: String, fallback: &str) -> String {
    let text = value.replace(['\r', '\n'], " ").trim().to_string();
    if text.is_empty() {
        return fallback.to_string();
    }
    if text.chars().count() > 220 {
        format!("{}...", text.chars().take(217).collect::<String>())
    } else {
        text
    }
}

fn map_http_error(status: u16, payload: Option<&Value>) -> InternalApiError {
    let upstream_message = extract_error_text(payload);
    match status {
        400 | 404 => {
            let code = if status == 400 {
                "bad_request"
            } else {
                "not_found"
            };
            let fallback = if status == 400 {
                "Bot internal API rejected the request."
            } else {
                "Requested resource was not found."
            };
            InternalApiError {
                status,
                code: code.to_string(),
                message: sanitize_message(upstream_message, fallback),
            }
        }
        401 | 403 => InternalApiError {
            status: 502,
            code: "upstream_auth_failed".to_string(),
            message: "Dashboard service failed to authenticate with bot internal API.".to_string(),
        },
        429 => InternalApiError {
            status: 503,
            code: "upstream_rate_limited".to_string(),
            message: "Bot internal API is currently rate limited.".to_string(),
        },
        500..=599 => {
            let code = upstream_error_code(payload);
            InternalApiError {
                status: 502,
                code: if code.is_empty() {
                    "upstream_unavailable".to_string()
                } else {
                    code
                },
                message: sanitize_message(
                    upstream_message,
                    "Bot internal API is currently unavailable.",
                ),
            }
        }
        _ => InternalApiError {
            status: 502,
            code: "upstream_error".to_string(),
            message: "Bot internal API request failed.".to_string(),
        },
    }
}

fn fingerprint_hex(value: &str) -> String {
    let mut out = [0u8; 6];
    pbkdf2_hmac::<Sha256>(
        value.as_bytes(),
        FINGERPRINT_SALT,
        FINGERPRINT_ITERATIONS,
        &mut out,
    );
    hex::encode(out)
}

fn analytics_identity_fields(dsn: &str) -> (String, String, String) {
    let norm = |v: Option<String>| v.unwrap_or_default().trim().to_lowercase();
    if dsn.starts_with("postgres://") || dsn.starts_with("postgresql://") {
        if let Ok(url) = Url::parse(dsn) {
            return (
                norm(url.host_str().map(str::to_string)),
                norm(url.port().map(|p| p.to_string())),
                norm(Some(url.path().trim_start_matches('/').to_string())),
            );
        }
    }
    let get = |key: &str| -> Option<String> {
        dsn.split_whitespace()
            .find(|s| s.starts_with(&format!("{key}=")))
            .and_then(|s| s.split_once('='))
            .map(|(_, v)| v.trim_matches('\'').to_string())
    };
    (
        norm(get("host")),
        norm(get("port")),
        norm(get("dbname").or_else(|| get("database"))),
    )
}

fn local_analytics_fingerprint() -> Option<String> {
    let dsn = std::env::var("TWITCH_ANALYTICS_DSN")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_default();
    if dsn.trim().is_empty() {
        return None;
    }
    let (host, port, dbname) = analytics_identity_fields(&dsn);
    Some(format!(
        "pg:{}",
        fingerprint_hex(&format!("{host}|{port}|{dbname}"))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn startup_cache(
        local_fp: &str,
        internal_fp: Option<&str>,
        mismatch: bool,
    ) -> AnalyticsDbFingerprintStartup {
        AnalyticsDbFingerprintStartup {
            analytics_db_fingerprint: Some(local_fp.to_string()),
            internal_api_analytics_db_fingerprint: internal_fp.map(ToOwned::to_owned),
            analytics_db_fingerprint_mismatch: mismatch,
        }
    }

    fn internal_config(server: &MockServer) -> InternalApiConfig {
        InternalApiConfig {
            base_url: server.uri(),
            token: "tok".to_string(),
        }
    }

    async fn json_response(
        cache: AnalyticsDbFingerprintStartup,
        internal_config: Option<InternalApiConfig>,
        oauth_ready: bool,
    ) -> (StatusCode, Value) {
        let response = readyz_response(cache, internal_config, oauth_ready, None).await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn readyz_ohne_upstream_und_oauth_liefert_503() {
        let (status, body) =
            json_response(startup_cache("pg:local", None, false), None, false).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["ok"], false);
        assert_eq!(body["status"], "degraded");
        assert_eq!(
            body["reasons"],
            json!(["internal_api_not_configured", "oauth_not_configured"])
        );
        assert_eq!(body["details"]["internalApiConfigured"], false);
        assert_eq!(body["details"]["oauthConfigured"], false);
    }

    #[tokio::test]
    async fn readyz_mit_upstream_und_oauth_ist_ready() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/twitch/v1/healthz"))
            .and(header("X-Internal-Token", "tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "service": "twitch-internal-api",
                "analyticsDbFingerprint": "pg:1686bea09e14"
            })))
            .mount(&server)
            .await;

        let (status, body) = json_response(
            startup_cache("pg:1686bea09e14", Some("pg:1686bea09e14"), false),
            Some(internal_config(&server)),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["status"], "ready");
        assert_eq!(body["reasons"], json!([]));
        assert_eq!(body["details"]["internalApiConfigured"], true);
        assert_eq!(body["details"]["oauthConfigured"], true);
        assert_eq!(
            body["details"]["internalApiAnalyticsDbFingerprint"],
            "pg:1686bea09e14"
        );
        assert_eq!(body["details"]["analyticsDbFingerprintMismatch"], false);
    }

    #[tokio::test]
    async fn readyz_cached_fingerprint_mismatch_liefert_503() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/twitch/v1/healthz"))
            .and(header("X-Internal-Token", "tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "service": "twitch-internal-api",
                "analyticsDbFingerprint": "pg:1686bea09e14"
            })))
            .mount(&server)
            .await;

        let (status, body) = json_response(
            startup_cache("pg:1686bea09e14", Some("pg:different"), true),
            Some(internal_config(&server)),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["ok"], false);
        assert_eq!(
            body["reasons"],
            json!(["analytics_db_fingerprint_mismatch"])
        );
        assert_eq!(body["details"]["analyticsDbFingerprintMismatch"], true);
        assert_eq!(
            body["details"]["internalApiAnalyticsDbFingerprint"],
            "pg:different"
        );
        assert_eq!(
            body["details"]["internalApiHealth"]["analyticsDbFingerprint"],
            "pg:1686bea09e14"
        );
    }

    #[tokio::test]
    async fn readyz_live_fingerprint_mismatch_bei_cached_false_bleibt_ready() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/twitch/v1/healthz"))
            .and(header("X-Internal-Token", "tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "service": "twitch-internal-api",
                "analyticsDbFingerprint": "pg:different"
            })))
            .mount(&server)
            .await;

        let (status, body) = json_response(
            startup_cache("pg:1686bea09e14", Some("pg:1686bea09e14"), false),
            Some(internal_config(&server)),
            true,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["reasons"], json!([]));
        assert_eq!(body["details"]["analyticsDbFingerprintMismatch"], false);
        assert_eq!(
            body["details"]["internalApiAnalyticsDbFingerprint"],
            "pg:1686bea09e14"
        );
        assert_eq!(
            body["details"]["internalApiHealth"]["analyticsDbFingerprint"],
            "pg:different"
        );
    }
}
