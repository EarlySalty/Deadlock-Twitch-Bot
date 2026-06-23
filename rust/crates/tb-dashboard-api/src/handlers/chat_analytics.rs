//! Handler für `/twitch/api/v2/chat-analytics`.
//!
//! Port von `bot/analytics/api_insights.py:_api_v2_chat_analytics`.
//! Auth: nur „eingeloggt" (KEIN Extended-Plan-Gate); `streamer` Pflicht,
//! `days` (7..365, Default 30), optionaler `timezone`-Param.

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
pub struct ChatAnalyticsQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    // Rohwert: nicht-numerisches `days` → Python-konformes 400-JSON, siehe query_int.
    #[serde(default)]
    pub days: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
}

/// `GET /twitch/api/v2/chat-analytics?streamer=&days=30&timezone=UTC`
pub async fn chat_analytics_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ChatAnalyticsQuery>,
) -> impl IntoResponse {
    // _require_v2_auth: jede gültige v2-Auth genügt, None → 401.
    if matches!(auth, DashboardAuthLevel::None) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    // days VOR streamer-Pflicht (Python-Reihenfolge in _api_v2_chat_analytics).
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    // IDOR-Guard: Partner werden auf den eigenen Login geklemmt (Cross-Account →
    // 403); Admin/Localhost dürfen `streamer` frei wählen. `required=true`:
    // fehlender Streamer → 400 (Python-Verhalten).
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
    let timezone = params.timezone.as_deref();

    match tb_analytics::chat_analytics::load_chat_analytics_payload(
        &pool, &streamer, days, timezone,
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("chat-analytics Fehler: {e}");
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
        for ddl in [
            "CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, avg_viewers REAL)",
            "CREATE TABLE twitch_session_viewers (session_id BIGINT, minutes_from_start INTEGER, viewer_count INTEGER)",
            "CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, content TEXT, is_command BOOLEAN, message_ts TIMESTAMPTZ)",
            "CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, messages INTEGER DEFAULT 0, seen_via_chatters_api BOOLEAN DEFAULT FALSE, is_first_time_streamer BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE twitch_chatter_rollup (streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, first_seen_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn streamer_pflicht_400() {
        let Some(pool) = make_pool("t_ca_h1").await else {
            return;
        };
        let resp = chat_analytics_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ChatAnalyticsQuery {
                streamer: None,
                days: None,
                timezone: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn none_auth_401() {
        let Some(pool) = make_pool("t_ca_h2").await else {
            return;
        };
        let resp = chat_analytics_handler(
            DashboardAuthLevel::None,
            State(pool),
            Query(ChatAnalyticsQuery {
                streamer: Some("nani".into()),
                days: None,
                timezone: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn localhost_200() {
        let Some(pool) = make_pool("t_ca_h3").await else {
            return;
        };
        let resp = chat_analytics_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ChatAnalyticsQuery {
                streamer: Some("nani".into()),
                days: Some("30".into()),
                timezone: None,
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

    /// IDOR-Guard: Partner mit fremdem ?streamer= → 403 (vor jedem DB-Zugriff).
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = make_pool("t_ca_h5").await else {
            return;
        };
        let resp = chat_analytics_handler(
            partner("earlysalty"),
            State(pool),
            Query(ChatAnalyticsQuery {
                streamer: Some("ismile_e".into()),
                days: Some("30".into()),
                timezone: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn nicht_numerische_days_400_python_shape() {
        let Some(pool) = make_pool("t_ca_h4").await else {
            return;
        };
        let resp = chat_analytics_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ChatAnalyticsQuery {
                streamer: Some("nani".into()),
                days: Some("x".into()),
                timezone: None,
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
