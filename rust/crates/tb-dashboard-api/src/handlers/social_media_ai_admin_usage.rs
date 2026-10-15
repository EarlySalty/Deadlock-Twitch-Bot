use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{header::CACHE_CONTROL, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::de::DeserializeOwned;
use sqlx::PgPool;
use tb_http_core::ApiError;
use tb_llm::social_editor::{
    admin_limits::{self, GlobalControlChange, LimitError, PlanLimitChange, UserLimitChange},
    admin_usage::{self, UsageFilter},
};

use crate::auth::{
    level::{AuthenticatedAdminSessionId, AuthenticatedPartnerSessionId, DashboardAuthLevel},
    session::DashboardAuthState,
};

pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/twitch/api/admin/social-ai/usage", get(usage))
        .route("/twitch/api/admin/social-ai/limits", get(limits))
        .route(
            "/twitch/api/admin/social-ai/external-consent",
            get(external_consent).put(save_external_consent),
        )
        .route(
            "/twitch/api/admin/social-ai/limits/global",
            post(save_global),
        )
        .route("/twitch/api/admin/social-ai/limits/user", post(save_user))
        .route("/twitch/api/admin/social-ai/limits/plan", post(save_plan))
        .layer(DefaultBodyLimit::max(8192))
}

pub(crate) fn failure(status: StatusCode, message: &'static str) -> ApiError {
    let mut error = ApiError::bad_request(message);
    error.status = status;
    error
}

fn limit_error(error: LimitError) -> ApiError {
    match error {
        LimitError::Invalid => ApiError::bad_request("Die Grenze oder Begründung ist ungültig."),
        LimitError::Conflict => failure(StatusCode::CONFLICT, "Dieser Stand wurde inzwischen geändert. Bitte neu laden und die Änderung erneut prüfen."),
        LimitError::UnknownUser => failure(StatusCode::NOT_FOUND, "Diese Twitch-Nutzer-ID ist nicht im System vorhanden."),
        LimitError::Unavailable => failure(StatusCode::SERVICE_UNAVAILABLE, "Der gespeicherte Stand konnte nicht bestätigt werden. Bitte neu laden, bevor du erneut speicherst."),
    }
}

fn catalog() -> Vec<(&'static str, &'static str)> {
    tb_analytics::billing::catalog::BILLING_PLANS
        .iter()
        .map(|plan| (plan.id, plan.name))
        .collect()
}

pub(crate) fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    let mut value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| ApiError::bad_request("Die Anfrage ist ungültig."))?;
    if let Some(object) = value.as_object_mut() {
        object.remove("csrf_token");
    }
    serde_json::from_value(value)
        .map_err(|_| ApiError::bad_request("Die Änderungsfelder sind ungültig."))
}

pub(crate) async fn actor(
    auth: &DashboardAuthLevel,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
) -> Result<String, ApiError> {
    if let Some(Extension(state)) = state {
        if let Some(Extension(session)) = session {
            return state
                .load_admin_session_user_id(&session.0)
                .await
                .ok()
                .flatten()
                .map(|id| format!("discord:{id}"))
                .ok_or_else(|| {
                    failure(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Die Admin-Identität konnte nicht bestätigt werden.",
                    )
                });
        }
        if let Some(Extension(partner)) = partner {
            return state
                .load_partner_session(&partner.0)
                .await
                .ok()
                .flatten()
                .map(|identity| format!("twitch:{}", identity.twitch_user_id))
                .ok_or_else(|| {
                    failure(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Die Admin-Identität konnte nicht bestätigt werden.",
                    )
                });
        }
    }
    Ok(match auth {
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            format!("twitch:{}", actor.twitch_user_id)
        }
        _ => "internal-admin".to_owned(),
    })
}

async fn limits_response(pool: &PgPool) -> Result<Response, ApiError> {
    let data = admin_limits::processing_limits(pool, &catalog())
        .await
        .map_err(limit_error)?;
    Ok(([(CACHE_CONTROL, "no-store")], Json(data)).into_response())
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalConsent {
    enabled: bool,
}

async fn external_consent(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let value = sqlx::query_scalar::<_, Option<String>>(
        "SELECT value::text FROM social_media_settings WHERE key = $1",
    )
    .bind(tb_social_media::settings::KEY_EXTERNAL_LLM_CONSENT)
    .fetch_optional(&pool)
    .await
    .map_err(|_| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Freigabe konnte nicht geladen werden.",
        )
    })?
    .flatten();
    let enabled = value
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        == Some(serde_json::Value::Bool(true));
    Ok((
        [(CACHE_CONTROL, "no-store")],
        Json(ExternalConsent { enabled }),
    )
        .into_response())
}

async fn save_external_consent(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let change: ExternalConsent = parse(&body)?;
    let actor = actor(&auth, state, session, partner).await?;
    tb_social_media::settings::set_setting(
        &pool,
        tb_social_media::settings::KEY_EXTERNAL_LLM_CONSENT,
        &serde_json::Value::Bool(change.enabled),
        Some(&actor),
    )
    .await
    .map_err(|_| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Freigabe konnte nicht gespeichert werden. Bitte lade den Stand neu.",
        )
    })?;
    Ok(([(CACHE_CONTROL, "no-store")], Json(change)).into_response())
}

pub async fn limits(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    limits_response(&pool).await
}

pub async fn usage(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    query: Result<Query<UsageFilter>, axum::extract::rejection::QueryRejection>,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let Query(filter) =
        query.map_err(|_| ApiError::bad_request("Die Verbrauchsfilter sind ungültig."))?;
    if !filter.validate() {
        return Err(ApiError::bad_request(
            "Die Verbrauchsfilter liegen außerhalb der erlaubten Grenzen.",
        ));
    }
    let data = admin_usage::usage_report(&pool, &filter)
        .await
        .map_err(|_| {
            failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "Die Verbrauchserfassung ist nicht verfügbar.",
            )
        })?;
    Ok(([(CACHE_CONTROL, "no-store")], Json(data)).into_response())
}

pub async fn save_global(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let change: GlobalControlChange = parse(&body)?;
    let actor = actor(&auth, state, session, partner).await?;
    admin_limits::save_global_control(&pool, &change, &actor)
        .await
        .map_err(limit_error)?;
    limits_response(&pool).await
}

pub async fn save_user(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let change: UserLimitChange = parse(&body)?;
    let actor = actor(&auth, state, session, partner).await?;
    admin_limits::save_user_limit(&pool, &change, &actor)
        .await
        .map_err(limit_error)?;
    limits_response(&pool).await
}

pub async fn save_plan(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    session: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let change: PlanLimitChange = parse(&body)?;
    let actor = actor(&auth, state, session, partner).await?;
    admin_limits::save_plan_limit(&pool, &change, &actor, &catalog())
        .await
        .map_err(limit_error)?;
    limits_response(&pool).await
}

#[cfg(test)]
#[path = "social_media_ai_admin_usage_tests.rs"]
mod tests;
