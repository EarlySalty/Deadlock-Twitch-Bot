//! Handler für `GET /twitch/api/v2/title-performance`.
//!
//! Port von `bot/analytics/api_performance.py:_load_title_performance_payload_sync` (Z.657–754).
//! `peerBenchmark` wird als null zurückgegeben — `_get_peer_group_stats` ist nicht portiert.

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

#[derive(Deserialize)]
pub struct TitleQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<i32>,
    #[serde(default)]
    pub limit: Option<i32>,
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

/// Python `_extract_title_keywords` — Stop-Word-Filter + 3+-Zeichen-Wörter, max 5.
fn extract_title_keywords(title: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "der", "die", "das", "und", "oder", "mit", "fur", "the", "and", "or", "with", "for",
        "to", "a", "an",
    ];
    let words: Vec<String> = title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(w))
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect();
    words.into_iter().take(5).collect()
}

/// `GET /twitch/api/v2/title-performance?streamer=&days=30&limit=20`
pub async fn title_performance_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<TitleQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response(),
    };
    let days = clamp(params.days.unwrap_or(30), 7, 365);
    let limit = clamp(params.limit.unwrap_or(20), 5, 50) as i64;
    let since: DateTime<Utc> = Utc::now() - Duration::days(days as i64);

    let rows = sqlx::query(
        r#"SELECT
               s.stream_title,
               COUNT(*) AS usage_count,
               AVG(s.avg_viewers) AS avg_viewers,
               AVG(s.retention_10m) AS avg_retention,
               AVG(CASE WHEN s.follower_delta IS NOT NULL
                        AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                        THEN s.follower_delta ELSE NULL END) AS avg_followers,
               MAX(s.peak_viewers) AS peak_viewers
           FROM twitch_stream_sessions s
           WHERE s.started_at >= $1
             AND LOWER(s.streamer_login) = $2
             AND s.ended_at IS NOT NULL
             AND s.stream_title IS NOT NULL
             AND s.stream_title != ''
           GROUP BY s.stream_title
           ORDER BY avg_viewers DESC
           LIMIT $3"#,
    )
    .bind(since)
    .bind(&streamer)
    .bind(limit)
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("title-performance DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let titles: Vec<serde_json::Value> = rows.iter().map(|r| {
                let title: String = r.try_get("stream_title").unwrap_or_default();
                let keywords = extract_title_keywords(&title);
                json!({
                    "title": title,
                    "usageCount": r.try_get::<i64, _>("usage_count").unwrap_or(0),
                    "avgViewers": r.try_get::<f64, _>("avg_viewers").map(|v| (v * 10.0).round() / 10.0).unwrap_or(0.0),
                    "avgRetention10m": r.try_get::<f64, _>("avg_retention").map(|v| (v * 1000.0).round() / 10.0).unwrap_or(0.0),
                    "avgFollowerGain": r.try_get::<f64, _>("avg_followers").map(|v| (v * 10.0).round() / 10.0).unwrap_or(0.0),
                    "peakViewers": r.try_get::<i32, _>("peak_viewers").unwrap_or(0),
                    "keywords": keywords,
                })
            }).collect();
            Json(json!({
                "titles": titles,
                "peerBenchmark": null,
            })).into_response()
        }
    }
}
