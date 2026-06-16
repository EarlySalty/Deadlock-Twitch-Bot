//! Handler für `GET /twitch/api/v2/retention-curve`.
//!
//! Port von `bot/analytics/api_performance.py:_load_retention_curve_payload_sync` (Z.1623–1750).
//! Python holt 50 Sessions, lädt alle viewer-rows und berechnet per-Minute Quartile in Python.
//! Hier: eine einzige CTE macht das mit Postgres PERCENTILE_CONT.
//! Drop-Events: Punkte wo Median > 10 % fällt — wird in Rust berechnet.

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
use crate::query_int::parse_bounded_query_int;

#[derive(Deserialize)]
pub struct RetentionQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    // Rohwert: nicht-numerisches `days` → Python-konformes 400-JSON, siehe query_int.
    #[serde(default)]
    pub days: Option<String>,
}

/// `GET /twitch/api/v2/retention-curve?streamer=&days=30`
pub async fn retention_curve_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<RetentionQuery>,
) -> impl IntoResponse {
    // Python _api_v2_retention_curve: _require_v2_auth + _require_extended_plan.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }

    // days VOR streamer-Pflicht (Python-Reihenfolge in _api_v2_retention_curve).
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    // Letzte 50 Sessions wie Python; Viewer-Retention per Minute normalisiert auf peak_viewers,
    // dann PERCENTILE_CONT für p25/p50/p75.
    let rows = sqlx::query(
        r#"WITH recent_sessions AS (
               SELECT id, peak_viewers
               FROM twitch_stream_sessions
               WHERE LOWER(streamer_login) = $1
                 AND started_at >= $2
                 AND ended_at IS NOT NULL
               ORDER BY started_at DESC
               LIMIT 50
           ),
           normalized AS (
               SELECT sv.minutes_from_start AS minute,
                      sv.viewer_count::float / GREATEST(rs.peak_viewers, 1) AS retention
               FROM twitch_session_viewers sv
               JOIN recent_sessions rs ON rs.id = sv.session_id
               WHERE sv.minutes_from_start <= 180
                 AND sv.viewer_count IS NOT NULL
           ),
           stats AS (
               SELECT minute,
                      PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY retention) AS median_ret,
                      PERCENTILE_CONT(0.25) WITHIN GROUP (ORDER BY retention) AS p25,
                      PERCENTILE_CONT(0.75) WITHIN GROUP (ORDER BY retention) AS p75,
                      COUNT(*) AS sample_count
               FROM normalized
               GROUP BY minute
           )
           SELECT minute, median_ret, p25, p75, sample_count
           FROM stats
           ORDER BY minute"#,
    )
    .bind(&streamer)
    .bind(since)
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("retention-curve DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) if rows.is_empty() => {
            Json(json!({"retention_curve": [], "drop_events": [], "sessions_used": 0})).into_response()
        }
        Ok(rows) => {
            let curve: Vec<serde_json::Value> = rows.iter().map(|r| {
                let median: f64 = r.try_get("median_ret").unwrap_or(0.0);
                json!({
                    "minute": r.try_get::<i32, _>("minute").unwrap_or(0),
                    "median_retention": (median * 1000.0).round() / 1000.0,
                    "p25": (r.try_get::<f64, _>("p25").unwrap_or(0.0) * 1000.0).round() / 1000.0,
                    "p75": (r.try_get::<f64, _>("p75").unwrap_or(0.0) * 1000.0).round() / 1000.0,
                    "sample_count": r.try_get::<i64, _>("sample_count").unwrap_or(0),
                })
            }).collect();

            // Drop-Events: wo Median > 10 % fällt (Python-Parität)
            let mut drop_events: Vec<serde_json::Value> = vec![];
            let mut avg_watch_min: Option<i32> = None;
            // Python prüft ab curve[0]: schon die erste Minute kann < 0.5 Retention haben.
            if let Some(first) = curve.first() {
                if first["median_retention"].as_f64().unwrap_or(0.0) < 0.5 {
                    avg_watch_min = Some(first["minute"].as_i64().unwrap_or(0) as i32);
                }
            }
            for i in 1..curve.len() {
                let prev_ret = curve[i-1]["median_retention"].as_f64().unwrap_or(0.0);
                let cur_ret = curve[i]["median_retention"].as_f64().unwrap_or(0.0);
                let cur_min = curve[i]["minute"].as_i64().unwrap_or(0) as i32;

                if avg_watch_min.is_none() && cur_ret < 0.5 {
                    avg_watch_min = Some(cur_min);
                }
                if prev_ret > 0.0 {
                    let delta = (cur_ret - prev_ret) / prev_ret;
                    if delta < -0.10 {
                        drop_events.push(json!({
                            "minute": cur_min,
                            "drop_pct": ((delta.abs() * 1000.0).round() / 10.0),
                            "type": "unknown",
                        }));
                    }
                }
            }

            // Echte Session-Anzahl (Python sessions_used = len(recent_sessions), ≤50) —
            // rows sind Minuten-Aggregate, nicht Sessions.
            let sessions_used: i64 = sqlx::query_scalar(
                r#"SELECT COUNT(*) FROM (
                       SELECT id FROM twitch_stream_sessions
                       WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL
                       ORDER BY started_at DESC LIMIT 50
                   ) s"#,
            )
            .bind(&streamer)
            .bind(since)
            .fetch_one(&pool)
            .await
            .unwrap_or(0);

            Json(json!({
                "retention_curve": curve,
                "drop_events": drop_events,
                "avg_watch_duration_min": avg_watch_min,
                "sessions_used": sessions_used,
                "window_days": days,
            })).into_response()
        }
    }
}
