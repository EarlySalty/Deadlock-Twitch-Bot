//! Admin-only global Deadlock category reporting. No raw-text endpoint.
use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct Period {
    pub days: Option<String>,
}

pub async fn handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(period): Query<Period>,
) -> Response {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return error.into_response();
    }
    let days = match period.days.as_deref().unwrap_or("7") {
        "7" => 7,
        "30" => 30,
        "90" => 90,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"days must be 7, 30 or 90"})),
            )
                .into_response()
        }
    };
    match tb_analytics::category::report(&pool, days).await {
        Ok(report) => {
            ([(header::CACHE_CONTROL, "private, no-store")], Json(report)).into_response()
        }
        Err(error) => {
            tracing::error!(%error,"category collector report failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"category_data_unavailable"})),
            )
                .into_response()
        }
    }
}

pub async fn page(auth: DashboardAuthLevel) -> Response {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return error.into_response();
    }
    super::spa::main_domain_spa_shell_handler().await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql:///not_used")
            .unwrap()
    }
    #[tokio::test]
    async fn unauthenticated_and_partner_requests_never_reach_storage() {
        assert_eq!(
            handler(
                DashboardAuthLevel::None,
                State(pool()),
                Query(Period { days: None })
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "partner".into(),
            twitch_user_id: "1".into(),
            display_name: "Partner".into(),
        };
        assert_eq!(
            handler(
                partner.clone(),
                State(pool()),
                Query(Period {
                    days: Some("7".into())
                })
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(page(partner).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            page(DashboardAuthLevel::None).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn invalid_period_is_not_silently_clamped() {
        for days in ["0", "8", "365", "NaN", "7;DROP TABLE category_channels"] {
            assert_eq!(
                handler(
                    DashboardAuthLevel::admin(),
                    State(pool()),
                    Query(Period {
                        days: Some(days.into())
                    })
                )
                .await
                .status(),
                StatusCode::BAD_REQUEST
            );
        }
    }
}
