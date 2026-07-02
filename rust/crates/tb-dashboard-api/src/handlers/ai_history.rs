//! Handler für `GET /twitch/api/v2/ai/history`.
//!
//! Port von `bot/analytics/api_ai.py:_api_v2_ai_history`. Auth: eingeloggt; der
//! **abgefragte Streamer** braucht das konsolidierte `analytics`-Flag, sonst 403
//! — außer Admin. `streamer` Pflicht, `limit` 1..50 (Default 20).

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
pub struct AiHistoryQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub limit: Option<i32>,
}

/// AI-Modell des Streamer-Plans: konsolidiertes `analytics`-Flag → opus, sonst None.
async fn ai_plan_model(pool: &PgPool, streamer: &str) -> Option<&'static str> {
    match tb_analytics::plan::resolve_plan_snapshot(pool, streamer, "").await {
        Ok(s) if s.entitlements.contains(&"analytics") => Some("opus"),
        _ => None,
    }
}

/// `GET /twitch/api/v2/ai/history?streamer=&limit=20`
pub async fn ai_history_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<AiHistoryQuery>,
) -> impl IntoResponse {
    if matches!(auth, DashboardAuthLevel::None) {
        // Python-Parität (api_v2.py:1258-1262): voller Message-Text + loginUrl.
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Authentication required. Use Twitch login, an admin token, or access from localhost.",
                "loginUrl": "/twitch/auth/login?next=%2Fanalyse",
            })),
        )
            .into_response();
    }
    // IDOR-Guard: Partner werden auf den eigenen Login geklemmt (Cross-Account →
    // 403); Admin/Localhost dürfen `streamer` frei wählen.
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "streamer parameter required" })),
                )
                    .into_response();
            }
            Err(resp) => return resp,
        };
    // AI-Plan-Gate: Admin bypass; sonst muss der Streamer einen AI-Plan haben.
    let privileged = matches!(auth, DashboardAuthLevel::Admin { .. });
    if !privileged && ai_plan_model(&pool, &streamer).await.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "plan_required",
                "required_entitlements": ["analytics"],
            })),
        )
            .into_response();
    }
    let limit = params.limit.unwrap_or(20).clamp(1, 50) as i64;

    match tb_analytics::ai_history::load_ai_history(&pool, &streamer, limit).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("ai/history Fehler: {e}");
            // Python `analytics_internal_error_response` (error_utils.py:5-17).
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Analytics-Daten konnten nicht geladen werden.",
                    "code": "analytics_request_failed",
                })),
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
        sqlx::query(
            "CREATE TABLE ai_analyses ( \
                id BIGSERIAL PRIMARY KEY, \
                streamer TEXT NOT NULL, \
                days INTEGER NOT NULL, \
                model TEXT NOT NULL, \
                generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
                data_snapshot JSONB NOT NULL, \
                points JSONB NOT NULL )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn streamer_pflicht_400() {
        let Some(pool) = make_pool("t_aih_h1").await else {
            return;
        };
        let resp = ai_history_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(AiHistoryQuery {
                streamer: None,
                limit: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// B16-FIX-AIHISTORY-ERRORSHAPE: 401-Body trägt vollen Message-Text + loginUrl
    /// (Python api_v2.py:1258-1262), nicht das alte `{"error":"unauthorized"}`.
    #[tokio::test]
    async fn unauth_401_python_shape() {
        let Some(pool) = make_pool("t_aih_h_401").await else {
            return;
        };
        let resp = ai_history_handler(
            DashboardAuthLevel::None,
            State(pool),
            Query(AiHistoryQuery {
                streamer: Some("nani".into()),
                limit: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["error"],
            "Authentication required. Use Twitch login, an admin token, or access from localhost."
        );
        assert_eq!(body["loginUrl"], "/twitch/auth/login?next=%2Fanalyse");
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    // IDOR-Guard: Partner mit fremdem ?streamer= → 403.
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = make_pool("t_aih_idor").await else {
            return;
        };
        let resp = ai_history_handler(
            partner("earlysalty"),
            State(pool),
            Query(AiHistoryQuery {
                streamer: Some("ismile_e".into()),
                limit: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn localhost_bypass_200() {
        let Some(pool) = make_pool("t_aih_h2").await else {
            return;
        };
        // Admin bypasst das AI-Plan-Gate → 200 (leere Historie).
        let resp = ai_history_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(AiHistoryQuery {
                streamer: Some("nani".into()),
                limit: Some(10),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
