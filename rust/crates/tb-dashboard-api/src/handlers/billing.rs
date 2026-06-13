//! Handler für `POST /twitch/api/billing/trial/start`.
//!
//! Port von `bot/dashboard/routes_billing.py:api_billing_trial_start`.
//! Startet den einmaligen 30-Tage-Analytics-Trial für den eingeloggten Partner.
//! Status-Mapping identisch zu Python (granted→200, already_used/has_paid_plan→409,
//! error→500, kein Partner→401 mit Login-URL).

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use sqlx::PgPool;
use tb_analytics::trial::{start_trial_for_user, TrialOutcome};

use crate::auth::level::DashboardAuthLevel;

/// `POST /twitch/api/billing/trial/start`
pub async fn start_trial_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let (twitch_user_id, twitch_login) = match &auth {
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
        } => (twitch_user_id, twitch_login),
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "status": "unauthenticated",
                    "login_url": "/twitch/auth/login?next=%2Ftwitch%2Fpricing",
                })),
            )
                .into_response();
        }
    };

    let outcome = start_trial_for_user(&pool, twitch_user_id, twitch_login).await;
    let status = match outcome {
        TrialOutcome::Granted => StatusCode::OK,
        TrialOutcome::AlreadyUsed | TrialOutcome::HasPaidPlan => StatusCode::CONFLICT,
        TrialOutcome::Error => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "status": outcome.as_str() }))).into_response()
}
