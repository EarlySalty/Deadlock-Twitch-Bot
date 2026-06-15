//! Stub-Handler für Routen die früher an den Python-Legacy-Proxy weitergeleitet
//! wurden. Python läuft nicht mehr — diese Handler ersetzen die Legacy-Routen.

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::Deserialize;

use crate::handlers::streamers::{ChatActionExt, ChatActionResult};
use tb_domain::normalize_twitch_login;
use tb_http_core::AuthLevel;

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

/// Body von `POST /streamers/:login/chat-action` (Python `streamer_chat_action`,
/// routes/streamers.py:470). `mode` default `message`, `color` default `purple`,
/// `message` ist Pflicht (leer → 400).
#[derive(Deserialize, Default)]
pub struct ChatActionBody {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Owner-Chat-Action: sendet eine Nachricht/Ankündigung über den live rotierten
/// Bot-User-Token (Bot-Token-Bridge, [`ChatActionPort`]). Ohne Port (Chat aus /
/// Token nicht gebootet) → 503 statt stummen Scheiterns. Twitch-Drops
/// (Stummschaltung, Channel-Settings) werden als `ok=false` durchgereicht, NIE
/// als Erfolg gefälscht (Python-Parität).
pub async fn chat_action_handler(
    auth: AuthLevel,
    Path(login): Path<String>,
    Extension(ChatActionExt(port)): Extension<ChatActionExt>,
    body: Option<Json<ChatActionBody>>,
) -> impl IntoResponse {
    if !auth.is_privileged() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"ok": false, "error": "unauthorized"})),
        );
    }

    let Some(login) = normalize_twitch_login(&login) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"ok": false, "error": "invalid login"})),
        );
    };

    let body = body.map(|Json(b)| b).unwrap_or_default();
    let mode = body
        .mode
        .as_deref()
        .map(|m| m.trim().to_lowercase())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "message".to_string());
    let color = body
        .color
        .as_deref()
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "purple".to_string());
    let message = body.message.as_deref().map(str::trim).unwrap_or_default();
    if message.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "ok": false, "login": login, "error": "message is required"
            })),
        );
    }

    let Some(port) = port else {
        tracing::warn!(login, "chat-action ohne ChatActionPort — Bot-Token-Bridge nicht aktiv");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "ok": false,
                "login": login,
                "error": "Bot-Token nicht verfügbar — nativer Chat ist aus (TB_CHAT_ENABLED)."
            })),
        );
    };

    match port.send_chat_action(&login, &mode, &color, message).await {
        ChatActionResult::Sent { label } => (
            StatusCode::OK,
            Json(serde_json::json!({"ok": true, "login": login, "message": label})),
        ),
        ChatActionResult::Dropped { code, message: drop_msg } => {
            tracing::info!(login, code, "chat-action von Twitch verworfen");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": false,
                    "login": login,
                    "message": format!("Chat-Aktion für {login} konnte nicht gesendet werden"),
                    "drop_reason": {"code": code, "message": drop_msg}
                })),
            )
        }
        ChatActionResult::UnknownChannel => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": false,
                "login": login,
                "message": format!("Für {login} fehlt die Twitch User-ID")
            })),
        ),
        ChatActionResult::Failed { reason } => {
            tracing::warn!(login, reason, "chat-action fehlgeschlagen");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": false,
                    "login": login,
                    "message": format!("Chat-Aktion für {login} konnte nicht gesendet werden")
                })),
            )
        }
    }
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

// ---------------------------------------------------------------------------
// Tests — chat-action gegen einen Fake-ChatActionPort (kein DB/Helix)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod chat_action_tests {
    use super::*;
    use crate::handlers::streamers::{ChatActionPort, ChatActionResult};
    use axum::response::IntoResponse;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// Fake-Port: zählt Aufrufe, merkt sich die letzten Argumente und gibt ein
    /// vorgegebenes Ergebnis zurück.
    struct FakePort {
        calls: AtomicUsize,
        last: Mutex<Option<(String, String, String, String)>>,
        result: Mutex<ChatActionResult>,
    }

    impl FakePort {
        fn new(result: ChatActionResult) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                last: Mutex::new(None),
                result: Mutex::new(result),
            })
        }
    }

    #[async_trait::async_trait]
    impl ChatActionPort for FakePort {
        async fn send_chat_action(
            &self,
            login: &str,
            mode: &str,
            color: &str,
            message: &str,
        ) -> ChatActionResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some((
                login.to_string(),
                mode.to_string(),
                color.to_string(),
                message.to_string(),
            ));
            self.result.lock().unwrap().clone()
        }
    }

    async fn run(
        port: Option<Arc<dyn ChatActionPort>>,
        login: &str,
        body: Option<serde_json::Value>,
        auth: AuthLevel,
    ) -> (StatusCode, serde_json::Value) {
        let json_body = body.map(|v| Json(serde_json::from_value(v).unwrap()));
        let resp = chat_action_handler(
            auth,
            Path(login.to_string()),
            Extension(ChatActionExt(port)),
            json_body,
        )
        .await
        .into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, value)
    }

    #[tokio::test]
    async fn sendet_genau_einmal_und_meldet_ok() {
        let port = FakePort::new(ChatActionResult::Sent {
            label: "Nachricht an nani gesendet".to_string(),
        });
        let (status, body) = run(
            Some(port.clone()),
            "Nani",
            Some(serde_json::json!({"message": "Hallo Chat"})),
            AuthLevel::Admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], serde_json::json!(true));
        assert_eq!(port.calls.load(Ordering::SeqCst), 1, "genau 1 Send erwartet");
        let last = port.last.lock().unwrap().clone().unwrap();
        // Login normalisiert (lowercase), Defaults message/purple.
        assert_eq!(last.0, "nani");
        assert_eq!(last.1, "message");
        assert_eq!(last.2, "purple");
        assert_eq!(last.3, "Hallo Chat");
    }

    #[tokio::test]
    async fn dropped_wird_als_ok_false_durchgereicht_nicht_gefaket() {
        let port = FakePort::new(ChatActionResult::Dropped {
            code: "channel_settings".to_string(),
            message: "Blocked".to_string(),
        });
        let (status, body) = run(
            Some(port),
            "nani",
            Some(serde_json::json!({"message": "x"})),
            AuthLevel::Admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], serde_json::json!(false), "Drop darf NIE ok=true sein");
        assert_eq!(body["drop_reason"]["code"], serde_json::json!("channel_settings"));
    }

    #[tokio::test]
    async fn leere_message_gibt_400_ohne_port_aufruf() {
        let port = FakePort::new(ChatActionResult::Sent {
            label: "x".to_string(),
        });
        let (status, _body) = run(
            Some(port.clone()),
            "nani",
            Some(serde_json::json!({"message": "   "})),
            AuthLevel::Admin,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ohne_port_gibt_503() {
        let (status, body) = run(
            None,
            "nani",
            Some(serde_json::json!({"message": "x"})),
            AuthLevel::Admin,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["ok"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn ohne_auth_gibt_401() {
        let port = FakePort::new(ChatActionResult::Sent {
            label: "x".to_string(),
        });
        let (status, _body) = run(
            Some(port.clone()),
            "nani",
            Some(serde_json::json!({"message": "x"})),
            AuthLevel::None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn announcement_mode_und_color_werden_durchgereicht() {
        let port = FakePort::new(ChatActionResult::Sent {
            label: "Announcement an nani gesendet".to_string(),
        });
        let (status, body) = run(
            Some(port.clone()),
            "nani",
            Some(serde_json::json!({"mode": "Announcement", "color": "Blue", "message": "Hi"})),
            AuthLevel::Admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], serde_json::json!(true));
        let last = port.last.lock().unwrap().clone().unwrap();
        assert_eq!(last.1, "announcement");
        assert_eq!(last.2, "blue");
    }
}
