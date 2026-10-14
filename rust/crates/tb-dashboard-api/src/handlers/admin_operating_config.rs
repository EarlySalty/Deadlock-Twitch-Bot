use crate::auth::level::DashboardAuthLevel;
use axum::{
    body::Bytes,
    http::{header::CACHE_CONTROL, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tb_config::{
    editor::{inspect_catalog, load_saved, save_catalog, EditError, OperatingOptions, SavedConfig},
    file::FileError,
    settings_catalog::{CatalogError, CatalogResponse, ChangeRequest},
    BotConfigSnapshot,
};
use tb_http_core::ApiError;
use tb_social_media::ai_editor::EditorRuntime;

const SOCIAL_AI_INVALID: &str = "Die KI-Schnitt-Einstellungen bestehen die Gesamtprüfung nicht. Anbieter, Preise, Grenzwerte und Ordner müssen zusammenpassen.";
const CATALOG_REQUEST_INVALID: &str = "Die Änderungsliste ist leer, zu lang, enthält ein Feld doppelt oder ein nicht änderbares Feld.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    expected_revision: String,
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

async fn response(saved: SavedConfig) -> Result<axum::response::Response, ApiError> {
    let active = active()?;
    let bot = bot_fingerprint(active).await;
    Ok(([(axum::http::header::CACHE_CONTROL, "no-store")], Json(json!({
        "saved_revision": saved.revision,
        "saved_fingerprint": saved.snapshot.fingerprint(),
        "options": OperatingOptions::from(&saved.snapshot.settings().database),
        "services": [
            { "name": "Dashboard", "active_fingerprint": active.fingerprint(), "restart_required": active.fingerprint() != saved.snapshot.fingerprint() },
            { "name": "Twitch-Bot", "active_fingerprint": bot, "restart_required": bot.as_deref().map(|value| value != saved.snapshot.fingerprint()) },
        ],
        "activation": "restart_required",
    }))).into_response())
}

pub async fn get_handler(auth: DashboardAuthLevel) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let source = active()?.source().to_path_buf();
    let saved = tokio::task::spawn_blocking(move || load_saved(&source))
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
    if request.expected_revision.len() != 64
        || !request
            .expected_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError::bad_request(
            "Der erwartete Konfigurationsstand fehlt oder ist ungültig.",
        ));
    }
    let source = active()?.source().to_path_buf();
    let saved = tokio::task::spawn_blocking(move || {
        tb_config::editor::save(&source, &request.expected_revision, &request.options)
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(edit_failure)?;
    response(saved).await
}

fn edit_failure(error: EditError) -> ApiError {
    match error {
        EditError::Conflict => failure(StatusCode::CONFLICT, "Die Konfiguration wurde inzwischen geändert. Bitte neu laden und Änderungen erneut prüfen."),
        EditError::Busy => failure(StatusCode::CONFLICT, "Die Konfiguration wird gerade gespeichert. Bitte erneut versuchen."),
        EditError::Invalid(_) => ApiError::bad_request("Ein Betriebswert liegt außerhalb des erlaubten Bereichs."),
        EditError::UnsafeLocation => failure(StatusCode::SERVICE_UNAVAILABLE, "Die Betriebsdatei ist noch nicht sicher außerhalb des Deploy-Checkouts eingerichtet."),
        EditError::Io => ApiError::internal_with("Der Speicherstand konnte nicht bestätigt werden. Bitte neu laden, bevor du erneut speicherst."),
    }
}

fn structured(status: StatusCode, error: &str, message: &str, field: Option<&str>) -> ApiError {
    let mut body = json!({ "error": error, "message": message });
    if let Some(field) = field {
        body["path"] = json!(field);
        body["field"] = json!(field);
    }
    let mut failure = ApiError::bad_request_with_body(body);
    failure.status = status;
    failure
}

fn validation_failure(error: FileError) -> ApiError {
    let message = match error.field {
        Some("social_ai") => SOCIAL_AI_INVALID.to_owned(),
        Some("settings_catalog") => CATALOG_REQUEST_INVALID.to_owned(),
        _ => error.to_string(),
    };
    structured(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation",
        &message,
        error.field,
    )
}

fn settings_failure(error: EditError) -> ApiError {
    match error {
        EditError::Conflict => structured(
            StatusCode::CONFLICT,
            "stale_revision",
            CatalogError::StaleRevision.message(),
            None,
        ),
        EditError::Invalid(error) => validation_failure(error),
        other => edit_failure(other),
    }
}

fn check_candidate(source: &Path, candidate: &str) -> Result<(), EditError> {
    let snapshot = BotConfigSnapshot::parse(candidate, source)?;
    match snapshot.settings().social_ai.as_ref() {
        Some(section) => EditorRuntime::validate_packaged(section)
            .map_err(|_| EditError::Invalid(FileError::invalid("social_ai"))),
        None => Ok(()),
    }
}

fn save_and_inspect(source: &Path, request: &ChangeRequest) -> Result<CatalogResponse, EditError> {
    save_catalog(source, request, |candidate| {
        check_candidate(source, candidate)
    })?;
    inspect_catalog(source)
}

fn catalog_response(catalog: CatalogResponse) -> Response {
    ([(CACHE_CONTROL, "no-store")], Json(catalog)).into_response()
}

async fn read_settings(source: PathBuf) -> Result<Response, ApiError> {
    let catalog = tokio::task::spawn_blocking(move || inspect_catalog(&source))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| {
            failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "Die gespeicherte Betriebskonfiguration ist nicht lesbar oder ungültig.",
            )
        })?;
    Ok(catalog_response(catalog))
}

async fn write_settings(source: PathBuf, body: Bytes) -> Result<Response, ApiError> {
    let request: ChangeRequest = serde_json::from_slice(&body).map_err(|_| {
        structured(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Die Änderungsliste ist ungültig; Eingabewerte werden nicht ausgegeben.",
            None,
        )
    })?;
    let catalog = tokio::task::spawn_blocking(move || save_and_inspect(&source, &request))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(settings_failure)?;
    Ok(catalog_response(catalog))
}

pub async fn settings_get_handler(auth: DashboardAuthLevel) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    read_settings(active()?.source().to_path_buf()).await
}

pub async fn settings_save_handler(
    auth: DashboardAuthLevel,
    body: Bytes,
) -> Result<Response, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    write_settings(active()?.source().to_path_buf(), body).await
}

#[cfg(test)]
#[path = "admin_operating_config_settings_tests.rs"]
mod settings_tests;

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
        let payload = json!({ "expected_revision": "a".repeat(64), "options": { "pool_max": 10, "acquire_timeout_ms": 5000, "connect_timeout_seconds": 5, "model": "unapproved" } });
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
