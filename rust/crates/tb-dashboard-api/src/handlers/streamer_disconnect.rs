//! `POST /twitch/api/v2/streamer/disconnect-bot`.
//!
//! Streamer-Selbstbedienung für „Bot vom Kanal trennen“ im Verwaltungs-Dashboard.
//! Fachlich identisch zur Admin-Route
//! (`/twitch/api/admin/streamers/{login}/disconnect-bot`) — dieselbe Kette in der
//! internen Bot-API, dieselbe Reihenfolge, derselbe Teilschritt-Report. Der
//! Unterschied ist ausschließlich, wer den Login bestimmt: hier die Session,
//! dort der Pfad-Parameter. Ein Partner kann damit niemanden außer sich selbst
//! trennen.
//!
//! Zwei Bestätigungsstufen wie im Admin-Pfad: das Frontend warnt zuerst, danach
//! muss der eigene Login abgetippt werden. Geprüft wird die Eingabe hier, nicht
//! nur im Browser.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::auth::level::DashboardAuthLevel;
use crate::handlers::admin_streamers::{call_internal_disconnect, confirmation_matches};

/// Login des Streamers, der sich selbst trennen will.
///
/// Ein per Twitch-OAuth eingeloggter Admin hat einen eigenen Kanal (`actor`) und
/// darf diesen Weg genauso nutzen — sonst käme genau der Betreiber im eigenen
/// Verwaltungs-Dashboard nicht an die Aktion. Ein Discord-Admin ohne
/// Twitch-Identität hat keinen eigenen Kanal; für fremde Kanäle gibt es die
/// Admin-Streamerseite mit Pfad-Login.
#[allow(clippy::result_large_err)]
fn resolve_self_login(auth: &DashboardAuthLevel) -> Result<String, Response> {
    let bad_request = |error: &str, message: &str| {
        Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error, "message": message })),
        )
            .into_response())
    };
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            let login = twitch_login.trim().to_lowercase();
            if login.is_empty() {
                return bad_request(
                    "no_login",
                    "Zu dieser Sitzung gehört kein Twitch-Kanal — nichts geändert.",
                );
            }
            Ok(login)
        }
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            let login = actor.twitch_login.trim().to_lowercase();
            if login.is_empty() {
                return bad_request(
                    "no_login",
                    "Zu dieser Sitzung gehört kein Twitch-Kanal — nichts geändert.",
                );
            }
            Ok(login)
        }
        DashboardAuthLevel::Admin { actor: None } => bad_request(
            "admin_session",
            "Admin-Sitzung ohne eigenen Twitch-Kanal — bitte die Admin-Streamerseite nutzen.",
        ),
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()),
    }
}

/// Body: `{"confirm_login": "<eigener login>"}`.
pub async fn post_handler(auth: DashboardAuthLevel, body: axum::body::Bytes) -> Response {
    let login = match resolve_self_login(&auth) {
        Ok(login) => login,
        Err(response) => return response,
    };
    let confirm = confirm_login_from_body(&body);
    if !confirmation_matches(&confirm, &login) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "confirmation_mismatch",
                "message": "Bestätigung stimmt nicht mit deinem Kanalnamen überein — nichts geändert.",
            })),
        )
            .into_response();
    }

    tracing::warn!(
        login = %login,
        source = "selfservice",
        "Bot-Trennung durch den Streamer selbst ausgelöst"
    );
    call_internal_disconnect(&login).await
}

/// `confirm_login` aus dem Body ziehen. Ein kaputter Body ist keine
/// Bestätigung — dann bleibt der Wert leer und der Aufruf scheitert an der
/// Prüfung, statt zu trennen.
fn confirm_login_from_body(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("confirm_login")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "12345".to_string(),
            display_name: login.to_string(),
        }
    }

    #[test]
    fn partner_session_liefert_eigenen_login_klein() {
        let login = resolve_self_login(&partner("EarlySalty")).expect("Partner erlaubt");
        assert_eq!(login, "earlysalty");
    }

    #[test]
    fn twitch_admin_darf_den_eigenen_kanal_trennen() {
        let auth = DashboardAuthLevel::Admin {
            actor: Some(crate::auth::level::AdminActor {
                twitch_user_id: "42".to_string(),
                twitch_login: "EarlySalty".to_string(),
            }),
        };
        assert_eq!(
            resolve_self_login(&auth).expect("Twitch-Admin hat einen eigenen Kanal"),
            "earlysalty"
        );
    }

    #[test]
    fn discord_admin_ohne_twitch_identitaet_wird_abgewiesen() {
        let err = resolve_self_login(&DashboardAuthLevel::admin())
            .expect_err("Discord-Admin hat keinen eigenen Kanal");
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn ohne_session_401() {
        let err = resolve_self_login(&DashboardAuthLevel::None).expect_err("keine Session");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn confirm_login_wird_aus_body_gelesen() {
        assert_eq!(
            confirm_login_from_body(br#"{"confirm_login":"earlysalty"}"#),
            "earlysalty"
        );
    }

    #[test]
    fn kaputter_body_bestaetigt_nichts() {
        assert_eq!(confirm_login_from_body(b"kein json"), "");
        assert_eq!(confirm_login_from_body(b"{}"), "");
        assert!(!confirmation_matches(
            &confirm_login_from_body(b"{}"),
            "earlysalty"
        ));
    }

    #[test]
    fn fremder_login_bestaetigt_nicht() {
        assert!(!confirmation_matches("andererkanal", "earlysalty"));
        assert!(confirmation_matches("EarlySalty", "earlysalty"));
    }
}
