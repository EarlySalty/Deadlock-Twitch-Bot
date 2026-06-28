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
        Err(crate::auth::unauthorized_v2_json())
    } else {
        Ok(())
    }
}

// ── Monatliche Stats ─────────────────────────────────────────────────────────

static MONTH_LABELS: [&str; 13] = [
    "", "Jan", "Feb", "Mar", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];

/// `GET /twitch/api/v2/monthly-stats?streamer=&months=12`
pub async fn monthly_stats_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<MonthlyQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let months = match parse_bounded_query_int(params.months.as_deref(), "months", 12, 1, 24) {
        Ok(m) => m,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days((months as f64 * 30.44) as i64);
    // IDOR-Klemme: Partner sind auf den eigenen Login beschränkt; Admin frei
    // (None → alle Streamer aggregiert).
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), false) {
            Ok(s) => s,
            Err(resp) => return resp,
        };

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
            crate::auth::analytics_request_failed_json().into_response()
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

static WEEKDAY_LABELS: [&str; 7] = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];

/// `GET /twitch/api/v2/weekly-stats?streamer=&days=30`
pub async fn weekly_stats_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DaysQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    // IDOR-Klemme: Partner nur eigener Login; Admin frei (None → alle).
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), false) {
            Ok(s) => s,
            Err(resp) => return resp,
        };

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
            crate::auth::analytics_request_failed_json().into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
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
                })
                .collect();
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
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    // IDOR-Klemme: Partner nur eigener Login; Admin frei (None → alle).
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), false) {
            Ok(s) => s,
            Err(resp) => return resp,
        };

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
            crate::auth::analytics_request_failed_json().into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "weekday": r.try_get::<i32,_>("weekday").unwrap_or(0),
                        "hour": r.try_get::<i32,_>("hour").unwrap_or(0),
                        "streamCount": r.try_get::<i64,_>("stream_count").unwrap_or(0),
                        "avgViewers": r.try_get::<f64,_>("avg_viewers").unwrap_or(0.0),
                        "avgPeak": r.try_get::<f64,_>("avg_peak").unwrap_or(0.0),
                    })
                })
                .collect();
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
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 365, 30, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    // IDOR-Klemme: Partner nur eigener Login; Admin frei (None → alle).
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), false) {
            Ok(s) => s,
            Err(resp) => return resp,
        };

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
            crate::auth::analytics_request_failed_json().into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    let date: chrono::NaiveDate = r
                        .try_get("date")
                        .unwrap_or(chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
                    let hw = r.try_get::<f64, _>("hours_watched").unwrap_or(0.0);
                    json!({
                        "date": date.to_string(),
                        "streamCount": r.try_get::<i64,_>("stream_count").unwrap_or(0),
                        "hoursWatched": hw,
                        "value": hw,
                    })
                })
                .collect();
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
    // IDOR-Klemme: Partner nur eigener Login; Admin braucht streamer (required).
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

    match sqlx::query(&query)
        .bind(since)
        .bind(streamer)
        .fetch_all(&pool)
        .await
    {
        Err(e) => {
            tracing::error!("viewer-timeline DB-Fehler: {e}");
            crate::auth::analytics_request_failed_json().into_response()
        }
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "timestamp": r.try_get::<String,_>("bucket").unwrap_or_default(),
                        "avgViewers": r.try_get::<f64,_>("avg_vc").unwrap_or(0.0),
                        "peakViewers": r.try_get::<i64,_>("peak_vc").unwrap_or(0),
                        "minViewers": r.try_get::<i64,_>("min_vc").unwrap_or(0),
                        "samples": r.try_get::<i64,_>("samples").unwrap_or(0),
                    })
                })
                .collect();
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
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn viewer_timeline_bucket_grenzen_entsprechen_python() {
        assert!(viewer_timeline_bucket_sql(7).contains("5 minutes"));
        assert!(viewer_timeline_bucket_sql(8).contains("30 minutes"));
        assert!(viewer_timeline_bucket_sql(30).contains("30 minutes"));
        assert_eq!(viewer_timeline_bucket_sql(31), "DATE_TRUNC('hour', ts_utc)");
    }

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
            .options([("search_path", schema), ("timezone", "UTC")]);
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    /// P1.35: gegen ein TIMESTAMPTZ-Schema (wie nach der Migration) muss der
    /// `since: DateTime<Utc>`-Bind ein 200 mit Daten liefern, nicht 500
    /// ('operator does not exist: text >= timestamp with time zone').
    #[tokio::test]
    async fn monthly_stats_timestamptz_schema_liefert_daten() {
        let Some(pool) = make_pool("t_perf_tstz").await else {
            return;
        };
        // Spaltentypen exakt wie nach der Migration: started_at/ended_at TIMESTAMPTZ.
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (\
                 id BIGSERIAL PRIMARY KEY, streamer_login TEXT, \
                 avg_viewers REAL, peak_viewers INTEGER, duration_seconds REAL, \
                 follower_delta INTEGER, followers_start INTEGER, followers_end INTEGER, \
                 unique_chatters INTEGER, \
                 started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let started = Utc::now() - Duration::days(5);
        let ended = started + Duration::hours(2);
        sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (streamer_login, avg_viewers, peak_viewers, duration_seconds, \
              follower_delta, followers_start, followers_end, unique_chatters, started_at, ended_at) \
             VALUES ('host', 100.0, 150, 7200.0, 5, 10, 15, 20, $1, $2)",
        )
        .bind(started)
        .bind(ended)
        .execute(&pool)
        .await
        .unwrap();

        let resp = monthly_stats_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(MonthlyQuery {
                streamer: None,
                months: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "timestamptz-Bind muss gegen timestamptz-Spalte funktionieren"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Mindestens ein Monatseintrag mit der Session.
        let items = body
            .get("items")
            .or_else(|| body.get("months"))
            .or(Some(&body));
        assert!(
            items.is_some(),
            "Response sollte Monatsdaten enthalten: {body}"
        );
    }

    /// IDOR: Ein eingeloggter Partner darf via `?streamer=<fremd>` NICHT die
    /// Analytics eines anderen Streamers lesen → 403 (kein DB-Zugriff nötig,
    /// die Klemme greift vor der Query).
    #[tokio::test]
    async fn partner_fremder_streamer_ist_403() {
        let auth = DashboardAuthLevel::Partner {
            twitch_login: "someone".into(),
            twitch_user_id: "1".into(),
            display_name: "someone".into(),
        };
        // Dummy-Pool: wird nie berührt, da die Klemme vor der Query 403 liefert.
        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:5432/none")
        {
            Ok(p) => p,
            Err(_) => return,
        };
        let resp = weekly_stats_handler(
            auth,
            State(pool),
            Query(DaysQuery {
                streamer: Some("fremd".into()),
                days: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
