use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_social_media::clip_pool::{self, PoolError, SourceEligibility};

use crate::auth::level::DashboardAuthLevel;

#[cfg(test)]
#[path = "social_media_pipeline_tests.rs"]
mod tests;

#[derive(Debug, Deserialize)]
pub struct SourcesQuery {
    pub twitch_user_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub eligibility: Option<String>,
}

async fn owner(
    auth: &DashboardAuthLevel,
    pool: &PgPool,
    requested: Option<&str>,
) -> Result<String, Box<Response>> {
    if let DashboardAuthLevel::Partner { twitch_user_id, .. } = auth {
        if requested.is_some_and(|id| id != twitch_user_id) {
            return Err(Box::new(
                (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "forbidden",
                        "message": "Du kannst nur auf deinen eigenen Twitch-Account zugreifen."
                    })),
                )
                    .into_response(),
            ));
        }
    }
    super::social_media::required_streamer_scope(auth, pool, requested)
        .await
        .map_err(Box::new)
}

fn error_response(error: PoolError) -> Response {
    let (status, code) = match &error {
        PoolError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        PoolError::InvalidFilter
        | PoolError::InvalidSource
        | PoolError::Settings(tb_social_media::autopilot_settings::SettingsError::Invalid(_)) => {
            (StatusCode::BAD_REQUEST, "invalid_pool_input")
        }
        PoolError::NotSelectable | PoolError::Reserved | PoolError::Busy | PoolError::LeaseLost => {
            (StatusCode::CONFLICT, "pool_state_conflict")
        }
        PoolError::Database(_) | PoolError::Json(_) | PoolError::Settings(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "pool_unavailable")
        }
    };
    (
        status,
        Json(json!({"error": code, "message": error.to_string()})),
    )
        .into_response()
}

pub async fn sources_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<SourcesQuery>,
) -> Response {
    let owner = match owner(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(owner) => owner,
        Err(response) => return *response,
    };
    let eligibility = match query
        .eligibility
        .as_deref()
        .filter(|raw| !raw.is_empty())
        .map(SourceEligibility::parse)
        .transpose()
    {
        Ok(eligibility) => eligibility,
        Err(error) => return error_response(error),
    };
    match clip_pool::pipeline_sources(
        &pool,
        &owner,
        eligibility,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await
    {
        Ok(page) => Json(page).into_response(),
        Err(error) => error_response(error),
    }
}
