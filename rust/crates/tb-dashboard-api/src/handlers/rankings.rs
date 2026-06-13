//! Handler für Streamer-Rankings.
//!
//! Port von `bot/analytics/api_performance.py:_load_rankings_payload_sync` + `_api_v2_rankings`.
//! `GET /twitch/api/v2/rankings?metric=viewers&days=30&limit=20&exclude_external=0`
//!
//! Drei SQL-Varianten je nach `metric`; `exclude_external=1` → HAVING AVG(avg_viewers) <= 100.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

const EXTERNAL_REACH_AVG_THRESHOLD: f64 = 100.0;

#[derive(Deserialize)]
pub struct RankingsQuery {
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub days: Option<i32>,
    #[serde(default)]
    pub limit: Option<i32>,
    #[serde(default)]
    pub exclude_external: Option<String>,
}

fn clamp(v: i32, min: i32, max: i32) -> i32 {
    v.max(min).min(max)
}

fn require_auth(auth: &DashboardAuthLevel) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"unauthorized","message":"not authenticated"}))))
    } else {
        Ok(())
    }
}

/// `GET /twitch/api/v2/rankings`
pub async fn rankings_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<RankingsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let metric = params.metric.as_deref().unwrap_or("viewers");
    let days = clamp(params.days.unwrap_or(30), 7, 365);
    let limit = clamp(params.limit.unwrap_or(20), 5, 50) as i64;
    let exclude_external = params.exclude_external.as_deref() == Some("1");
    let since: DateTime<Utc> = Utc::now() - Duration::days(days as i64);

    // Jede der 6 Kombinationen (3 metrics × 2 threshold-Varianten) bekommt ihr eigenes SQL.
    // $1 = since, $2 = limit [, $3 = threshold wenn exclude_external=1].

    let result = match (metric, exclude_external) {
        ("retention", false) => sqlx::query(
            r#"SELECT s.streamer_login,
                      AVG(s.retention_10m) AS value
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3
               ORDER BY value DESC NULLS LAST
               LIMIT $2"#,
        ).bind(since).bind(limit).fetch_all(&pool).await,

        ("retention", true) => sqlx::query(
            r#"SELECT s.streamer_login,
                      AVG(s.retention_10m) AS value
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3 AND AVG(s.avg_viewers) <= $3
               ORDER BY value DESC NULLS LAST
               LIMIT $2"#,
        ).bind(since).bind(limit).bind(EXTERNAL_REACH_AVG_THRESHOLD).fetch_all(&pool).await,

        ("growth", false) => sqlx::query(
            r#"SELECT s.streamer_login,
                      SUM(CASE WHEN s.follower_delta IS NOT NULL
                               AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                               THEN s.follower_delta ELSE 0 END)::float8 AS value
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3
               ORDER BY value DESC NULLS LAST
               LIMIT $2"#,
        ).bind(since).bind(limit).fetch_all(&pool).await,

        ("growth", true) => sqlx::query(
            r#"SELECT s.streamer_login,
                      SUM(CASE WHEN s.follower_delta IS NOT NULL
                               AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                               THEN s.follower_delta ELSE 0 END)::float8 AS value
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3 AND AVG(s.avg_viewers) <= $3
               ORDER BY value DESC NULLS LAST
               LIMIT $2"#,
        ).bind(since).bind(limit).bind(EXTERNAL_REACH_AVG_THRESHOLD).fetch_all(&pool).await,

        // viewers (default)
        (_, false) => sqlx::query(
            r#"SELECT s.streamer_login,
                      AVG(s.avg_viewers) AS value
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3
               ORDER BY value DESC NULLS LAST
               LIMIT $2"#,
        ).bind(since).bind(limit).fetch_all(&pool).await,

        (_, true) => sqlx::query(
            r#"SELECT s.streamer_login,
                      AVG(s.avg_viewers) AS value
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3 AND AVG(s.avg_viewers) <= $3
               ORDER BY value DESC NULLS LAST
               LIMIT $2"#,
        ).bind(since).bind(limit).bind(EXTERNAL_REACH_AVG_THRESHOLD).fetch_all(&pool).await,
    };

    let is_retention = metric == "retention";

    match result {
        Err(e) => {
            tracing::error!("rankings DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows.iter().enumerate().map(|(idx, r)| {
                let raw: f64 = r.try_get::<f64, _>("value").unwrap_or(0.0);
                let value = if is_retention { raw * 100.0 } else { raw };
                json!({
                    "rank": idx + 1,
                    "login": r.try_get::<String, _>("streamer_login").unwrap_or_default(),
                    "value": value,
                    "trend": "same",
                    "trendValue": 0,
                })
            }).collect();
            Json(json!(items)).into_response()
        }
    }
}
