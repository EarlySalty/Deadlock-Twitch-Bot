//! Handler für `/twitch/api/v2/coaching`.
//!
//! Port von `bot/analytics/api_insights.py:_api_v2_coaching`.
//! Auth + Extended-Plan-Gate, `streamer` (Pflicht, Original-Case wird echot),
//! `days` (7..365, Default 30). Lädt die volle regelbasierte Coaching-Engine
//! ([`tb_analytics::coaching::get_coaching_data`]).

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
use crate::query_int::parse_bounded_query_int;

#[derive(Deserialize)]
pub struct CoachingQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    // Rohwert statt Option<i32>: nicht-numerisches `days` muss Python-konform
    // 400-JSON liefern ({"error":"days must be an integer"}), nicht den
    // generischen serde-Plaintext-400. Siehe crate::query_int.
    #[serde(default)]
    pub days: Option<String>,
}

/// `GET /twitch/api/v2/coaching?streamer=&days=30`
pub async fn coaching_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<CoachingQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    // days VOR der streamer-Pflichtprüfung (Python-Reihenfolge in _api_v2_coaching:
    // erst _parse_bounded_query_int → 400, dann streamer-Check).
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    // IDOR-Guard: Partner auf eigenen Login geklemmt (Cross-Account → 403),
    // Admin/Localhost frei. `required=true`: fehlender Streamer → 400. Der
    // Resolver lowercased den Login (für Partner ohnehin der eigene); get_coaching_data
    // lowercased intern weiter — der Echo-Wert ist damit kleingeschrieben.
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Streamer required" })),
                )
                    .into_response();
            }
            Err(resp) => return resp,
        };

    match tb_analytics::coaching::get_coaching_data(&pool, &streamer, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("coaching Fehler: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "internal" })),
            )
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
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn streamer_pflicht_400() {
        let Some(pool) = make_pool("t_coaching_h1").await else {
            return;
        };
        let resp = coaching_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(CoachingQuery {
                streamer: None,
                days: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn leerer_streamer_empty_200() {
        let Some(pool) = make_pool("t_coaching_h2").await else {
            return;
        };
        // Localhost bypasst Gate, streamer vorhanden, keine Sessions → empty:true/200.
        let resp = coaching_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(CoachingQuery {
                streamer: Some("Nani".into()),
                days: Some("30".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    /// IDOR-Guard: Partner mit fremdem ?streamer= → 403 (Plan-Gate ODER Scope).
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = make_pool("t_coaching_h4").await else {
            return;
        };
        sqlx::query("CREATE TABLE IF NOT EXISTS streamer_plans (twitch_login TEXT, plan_id TEXT)")
            .execute(&pool)
            .await
            .ok();
        let resp = coaching_handler(
            partner("earlysalty"),
            State(pool),
            Query(CoachingQuery {
                streamer: Some("ismile_e".into()),
                days: Some("30".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn nicht_numerische_days_400_python_shape() {
        let Some(pool) = make_pool("t_coaching_h3").await else {
            return;
        };
        // Python: _parse_bounded_query_int → {"error":"days must be an integer"}, 400.
        // days-Check läuft VOR streamer-Pflicht (deshalb streamer: None ok).
        let resp = coaching_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(CoachingQuery {
                streamer: None,
                days: Some("abc".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, json!({ "error": "days must be an integer" }));
    }
}
