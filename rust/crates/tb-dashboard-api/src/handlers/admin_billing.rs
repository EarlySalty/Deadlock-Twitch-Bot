//! Admin-Billing-Übersichten (Read-Only).
//!
//! Port von `bot/analytics/api_admin.py:_api_admin_billing_subscriptions` +
//! `_api_admin_billing_affiliates`. Zwei GET-Endpoints liefern die Stripe-Abos
//! (+ manuelle Plan-Overrides) bzw. die Affiliate-Konten. Admin über
//! `DashboardAuthLevel`.

use crate::auth::level::DashboardAuthLevel;
use axum::{extract::State, response::IntoResponse, Json};
use sqlx::PgPool;
use tb_http_core::ApiError;

fn db_error(e: sqlx::Error) -> ApiError {
    tracing::error!("admin_billing SELECT-Fehler: {e}");
    ApiError::internal()
}

/// `GET /twitch/api/admin/billing/subscriptions` — Stripe-Abos (Admin).
pub async fn subscriptions_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let payload = tb_analytics::admin_billing::load_billing_subscriptions(&pool)
        .await
        .map_err(db_error)?;
    Ok(Json(payload))
}

/// `GET /twitch/api/admin/billing/affiliates` — Affiliate-Konten (Admin).
pub async fn affiliates_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let payload = tb_analytics::admin_billing::load_billing_affiliates(&pool)
        .await
        .map_err(db_error)?;
    Ok(Json(payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::Value;
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
            "CREATE TABLE twitch_billing_subscriptions (stripe_subscription_id TEXT PRIMARY KEY, customer_reference TEXT, plan_id TEXT, status TEXT, current_period_end TEXT, updated_at TEXT)",
            "CREATE TABLE streamer_plans (twitch_login TEXT PRIMARY KEY, manual_plan_id TEXT, manual_plan_expires_at TEXT)",
            "CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, email TEXT, stripe_account_id TEXT, stripe_connect_status TEXT, updated_at TEXT, created_at TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn body_json(r: Result<impl IntoResponse, ApiError>) -> (StatusCode, Value) {
        let resp = r.into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn partner_auth() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "partner".into(),
            twitch_user_id: "100".into(),
            display_name: "Partner".into(),
        }
    }

    #[tokio::test]
    async fn unauth_auth_required_401() {
        let Some(pool) = make_pool("t_acbill_unauth").await else {
            return;
        };
        let (s, _) =
            body_json(subscriptions_handler(DashboardAuthLevel::None, State(pool.clone())).await)
                .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _) =
            body_json(affiliates_handler(DashboardAuthLevel::None, State(pool)).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn partner_admin_required_403() {
        let Some(pool) = make_pool("t_acbill_partner").await else {
            return;
        };
        let (s, j) =
            body_json(subscriptions_handler(partner_auth(), State(pool.clone())).await).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert_eq!(j["error"], "admin_required");
        assert_eq!(j["required"], "admin");
        let (s, j) = body_json(affiliates_handler(partner_auth(), State(pool)).await).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert_eq!(j["error"], "admin_required");
        assert_eq!(j["required"], "admin");
    }

    #[tokio::test]
    async fn happy_liefert_items_count() {
        let Some(pool) = make_pool("t_acbill_ok").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_billing_subscriptions (stripe_subscription_id, customer_reference, status, updated_at) VALUES ('s1', 'nani', 'active', '2026-06-01T00:00:00+00:00')")
            .execute(&pool).await.unwrap();
        let (s, j) = body_json(
            subscriptions_handler(DashboardAuthLevel::admin(), State(pool.clone())).await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["count"], 1);
        assert_eq!(j["items"][0]["login"], "nani");

        let (s, j) =
            body_json(affiliates_handler(DashboardAuthLevel::admin(), State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["count"], 0);
        assert!(j["items"].is_array());
    }
}
