//! Streamer-Vorschläge aus der Discord-Community (Community-Streamer-Brücke,
//! Paket F, Twitch-Seite).
//!
//! - `POST /internal/twitch/v1/scout/community-suggestion`
//!   Body `{"twitch_login","suggested_by_discord_id","reason","idempotency_key"}`.
//!   Login wird per Helix auf die Twitch-User-ID aufgelöst. Antwort
//!   `{"status":"created"|"already_known"|"already_partner"|"blocked"|"not_found","twitch_user_id":...}`.
//!   Der Kanal landet als Scout-Kandidat (`vorgeschlagen`, Quelle `community`);
//!   die Admin-Freigabe bleibt der einzige Weg zur Outreach.
//! - `GET /internal/twitch/v1/scout/community-suggestions/outcomes?updated_since=&limit=`
//!   je Community-Kandidat erster Vorschlagender und Partnerstand (Paket C).
//!
//! Auth wie alle internen Routen: `X-Internal-Token` + Loopback (Router-Layer).

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_http_core::ApiError;
use tb_scout::community::{
    gueltige_discord_id, gueltiger_idempotency_key, liste_ergebnisse, normalisiere_vorschlag_login,
    vorschlag_einreichen, vorschlag_wiederholen, VorschlagEingabe, VorschlagFehler,
    VorschlagStatus,
};
use tb_transport_twitch::HelixClient;

use super::community_points::parse_cursor_params;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionBody {
    pub twitch_login: String,
    pub suggested_by_discord_id: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub idempotency_key: String,
}

/// Geprüfte Eingabe ohne Helix-Auflösung.
#[derive(Debug, PartialEq, Eq)]
pub struct GepruefterVorschlag {
    pub login: String,
    pub discord_id: String,
    pub reason: Option<String>,
    pub idempotency_key: String,
}

pub fn pruefe_body(body: SuggestionBody) -> Option<GepruefterVorschlag> {
    let login = normalisiere_vorschlag_login(&body.twitch_login)?;
    let discord_id = body.suggested_by_discord_id.trim().to_string();
    let idempotency_key = body.idempotency_key.trim().to_string();
    (gueltige_discord_id(&discord_id) && gueltiger_idempotency_key(&idempotency_key)).then_some(
        GepruefterVorschlag {
            login,
            discord_id,
            reason: body.reason,
            idempotency_key,
        },
    )
}

fn antwort(status: VorschlagStatus, twitch_user_id: Option<&str>) -> Response {
    (
        StatusCode::OK,
        Json(json!({"status": status.as_str(), "twitch_user_id": twitch_user_id})),
    )
        .into_response()
}

fn fehler(code: StatusCode, error: &str, message: &str) -> Response {
    (code, Json(json!({"error": error, "message": message}))).into_response()
}

/// `POST /internal/twitch/v1/scout/community-suggestion`
pub async fn suggestion_handler(
    State(pool): State<PgPool>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    body: Result<Json<SuggestionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return fehler(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Body muss twitch_login, suggested_by_discord_id und idempotency_key enthalten",
        );
    };
    let Some(eingabe) = pruefe_body(body) else {
        return fehler(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "twitch_login, suggested_by_discord_id oder idempotency_key ungültig",
        );
    };
    match vorschlag_wiederholen(
        &pool,
        &eingabe.idempotency_key,
        &eingabe.login,
        &eingabe.discord_id,
        eingabe.reason.as_deref(),
    )
    .await
    {
        Ok(Some((status, user_id))) => return antwort(status, Some(&user_id)),
        Ok(None) => {}
        Err(error) => return vorschlag_fehler(error),
    }
    let Some(helix) = helix.as_ref().as_ref() else {
        return fehler(
            StatusCode::SERVICE_UNAVAILABLE,
            "twitch_unavailable",
            "Twitch-Anbindung nicht verfügbar",
        );
    };
    let user = match helix.get_users(&[eingabe.login.as_str()]).await {
        Ok(users) => users
            .get(&eingabe.login)
            .map(|u| (u.id.trim().to_string(), u.login.trim().to_ascii_lowercase())),
        Err(error) => {
            tracing::warn!(%error, "Community-Vorschlag: Helix-Abfrage fehlgeschlagen");
            return fehler(
                StatusCode::SERVICE_UNAVAILABLE,
                "twitch_unavailable",
                "Twitch antwortet gerade nicht",
            );
        }
    };
    let Some((user_id, _user_login)) =
        user.filter(|(id, login)| !id.is_empty() && !login.is_empty())
    else {
        return antwort(VorschlagStatus::NotFound, None);
    };
    let ergebnis = vorschlag_einreichen(
        &pool,
        &VorschlagEingabe {
            twitch_user_id: user_id.clone(),
            twitch_login: eingabe.login,
            discord_id: eingabe.discord_id,
            grund: eingabe.reason,
            idempotency_key: eingabe.idempotency_key,
        },
    )
    .await;
    match ergebnis {
        Ok(status) => {
            tracing::info!(
                status = status.as_str(),
                twitch_user_id = %user_id,
                "Community-Vorschlag verarbeitet"
            );
            antwort(status, Some(&user_id))
        }
        Err(error) => vorschlag_fehler(error),
    }
}

fn vorschlag_fehler(error: VorschlagFehler) -> Response {
    match error {
        VorschlagFehler::Konflikt => fehler(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "idempotency_key wurde schon für einen anderen Vorschlag benutzt",
        ),
        VorschlagFehler::IdentitaetUngeklaert => fehler(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_unresolved",
            "Die gespeicherte Kanalidentität muss zuerst geprüft werden",
        ),
        VorschlagFehler::Db(error) => {
            tracing::error!(%error, "Community-Vorschlag konnte nicht gespeichert werden");
            ApiError::internal().into_response()
        }
    }
}

/// `GET /internal/twitch/v1/scout/community-suggestions/outcomes`
pub async fn outcomes_handler(
    State(pool): State<PgPool>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let cursor = parse_cursor_params(&params)?;
    let seite = liste_ergebnisse(&pool, cursor.since, cursor.limit)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Vorschlags-Ergebnisse konnten nicht gelesen werden");
            ApiError::internal()
        })?;
    Ok(Json(seite))
}

#[cfg(test)]
#[path = "scout_community_tests.rs"]
mod tests;
