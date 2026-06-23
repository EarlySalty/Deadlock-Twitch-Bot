//! Handler für `/twitch/api/v2/monetization`.
//!
//! Port von `bot/analytics/api_insights.py:_api_v2_monetization`.
//! Auth + Extended-Plan-Gate, `streamer` (optional, leer = ohne Filter),
//! `days` (7..90, Default 30).

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
pub struct MonetizationQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    // Rohwert: nicht-numerisches `days` → Python-konformes 400-JSON, siehe query_int.
    #[serde(default)]
    pub days: Option<String>,
}

/// `GET /twitch/api/v2/monetization?streamer=&days=30`
pub async fn monetization_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<MonetizationQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    // streamer: getrimmt + kleingeschrieben, "" wenn fehlend (Python `.strip().lower()`).
    let streamer = params.streamer.as_deref().unwrap_or("").trim().to_lowercase();
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 90) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };

    match tb_analytics::monetization::load_monetization_payload(&pool, &streamer, days).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("monetization SELECT-Fehler: {e}");
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
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        // is_automatic/is_gift sind im echten Schema INTEGER (0/1); der Loader
        // filtert via `= 1`. BOOLEAN-Fixtures machten `boolean = 1` zum Typfehler
        // (Handler-500) — prod-treu INTEGER, damit der Test den realen Pfad trifft.
        sqlx::query("CREATE TABLE twitch_ad_break_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, duration_seconds INTEGER, is_automatic INTEGER DEFAULT 0, started_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_bits_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, amount INTEGER, received_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_subscription_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, is_gift INTEGER DEFAULT 0, received_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_hype_train_events (id BIGSERIAL PRIMARY KEY, session_id BIGINT, level INTEGER, duration_seconds INTEGER, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn localhost_liefert_200() {
        let Some(pool) = make_pool("t_mon_handler").await else { return };
        // Localhost = privilegiert (bypass Paywall) → 200.
        let resp = monetization_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(MonetizationQuery { streamer: None, days: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn nicht_numerische_days_400_python_shape() {
        let Some(pool) = make_pool("t_mon_handler_400").await else { return };
        let resp = monetization_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(MonetizationQuery { streamer: None, days: Some("nope".into()) }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, json!({ "error": "days must be an integer" }));
    }
}
