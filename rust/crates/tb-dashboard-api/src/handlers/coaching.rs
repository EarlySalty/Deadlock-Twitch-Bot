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

#[derive(Deserialize)]
pub struct CoachingQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<i32>,
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
    // streamer NICHT kleinschreiben: get_coaching_data echot ihn original und
    // lowercased nur intern für die Queries.
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Streamer required" }))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;

    match tb_analytics::coaching::get_coaching_data(&pool, &streamer, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("coaching Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal" }))).into_response()
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
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn streamer_pflicht_400() {
        let Some(pool) = make_pool("t_coaching_h1").await else { return };
        let resp = coaching_handler(
            DashboardAuthLevel::Localhost,
            State(pool),
            Query(CoachingQuery { streamer: None, days: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn leerer_streamer_empty_200() {
        let Some(pool) = make_pool("t_coaching_h2").await else { return };
        // Localhost bypasst Gate, streamer vorhanden, keine Sessions → empty:true/200.
        let resp = coaching_handler(
            DashboardAuthLevel::Localhost,
            State(pool),
            Query(CoachingQuery { streamer: Some("Nani".into()), days: Some(30) }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
