//! Handler für `GET /twitch/api/v2/retention-curve`.
//!
//! Port von `bot/analytics/api_performance.py:_load_retention_curve_payload_sync` (Z.1623–1750).
//! Python holt 50 Sessions, lädt alle viewer-rows und berechnet per-Minute Quartile in Python.
//! Hier: eine einzige CTE macht das mit Postgres PERCENTILE_CONT.
//! Drop-Events: Punkte wo Median > 10 % fällt — wird in Rust berechnet.

use std::collections::HashSet;

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
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"Streamer required"})),
                )
                    .into_response()
            }
            Err(resp) => return resp,
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
            crate::auth::analytics_request_failed_json().into_response()
        }
        Ok(rows) if rows.is_empty() => {
            Json(json!({"retention_curve": [], "drop_events": [], "sessions_used": 0}))
                .into_response()
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

            let ad_times: HashSet<i32> = sqlx::query_scalar(
                r#"WITH recent_sessions AS (
                       SELECT id, started_at
                       FROM twitch_stream_sessions
                       WHERE LOWER(streamer_login) = $1
                         AND started_at >= $2
                         AND ended_at IS NOT NULL
                       ORDER BY started_at DESC
                       LIMIT 50
                   )
                   SELECT FLOOR(EXTRACT(EPOCH FROM (a.started_at - rs.started_at)) / 60.0)::int AS minute
                   FROM twitch_ad_break_events a
                   JOIN recent_sessions rs ON rs.id = a.session_id"#,
            )
            .bind(&streamer)
            .bind(since)
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();

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
                let prev_ret = curve[i - 1]["median_retention"].as_f64().unwrap_or(0.0);
                let cur_ret = curve[i]["median_retention"].as_f64().unwrap_or(0.0);
                let cur_min = curve[i]["minute"].as_i64().unwrap_or(0) as i32;

                if avg_watch_min.is_none() && cur_ret < 0.5 {
                    avg_watch_min = Some(cur_min);
                }
                if prev_ret > 0.0 {
                    let delta = (cur_ret - prev_ret) / prev_ret;
                    if delta < -0.10 {
                        let event_type = if ad_times.contains(&cur_min) {
                            "ad_break"
                        } else {
                            "unknown"
                        };
                        drop_events.push(json!({
                            "minute": cur_min,
                            "drop_pct": ((delta.abs() * 1000.0).round() / 10.0),
                            "type": event_type,
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
            }))
            .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                started_at TIMESTAMPTZ NOT NULL,
                ended_at TIMESTAMPTZ,
                peak_viewers INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_session_viewers (
                session_id BIGINT NOT NULL,
                minutes_from_start INTEGER NOT NULL,
                viewer_count INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_ad_break_events (
                id BIGSERIAL PRIMARY KEY,
                session_id BIGINT NOT NULL,
                started_at TIMESTAMPTZ NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn drop_event_at_ad_break_minute_is_labeled_ad_break() {
        let Some(pool) = make_pool("t_retention_curve_ad_break").await else {
            return;
        };
        let session_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, peak_viewers)
             VALUES ('nani', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours', 100)
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_session_viewers (session_id, minutes_from_start, viewer_count)
             VALUES ($1, 0, 100), ($1, 1, 80)",
        )
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_ad_break_events (session_id, started_at)
             SELECT id, started_at + INTERVAL '1 minute'
             FROM twitch_stream_sessions WHERE id = $1",
        )
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        let resp = retention_curve_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(RetentionQuery {
                streamer: Some("nani".into()),
                days: Some("7".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["drop_events"][0]["minute"], 1);
        assert_eq!(body["drop_events"][0]["type"], "ad_break");
    }

    /// Richtet ein berechtigtes Partner-Plan-Snapshot ein (Manual-Override mit
    /// Analytics-Plan), damit `extended_gate` für den Partner passiert.
    async fn grant_partner_analytics(pool: &PgPool, login: &str) {
        sqlx::query(
            "CREATE TABLE streamer_plans (twitch_user_id TEXT, twitch_login TEXT, manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT, manual_plan_updated_at TEXT)",
        ).execute(pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_billing_subscriptions (customer_reference TEXT, plan_id TEXT, status TEXT, current_period_end TEXT, updated_at TEXT)")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO streamer_plans (twitch_login, manual_plan_id) VALUES ($1, 'analysis_dashboard')")
            .bind(login).execute(pool).await.unwrap();
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: String::new(),
            display_name: login.to_string(),
        }
    }

    /// IDOR: ein berechtigter Partner darf NICHT die Retention-Kurve eines fremden
    /// Streamers lesen (`?streamer=<fremd>` → 403).
    #[tokio::test]
    async fn partner_fremder_streamer_ist_forbidden() {
        let Some(pool) = make_pool("t_retention_idor").await else {
            return;
        };
        grant_partner_analytics(&pool, "earlysalty").await;
        let resp = retention_curve_handler(
            partner("earlysalty"),
            State(pool),
            Query(RetentionQuery {
                streamer: Some("ismile_e".into()),
                days: Some("7".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
