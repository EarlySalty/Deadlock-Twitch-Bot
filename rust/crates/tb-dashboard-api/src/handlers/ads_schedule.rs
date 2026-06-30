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
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize)]
pub struct AdsQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

fn require_auth(auth: &DashboardAuthLevel) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err(crate::auth::unauthorized_v2_json())
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

    // IDOR-Guard: Partner werden auf den eigenen Login geklemmt (Cross-Account →
    // 403); Admin/Localhost dürfen `streamer` frei wählen.
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

    let rows = sqlx::query!(
        r#"SELECT twitch_login, next_ad_at, last_ad_at, duration,
                  preroll_free_time, snooze_count, snooze_refresh_at,
                  snapshot_at AS "snapshot_at?"
           FROM twitch_ads_schedule_snapshot
           WHERE LOWER(twitch_login) = $1
           ORDER BY snapshot_at DESC
           LIMIT 50"#,
        streamer
    )
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("ads-schedule DB-Fehler: {e}");
            crate::auth::analytics_request_failed_json().into_response()
        }
        Ok(rows) if rows.is_empty() => {
            Json(json!({"current": null, "history": []})).into_response()
        }
        Ok(rows) => {
            let first = &rows[0];
            let current = json!({
                "next_ad_at": opt_iso(first.next_ad_at),
                "last_ad_at": opt_iso(first.last_ad_at),
                "duration": first.duration,
                "preroll_free_time": first.preroll_free_time,
                "snooze_count": first.snooze_count,
                "snooze_refresh_at": opt_iso(first.snooze_refresh_at),
                "snapshot_at": opt_iso(first.snapshot_at),
            });
            let history: Vec<serde_json::Value> = rows
                .iter()
                .take(10)
                .map(|r| {
                    json!({
                        "snapshot_at": opt_iso(r.snapshot_at),
                        "next_ad_at": opt_iso(r.next_ad_at),
                        "duration": r.duration,
                        "preroll_free_time": r.preroll_free_time,
                    })
                })
                .collect();
            Json(json!({"current": current, "history": history})).into_response()
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
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    // IDOR-Guard: Partner mit fremdem ?streamer= → 403 (vor jedem DB-Zugriff).
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = make_pool("t_ads_idor").await else {
            return;
        };
        let resp = ads_schedule_handler(
            partner("earlysalty"),
            State(pool),
            Query(AdsQuery {
                streamer: Some("ismile_e".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
