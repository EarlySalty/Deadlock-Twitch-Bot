//! `POST /social-media/api/clips/{clip_db_id}/clip-contest`: Knopf
//! "Für Clip-Contest einreichen" im Social-Studio (Community-Streamer-Brücke,
//! Paket E).
//!
//! Einreichungen gehen an die interne Bot-API. Der Dashboard-Pool liest nur.
//! Die Akteur-ID stammt aus einer echten Twitch-OAuth-Session; die Bot-API
//! prüft Kanalzugehörigkeit, aktive Partnerschaft, Helix und Idempotenz.

use super::{
    guard_partner_access_for_clip, invalid_clip_db_id, normalize_id, require_clip_in_scope,
    require_sm_access,
};
use crate::auth::level::{AuthenticatedPartnerSessionId, DashboardAuthLevel};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_chat::clip_contest_submit::{valid_twitch_id, SubmitOutcome};

/// Nur die Auth-Schicht kann die Extension nach einer echten Twitch-Session setzen.
fn twitch_actor<'a>(
    auth: &'a DashboardAuthLevel,
    session: Option<&AuthenticatedPartnerSessionId>,
) -> Option<&'a str> {
    session.filter(|s| !s.0.is_empty())?;
    match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => Some(twitch_user_id.as_str()),
        DashboardAuthLevel::Admin { actor: Some(actor) } => Some(actor.twitch_user_id.as_str()),
        _ => None,
    }
    .filter(|id| valid_twitch_id(id))
}

async fn submit_to_producer(
    url: &str,
    token: &str,
    clip_db_id: i64,
    actor: &str,
) -> Result<Response, ()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| ())?;
    let response = client
        .post(url)
        .header("X-Internal-Token", token)
        .json(&json!({"clip_db_id": clip_db_id, "actor_twitch_user_id": actor}))
        .send()
        .await
        .map_err(|_| ())?;
    let status = response.status();
    let bytes = crate::uplink_config::bounded_body(response, 16 * 1024)
        .await
        .map_err(|_| ())?;
    let payload: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if !payload["status"].is_string() || !payload["message"].is_string() {
        return Err(());
    }
    Ok((status, Json(payload)).into_response())
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
    session: Option<Extension<AuthenticatedPartnerSessionId>>,
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
    let Some(actor) = twitch_actor(&auth, session.as_ref().map(|s| &s.0)) else {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "status": "twitch_session_required",
                "message": "Bitte melde dich mit dem Twitch-Konto dieses Kanals an.",
                "clip_url": Value::Null,
            })),
        )
            .into_response();
    };
    let Ok(settings) = tb_config::runtime::settings() else {
        return unavailable();
    };
    let config = &settings.internal_api;
    if !config.host.is_loopback() {
        return unavailable();
    }
    let Some(token) = crate::uplink_config::brain_service_token().filter(|t| !t.trim().is_empty())
    else {
        return unavailable();
    };
    let url = format!(
        "http://{}/internal/twitch/v1/clips/contest/submit",
        std::net::SocketAddr::new(config.host, config.port)
    );
    match submit_to_producer(&url, &token, clip_db_id, actor).await {
        Ok(response) => response,
        Err(()) => unavailable(),
    }
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

    #[test]
    fn nur_echte_twitch_session_liefert_akteur_id() {
        let partner = DashboardAuthLevel::Partner {
            twitch_user_id: "456".into(),
            twitch_login: "anzeige".into(),
            display_name: "Anzeige".into(),
        };
        let session = AuthenticatedPartnerSessionId("synthetische-session".into());
        assert_eq!(twitch_actor(&partner, Some(&session)), Some("456"));
        assert_eq!(twitch_actor(&partner, None), None);
        let owner = DashboardAuthLevel::Admin {
            actor: Some(crate::auth::level::AdminActor {
                twitch_user_id: "456".into(),
                twitch_login: "owner".into(),
            }),
        };
        assert_eq!(twitch_actor(&owner, None), None);
        assert_eq!(twitch_actor(&owner, Some(&session)), Some("456"));
        assert_eq!(
            twitch_actor(&DashboardAuthLevel::None, Some(&session)),
            None
        );
    }

    #[tokio::test]
    async fn dashboard_uebermittelt_serveridentitaet_ueber_den_authentifizierten_httpweg() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/internal/twitch/v1/clips/contest/submit"))
            .and(header("X-Internal-Token","synthetischer-token"))
            .and(body_json(json!({"clip_db_id":42,"actor_twitch_user_id":"456"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"already_in","message":"Der Clip wurde bereits eingereicht und wird nicht erneut gesendet.","clip_url":"https://clips.twitch.tv/AbcDef"})))
            .expect(1).mount(&server).await;
        let response = submit_to_producer(
            &format!("{}/internal/twitch/v1/clips/contest/submit", server.uri()),
            "synthetischer-token",
            42,
            "456",
        )
        .await
        .unwrap();
        let (status, value) = body(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["status"], "already_in");
    }

    #[tokio::test]
    async fn ohne_anmeldung_kein_zugriff() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid@127.0.0.1:1/none")
            .unwrap();
        let response = submit_clip_contest_handler(
            DashboardAuthLevel::None,
            State(pool),
            Path("1".into()),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
