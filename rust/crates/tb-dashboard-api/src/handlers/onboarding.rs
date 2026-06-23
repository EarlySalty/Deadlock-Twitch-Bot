//! GET/POST `/twitch/api/v2/streamer/onboarding`.
//!
//! Streamer-Selbstbedienung für den resumierbaren Onboarding-Wizard. Die
//! Identität kommt ausschließlich aus der Partner-Session (`twitch_user_id`).

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize)]
pub struct OnboardingUpdate {
    pub current_step: Option<i32>,
    pub completed: Option<bool>,
}

#[allow(clippy::result_large_err)]
fn resolve_partner(auth: &DashboardAuthLevel) -> Result<(String, String), Response> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
            ..
        } => {
            let user_id = twitch_user_id.trim();
            let login = twitch_login.trim().to_ascii_lowercase();
            if user_id.is_empty() || login.is_empty() {
                Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "unauthorized" })),
                )
                    .into_response())
            } else {
                Ok((user_id.to_string(), login))
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

fn onboarding_json(step: i32, completed: bool, discord: bool, steam: bool) -> Value {
    json!({
        "current_step": step,
        "completed": completed,
        "discord_linked": discord,
        "steam_linked": steam,
    })
}

pub async fn get_status(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    let (twitch_user_id, _) = match resolve_partner(&auth) {
        Ok(identity) => identity,
        Err(resp) => return resp,
    };

    let onboarding = sqlx::query_as::<_, (i32, bool)>(
        "SELECT current_step, completed \
         FROM streamer_onboarding \
         WHERE twitch_user_id = $1",
    )
    .bind(&twitch_user_id)
    .fetch_optional(&pool)
    .await;

    let (current_step, completed) = match onboarding {
        Ok(Some(row)) => row,
        Ok(None) => (0, false),
        Err(error) => {
            tracing::error!(%error, "onboarding GET DB-Fehler");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response();
        }
    };

    let discord_id = tb_chat::stats::resolve_discord_id(&pool, &twitch_user_id).await;
    let discord_linked = discord_id.is_some();
    let steam_linked = match discord_id.as_deref() {
        Some(discord_id) => tb_chat::stats::fetch_rank(discord_id, false)
            .await
            .map(|rank| rank.linked)
            .unwrap_or(false),
        None => false,
    };

    Json(onboarding_json(
        current_step,
        completed,
        discord_linked,
        steam_linked,
    ))
    .into_response()
}

pub async fn post_status(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<OnboardingUpdate>,
) -> Response {
    let (twitch_user_id, twitch_login) = match resolve_partner(&auth) {
        Ok(identity) => identity,
        Err(resp) => return resp,
    };

    let result = sqlx::query_as::<_, (i32, bool)>(
        "INSERT INTO streamer_onboarding (
             twitch_user_id, twitch_login, current_step, completed, completed_at, updated_at
         ) VALUES (
             $1, $2, COALESCE($3, 0), COALESCE($4, FALSE),
             CASE WHEN COALESCE($4, FALSE) THEN NOW() ELSE NULL END,
             NOW()
         )
         ON CONFLICT (twitch_user_id) DO UPDATE SET
             twitch_login = EXCLUDED.twitch_login,
             current_step = COALESCE($3, streamer_onboarding.current_step),
             completed = COALESCE($4, streamer_onboarding.completed),
             completed_at = CASE
                 WHEN $4 = TRUE THEN NOW()
                 WHEN $4 = FALSE THEN NULL
                 ELSE streamer_onboarding.completed_at
             END,
             updated_at = NOW()
         RETURNING current_step, completed",
    )
    .bind(&twitch_user_id)
    .bind(&twitch_login)
    .bind(body.current_step)
    .bind(body.completed)
    .fetch_one(&pool)
    .await;

    match result {
        Ok((current_step, completed)) => Json(json!({
            "ok": true,
            "current_step": current_step,
            "completed": completed,
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "onboarding POST DB-Fehler");
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
    fn onboarding_json_shape() {
        assert_eq!(
            onboarding_json(2, false, true, false),
            json!({
                "current_step": 2,
                "completed": false,
                "discord_linked": true,
                "steam_linked": false,
            })
        );
    }
}
