//! Handler für Performance-Analytics-Endpoints.
//!
//! Port von `bot/analytics/api_performance.py`:
//! - `GET /twitch/api/v2/monthly-stats`   (_load_monthly_stats_payload Z.125–166)
//! - `GET /twitch/api/v2/weekly-stats`    (_load_weekly_stats_payload Z.169–204)
//! - `GET /twitch/api/v2/hourly-heatmap`  (_load_hourly_heatmap_payload Z.64–93)
//! - `GET /twitch/api/v2/calendar-heatmap`(_load_calendar_heatmap_payload Z.96–122)
//!
//! Alle vier lesen aus `twitch_stream_sessions` in Postgres.
//! Auth: `DashboardAuthLevel::None` → 401; sonst erlaubt.

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

// ── Request-Parameter ────────────────────────────────────────────────────────

/// Gemeinsamer Query-Parameter: optionaler `streamer`-Login.
#[derive(Deserialize, Default)]
pub struct StreamerQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct MonthlyQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    // Rohwert: nicht-numerisches `months` → Python-konformes 400-JSON, siehe query_int.
    #[serde(default)]
    pub months: Option<String>,
}

#[derive(Deserialize)]
pub struct DaysQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    // Rohwert: nicht-numerisches `days` → Python-konformes 400-JSON, siehe query_int.
    #[serde(default)]
    pub days: Option<String>,
}

/// Python-`_require_v2_auth`-Parität: None → 401.
fn require_auth(auth: &DashboardAuthLevel) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"unauthorized","message":"not authenticated"}))))
    } else {
        Ok(())
    }
}

// ── Monatliche Stats ─────────────────────────────────────────────────────────

static MONTH_LABELS: [&str; 13] = ["","Jan","Feb","Mar","Apr","Mai","Jun","Jul","Aug","Sep","Okt","Nov","Dez"];

/// `GET /twitch/api/v2/monthly-stats?streamer=&months=12`
pub async fn monthly_stats_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<MonthlyQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) { return e.into_response(); }

    let months = match parse_bounded_query_int(params.months.as_deref(), "months", 12, 1, 24) {
        Ok(m) => m,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days((months as f64 * 30.44) as i64);
    let streamer = params.streamer.as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let rows = sqlx::query(
        r#"
        SELECT
            EXTRACT(YEAR  FROM s.started_at)::integer AS year,
            EXTRACT(MONTH FROM s.started_at)::integer AS month,
            SUM(s.avg_viewers * s.duration_seconds / 3600.0) AS hours_watched,
            SUM(s.duration_seconds / 3600.0)::float8 AS airtime,
            AVG(s.avg_viewers) AS avg_viewers,
            MAX(s.peak_viewers)::bigint AS peak_viewers,
            SUM(CASE WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                     THEN s.follower_delta ELSE 0 END) AS follower_delta,
            SUM(s.unique_chatters) AS total_chatter_sessions,
            COUNT(*) AS stream_count
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND (COALESCE($2, '') = '' OR LOWER(s.streamer_login) = $2)
        GROUP BY 1, 2
        ORDER BY 1 DESC, 2 DESC
        "#,
    )
    .bind(since)
    .bind(streamer.as_deref())
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("monthly-stats DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows.iter().map(|r| {
                let year: i32 = r.try_get("year").unwrap_or(0);
                let month: i32 = r.try_get("month").unwrap_or(0);
                let label = MONTH_LABELS.get(month as usize).copied().unwrap_or("");
                json!({
                    "year": year,
                    "month": month,
                    "monthLabel": label,
                    "totalHoursWatched": r.try_get::<f64,_>("hours_watched").unwrap_or(0.0),
                    "totalAirtime": r.try_get::<f64,_>("airtime").unwrap_or(0.0),
                    "avgViewers": r.try_get::<f64,_>("avg_viewers").unwrap_or(0.0),
                    "peakViewers": r.try_get::<i64,_>("peak_viewers").unwrap_or(0),
                    "followerDelta": r.try_get::<i64,_>("follower_delta").unwrap_or(0),
                    "totalChatterSessions": r.try_get::<i64,_>("total_chatter_sessions").unwrap_or(0),
                    "streamCount": r.try_get::<i64,_>("stream_count").unwrap_or(0),
                })
            }).collect();
            Json(json!(items)).into_response()
        }
    }
}

// ── Wochentagsanalyse ────────────────────────────────────────────────────────

static WEEKDAY_LABELS: [&str; 7] = ["So","Mo","Di","Mi","Do","Fr","Sa"];

/// `GET /twitch/api/v2/weekly-stats?streamer=&days=30`
pub async fn weekly_stats_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DaysQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) { return e.into_response(); }

    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    let streamer = params.streamer.as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let rows = sqlx::query(
        r#"
        SELECT
            EXTRACT(DOW FROM s.started_at)::integer AS weekday,
            COUNT(*) AS stream_count,
            AVG(s.duration_seconds / 3600.0)::float8 AS avg_hours,
            AVG(s.avg_viewers) AS avg_viewers,
            AVG(s.peak_viewers)::float8 AS avg_peak,
            SUM(CASE WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                     THEN s.follower_delta ELSE 0 END) AS total_followers
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND (COALESCE($2, '') = '' OR LOWER(s.streamer_login) = $2)
        GROUP BY 1
        ORDER BY 1
        "#,
    )
    .bind(since)
    .bind(streamer.as_deref())
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("weekly-stats DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows.iter().map(|r| {
                let wd: i32 = r.try_get("weekday").unwrap_or(0);
                let label = WEEKDAY_LABELS.get(wd as usize).copied().unwrap_or("");
                json!({
                    "weekday": wd,
                    "weekdayLabel": label,
                    "streamCount": r.try_get::<i64,_>("stream_count").unwrap_or(0),
                    "avgHours": r.try_get::<f64,_>("avg_hours").unwrap_or(0.0),
                    "avgViewers": r.try_get::<f64,_>("avg_viewers").unwrap_or(0.0),
                    "avgPeak": r.try_get::<f64,_>("avg_peak").unwrap_or(0.0),
                    "totalFollowers": r.try_get::<i64,_>("total_followers").unwrap_or(0),
                })
            }).collect();
            Json(json!(items)).into_response()
        }
    }
}

// ── Stündliches Heatmap ──────────────────────────────────────────────────────

/// `GET /twitch/api/v2/hourly-heatmap?streamer=&days=30`
pub async fn hourly_heatmap_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DaysQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) { return e.into_response(); }

    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    let streamer = params.streamer.as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let rows = sqlx::query(
        r#"
        SELECT
            EXTRACT(DOW  FROM s.started_at)::integer AS weekday,
            EXTRACT(HOUR FROM s.started_at)::integer AS hour,
            COUNT(*) AS stream_count,
            AVG(s.avg_viewers) AS avg_viewers,
            AVG(s.peak_viewers)::float8 AS avg_peak
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND (COALESCE($2, '') = '' OR LOWER(s.streamer_login) = $2)
        GROUP BY 1, 2
        "#,
    )
    .bind(since)
    .bind(streamer.as_deref())
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("hourly-heatmap DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows.iter().map(|r| json!({
                "weekday": r.try_get::<i32,_>("weekday").unwrap_or(0),
                "hour": r.try_get::<i32,_>("hour").unwrap_or(0),
                "streamCount": r.try_get::<i64,_>("stream_count").unwrap_or(0),
                "avgViewers": r.try_get::<f64,_>("avg_viewers").unwrap_or(0.0),
                "avgPeak": r.try_get::<f64,_>("avg_peak").unwrap_or(0.0),
            })).collect();
            Json(json!(items)).into_response()
        }
    }
}

// ── Kalender-Heatmap ──────────────────────────────────────────────────────────

/// `GET /twitch/api/v2/calendar-heatmap?streamer=&days=365`
pub async fn calendar_heatmap_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DaysQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) { return e.into_response(); }

    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 365, 30, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    let streamer = params.streamer.as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let rows = sqlx::query(
        r#"
        SELECT
            DATE(s.started_at) AS date,
            COUNT(*) AS stream_count,
            SUM(s.avg_viewers * s.duration_seconds / 3600.0) AS hours_watched
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND s.ended_at IS NOT NULL
          AND (COALESCE($2, '') = '' OR LOWER(s.streamer_login) = $2)
        GROUP BY DATE(s.started_at)
        ORDER BY DATE(s.started_at)
        "#,
    )
    .bind(since)
    .bind(streamer.as_deref())
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("calendar-heatmap DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows.iter().map(|r| {
                let date: chrono::NaiveDate = r.try_get("date").unwrap_or(chrono::NaiveDate::from_ymd_opt(1970,1,1).unwrap());
                let hw = r.try_get::<f64,_>("hours_watched").unwrap_or(0.0);
                json!({
                    "date": date.to_string(),
                    "streamCount": r.try_get::<i64,_>("stream_count").unwrap_or(0),
                    "hoursWatched": hw,
                    "value": hw,
                })
            }).collect();
            Json(json!(items)).into_response()
        }
    }
}

/// `GET /twitch/api/v2/viewer-timeline?streamer=&days=7`
pub async fn viewer_count_timeline_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DaysQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }

    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 7, 1, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let Some(streamer) = params.streamer.as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
    else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response();
    };
    let since = Utc::now() - Duration::days(days);
    let bucket_sql = viewer_timeline_bucket_sql(days);
    let query = format!(
        "SELECT TO_CHAR({bucket_sql}, 'YYYY-MM-DD HH24:MI') AS bucket, \
                ROUND(AVG(viewer_count)::numeric, 1)::float8 AS avg_vc, \
                MAX(viewer_count)::bigint AS peak_vc, \
                MIN(viewer_count)::bigint AS min_vc, \
                COUNT(*)::bigint AS samples \
         FROM twitch_stats_tracked \
         WHERE ts_utc >= $1 AND LOWER(streamer) = $2 \
         GROUP BY 1 ORDER BY 1"
    );

    match sqlx::query(&query).bind(since).bind(streamer).fetch_all(&pool).await {
        Err(e) => {
            tracing::error!("viewer-timeline DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows.iter().map(|r| json!({
                "timestamp": r.try_get::<String,_>("bucket").unwrap_or_default(),
                "avgViewers": r.try_get::<f64,_>("avg_vc").unwrap_or(0.0),
                "peakViewers": r.try_get::<i64,_>("peak_vc").unwrap_or(0),
                "minViewers": r.try_get::<i64,_>("min_vc").unwrap_or(0),
                "samples": r.try_get::<i64,_>("samples").unwrap_or(0),
            })).collect();
            Json(json!(items)).into_response()
        }
    }
}

fn viewer_timeline_bucket_sql(days: i64) -> &'static str {
    if days <= 7 {
        "DATE_TRUNC('hour', ts_utc) \
         + FLOOR(EXTRACT(MINUTE FROM ts_utc) / 5) * INTERVAL '5 minutes'"
    } else if days <= 30 {
        "DATE_TRUNC('hour', ts_utc) \
         + CASE WHEN EXTRACT(MINUTE FROM ts_utc) < 30 \
                THEN INTERVAL '0 minutes' ELSE INTERVAL '30 minutes' END"
    } else {
        "DATE_TRUNC('hour', ts_utc)"
    }
}

#[cfg(test)]
mod tests {
    use super::viewer_timeline_bucket_sql;

    #[test]
    fn viewer_timeline_bucket_grenzen_entsprechen_python() {
        assert!(viewer_timeline_bucket_sql(7).contains("5 minutes"));
        assert!(viewer_timeline_bucket_sql(8).contains("30 minutes"));
        assert!(viewer_timeline_bucket_sql(30).contains("30 minutes"));
        assert_eq!(viewer_timeline_bucket_sql(31), "DATE_TRUNC('hour', ts_utc)");
    }
}
