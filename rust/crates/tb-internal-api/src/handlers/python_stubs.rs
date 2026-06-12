//! Stub-Handler für Routen die früher an den Python-Legacy-Proxy weitergeleitet
//! wurden. Python läuft nicht mehr — diese Handler ersetzen die Legacy-Routen.

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// GET /internal/twitch/v1/debug/observability
// ---------------------------------------------------------------------------

pub async fn observability_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "note": "Python process not running — Rust handles all bot state natively.",
        "processes": {}
    }))
}

// ---------------------------------------------------------------------------
// GET /internal/twitch/v1/debug/chatters/:login
// ---------------------------------------------------------------------------

pub async fn chatters_debug_handler(Path(login): Path<String>) -> impl IntoResponse {
    Json(serde_json::json!({
        "login": login,
        "chatters": [],
        "note": "Python process not running — chatter tracking lives in Rust."
    }))
}

// ---------------------------------------------------------------------------
// POST /internal/twitch/v1/eventsub/processing/requeue
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct RequeueBody {
    #[serde(default)]
    work_id: Option<String>,
}

pub async fn eventsub_requeue_handler(
    body: Option<Json<RequeueBody>>,
) -> impl IntoResponse {
    let work_id = body.and_then(|b| b.work_id.clone()).unwrap_or_default();
    Json(serde_json::json!({
        "ok": true,
        "requeued": 0,
        "work_id": work_id,
        "note": "Rust processes EventSub events natively — manual requeue not needed."
    }))
}

// ---------------------------------------------------------------------------
// POST /internal/twitch/v1/streamers/:login/chat-action
// ---------------------------------------------------------------------------

/// Chat-Action braucht den rotierten Bot-User-Token (verwaltete von tb-bot).
/// Ohne Bot-Token-Bridge gibt 503 zurück statt stumm zu scheitern.
pub async fn chat_action_handler(Path(login): Path<String>) -> impl IntoResponse {
    tracing::warn!(
        login,
        "chat-action aufgerufen — Bot-Token nicht in tb-internal-api verfügbar"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "ok": false,
            "error": "Bot-Token nicht verfügbar. Chat-Action erfordert tb-bot Bridge — noch nicht implementiert.",
            "login": login
        })),
    )
}

// ---------------------------------------------------------------------------
// POST /internal/twitch/v1/raid/requirements
// ---------------------------------------------------------------------------

/// Login kommt als JSON-Body `{"login": "..."}` (s. http_client.py:651).
#[derive(Deserialize, Default)]
pub struct RaidRequirementsBody {
    #[serde(default)]
    pub login: Option<String>,
}

/// Discord-DM-Versand für Raid-Anforderungen fehlt ohne Python Discord-Bot.
pub async fn raid_requirements_handler(
    body: Option<Json<RaidRequirementsBody>>,
) -> impl IntoResponse {
    let login = body
        .and_then(|b| b.login.clone())
        .unwrap_or_else(|| "unknown".to_string());
    tracing::warn!(
        login,
        "raid/requirements aufgerufen — Discord-DM noch nicht via Rust implementiert"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "ok": false,
            "error": "Discord DM nicht verfügbar — Python Bridge entfernt. Zu implementieren via Master-Broker (8770).",
            "login": login
        })),
    )
}
