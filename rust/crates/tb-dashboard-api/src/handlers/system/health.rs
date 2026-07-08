//! Handler für `GET /twitch/api/admin/system/health`.

use crate::auth::level::DashboardAuthLevel;
use axum::{extract::State, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::system_health::{raw_chat_health, system_last_tick};
use tb_http_core::ApiError;

use crate::process_info;

/// Schwelle (Sekunden) ab der ein Raw-Chat-Lag als Warnung gemeldet wird.
/// Python-Parität: `bot/analytics/api_admin.py` verwendet 900s (15 min).
const RAW_CHAT_LAG_WARN_SECONDS: i64 = 900;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceWarning {
    pub level: &'static str,
    pub code: &'static str,
    pub message: String,
    pub timestamp: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub uptime_seconds: u64,
    pub memory_rss_bytes: Option<u64>,
    pub rust_version: &'static str,
    pub process_id: u32,
    pub last_tick_at: Option<String>,
    pub last_tick_age_seconds: Option<i64>,
    pub raw_chat_lag_seconds: Option<i64>,
    pub raw_chat_lag_streamer: Option<String>,
    pub raw_chat_last_message_at: Option<String>,
    pub raw_chat_last_insert_ok_at: Option<String>,
    pub raw_chat_last_insert_error_at: Option<String>,
    pub raw_chat_last_error: Option<String>,
    /// Lokal (Dashboard-DSN) berechneter Analytics-DB-Fingerprint.
    pub analytics_db_fingerprint: Option<String>,
    /// Fingerprint des Bot/Internal-API-Prozesses (über `/healthz` gelesen).
    pub internal_analytics_db_fingerprint: Option<String>,
    /// `true` wenn beide Fingerprints vorliegen und sich unterscheiden
    /// (Dashboard und Bot zeigen auf verschiedene Analytics-Datenbanken).
    pub analytics_db_fingerprint_mismatch: bool,
    pub service_warnings: Vec<ServiceWarning>,
}

fn fmt_dt(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

// ── Analytics-DB-Fingerprint (Parität zu tb-internal-api/healthz.rs) ───────────

const FINGERPRINT_SALT: &[u8] = b"deadlock.analytics-db-fingerprint.v1";
const FINGERPRINT_ITERATIONS: u32 = 100_000;

fn fingerprint_hex(value: &str) -> String {
    let mut out = [0u8; 6];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
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
        if let Ok(url) = url::Url::parse(dsn) {
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

/// Lokaler Analytics-DB-Fingerprint aus dem Dashboard-DSN.
/// `None` wenn kein DSN gesetzt ist (kein Vergleich möglich).
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

/// Basis-URL der Internal-API (gleiche Konvention wie admin_chat_action.rs).
fn internal_base_url() -> String {
    if let Some(explicit) = std::env::var("TWITCH_INTERNAL_API_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return explicit.trim_end_matches('/').to_string();
    }
    let host = std::env::var("TWITCH_INTERNAL_API_HOST")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = std::env::var("TWITCH_INTERNAL_API_PORT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "8776".to_string());
    format!("http://{host}:{port}")
}

/// Liest den `analyticsDbFingerprint` vom Internal-API-`/healthz`-Endpoint.
/// Fehler/Timeout → `None` (kein Mismatch-Alarm bei unerreichbarem Upstream).
/// Ohne gesetztes Internal-Token wird kein Aufruf versucht.
async fn fetch_internal_fingerprint() -> Option<String> {
    let token = std::env::var("TWITCH_INTERNAL_API_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    let url = format!("{}/internal/twitch/v1/healthz", internal_base_url());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let resp = client
        .get(&url)
        .header("X-Internal-Token", token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("analyticsDbFingerprint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `GET /twitch/api/admin/system/health`
pub async fn health_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }

    // Bug A: ApiError::internal() ohne Argument
    let last_tick = system_last_tick(&pool).await.map_err(|e| {
        tracing::error!("system_last_tick Fehler: {e}");
        ApiError::internal()
    })?;

    let last_tick_age_seconds =
        last_tick.map(|dt| Utc::now().signed_duration_since(dt).num_seconds());

    let chat = raw_chat_health(&pool).await.map_err(|e| {
        tracing::error!("raw_chat_health Fehler: {e}");
        ApiError::internal()
    })?;

    let mut warnings: Vec<ServiceWarning> = Vec::new();

    let (lag, lag_streamer, last_msg_at, last_ok_at, last_err_at, last_error) = match chat {
        Some(h) => {
            // Bug D: RAW_CHAT_LAG Warning nur wenn is_live_scope = true
            if h.is_live_scope {
                if let (Some(lag), Some(ref streamer)) = (h.lag_seconds, &h.streamer_login) {
                    // P3.17: Python-Schwelle für RAW_CHAT_LAG ist 900s (15 min),
                    // nicht 120s — ein ~3-Minuten-Lag ist im normalen Betrieb
                    // unkritisch und soll keinen Alarm auslösen.
                    if lag >= RAW_CHAT_LAG_WARN_SECONDS {
                        warnings.push(ServiceWarning {
                            level: "warn",
                            code: "RAW_CHAT_LAG",
                            message: format!("Raw-Chat-Lag {lag}s für live Streamer {streamer}"),
                            timestamp: Utc::now().to_rfc3339(),
                        });
                    }
                }
            }
            if let Some(ref err) = h.last_error {
                if !err.is_empty() {
                    warnings.push(ServiceWarning {
                        level: "error",
                        code: "RAW_CHAT_ERROR",
                        message: err.clone(),
                        timestamp: Utc::now().to_rfc3339(),
                    });
                }
            }
            (
                h.lag_seconds,
                h.streamer_login,
                h.last_message_at.map(fmt_dt),
                h.last_insert_ok_at.map(fmt_dt),
                h.last_insert_error_at.map(fmt_dt),
                h.last_error,
            )
        }
        None => (None, None, None, None, None, None),
    };

    // P2.79: Analytics-DB-Fingerprint von Dashboard und Bot vergleichen.
    // Nur wenn beide Werte vorliegen UND abweichen → Warnung. Fehlt der
    // Upstream (Internal-API down) oder ein DSN, wird kein Alarm ausgelöst.
    let local_fp = local_analytics_fingerprint();
    let internal_fp = fetch_internal_fingerprint().await;
    let fingerprint_mismatch = match (&local_fp, &internal_fp) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };
    if fingerprint_mismatch {
        warnings.push(ServiceWarning {
            level: "error",
            code: "analytics_db_fingerprint_mismatch",
            message: "Dashboard und Bot greifen auf unterschiedliche Analyse-Datenbanken zu – die angezeigten Werte können abweichen.".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        });
    }

    Ok(Json(HealthResponse {
        uptime_seconds: process_info::uptime_secs(),
        memory_rss_bytes: process_info::memory_rss_bytes(),
        rust_version: env!("CARGO_PKG_VERSION"),
        process_id: process_info::pid(),
        last_tick_at: last_tick.map(fmt_dt),
        last_tick_age_seconds,
        raw_chat_lag_seconds: lag,
        raw_chat_lag_streamer: lag_streamer,
        raw_chat_last_message_at: last_msg_at,
        raw_chat_last_insert_ok_at: last_ok_at,
        raw_chat_last_insert_error_at: last_err_at,
        raw_chat_last_error: last_error,
        analytics_db_fingerprint: local_fp,
        internal_analytics_db_fingerprint: internal_fp,
        analytics_db_fingerprint_mismatch: fingerprint_mismatch,
        service_warnings: warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Gibt die DSN zurück oder bricht den Test ab.
    /// Mit `TB_TEST_REQUIRE_DB=1` wird statt des stillen Skips ein panic ausgelöst.
    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");
        // Bug B: INTEGER NOT NULL DEFAULT 0 statt BOOLEAN
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login  TEXT PRIMARY KEY,
                is_live         INTEGER NOT NULL DEFAULT 0,
                last_seen_at    TEXT,
                last_started_at TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL live_state");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raw_chat_ingest_health (
                streamer_login                TEXT PRIMARY KEY,
                last_raw_chat_message_at      TEXT,
                last_raw_chat_insert_ok_at    TEXT,
                last_raw_chat_insert_error_at TEXT,
                last_raw_chat_error           TEXT,
                raw_chat_lag_seconds          INTEGER,
                updated_at                    TEXT NOT NULL DEFAULT (NOW()::text)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL raw_chat");
        sqlx::query("TRUNCATE twitch_live_state, twitch_raw_chat_ingest_health")
            .execute(&pool)
            .await
            .expect("TRUNCATE");
        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/system/health", get(health_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    #[tokio::test]
    async fn returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_health_unauth").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/health")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_200_mit_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_health_auth").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/health")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v["uptimeSeconds"].as_u64().is_some());
        assert!(v["processId"].as_u64().is_some());
        assert!(v["serviceWarnings"].is_array());
        // P2.79: Fingerprint-Felder müssen immer vorhanden sein.
        assert!(v.get("analyticsDbFingerprintMismatch").is_some());
        assert!(v.get("analyticsDbFingerprint").is_some());
        assert!(v.get("internalAnalyticsDbFingerprint").is_some());
    }

    // P2.79: Fingerprint stimmt mit dem Internal-API-Algorithmus überein
    // (gleicher Salt/Iterationen/Feld-Layout wie tb-internal-api/healthz.rs).
    #[test]
    fn fingerprint_entspricht_internal_api_referenz() {
        // Referenzwert aus tb-internal-api/src/handlers/healthz.rs Tests.
        let (host, port, dbname) =
            analytics_identity_fields("postgres://u:p@localhost:5432/deadlock");
        let fp = format!("pg:{}", fingerprint_hex(&format!("{host}|{port}|{dbname}")));
        assert_eq!(fp, "pg:1686bea09e14");
    }

    #[test]
    fn fingerprint_mismatch_nur_bei_zwei_unterschiedlichen() {
        // Beide gesetzt + gleich → kein Mismatch.
        let same = match (&Some("pg:a".to_string()), &Some("pg:a".to_string())) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        };
        assert!(!same);
        // Beide gesetzt + verschieden → Mismatch.
        let diff = match (&Some("pg:a".to_string()), &Some("pg:b".to_string())) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        };
        assert!(diff);
        // Einer fehlt → kein Mismatch.
        let one = match (&Some("pg:a".to_string()), &None::<String>) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        };
        assert!(!one);
    }

    // P3.17: 3-Minuten-Lag (180s) liegt unter der 900s-Schwelle → keine Warnung.
    #[tokio::test]
    async fn raw_chat_lag_unter_900s_keine_warnung() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_health_lag180").await;
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_seen_at) \
             VALUES ('lag_streamer', 1, NOW()::text)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raw_chat_ingest_health \
             (streamer_login, last_raw_chat_message_at, raw_chat_lag_seconds) \
             VALUES ('lag_streamer', (NOW() - INTERVAL '180 seconds')::text, 180)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/admin/system/health")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        let warnings = v["serviceWarnings"].as_array().unwrap();
        assert!(
            !warnings.iter().any(|w| w["code"] == "RAW_CHAT_LAG"),
            "180s-Lag darf keine RAW_CHAT_LAG-Warnung erzeugen"
        );
    }
}
