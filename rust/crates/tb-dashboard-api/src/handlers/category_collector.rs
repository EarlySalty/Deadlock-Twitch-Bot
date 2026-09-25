//! Admin-only aggregates for the independent global Deadlock collector.
//! No raw chat, per-chatter identities, geography guesses or write endpoints.

use axum::{
    extract::{RawQuery, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

const METRICS_SQL: &str = include_str!("category_collector.sql");

fn response(status: StatusCode, body: Value) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

fn parse_days(query: Option<&str>) -> Result<i32, ()> {
    let mut days = None;
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if key != "days" {
            continue;
        }
        // Reject duplicate/ambiguous inputs rather than silently using one.
        if days.is_some() {
            return Err(());
        }
        days = Some(match value.as_ref() {
            "7" => 7,
            "30" => 30,
            "90" => 90,
            _ => return Err(()),
        });
    }
    Ok(days.unwrap_or(7))
}

async fn load(pool: &PgPool, days: i32, end: DateTime<Utc>) -> Result<Value, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout = '8s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL TIME ZONE 'UTC'")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query_scalar::<_, Value>(METRICS_SQL)
        .bind(days)
        .bind(end)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result)
}

/// GET /twitch/api/v2/admin/category-collector?days=7|30|90
pub async fn handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    RawQuery(query): RawQuery,
) -> Response {
    // Authorize BEFORE parsing input or accessing collector tables.
    if let Some(error) = crate::auth::require_admin(&auth) {
        let mut reply = error.into_response();
        reply.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        return reply;
    }
    let days = match parse_days(query.as_deref()) {
        Ok(days) => days,
        Err(()) => {
            return response(StatusCode::BAD_REQUEST, json!({
                "error": "invalid_days", "message": "Zeitraum muss 7, 30 oder 90 Tage sein."
            }));
        }
    };
    match tokio::time::timeout(std::time::Duration::from_secs(12), load(&pool, days, Utc::now())).await {
        Ok(Ok(data)) => response(StatusCode::OK, data),
        Ok(Err(error)) => {
            let schema_missing = error.as_database_error().and_then(|e| e.code()).as_deref() == Some("42P01");
            // Log the class only: SQL details may contain private data.
            tracing::warn!(schema_missing, "Kategoriesammler-Auswertung nicht verfügbar");
            response(StatusCode::SERVICE_UNAVAILABLE, json!({
                "error": if schema_missing { "collector_schema_missing" } else { "collector_unavailable" },
                "message": "Die Auswertung des Kategoriesammlers ist derzeit nicht verfügbar."
            }))
        }
        Err(_) => response(StatusCode::GATEWAY_TIMEOUT, json!({
            "error": "collector_timeout", "message": "Die Auswertung konnte nicht rechtzeitig geladen werden."
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn unreachable_pool() -> PgPool {
        PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(100))
            .connect_lazy("postgres://test:test@127.0.0.1:1/unreachable")
            .unwrap()
    }

    #[test]
    fn only_unambiguous_supported_periods() {
        assert_eq!(parse_days(None), Ok(7));
        for day in [7, 30, 90] {
            assert_eq!(parse_days(Some(&format!("days={day}"))), Ok(day));
        }
        for query in ["days=0", "days=91", "days=30foo", "days=", "days=-7", "days=7&days=30", "days=7.0"] {
            assert!(parse_days(Some(query)).is_err(), "accepted {query}");
        }
    }

    #[tokio::test]
    async fn anonymous_denied_before_database_and_query_validation() {
        let reply = handler(DashboardAuthLevel::None, State(unreachable_pool()), RawQuery(Some("days=bad".into()))).await;
        assert_eq!(reply.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(reply.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn partner_denied_before_database() {
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "partner".into(), twitch_user_id: "123".into(), display_name: "Partner".into()
        };
        let reply = handler(partner, State(unreachable_pool()), RawQuery(None)).await;
        assert_eq!(reply.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_invalid_days_is_bad_request_not_backend_failure() {
        let reply = handler(DashboardAuthLevel::admin(), State(unreachable_pool()), RawQuery(Some("days=30foo".into()))).await;
        assert_eq!(reply.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unavailable_database_is_not_an_empty_success() {
        let reply = handler(DashboardAuthLevel::admin(), State(unreachable_pool()), RawQuery(None)).await;
        assert_eq!(reply.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(reply.into_body(), 4096).await.unwrap();
        let body = String::from_utf8_lossy(&bytes);
        assert!(body.contains("collector_unavailable"));
        assert!(!body.contains("postgres"));
        assert!(!body.contains("test:test"));
    }
}
