use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{header::CACHE_CONTROL, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_http_core::ApiError;
use tb_social_media::ai_editor::{
    skill_registry::{self, ChangeProposal, SkillHistory, SkillRegistryError},
    SkillId,
};

use crate::auth::{
    level::{AuthenticatedAdminSessionId, AuthenticatedPartnerSessionId, DashboardAuthLevel},
    session::DashboardAuthState,
};
use crate::social_media_ai_admin_usage::{actor, failure, parse};

pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/twitch/api/admin/social-ai/skills", get(skills))
        .route(
            "/twitch/api/admin/social-ai/skills/{skill_id}/rollback",
            post(rollback),
        )
        .layer(DefaultBodyLimit::max(4096))
}

#[derive(Debug, Serialize)]
struct SkillsReport {
    rollback_available: bool,
    skills: Vec<SkillHistory>,
    proposals: Vec<ChangeProposal>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RollbackChange {
    expected_version: String,
    target_version: String,
    reason: String,
}

fn registry_error(error: SkillRegistryError) -> ApiError {
    match error {
        SkillRegistryError::Invalid => {
            ApiError::bad_request("Die Zielversion oder Begründung ist ungültig.")
        }
        SkillRegistryError::Conflict => failure(
            StatusCode::CONFLICT,
            "Die aktive Version wurde inzwischen geändert. Bitte neu laden und erneut prüfen.",
        ),
        SkillRegistryError::NotFound => failure(
            StatusCode::NOT_FOUND,
            "Diese Skill-Version ist nicht vorhanden.",
        ),
        SkillRegistryError::Storage => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Skill-Versionen sind gerade nicht verfügbar.",
        ),
    }
}

fn version_number(value: &str) -> Result<i32, ApiError> {
    value
        .parse::<i32>()
        .ok()
        .filter(|version| *version >= 1)
        .ok_or_else(|| ApiError::bad_request("Die Versionsangabe ist ungültig."))
}

async fn report(pool: &PgPool) -> Result<Response, ApiError> {
    let skills = skill_registry::history(pool)
        .await
        .map_err(registry_error)?;
    let proposals = skill_registry::proposals(pool)
        .await
        .map_err(registry_error)?;
    let data = SkillsReport {
        rollback_available: true,
        skills,
        proposals,
    };
    Ok(([(CACHE_CONTROL, "no-store")], Json(data)).into_response())
}

pub async fn skills(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    report(&pool).await
}

pub async fn rollback(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(skill_id): Path<String>,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let id = SkillId::parse(&skill_id)
        .ok_or_else(|| failure(StatusCode::NOT_FOUND, "Dieser Skill ist unbekannt."))?;
    let change: RollbackChange = parse(&body)?;
    let expected = version_number(&change.expected_version)?;
    let target = version_number(&change.target_version)?;
    let actor = actor(&auth, state, session, partner).await?;
    skill_registry::rollback(&pool, id, expected, target, &change.reason, &actor)
        .await
        .map_err(registry_error)?;
    report(&pool).await
}

#[cfg(test)]
#[path = "social_media_ai_admin_skills_tests.rs"]
mod tests;
