//! Schreibpfad für Clip-Einreichungen aus einer verifizierten Dashboard-Session.
//! Der Router schützt diesen Pfad mit Dienst-Token und Loopback-Prüfung.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tb_chat::clip_contest_submit::{
    canonical_clip_url, valid_twitch_id, ClipContestSubmitter, SubmitOutcome, SubmitRequest,
    SubmitVia,
};

#[derive(Clone)]
pub struct ClipContestExt(pub Option<Arc<ClipContestSubmitter>>);

/// Die Akteur-ID setzt ausschließlich die verifizierte Dashboard-Auth-Schicht.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardClipRequest {
    pub clip_db_id: i64,
    pub actor_twitch_user_id: String,
}

pub fn outcome_response(outcome: &SubmitOutcome) -> Response {
    let status = match outcome {
        SubmitOutcome::Accepted { .. } | SubmitOutcome::AlreadyIn { .. } => StatusCode::OK,
        SubmitOutcome::Rejected { .. }
        | SubmitOutcome::InvalidUrl
        | SubmitOutcome::NoRecentClip => StatusCode::UNPROCESSABLE_ENTITY,
        SubmitOutcome::ClipNotFound => StatusCode::NOT_FOUND,
        SubmitOutcome::ForeignClip | SubmitOutcome::NotPartner => StatusCode::FORBIDDEN,
        SubmitOutcome::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        SubmitOutcome::InFlight => StatusCode::CONFLICT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    let clip_url = match outcome {
        SubmitOutcome::Accepted { clip_url, .. } | SubmitOutcome::AlreadyIn { clip_url } => {
            Some(clip_url)
        }
        _ => None,
    };
    (status, Json(json!({"status": outcome.code(), "message": outcome.reply().unwrap_or_else(|| "Die Einreichung läuft schon.".into()), "clip_url": clip_url}))).into_response()
}

pub async fn submit_handler(
    State(pool): State<PgPool>,
    submitter: Option<Extension<ClipContestExt>>,
    body: Result<Json<DashboardClipRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if body.clip_db_id <= 0 || !valid_twitch_id(&body.actor_twitch_user_id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let row: Option<(String, Option<String>, String)> = match sqlx::query_as(
        "SELECT clip_id, twitch_user_id, source_kind FROM twitch_clips_social_media WHERE id = $1",
    )
    .bind(body.clip_db_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(error) => {
            tracing::error!(%error, "Clip-Contest: Producer kann Clip nicht lesen");
            return outcome_response(&SubmitOutcome::StoreUnavailable);
        }
    };
    let Some((clip_id, owner_id, source)) = row else {
        return outcome_response(&SubmitOutcome::ClipNotFound);
    };
    if owner_id.as_deref() != Some(body.actor_twitch_user_id.as_str()) {
        return outcome_response(&SubmitOutcome::ForeignClip);
    }
    if source != "twitch" {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "status": "not_twitch_clip",
                "message": "Nur Twitch-Clips können am Wochen-Contest teilnehmen.",
                "clip_url": null,
            })),
        )
            .into_response();
    }
    let Some(Extension(ClipContestExt(Some(submitter)))) = submitter else {
        return outcome_response(&SubmitOutcome::BrokerUnavailable);
    };
    // Der gemeinsame Dienst prüft aktive Partnerschaft anhand der ID,
    // Helix-Ownership, Tageslimit und dauerhaft gespeicherte Idempotenz.
    let outcome = submitter
        .submit(SubmitRequest {
            broadcaster_id: body.actor_twitch_user_id.clone(),
            broadcaster_login: String::new(),
            submitted_by: Some(body.actor_twitch_user_id),
            clip_url: Some(canonical_clip_url(&clip_id)),
            via: SubmitVia::Dashboard,
        })
        .await;
    outcome_response(&outcome)
}

#[cfg(test)]
#[path = "clip_contest_tests.rs"]
mod tests;
