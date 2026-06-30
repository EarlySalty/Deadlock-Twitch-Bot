//! GET/POST `/twitch/api/v2/streamer/tip-settings`.
//!
//! Streamer-Selbstbedienung für das Go-Live-Tipp-Opt-out. Die Identität kommt
//! ausschließlich aus der Partner-Session (`twitch_user_id`); Admin/Localhost
//! adressieren hier keinen fremden Kanal.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize)]
pub struct TipSettingsUpdate {
    pub opt_out: bool,
}

#[derive(Serialize)]
struct TipSettingsResponse {
    opt_out: bool,
}

#[allow(clippy::result_large_err)]
fn resolve_twitch_user_id(auth: &DashboardAuthLevel) -> Result<String, Response> {
    match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => {
            let user_id = twitch_user_id.trim();
            if user_id.is_empty() {
                Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "unauthorized" })),
                )
                    .into_response())
            } else {
                Ok(user_id.to_string())
            }
        }
        DashboardAuthLevel::Admin { .. } => Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "partner required" })),
        )
            .into_response()),
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()),
    }
}

pub async fn get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    let twitch_user_id = match resolve_twitch_user_id(&auth) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    match sqlx::query_scalar!(
        "SELECT opt_out FROM twitch_tip_settings WHERE twitch_user_id = $1",
        twitch_user_id
    )
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(opt_out)) => Json(TipSettingsResponse { opt_out }).into_response(),
        Ok(None) => Json(TipSettingsResponse { opt_out: false }).into_response(),
        Err(error) => {
            tracing::error!(%error, "tip-settings GET DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
        }
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<TipSettingsUpdate>,
) -> Response {
    let twitch_user_id = match resolve_twitch_user_id(&auth) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let result = sqlx::query!(
        "INSERT INTO twitch_tip_settings (twitch_user_id, opt_out, updated_at) \
         VALUES ($1, $2, NOW()) \
         ON CONFLICT (twitch_user_id) DO UPDATE \
         SET opt_out = EXCLUDED.opt_out, updated_at = NOW()",
        twitch_user_id,
        body.opt_out
    )
    .execute(&pool)
    .await;

    match result {
        Ok(_) => Json(json!({ "ok": true, "opt_out": body.opt_out })).into_response(),
        Err(error) => {
            tracing::error!(%error, "tip-settings POST DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tip_settings_response_json_shape() {
        let body = serde_json::to_value(TipSettingsResponse { opt_out: true }).unwrap();
        assert_eq!(body, json!({ "opt_out": true }));
    }
}
