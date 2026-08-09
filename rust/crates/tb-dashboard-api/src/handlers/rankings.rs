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
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;
use crate::query_int::parse_bounded_query_int;

const EXTERNAL_REACH_AVG_THRESHOLD: f64 = 100.0;

struct RankingRow {
    streamer_login: String,
    value: Option<f64>,
}

#[derive(Deserialize)]
pub struct RankingsQuery {
    #[serde(default)]
    pub metric: Option<String>,
    // Rohwerte: nicht-numerisch → Python-konformes 400-JSON, siehe query_int.
    #[serde(default)]
    pub days: Option<String>,
    #[serde(default)]
    pub limit: Option<String>,
    #[serde(default)]
    pub exclude_external: Option<String>,
}

fn require_auth(auth: &DashboardAuthLevel) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err(crate::auth::unauthorized_v2_json())
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
    // Premium-Gate (Pricing-Umbau 2026-08-09): Ranking-Vergleich ueber Zeitraeume ist Premium.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }

    let metric = params.metric.as_deref().unwrap_or("viewers");
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let limit = match parse_bounded_query_int(params.limit.as_deref(), "limit", 20, 5, 50) {
        Ok(l) => l,
        Err(resp) => return resp.into_response(),
    };
    let exclude_external = params.exclude_external.as_deref() == Some("1");
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    // Jede der 6 Kombinationen (3 metrics × 2 threshold-Varianten) bekommt ihr eigenes SQL.
    // $1 = since, $2 = limit [, $3 = threshold wenn exclude_external=1].

    let result = match (metric, exclude_external) {
        ("retention", false) => {
            sqlx::query_as!(
                RankingRow,
                r#"SELECT s.streamer_login,
                      AVG(s.retention_10m) AS "value?"
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3
               ORDER BY 2 DESC NULLS LAST
               LIMIT $2"#,
                since,
                limit
            )
            .fetch_all(&pool)
            .await
        }

        ("retention", true) => {
            sqlx::query_as!(
                RankingRow,
                r#"SELECT s.streamer_login,
                      AVG(s.retention_10m) AS "value?"
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3 AND AVG(s.avg_viewers) <= $3
               ORDER BY 2 DESC NULLS LAST
               LIMIT $2"#,
                since,
                limit,
                EXTERNAL_REACH_AVG_THRESHOLD
            )
            .fetch_all(&pool)
            .await
        }

        ("growth", false) => {
            sqlx::query_as!(
                RankingRow,
                r#"SELECT s.streamer_login,
                      SUM(CASE WHEN s.follower_delta IS NOT NULL
                               AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                               THEN s.follower_delta ELSE 0 END)::float8 AS "value?"
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3
               ORDER BY 2 DESC NULLS LAST
               LIMIT $2"#,
                since,
                limit
            )
            .fetch_all(&pool)
            .await
        }

        ("growth", true) => {
            sqlx::query_as!(
                RankingRow,
                r#"SELECT s.streamer_login,
                      SUM(CASE WHEN s.follower_delta IS NOT NULL
                               AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                               THEN s.follower_delta ELSE 0 END)::float8 AS "value?"
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3 AND AVG(s.avg_viewers) <= $3
               ORDER BY 2 DESC NULLS LAST
               LIMIT $2"#,
                since,
                limit,
                EXTERNAL_REACH_AVG_THRESHOLD
            )
            .fetch_all(&pool)
            .await
        }

        // viewers (default)
        (_, false) => {
            sqlx::query_as!(
                RankingRow,
                r#"SELECT s.streamer_login,
                      AVG(s.avg_viewers) AS "value?"
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3
               ORDER BY 2 DESC NULLS LAST
               LIMIT $2"#,
                since,
                limit
            )
            .fetch_all(&pool)
            .await
        }

        (_, true) => {
            sqlx::query_as!(
                RankingRow,
                r#"SELECT s.streamer_login,
                      AVG(s.avg_viewers) AS "value?"
               FROM twitch_stream_sessions s
               WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL
               GROUP BY s.streamer_login
               HAVING COUNT(*) >= 3 AND AVG(s.avg_viewers) <= $3
               ORDER BY 2 DESC NULLS LAST
               LIMIT $2"#,
                since,
                limit,
                EXTERNAL_REACH_AVG_THRESHOLD
            )
            .fetch_all(&pool)
            .await
        }
    };

    let is_retention = metric == "retention";

    match result {
        Err(e) => {
            tracing::error!("rankings DB-Fehler: {e}");
            crate::auth::analytics_request_failed_json().into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .enumerate()
                .map(|(idx, r)| {
                    let raw = r.value.unwrap_or(0.0);
                    let value = if is_retention { raw * 100.0 } else { raw };
                    json!({
                        "rank": idx + 1,
                        "login": r.streamer_login,
                        "value": value,
                        "trend": "same",
                        "trendValue": 0,
                    })
                })
                .collect();
            Json(json!(items)).into_response()
        }
    }
}
