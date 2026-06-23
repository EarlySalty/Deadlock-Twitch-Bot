//! Handler für `/twitch/api/v2/exp/*` (experimentelle Analytics).
//!
//! Port von `bot/analytics/api_experimental.py`. Auth + Extended-Plan-Gate,
//! `streamer` (Pflicht), `days` (1..365, Default 30), dann die Daten aus
//! [`tb_analytics::exp_analytics`].

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
pub struct ExpQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<i32>,
}

/// `GET /twitch/api/v2/exp/overview?streamer=&days=30`
pub async fn exp_overview_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ExpQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "streamer parameter required" }))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(1, 365) as i64;

    match tb_analytics::exp_analytics::load_exp_overview(&pool, &streamer, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("exp/overview SELECT-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal" }))).into_response()
        }
    }
}

/// `GET /twitch/api/v2/exp/game-breakdown?streamer=&days=30`
pub async fn exp_game_breakdown_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ExpQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "streamer parameter required" }))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(1, 365) as i64;

    match tb_analytics::exp_analytics::load_exp_game_breakdown(&pool, &streamer, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("exp/game-breakdown SELECT-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal" }))).into_response()
        }
    }
}

/// `GET /twitch/api/v2/exp/game-transitions?streamer=&days=30`
pub async fn exp_game_transitions_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ExpQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "streamer parameter required" }))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(1, 365) as i64;

    match tb_analytics::exp_analytics::load_exp_game_transitions(&pool, &streamer, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("exp/game-transitions SELECT-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal" }))).into_response()
        }
    }
}

/// `GET /twitch/api/v2/exp/growth-curves?streamer=&days=30`
pub async fn exp_growth_curves_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ExpQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "streamer parameter required" }))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(1, 365) as i64;

    match tb_analytics::exp_analytics::load_exp_growth_curves(&pool, &streamer, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("exp/growth-curves SELECT-Fehler: {e}");
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
        Some(PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap())
    }

    #[tokio::test]
    async fn streamer_pflicht_localhost() {
        let Some(pool) = make_pool("t_exp_h").await else { return };
        sqlx::query("CREATE TABLE exp_sessions (streamer TEXT, started_at TEXT, ended_at TEXT, game_name TEXT, avg_viewers REAL)").execute(&pool).await.unwrap();
        // Localhost = privilegiert (bypass Paywall); fehlender streamer → 400.
        let resp = exp_overview_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ExpQuery { streamer: None, days: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
