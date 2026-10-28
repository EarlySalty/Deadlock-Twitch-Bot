use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_social_media::autopilot_publish as publish;
use tb_social_media::autopilot_settings::NotificationPreferences;
use tb_social_media::clip_pool::{self, PoolError, PoolPreferences};
use tb_social_media::produced_video::ProducedVideoError;

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PauseInput {
    pub paused: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalInput {
    pub destinations: Vec<publish::DestinationDecision>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleInput {
    pub platforms: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryInput {
    pub platform: String,
}

fn publish_error(error: publish::PublishError) -> Response {
    use publish::PublishError as E;
    let (status, code) = match &error {
        E::NotFound | E::Video(ProducedVideoError::NotFound) => {
            (StatusCode::NOT_FOUND, "not_found")
        }
        E::Video(ProducedVideoError::Ownership) => (StatusCode::FORBIDDEN, "forbidden"),
        E::Confirmation
        | E::PendingChoice
        | E::NoPlatforms
        | E::Invalid(_)
        | E::Settings(tb_social_media::autopilot_settings::SettingsError::Invalid(_))
        | E::Video(ProducedVideoError::NoPlatforms | ProducedVideoError::Invalid) => {
            (StatusCode::BAD_REQUEST, "invalid_publish_input")
        }
        E::State
        | E::ApprovalRequired
        | E::RetryBlocked
        | E::ArtifactDeleted
        | E::NoSlot
        | E::Video(_) => (StatusCode::CONFLICT, "publish_state_conflict"),
        E::Schedule(_) | E::Settings(_) | E::Database(_) | E::Json(_) | E::Io(_) => {
            tracing::warn!(%error, "Auto-Pilot publication request failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "publish_unavailable")
        }
    };
    (
        status,
        Json(json!({"error": code, "message": error.to_string()})),
    )
        .into_response()
}

fn publish_result(result: Result<(), publish::PublishError>) -> Response {
    match result {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(error) => publish_error(error),
    }
}

pub async fn publish_snapshot_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match publish::snapshot(&pool, &owner).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => publish_error(error),
    }
}

pub async fn publish_mode_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Json(input): Json<publish::ModeTransition>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    publish_result(publish::transition_mode(&pool, &owner, &input).await)
}

pub async fn publish_pause_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Json(input): Json<PauseInput>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    publish_result(publish::set_paused(&pool, &owner, input.paused).await)
}

pub async fn publish_notifications_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Json(input): Json<NotificationPreferences>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match publish::save_notifications(&pool, &owner, &input).await {
        Ok(preferences) => Json(preferences).into_response(),
        Err(error) => publish_error(error),
    }
}

pub async fn publish_approval_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Path(id): Path<i64>,
    Json(input): Json<ApprovalInput>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    publish_result(publish::approve(&pool, &owner, id, &input.destinations).await)
}

pub async fn publish_schedule_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Path(id): Path<i64>,
    Json(input): Json<ScheduleInput>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    publish_result(publish::schedule(&pool, &owner, id, &input.platforms).await)
}

pub async fn publish_retry_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Path(id): Path<i64>,
    Json(input): Json<RetryInput>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    publish_result(publish::retry(&pool, &owner, id, &input.platform).await)
}

pub async fn pipeline_policy_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match tb_social_media::autopilot_settings::load_policy(&pool, &owner).await {
        Ok(policy) => Json(policy).into_response(),
        Err(error) => error_response(error.into()),
    }
}

pub async fn pipeline_policy_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Json(input): Json<tb_social_media::autopilot_settings::PipelinePolicy>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match tb_social_media::autopilot_settings::save_policy(&pool, &owner, &input).await {
        Ok(policy) => Json(policy).into_response(),
        Err(error) => error_response(error.into()),
    }
}

pub async fn tiktok_publishing_get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match tb_social_media::tiktok_publishing::view(&pool, &owner).await {
        Ok(settings) => Json(settings).into_response(),
        Err(error) => error_response(error.into()),
    }
}

pub async fn tiktok_publishing_put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OwnerQuery>,
    Json(input): Json<tb_social_media::tiktok_publishing::TikTokPublishingInput>,
) -> Response {
    let (_, owner) = match target(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(value) => value,
        Err(error) => return *error,
    };
    match tb_social_media::tiktok_publishing::save_view(&pool, &owner, &input).await {
        Ok(settings) => Json(settings).into_response(),
        Err(error) => error_response(error.into()),
    }
}
