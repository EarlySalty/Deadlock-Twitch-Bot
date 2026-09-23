//! Admin-only, read-only bridge to the pinned Deadlock Brain Rust reasoner.
use crate::auth::level::DashboardAuthLevel;
use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use dbrain_reasoner::{
    lab::{self, LabRequest},
    ReasonerError,
};
use serde::Serialize;
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions, PgPool,
};
use std::{str::FromStr, time::Duration};
use tokio::sync::OnceCell;

static BRAIN_POOL: OnceCell<PgPool> = OnceCell::const_new();

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"error":code,"message":message})),
    )
        .into_response()
}
fn success(value: impl Serialize) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

async fn brain_pool() -> Result<PgPool, Box<Response>> {
    // A dedicated read-only DSN is preferred. Existing installations can use
    // the central DSN already supplied by their approved secret bootstrap.
    // Neither its value nor database errors are sent to the browser/logged.
    BRAIN_POOL.get_or_try_init(|| async {
        let dsn = std::env::var("DEADLOCK_BRAIN_READONLY_DSN").ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| std::env::var("DEADLOCK_CENTRAL_DSN").ok().filter(|v| !v.trim().is_empty()))
            .ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "brain_not_configured",
                "Brain ist noch nicht angebunden: Der Dashboard-Dienst benötigt einen autorisierten read-only Brain-Datenbankzugang."))?;
        let options = PgConnectOptions::from_str(&dsn)
            .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "brain_configuration_invalid", "Die Brain-Verbindung ist ungültig konfiguriert."))?
            .application_name("twitch-brain-build-lab")
            .options([("default_transaction_read_only", "on"), ("statement_timeout", "20000")])
            .disable_statement_logging();
        PgPoolOptions::new().max_connections(3).acquire_timeout(Duration::from_secs(8))
            .idle_timeout(Duration::from_secs(60)).connect_with(options).await
            .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "brain_unavailable", "Die Brain-Datenbank ist derzeit nicht erreichbar oder der Zugriff ist nicht freigegeben."))
    }).await.cloned().map_err(Box::new)
}

fn reasoner_error(err: ReasonerError) -> Response {
    match err {
        ReasonerError::HeroNotFound(_) => error(StatusCode::NOT_FOUND, "hero_not_found", "Der Held ist in Brain nicht vorhanden."),
        ReasonerError::MissingSnapshot(_) => error(StatusCode::UNPROCESSABLE_ENTITY, "snapshot_missing", "Für diesen Helden fehlen erforderliche Helden-, Waffen- oder Fähigkeitsdaten."),
        ReasonerError::Data(message) if message.starts_with("Build-Labor ist ausgelastet") =>
            error(StatusCode::TOO_MANY_REQUESTS, "brain_busy", "Beide Rechenplätze sind belegt. Bitte später erneut berechnen."),
        ReasonerError::Data(_) => error(StatusCode::UNPROCESSABLE_ENTITY, "brain_evidence_incomplete", "Die vorhandenen Daten erlauben keinen belastbaren Plan. Es wird kein Ersatzbuild erfunden."),
        ReasonerError::Db(_) => error(StatusCode::SERVICE_UNAVAILABLE, "brain_data_unavailable", "Brain-Daten fehlen oder sind für diesen Dienst nicht lesbar. Import und Datenbankrechte prüfen."),
        ReasonerError::Ai(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "unexpected_ai_path", "Der KI-Pfad ist im Testlabor nicht erlaubt."),
    }
}

pub async fn catalog_handler(auth: DashboardAuthLevel) -> Response {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }
    let pool = match brain_pool().await {
        Ok(pool) => pool,
        Err(err) => return *err,
    };
    match lab::catalog(&pool).await {
        Ok(catalog) => success(catalog),
        Err(err) => reasoner_error(err),
    }
}

pub async fn build_handler(auth: DashboardAuthLevel, Json(input): Json<LabRequest>) -> Response {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }
    if let Err(err) = input.config() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_scenario",
            &err.to_string(),
        );
    }
    let pool = match brain_pool().await {
        Ok(pool) => pool,
        Err(err) => return *err,
    };
    match tokio::time::timeout(Duration::from_secs(150), lab::build(pool, input)).await {
        Ok(Ok(report)) => success(report),
        Ok(Err(err)) => reasoner_error(err),
        Err(_) => error(StatusCode::GATEWAY_TIMEOUT, "brain_timeout", "Die Berechnung hat das Antwortlimit erreicht. Ein noch rechnender Auftrag hält seinen Rechenplatz bis zum Ende; es wurde nichts veröffentlicht."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> LabRequest {
        serde_json::from_value(json!({"hero":"Warden"})).unwrap()
    }
    #[tokio::test]
    async fn anonymous_is_rejected_before_any_brain_access() {
        assert_eq!(
            catalog_handler(DashboardAuthLevel::None).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            build_handler(DashboardAuthLevel::None, Json(request()))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn partner_is_rejected_before_any_brain_access() {
        let auth = DashboardAuthLevel::Partner {
            twitch_login: "test".into(),
            twitch_user_id: "123".into(),
            display_name: "Test".into(),
        };
        assert_eq!(
            catalog_handler(auth.clone()).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            build_handler(auth, Json(request())).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn invalid_scenario_is_rejected_before_connection() {
        let mut input = request();
        input.channel_uptime = 20.0;
        let response = build_handler(DashboardAuthLevel::admin(), Json(input)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    #[test]
    fn overloaded_reasoner_is_not_a_server_error() {
        assert_eq!(
            reasoner_error(ReasonerError::Data(
                "Build-Labor ist ausgelastet; bitte erneut versuchen.".into()
            ))
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    #[tokio::test]
    async fn database_error_details_are_not_disclosed() {
        let response = reasoner_error(ReasonerError::Data("private source payload".into()));
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private source payload"));
    }
}
