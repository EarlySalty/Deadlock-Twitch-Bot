//! Handler für `GET /twitch/api/v2/category-activity-series`.
//!
//! Port von `bot/analytics/api_performance.py:_api_v2_category_activity_series`.
//! Auth + Extended-Plan-Gate (Paywall), dann die Aggregat-Series aus
//! [`tb_analytics::category_activity`].

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
pub struct ActivityQuery {
    #[serde(default)]
    pub days: Option<i32>,
}

/// `GET /twitch/api/v2/category-activity-series?days=30`
pub async fn category_activity_series_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ActivityQuery>,
) -> impl IntoResponse {
    // Python: _require_v2_auth + _require_extended_plan (Paywall-Feature).
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;

    match tb_analytics::category_activity::load_category_activity_series(&pool, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("category-activity-series SELECT-Fehler: {e}");
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
    async fn none_auth_blockiert() {
        let Some(pool) = make_pool("t_catact_h").await else { return };
        let resp = category_activity_series_handler(
            DashboardAuthLevel::None,
            State(pool),
            Query(ActivityQuery { days: None }),
        )
        .await
        .into_response();
        // extended_gate gibt für None 401 zurück (kein 200/500).
        assert_ne!(resp.status(), StatusCode::OK);
        assert!(resp.status().is_client_error());
    }
}
