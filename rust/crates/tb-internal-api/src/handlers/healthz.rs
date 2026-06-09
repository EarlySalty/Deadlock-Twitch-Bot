//! Handler für `GET /internal/twitch/v1/healthz`.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::global_ban::db_schema_fingerprint;
use tb_http_core::{ApiError, AuthLevel};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbInfo {
    pub fingerprint: Option<String>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthzResponse {
    pub ok: bool,
    pub service: &'static str,
    pub analytics_db_fingerprint: Option<String>,
    pub analytics_db: DbInfo,
}

/// Parst Host, Port, DB-Name und User aus einem PostgreSQL-DSN.
///
/// Unterstützte Formate:
/// - `postgres://user:pass@host:port/dbname`
/// - `postgresql://user@host/dbname`
/// - `host=… port=… dbname=… user=…` (Key-Value)
fn parse_dsn(dsn: &str) -> DbInfo {
    // URL-Format
    if dsn.starts_with("postgres://") || dsn.starts_with("postgresql://") {
        if let Ok(url) = url::Url::parse(dsn) {
            return DbInfo {
                fingerprint: None,
                host: url.host_str().unwrap_or("unknown").to_string(),
                port: url.port().unwrap_or(5432),
                database: url.path().trim_start_matches('/').to_string(),
                user: url.username().to_string(),
            };
        }
    }
    // Key-Value-Format: host=... port=... dbname=... user=...
    let get = |key: &str| -> Option<String> {
        dsn.split_whitespace()
            .find(|s| s.starts_with(&format!("{key}=")))
            .and_then(|s| s.split_once('='))
            .map(|(_, v)| v.trim_matches('\'').to_string())
    };
    DbInfo {
        fingerprint: None,
        host: get("host").unwrap_or_else(|| "unknown".to_string()),
        port: get("port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(5432),
        database: get("dbname").unwrap_or_else(|| "unknown".to_string()),
        user: get("user").unwrap_or_else(|| "unknown".to_string()),
    }
}

/// `GET /internal/twitch/v1/healthz`
pub async fn healthz_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let dsn = std::env::var("TWITCH_ANALYTICS_DSN").unwrap_or_default();
    let fingerprint = db_schema_fingerprint(&pool).await.ok();

    let mut db_info = parse_dsn(&dsn);
    db_info.fingerprint = fingerprint.clone();

    Ok(Json(HealthzResponse {
        ok: true,
        service: "twitch-internal-api",
        analytics_db_fingerprint: fingerprint,
        analytics_db: db_info,
    }))
}
