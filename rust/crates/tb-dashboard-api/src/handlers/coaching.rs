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
use crate::language::RequestLanguage;
use crate::query_int::parse_bounded_query_int;

#[derive(Deserialize)]
pub struct CoachingQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<String>,
}

pub async fn coaching_handler(
    auth: DashboardAuthLevel,
    RequestLanguage(lang): RequestLanguage,
    State(pool): State<PgPool>,
    Query(params): Query<CoachingQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 3650) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
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

    match tb_analytics::coaching::get_coaching_data(&pool, &streamer, days, lang).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("coaching Fehler: {e}");
            crate::auth::analytics_request_failed_json().into_response()
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
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE"
        )))
        .execute(&admin)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
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
            RequestLanguage::default(),
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
            RequestLanguage::default(),
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
            RequestLanguage::default(),
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
            RequestLanguage::default(),
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
