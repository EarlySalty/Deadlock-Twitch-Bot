//! Handler für `/twitch/api/v2/tag-analysis-extended`.
//!
//! Port von `bot/analytics/api_performance.py:_api_v2_tag_analysis_extended`.
//! Auth + Extended-Plan-Gate, `streamer` (optional → ohne Filter über alle),
//! `days` (7..365, Default 30), `limit` (5..50, Default 20).

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
pub struct TagAnalysisQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<i32>,
    #[serde(default)]
    pub limit: Option<i32>,
}

/// `GET /twitch/api/v2/tag-analysis-extended?streamer=&days=30&limit=20`
pub async fn tag_analysis_extended_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<TagAnalysisQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    // streamer optional: "" → None (Python `.strip() or None`).
    let streamer = params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;
    let limit = params.limit.unwrap_or(20).clamp(5, 50) as i64;

    match tb_analytics::tag_analysis::load_tag_analysis_extended(&pool, streamer, days, limit).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("tag-analysis-extended SELECT-Fehler: {e}");
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
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema), ("timezone", "UTC")]);
        Some(PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap())
    }

    #[tokio::test]
    async fn localhost_liefert_200_leer() {
        let Some(pool) = make_pool("t_tagx_handler").await else { return };
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, tags TEXT, avg_viewers REAL, retention_10m REAL, follower_delta INTEGER, followers_start INTEGER, followers_end INTEGER, duration_seconds REAL, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        // Localhost = privilegiert (bypass Paywall) → 200 statt 401.
        let resp = tag_analysis_extended_handler(
            DashboardAuthLevel::Localhost,
            State(pool),
            Query(TagAnalysisQuery { streamer: None, days: None, limit: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
