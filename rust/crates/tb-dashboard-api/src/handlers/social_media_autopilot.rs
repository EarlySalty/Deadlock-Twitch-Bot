use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_social_media::clip_pool::{self, PoolError, PoolPreferences};

use crate::auth::level::DashboardAuthLevel;

#[cfg(test)]
#[path = "social_media_autopilot_tests.rs"]
mod tests;

#[derive(Debug, Deserialize)]
pub struct OwnerQuery {
    pub twitch_user_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionInput {
    pub selected: bool,
}

async fn target(
    auth: &DashboardAuthLevel,
    pool: &PgPool,
    requested: Option<&str>,
) -> Result<(String, String), Box<Response>> {
    let scope = super::require_sm_access(auth, pool, requested)
        .await
        .map_err(Box::new)?;
    crate::auth::streamer_scope::resolve_clip_upload_target(pool, auth, scope.as_deref())
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

pub async fn pool_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match clip_pool::list(
        &pool,
        &owner,
        query.limit.unwrap_or(50),
        query.offset.unwrap_or(0),
    )
    .await
    {
        Ok(page) => Json(page).into_response(),
        Err(error) => error_response(error),
    }
}

pub async fn preferences_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match tb_social_media::autopilot_settings::load(&pool, &owner).await {
        Ok(settings) => Json(PoolPreferences::from(&settings)).into_response(),
        Err(error) => error_response(error.into()),
    }
}

pub async fn preferences_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Json(input): Json<PoolPreferences>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match clip_pool::save_preferences(&pool, &owner, &input).await {
        Ok(preferences) => Json(preferences).into_response(),
        Err(error) => error_response(error),
    }
}

pub async fn import_start_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
) -> Response {
    let (login, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match clip_pool::start_import(&pool, &owner, &login).await {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => error_response(error),
    }
}

pub async fn import_latest_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match clip_pool::latest_import(&pool, &owner).await {
        Ok(job) => Json(json!({"import": job})).into_response(),
        Err(error) => error_response(error),
    }
}

pub async fn import_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Path(id): Path<i64>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match clip_pool::get_import(&pool, &owner, id).await {
        Ok(job) => Json(job).into_response(),
        Err(error) => error_response(error),
    }
}

pub async fn import_resume_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Path(id): Path<i64>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match clip_pool::resume_import(&pool, &owner, id).await {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => error_response(error),
    }
}

pub async fn selection_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Path(clip_id): Path<String>,
    Json(input): Json<SelectionInput>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match clip_pool::set_selected(&pool, &owner, &clip_id, input.selected).await {
        Ok(()) => Json(json!({"clip_id": clip_id, "selected": input.selected})).into_response(),
        Err(error) => error_response(error),
    }
}
