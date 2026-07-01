//! Native Cutover-Stubs für bewusst nicht mehr bediente Legacy-Pfade.
//!
//! Diese Handler ersetzen den Python-Fallback für alte URLs, ohne die entfernten
//! Python-Features neu zu bauen.

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde_json::json;

/// `GET /twitch/raid/callback` — alter Python-Alias.
///
/// Der echte Twitch-OAuth-Callback ist `/callback/twitch`.
pub async fn raid_callback_gone_handler() -> Response {
    (
        StatusCode::GONE,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        legacy_gone_page(
            "Raid-Callback entfernt",
            "Dieser alte Raid-OAuth-Callback wird nicht mehr verwendet. Der aktive Twitch-Callback ist /callback/twitch.",
        ),
    )
        .into_response()
}

/// `GET /twitch/raid/requirements` — ehemaliger Discord-DM-Start.
///
/// Der Discord-DM-Versand wurde im Cutover bewusst entfernt; die interne API
/// beantwortet den entsprechenden POST ebenfalls mit 410.
pub async fn raid_requirements_gone_handler() -> Response {
    (
        StatusCode::GONE,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        legacy_gone_page(
            "Raid-Anforderungen entfernt",
            "Diese Aktivierungs-DM wird im Rust-Cutover nicht mehr verschickt. Nutze die aktuelle Raid-Autorisierung im Dashboard.",
        ),
    )
        .into_response()
}

/// `GET /social-media-admin` und Unterpfade — Feature bewusst zurueckgestellt.
///
/// Kein SPA-Bundle und keine Clip-Logik; der alte Einstieg fällt nur sauber auf
/// das aktuelle Dashboard zurück.
pub async fn social_media_admin_stub_redirect_handler() -> Response {
    Redirect::to("/twitch/dashboard").into_response()
}

/// `/twitch/api/live-announcement/{config,preview,test}` — der alte Builder ist
/// im Rust-Cutover bewusst entfernt. Diese API darf nicht in den Python-Fallback
/// fallen, sondern terminiert nativ mit einem JSON-Tombstone.
pub async fn live_announcement_builder_gone_handler() -> Response {
    (
        StatusCode::GONE,
        Json(json!({
            "error": "live_announcement_builder_removed",
            "message": "Live announcement builder API has been removed.",
        })),
    )
        .into_response()
}

fn legacy_gone_page(title: &str, body: &str) -> String {
    format!(
        concat!(
            "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            "<title>{}</title></head>",
            "<body style=\"font-family:system-ui,sans-serif;max-width:38rem;margin:4rem auto;padding:0 1rem\">",
            "<h1>{}</h1><p>{}</p><p><a href=\"/twitch/dashboard\">Zum Dashboard</a></p>",
            "</body></html>"
        ),
        html_escape(title),
        html_escape(title),
        html_escape(body)
    )
}

fn html_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
