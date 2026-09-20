//! Typisierte Betriebsoptionen hinter bestehender Admin- und CSRF-Prüfung.
//! Der Bot-Status stammt von dessen Health-API, nie vom Dashboard-Snapshot.

use crate::auth::level::DashboardAuthLevel;
use axum::{body::Bytes, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use tb_config::{
    editor::{EditError, OperatingOptions},
    BotConfigSnapshot,
};
use tb_http_core::ApiError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    expected_fingerprint: String,
    options: OperatingOptions,
    #[serde(default, rename = "csrf_token")]
    _csrf_token: Option<String>,
}

fn failure(status: StatusCode, message: &'static str) -> ApiError {
    let mut error = ApiError::bad_request(message);
    error.status = status;
    error
}

fn active() -> Result<&'static BotConfigSnapshot, ApiError> {
    tb_config::runtime::active().ok_or_else(|| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die zentrale Betriebskonfiguration ist in diesem Dienst noch nicht aktiv.",
        )
    })
}

async fn bot_fingerprint(active: &BotConfigSnapshot) -> Option<String> {
    let token = std::env::var("TWITCH_INTERNAL_API_TOKEN").ok()?;
    if token.trim().is_empty() {
        return None;
    }
    let config = &active.settings().internal_api;
    let url = format!(
        "http://{}/internal/twitch/v1/healthz",
        std::net::SocketAddr::new(config.host, config.port)
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let response = client
        .get(url)
        .header("X-Internal-Token", token)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = crate::uplink_config::bounded_body(response, 16 * 1024)
        .await
        .ok()?;
    let payload: Value = serde_json::from_slice(&bytes).ok()?;
    let fingerprint = payload.get("configFingerprint")?.as_str()?;
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(fingerprint.to_string())
}

async fn response(saved: BotConfigSnapshot) -> Result<Json<Value>, ApiError> {
    let active = active()?;
    let bot = bot_fingerprint(active).await;
    Ok(Json(json!({
        "saved_fingerprint": saved.fingerprint(),
        "options": OperatingOptions::from(&saved.settings().database),
        "services": [
            { "name": "Dashboard", "active_fingerprint": active.fingerprint(), "restart_required": active.fingerprint() != saved.fingerprint() },
            { "name": "Twitch-Bot", "active_fingerprint": bot, "restart_required": bot.as_deref().map(|value| value != saved.fingerprint()) },
        ],
        "activation": "restart_required",
    })))
}

pub async fn get_handler(auth: DashboardAuthLevel) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let source = active()?.source().to_path_buf();
    let saved = tokio::task::spawn_blocking(move || BotConfigSnapshot::load(&source))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| {
            failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "Die gespeicherte Betriebskonfiguration ist nicht lesbar oder ungültig.",
            )
        })?;
    response(saved).await
}

pub async fn save_handler(
    auth: DashboardAuthLevel,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    if body.len() > 4096 {
        return Err(ApiError::bad_request("Die Anfrage ist zu groß."));
    }
    let request: EditRequest = serde_json::from_slice(&body).map_err(|_| {
        ApiError::bad_request(
            "Die Betriebsoptionen sind ungültig; Eingabewerte werden nicht ausgegeben.",
        )
    })?;
    if request.expected_fingerprint.len() != 64
        || !request
            .expected_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError::bad_request(
            "Der erwartete Konfigurationsstand fehlt oder ist ungültig.",
        ));
    }
    let source = active()?.source().to_path_buf();
    let saved = tokio::task::spawn_blocking(move || tb_config::editor::save(&source, &request.expected_fingerprint, &request.options))
        .await.map_err(|_| ApiError::internal())?
        .map_err(|error| match error {
            EditError::Conflict => failure(StatusCode::CONFLICT, "Die Konfiguration wurde inzwischen geändert. Bitte neu laden und Änderungen erneut prüfen."),
            EditError::Busy => failure(StatusCode::CONFLICT, "Die Konfiguration wird gerade gespeichert. Bitte erneut versuchen."),
            EditError::Invalid(_) => ApiError::bad_request("Ein Betriebswert liegt außerhalb des erlaubten Bereichs."),
            EditError::UnsafeLocation => failure(StatusCode::SERVICE_UNAVAILABLE, "Die Betriebsdatei ist noch nicht sicher außerhalb des Deploy-Checkouts eingerichtet."),
            EditError::Io => ApiError::internal_with("Der Speicherstand konnte nicht bestätigt werden. Bitte neu laden, bevor du erneut speicherst."),
        })?;
    response(saved).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unauthentifiziert_ohne_dateizugriff_abgewiesen() {
        assert_eq!(
            get_handler(DashboardAuthLevel::None)
                .await
                .into_response()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            save_handler(DashboardAuthLevel::None, Bytes::from("{}"))
                .await
                .into_response()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn fremde_felder_und_modellwechsel_abgewiesen() {
        let payload = json!({ "expected_fingerprint": "a".repeat(64), "options": { "pool_max": 10, "acquire_timeout_ms": 5000, "connect_timeout_seconds": 5, "model": "unapproved" } });
        assert_eq!(
            save_handler(
                DashboardAuthLevel::admin(),
                Bytes::from(payload.to_string())
            )
            .await
            .into_response()
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
