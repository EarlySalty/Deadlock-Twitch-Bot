//! Handler für `GET /twitch/api/v2/ads-schedule`.
//!
//! Port von `bot/analytics/api_insights.py:_load_ads_schedule_payload` (Z.74–120).
//! Liest aus `twitch_ads_schedule_snapshot` — kein Plan-Gate.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize)]
pub struct AdsQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

fn require_auth(auth: &DashboardAuthLevel) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"unauthorized","message":"not authenticated"}))))
    } else {
        Ok(())
    }
}

fn opt_iso(ts: Option<chrono::DateTime<chrono::Utc>>) -> Option<String> {
    ts.map(|t| t.to_rfc3339())
}

/// `GET /twitch/api/v2/ads-schedule?streamer=`
pub async fn ads_schedule_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<AdsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response(),
    };

    let rows = sqlx::query(
        r#"SELECT twitch_login, next_ad_at, last_ad_at, duration,
                  preroll_free_time, snooze_count, snooze_refresh_at, snapshot_at
           FROM twitch_ads_schedule_snapshot
           WHERE LOWER(twitch_login) = $1
           ORDER BY snapshot_at DESC
           LIMIT 50"#,
    )
    .bind(&streamer)
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("ads-schedule DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) if rows.is_empty() => Json(json!({"current": null, "history": []})).into_response(),
        Ok(rows) => {
            let first = &rows[0];
            let current = json!({
                "next_ad_at": opt_iso(first.try_get("next_ad_at").ok()),
                "last_ad_at": opt_iso(first.try_get("last_ad_at").ok()),
                "duration": first.try_get::<i32, _>("duration").ok(),
                "preroll_free_time": first.try_get::<i32, _>("preroll_free_time").ok(),
                "snooze_count": first.try_get::<i32, _>("snooze_count").ok(),
                "snooze_refresh_at": opt_iso(first.try_get("snooze_refresh_at").ok()),
                "snapshot_at": opt_iso(first.try_get("snapshot_at").ok()),
            });
            let history: Vec<serde_json::Value> = rows.iter().take(10).map(|r| json!({
                "snapshot_at": opt_iso(r.try_get("snapshot_at").ok()),
                "next_ad_at": opt_iso(r.try_get("next_ad_at").ok()),
                "duration": r.try_get::<i32, _>("duration").ok(),
                "preroll_free_time": r.try_get::<i32, _>("preroll_free_time").ok(),
            })).collect();
            Json(json!({"current": current, "history": history})).into_response()
        }
    }
}
