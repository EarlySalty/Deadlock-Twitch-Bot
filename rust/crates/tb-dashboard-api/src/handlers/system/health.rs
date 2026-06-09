//! Handler für `GET /twitch/api/admin/system/health`.

use axum::{extract::State, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use tb_analytics::system_health::{raw_chat_health, system_last_tick};
use tb_http_core::{ApiError, AuthLevel};

use crate::process_info;

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
    pub service_warnings: Vec<ServiceWarning>,
}

fn fmt_dt(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

/// `GET /twitch/api/admin/system/health`
pub async fn health_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
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
                    if lag > 120 {
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

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema");
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
                last_seen_at    TIMESTAMPTZ,
                last_started_at TIMESTAMPTZ
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
                last_raw_chat_message_at      TIMESTAMPTZ,
                last_raw_chat_insert_ok_at    TIMESTAMPTZ,
                last_raw_chat_insert_error_at TIMESTAMPTZ,
                last_error                    TEXT,
                updated_at                    TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
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
    }
}
