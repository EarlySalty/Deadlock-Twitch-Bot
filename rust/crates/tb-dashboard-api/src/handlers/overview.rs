//! Handler für `GET /twitch/api/v2/overview`.
//!
//! Admin-only. Kein Partner-Session-Auth (deferred, ADR 0003).

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::overview::{overview_metrics, overview_session_count};
use tb_http_core::{ApiError, AuthLevel};

#[derive(Deserialize)]
pub struct OverviewParams {
    pub streamer: Option<String>,
    /// Zeitraum in Tagen. Default 30, min 7, max 365.
    #[serde(default = "default_days")]
    pub days: i64,
}

fn default_days() -> i64 {
    30
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum OverviewResponse {
    Empty { empty: bool, error: &'static str },
    Data(OverviewData),
}

#[derive(Serialize)]
pub struct OverviewData {
    pub streamer: Option<String>,
    pub days: i64,
    pub summary: OverviewSummary,
}

#[derive(Serialize)]
pub struct OverviewSummary {
    #[serde(rename = "avgViewers")]
    pub avg_viewers: f64,
    #[serde(rename = "peakViewers")]
    pub peak_viewers: i64,
    #[serde(rename = "totalHoursWatched")]
    pub total_hours_watched: f64,
    #[serde(rename = "totalAirtime")]
    pub total_airtime: f64,
    #[serde(rename = "followersDelta")]
    pub followers_delta: i64,
    #[serde(rename = "totalSessions")]
    pub total_sessions: i64,
}

/// `GET /twitch/api/v2/overview?streamer=<login>[&days=30]`
pub async fn overview_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<OverviewParams>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // days: clip to [7, 365]
    let days = params.days.clamp(7, 365);
    let since = (Utc::now() - Duration::days(days)).to_rfc3339();
    let login = params
        .streamer
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let login_ref = login.as_deref();

    // Existenz-Check
    let count = overview_session_count(&pool, &since, login_ref)
        .await
        .map_err(|_| ApiError::internal())?;

    if count == 0 {
        return Ok(Json(OverviewResponse::Empty {
            empty: true,
            error: "Keine Daten für den Zeitraum",
        }));
    }

    let metrics = overview_metrics(&pool, &since, login_ref)
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::internal)?;

    Ok(Json(OverviewResponse::Data(OverviewData {
        streamer: params.streamer,
        days,
        summary: OverviewSummary {
            avg_viewers: metrics.avg_avg_viewers.unwrap_or(0.0),
            peak_viewers: metrics.max_peak_viewers.unwrap_or(0),
            total_hours_watched: metrics.total_hours_watched.unwrap_or(0.0),
            total_airtime: metrics.total_airtime_hours.unwrap_or(0.0),
            followers_delta: metrics.total_followers.unwrap_or(0),
            total_sessions: metrics.session_count.unwrap_or(0),
        },
    })))
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
            .expect("connect test-db");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen fehlgeschlagen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id               BIGSERIAL PRIMARY KEY,
                streamer_login   TEXT NOT NULL,
                started_at       TIMESTAMPTZ NOT NULL,
                ended_at         TIMESTAMPTZ,
                avg_viewers      DOUBLE PRECISION,
                peak_viewers     BIGINT,
                duration_seconds BIGINT,
                follower_delta   BIGINT,
                followers_start  BIGINT,
                followers_end    BIGINT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        // Tabelle leeren damit Wiederholungsläufe nicht alte Daten sehen
        sqlx::query("TRUNCATE twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("TRUNCATE fehlgeschlagen");
        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/v2/overview", get(overview_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn admin_req(token: &str, streamer: &str) -> Request<Body> {
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        Request::builder()
            .uri(format!("/twitch/api/v2/overview?streamer={streamer}"))
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", token)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn returns_401_without_auth() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_handler_overview_unauth").await;
        let addr: SocketAddr = "1.2.3.4:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/v2/overview?streamer=x")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn returns_empty_for_unknown_streamer() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_handler_overview_empty").await;
        let res = make_router(pool, "tok")
            .oneshot(admin_req("tok", "nobody"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 256).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["empty"], true);
    }

    #[tokio::test]
    async fn returns_metrics_for_known_streamer() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_handler_overview_mit_daten").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (streamer_login, started_at, ended_at, avg_viewers, peak_viewers,
                 duration_seconds, follower_delta, followers_start, followers_end)
            VALUES
                ('streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours',
                 100.0, 200, 3600, 5, 1000, 1005)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let res = make_router(pool, "tok")
            .oneshot(admin_req("tok", "streamer_x"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!((v["summary"]["avgViewers"].as_f64().unwrap() - 100.0).abs() < 0.001);
        assert_eq!(v["summary"]["totalSessions"], 1);
    }
}
