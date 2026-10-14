use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_social_media::{
    autopilot_settings::ScheduleSlot,
    production_jobs::autopilot_schedule::{self, ScheduleError, SchedulePatch},
};

use super::{DashboardAuthLevel, StreamerQuery};

async fn owner_scope(
    auth: &DashboardAuthLevel,
    pool: &PgPool,
    requested: Option<&str>,
) -> Result<String, Box<Response>> {
    let authorized = super::require_sm_access(auth, pool, requested)
        .await
        .map_err(Box::new)?;
    let (_, owner) =
        crate::auth::streamer_scope::resolve_clip_upload_target(pool, auth, authorized.as_deref())
            .await?;
    Ok(owner)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotWrite {
    pub weekday: u8,
    pub local_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleWrite {
    pub twitch_user_id: Option<String>,
    pub enabled: Option<bool>,
    pub paused: Option<bool>,
    pub ai_editing_enabled: Option<bool>,
    pub lead_time_hours: Option<i32>,
    pub timezone: Option<String>,
    pub schedule_slots: Option<Vec<SlotWrite>>,
}

impl ScheduleWrite {
    fn patch(self) -> SchedulePatch {
        SchedulePatch {
            enabled: self.enabled,
            paused: self.paused,
            ai_editing_enabled: self.ai_editing_enabled,
            lead_time_hours: self.lead_time_hours,
            timezone: self.timezone,
            schedule_slots: self.schedule_slots.map(|slots| {
                slots
                    .into_iter()
                    .map(|slot| ScheduleSlot {
                        weekday: slot.weekday,
                        local_time: slot.local_time,
                    })
                    .collect()
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueOrderWrite {
    pub twitch_user_id: Option<String>,
    pub job_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueActionWrite {
    pub twitch_user_id: Option<String>,
}

fn invalid_body() -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_schedule", "message":"Die Zeitplandaten sind ungültig. Bitte prüfe deine Eingaben."}))).into_response()
}

fn failure(error: ScheduleError) -> Response {
    let (status, code) = match &error {
        ScheduleError::Invalid(_)
        | ScheduleError::Settings(tb_social_media::autopilot_settings::SettingsError::Invalid(_)) => {
            (StatusCode::BAD_REQUEST, "invalid_schedule")
        }
        ScheduleError::NotFound => (StatusCode::NOT_FOUND, "job_not_found"),
        ScheduleError::Started => (StatusCode::CONFLICT, "job_started"),
        ScheduleError::QueueChanged => (StatusCode::CONFLICT, "queue_changed"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "schedule_unavailable"),
    };
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        tracing::warn!(error = %error, "Auto-Pilot schedule request failed");
    }
    (
        status,
        Json(json!({"error":code,"message":error.to_string()})),
    )
        .into_response()
}

async fn response(pool: &PgPool, owner: &str) -> Response {
    match autopilot_schedule::snapshot(pool, owner).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => failure(error),
    }
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<StreamerQuery>,
) -> Response {
    let owner = match owner_scope(&auth, &pool, query.twitch_user_id.as_deref()).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    response(&pool, &owner).await
}

pub async fn put_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: Result<Json<ScheduleWrite>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return invalid_body();
    };
    let owner = match owner_scope(&auth, &pool, body.twitch_user_id.as_deref()).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    if let Err(error) = autopilot_schedule::save_schedule(&pool, &owner, &body.patch()).await {
        return failure(error);
    }
    response(&pool, &owner).await
}

pub async fn order_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: Result<Json<QueueOrderWrite>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return invalid_body();
    };
    let owner = match owner_scope(&auth, &pool, body.twitch_user_id.as_deref()).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    if let Err(error) = autopilot_schedule::reorder(&pool, &owner, &body.job_ids).await {
        return failure(error);
    }
    response(&pool, &owner).await
}

pub async fn next_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
    body: Result<Json<QueueActionWrite>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return invalid_body();
    };
    let owner = match owner_scope(&auth, &pool, body.twitch_user_id.as_deref()).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    if let Err(error) = autopilot_schedule::mark_next(&pool, &owner, id).await {
        return failure(error);
    }
    response(&pool, &owner).await
}

pub async fn cancel_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
    body: Result<Json<QueueActionWrite>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return invalid_body();
    };
    let owner = match owner_scope(&auth, &pool, body.twitch_user_id.as_deref()).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    if let Err(error) = autopilot_schedule::cancel(&pool, &owner, id).await {
        return failure(error);
    }
    response(&pool, &owner).await
}

#[cfg(test)]
#[path = "social_media_autopilot_schedule_tests.rs"]
mod tests;
