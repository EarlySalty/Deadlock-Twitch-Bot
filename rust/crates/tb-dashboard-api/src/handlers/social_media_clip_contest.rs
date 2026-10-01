//! `POST /social-media/api/clips/{clip_db_id}/clip-contest`: Knopf
//! "Für Clip-Contest einreichen" im Social-Studio (Community-Streamer-Brücke,
//! Paket E).
//!
//! Gleicher Dienst wie der Chat-Befehl `!clipcontest`
//! ([`tb_chat::clip_contest_submit`]): Partnerprüfung, Helix-Prüfung,
//! Tageslimit je Kanal, Doppelsend-Schutz und Weitergabe an den Master-Broker
//! (`POST /internal/master/v1/clips/submit`). Der Broker-Zugang ist derselbe
//! `BrokerRelay` wie im tb-bot: Basis-URL aus der Betriebskonfiguration,
//! Token aus dem bestehenden Infisical-Vertrag (kein neues Secret).
//!
//! Zugriff wie die übrigen Clip-Aktionen: Social-Media-Freigabe, Clip im
//! eigenen Kanal, Partner-Freigabe-Guard.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_chat::clip_contest_submit::{
    canonical_clip_url, BrokerClipRequest, BrokerClipResponse, BrokerClipStatus, ClipContestBroker,
    ClipContestSubmitter, HelixClipLookup, SubmitOutcome, SubmitRequest, SubmitVia,
};
use tb_transport_discord::BrokerRelay;

use super::{
    guard_partner_access_for_clip, invalid_clip_db_id, normalize_id, partner_identity,
    require_clip_in_scope, require_sm_access,
};
use crate::auth::level::DashboardAuthLevel;

/// Token-Reihenfolge wie `tb_config::BrokerConfig` im tb-bot.
fn master_broker_token() -> Option<String> {
    [
        "MASTER_BROKER_TOKEN",
        "MAIN_BOT_INTERNAL_TOKEN",
        "TWITCH_INTERNAL_API_TOKEN",
    ]
    .into_iter()
    .find_map(crate::uplink_config::platform_value)
}

fn master_broker() -> Option<BrokerRelay> {
    let base_url = tb_config::runtime::settings().ok()?.broker.base_url.clone();
    let token = master_broker_token()?;
    BrokerRelay::new(&tb_config::BrokerConfig { base_url, token }).ok()
}

struct RelayBroker(BrokerRelay);

#[async_trait::async_trait]
impl ClipContestBroker for RelayBroker {
    async fn submit(&self, request: &BrokerClipRequest) -> Result<BrokerClipResponse, String> {
        let result = self
            .0
            .submit_twitch_clip(request, &request.idempotency_key)
            .await
            .map_err(|error| error.to_string())?;
        let status = match result.status.as_str() {
            "accepted" => BrokerClipStatus::Accepted,
            "duplicate" => BrokerClipStatus::Duplicate,
            "rejected" => BrokerClipStatus::Rejected,
            other => return Err(format!("unbekannter Clip-Contest-Status {other}")),
        };
        Ok(BrokerClipResponse {
            status,
            submission_id: result.submission_id,
            reason: result.reason,
        })
    }
}

/// HTTP-Status je Ergebnis. Fachliche Ergebnisse (angenommen, schon drin)
/// sind 200, damit das Frontend nur einen Erfolgsweg kennt.
pub(crate) fn outcome_status(outcome: &SubmitOutcome) -> StatusCode {
    match outcome {
        SubmitOutcome::Accepted { .. } | SubmitOutcome::AlreadyIn { .. } => StatusCode::OK,
        SubmitOutcome::Rejected { .. }
        | SubmitOutcome::InvalidUrl
        | SubmitOutcome::NoRecentClip => StatusCode::UNPROCESSABLE_ENTITY,
        SubmitOutcome::ClipNotFound => StatusCode::NOT_FOUND,
        SubmitOutcome::ForeignClip | SubmitOutcome::NotPartner => StatusCode::FORBIDDEN,
        SubmitOutcome::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        SubmitOutcome::InFlight => StatusCode::CONFLICT,
        SubmitOutcome::BrokerUnavailable
        | SubmitOutcome::TwitchUnavailable
        | SubmitOutcome::StoreUnavailable => StatusCode::SERVICE_UNAVAILABLE,
    }
}

pub(crate) fn outcome_response(outcome: &SubmitOutcome) -> Response {
    let clip_url = match outcome {
        SubmitOutcome::Accepted { clip_url, .. } | SubmitOutcome::AlreadyIn { clip_url } => {
            Value::String(clip_url.clone())
        }
        _ => Value::Null,
    };
    let message = outcome
        .reply()
        .unwrap_or_else(|| "Die Einreichung läuft schon.".to_string());
    (
        outcome_status(outcome),
        Json(json!({
            "status": outcome.code(),
            "message": message,
            "clip_url": clip_url,
        })),
    )
        .into_response()
}

fn unavailable() -> Response {
    outcome_response(&SubmitOutcome::BrokerUnavailable)
}

/// `POST /social-media/api/clips/{clip_db_id}/clip-contest`
pub async fn submit_clip_contest_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(raw): Path<String>,
) -> Response {
    let scope = match require_sm_access(&auth, &pool, None).await {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(clip_db_id) = normalize_id(Some(&Value::String(raw))) else {
        return invalid_clip_db_id();
    };
    if let Err(e) = require_clip_in_scope(&pool, &auth, clip_db_id, scope.as_deref()).await {
        return e;
    }
    if let Some(guard_response) = guard_partner_access_for_clip(&pool, &auth, clip_db_id).await {
        return guard_response;
    }
    let row: Option<(String, Option<String>, String, String)> = match sqlx::query_as(
        "SELECT clip_id, twitch_user_id, streamer_login, source_kind
           FROM twitch_clips_social_media WHERE id = $1",
    )
    .bind(clip_db_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(error) => {
            tracing::warn!(%error, clip_db_id, "Clip-Contest: Clip nicht lesbar");
            return outcome_response(&SubmitOutcome::StoreUnavailable);
        }
    };
    let Some((clip_id, clip_owner_id, streamer_login, source_kind)) = row else {
        return super::clip_not_found();
    };
    if source_kind != "twitch" {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "status": "not_twitch_clip",
                "message": "Nur Twitch-Clips können am Wochen-Contest teilnehmen.",
                "clip_url": Value::Null,
            })),
        )
            .into_response();
    }
    // Partner reichen immer für den eigenen Kanal ein; Admins für den Kanal
    // des Clips. Helix prüft danach, dass der Clip wirklich dorthin gehört.
    let broadcaster_id = match partner_identity(&auth) {
        Some(id) => id.to_string(),
        None => match clip_owner_id.filter(|id| !id.trim().is_empty()) {
            Some(id) => id,
            None => return outcome_response(&SubmitOutcome::NotPartner),
        },
    };
    let Some(helix) = crate::uplink_config::runtime()
        .ok()
        .and_then(|runtime| runtime.helix.clone())
    else {
        return outcome_response(&SubmitOutcome::TwitchUnavailable);
    };
    let Some(relay) = master_broker() else {
        return unavailable();
    };
    let submitter = ClipContestSubmitter::new(
        pool.clone(),
        Arc::new(HelixClipLookup::new(helix)),
        Arc::new(RelayBroker(relay)),
    );
    let outcome = submitter
        .submit(SubmitRequest {
            broadcaster_id,
            broadcaster_login: streamer_login,
            submitted_by: partner_identity(&auth).map(str::to_string),
            clip_url: Some(canonical_clip_url(&clip_id)),
            via: SubmitVia::Dashboard,
        })
        .await;
    tracing::info!(
        clip_db_id,
        outcome = outcome.code(),
        "Clip-Contest aus dem Dashboard"
    );
    outcome_response(&outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn ergebnisse_werden_auf_http_und_text_abgebildet() {
        let (status, value) = body(outcome_response(&SubmitOutcome::Accepted {
            clip_url: "https://clips.twitch.tv/AbcDef".into(),
            submission_id: Some(1),
        }))
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            value,
            json!({
                "status": "accepted",
                "message": "Clip ist im Wochen-Contest, die Community stimmt im Discord ab.",
                "clip_url": "https://clips.twitch.tv/AbcDef"
            })
        );
        for (outcome, code) in [
            (SubmitOutcome::RateLimited, StatusCode::TOO_MANY_REQUESTS),
            (
                SubmitOutcome::BrokerUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (SubmitOutcome::ForeignClip, StatusCode::FORBIDDEN),
            (SubmitOutcome::InFlight, StatusCode::CONFLICT),
            (
                SubmitOutcome::Rejected {
                    reason: Some("not_partner".into()),
                },
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
        ] {
            let (status, value) = body(outcome_response(&outcome)).await;
            assert_eq!(status, code, "{outcome:?}");
            assert!(!value["message"].as_str().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn ohne_anmeldung_kein_zugriff() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid@127.0.0.1:1/none")
            .unwrap();
        let response =
            submit_clip_contest_handler(DashboardAuthLevel::None, State(pool), Path("1".into()))
                .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
