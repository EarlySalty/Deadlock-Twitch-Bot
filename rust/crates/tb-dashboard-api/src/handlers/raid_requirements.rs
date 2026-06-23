//! Native Dashboard-Route fuer `GET /twitch/raid/requirements`.
//!
//! Python-Vertrag (`bot/dashboard/raids/raid_mixin.py`): GET mit `?login=...`,
//! Admin darf jeden aktiven Partner triggern, Partner nur den eigenen Login.
//! Erfolgreich wird eine Discord-DM mit den Raid-OAuth-Anforderungen gesendet
//! und anschliessend nach `/twitch/admin?ok=...` redirectet. Die eigentliche
//! DM-/OAuth-Logik lebt im Bot-/Internal-API-Prozess und ist dort als
//! `POST /internal/twitch/v1/raid/requirements` verfuegbar.

use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_domain::login::normalize_twitch_login;
use tb_http_core::{INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER};

use super::legacy_form::redirect_with;
use crate::auth::level::DashboardAuthLevel;

const ADMIN_PATH: &str = "/twitch/admin";

#[derive(Debug, Deserialize, Default)]
pub struct RaidRequirementsQuery {
    #[serde(default)]
    pub login: Option<String>,
}

/// `GET /twitch/raid/requirements?login=<login>`
pub async fn raid_requirements_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<RaidRequirementsQuery>,
) -> Response {
    let Some(login) = normalize_twitch_login(q.login.as_deref().unwrap_or("")) else {
        return (StatusCode::BAD_REQUEST, "Missing login parameter").into_response();
    };

    if !auth.is_authenticated() {
        return (StatusCode::UNAUTHORIZED, "Dashboard session required").into_response();
    }
    if let Some(session_login) = partner_login(&auth) {
        if session_login != login {
            return (StatusCode::FORBIDDEN, "Forbidden streamer scope").into_response();
        }
    }

    let Some(canonical_login) = load_active_partner_login(&pool, &login).await else {
        return (StatusCode::NOT_FOUND, "Streamer not found").into_response();
    };

    match send_requirements(&canonical_login).await {
        RequirementsResult::Sent(message) => {
            redirect_with(ADMIN_PATH, "ok", &message).into_response()
        }
        RequirementsResult::BadRequest(message) => {
            (StatusCode::BAD_REQUEST, message).into_response()
        }
        RequirementsResult::Forbidden(message) => {
            (StatusCode::FORBIDDEN, message).into_response()
        }
        RequirementsResult::NotFound(message) => {
            (StatusCode::NOT_FOUND, message).into_response()
        }
        RequirementsResult::Unavailable(message) => {
            (StatusCode::SERVICE_UNAVAILABLE, message).into_response()
        }
        RequirementsResult::Failed(status, message) => (status, message).into_response(),
    }
}

fn partner_login(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            normalize_twitch_login(twitch_login)
        }
        _ => None,
    }
}

async fn load_active_partner_login(pool: &PgPool, login: &str) -> Option<String> {
    match sqlx::query_scalar::<_, String>(
        "SELECT twitch_login FROM twitch_partners \
         WHERE LOWER(twitch_login) = LOWER($1) \
           AND COALESCE(status, 'active') = 'active' \
         LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
    {
        Ok(row) => row.and_then(|value| normalize_twitch_login(&value)),
        Err(e) => {
            tracing::error!("raid requirements Partner-Lookup fehlgeschlagen: {e}");
            None
        }
    }
}

enum RequirementsResult {
    Sent(String),
    BadRequest(String),
    Forbidden(String),
    NotFound(String),
    Unavailable(String),
    Failed(StatusCode, String),
}

async fn send_requirements(login: &str) -> RequirementsResult {
    let Some(token) = nonempty_env("TWITCH_INTERNAL_API_TOKEN") else {
        return RequirementsResult::Unavailable("Raid bot not initialized".to_string());
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("raid requirements HTTP-Client konnte nicht gebaut werden: {e}");
            return RequirementsResult::Unavailable("Raid bot not initialized".to_string());
        }
    };

    let url = format!(
        "{}{}/raid/requirements",
        internal_base_url(),
        INTERNAL_API_BASE_PATH
    );
    let resp = match client
        .post(url)
        .header(INTERNAL_TOKEN_HEADER, token)
        .json(&json!({ "login": login }))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::warn!("raid requirements Internal-API nicht erreichbar: {e}");
            return RequirementsResult::Unavailable("Raid bot not initialized".to_string());
        }
    };

    let status = resp.status();
    let body = resp.json::<Value>().await.unwrap_or(Value::Null);
    if status.is_success() {
        let message = body
            .get("message")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("sent");
        return RequirementsResult::Sent(message.to_string());
    }

    let message = body
        .get("message")
        .or_else(|| body.get("error"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Raid bot not initialized")
        .to_string();

    match status {
        StatusCode::BAD_REQUEST => RequirementsResult::BadRequest(message),
        StatusCode::FORBIDDEN => RequirementsResult::Forbidden(message),
        StatusCode::NOT_FOUND => RequirementsResult::NotFound(message),
        StatusCode::SERVICE_UNAVAILABLE => RequirementsResult::Unavailable(message),
        other => RequirementsResult::Failed(other, message),
    }
}

fn internal_base_url() -> String {
    if let Some(explicit) = nonempty_env("TWITCH_INTERNAL_API_BASE_URL") {
        return explicit.trim_end_matches('/').to_string();
    }
    let host = nonempty_env("TWITCH_INTERNAL_API_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = nonempty_env("TWITCH_INTERNAL_API_PORT").unwrap_or_else(|| "8776".to_string());
    format!("http://{host}:{port}")
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
