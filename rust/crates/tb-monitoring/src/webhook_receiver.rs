//! Nativer EventSub-Webhook-Empfänger — ersetzt die Python-Bridge-Strecke
//! (Twitch → Caddy → 8765 Python → HTTP-Hop → 8776) durch direkten
//! In-Process-Dispatch (Twitch → Caddy → dieser Listener → Dispatcher).
//!
//! # Warum
//!
//! Die Python-Bridge ackte Notifications mit 204, verschluckte sie aber auf
//! stillen Pfaden (Befund 12.6.: `channel.chat.message` kam nie in Rust an,
//! „Dispatch completed" der Bridge ohne Rust-Empfang — kompensiert nur vom
//! 15s-Poll-Loop). Der native Empfänger hat genau drei Pfade, alle geloggt.
//!
//! # Sicherheit
//!
//! - HMAC-SHA256-Verifikation jeder Nachricht (Twitch-Formel:
//!   `message_id + timestamp + raw_body`, Header-Präfix `sha256=`),
//!   konstantzeitig via `hmac::Mac::verify_slice`.
//! - Kein internes Auth-Token — der Listener ist für den Public-Pfad gedacht
//!   (Caddy proxyt `/twitch/eventsub/callback` hierher); die Signatur IST die
//!   Authentifizierung.
//! - Replay-Schutz übernimmt der persistente Message-Guard des Dispatchers
//!   (Dedup über `Twitch-Eventsub-Message-Id`, antwortet 204 → kein Retry).

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

use crate::dispatch::EventSubDispatcher;

type HmacSha256 = Hmac<Sha256>;

/// Twitch-Message-Typen (Header `Twitch-Eventsub-Message-Type`).
const MSG_TYPE_VERIFICATION: &str = "webhook_callback_verification";
const MSG_TYPE_REVOCATION: &str = "revocation";
const MSG_TYPE_NOTIFICATION: &str = "notification";

/// EventSub-Webhook-Empfänger: verifiziert Signaturen und dispatcht direkt.
pub struct WebhookReceiver {
    secret: String,
    dispatcher: Arc<EventSubDispatcher>,
}

impl WebhookReceiver {
    pub fn new(secret: impl Into<String>, dispatcher: Arc<EventSubDispatcher>) -> Self {
        Self {
            secret: secret.into(),
            dispatcher,
        }
    }

    /// Router mit dem Twitch-Callback-Pfad (identisch zur public URL, damit
    /// Caddy 1:1 proxyen kann).
    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/twitch/eventsub/callback", post(handle_callback))
            .with_state(self)
    }

    /// Twitch-Signatur prüfen — delegiert an [`verify_eventsub_signature`].
    fn verify_signature(
        &self,
        message_id: &str,
        timestamp: &str,
        body: &[u8],
        signature: &str,
    ) -> bool {
        verify_eventsub_signature(&self.secret, message_id, timestamp, body, signature)
    }
}

/// Twitch-Signatur prüfen: `HMAC-SHA256(secret, message_id + timestamp + body)`,
/// Header-Präfix `sha256=`, konstantzeitig via `Mac::verify_slice`.
pub fn verify_eventsub_signature(
    secret: &str,
    message_id: &str,
    timestamp: &str,
    body: &[u8],
    signature: &str,
) -> bool {
    let Some(hex_sig) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(message_id.as_bytes());
    mac.update(timestamp.as_bytes());
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn header<'h>(headers: &'h HeaderMap, name: &str) -> &'h str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
}

async fn handle_callback(
    State(receiver): State<Arc<WebhookReceiver>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let message_id = header(&headers, "twitch-eventsub-message-id");
    let timestamp = header(&headers, "twitch-eventsub-message-timestamp");
    let signature = header(&headers, "twitch-eventsub-message-signature");
    let message_type = header(&headers, "twitch-eventsub-message-type").to_lowercase();
    let header_sub_type = header(&headers, "twitch-eventsub-subscription-type").to_string();

    if message_id.is_empty() || timestamp.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !receiver.verify_signature(message_id, timestamp, &body, signature) {
        tracing::warn!(
            message_id,
            sub_type = %header_sub_type,
            "eventsub_receiver: Signatur ungültig — abgelehnt"
        );
        return StatusCode::FORBIDDEN.into_response();
    }

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("eventsub_receiver: Body kein JSON: {e}");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    match message_type.as_str() {
        MSG_TYPE_VERIFICATION => {
            // Challenge-Echo als text/plain (Twitch-Vertrag).
            let challenge = parsed
                .get("challenge")
                .and_then(Value::as_str)
                .unwrap_or("");
            tracing::info!(
                sub_type = %header_sub_type,
                "eventsub_receiver: Challenge beantwortet"
            );
            (StatusCode::OK, challenge.to_string()).into_response()
        }
        MSG_TYPE_REVOCATION => {
            let reason = parsed
                .pointer("/subscription/status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            tracing::warn!(
                sub_type = %header_sub_type,
                reason,
                "eventsub_receiver: Subscription widerrufen"
            );
            StatusCode::NO_CONTENT.into_response()
        }
        MSG_TYPE_NOTIFICATION => {
            let sub_type = parsed
                .pointer("/subscription/type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or(header_sub_type.as_str())
                .to_string();
            match receiver
                .dispatcher
                .dispatch(&sub_type, Some(message_id), &parsed)
                .await
            {
                Ok(_) => StatusCode::NO_CONTENT.into_response(),
                Err(e) => {
                    // 503 → Twitch retried die Nachricht.
                    tracing::error!(
                        sub_type,
                        message_id,
                        "eventsub_receiver: Dispatch fehlgeschlagen — {e}"
                    );
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                }
            }
        }
        other => {
            tracing::debug!(message_type = other, "eventsub_receiver: unbekannter Typ — ack");
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, message_id: &str, timestamp: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(message_id.as_bytes());
        mac.update(timestamp.as_bytes());
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn signatur_roundtrip_und_manipulation() {
        let secret = "testsecret";
        let body = br#"{"subscription":{"type":"channel.chat.message"}}"#;
        let sig = sign(secret, "mid-1", "2026-06-12T10:00:00Z", body);

        assert!(verify_eventsub_signature(secret, "mid-1", "2026-06-12T10:00:00Z", body, &sig));
        // Manipulierter Body → ungültig
        assert!(!verify_eventsub_signature(secret, "mid-1", "2026-06-12T10:00:00Z", b"{}", &sig));
        // Falsche message_id → ungültig
        assert!(!verify_eventsub_signature(secret, "mid-2", "2026-06-12T10:00:00Z", body, &sig));
        // Fehlendes Präfix → ungültig
        assert!(!verify_eventsub_signature(secret, "mid-1", "2026-06-12T10:00:00Z", body, "deadbeef"));
        // Leere Signatur → ungültig
        assert!(!verify_eventsub_signature(secret, "mid-1", "2026-06-12T10:00:00Z", body, ""));
        // Falsches Secret → ungültig
        assert!(!verify_eventsub_signature("anderes", "mid-1", "2026-06-12T10:00:00Z", body, &sig));
    }
}
