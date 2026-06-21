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
use crate::subscriptions::RevocationSink;

type HmacSha256 = Hmac<Sha256>;

/// Twitch-Message-Typen (Header `Twitch-Eventsub-Message-Type`).
const MSG_TYPE_VERIFICATION: &str = "webhook_callback_verification";
const MSG_TYPE_REVOCATION: &str = "revocation";
const MSG_TYPE_NOTIFICATION: &str = "notification";

/// Maximales Nachrichten-Alter in Sekunden (Replay-Fenster, Python
/// `_MAX_MESSAGE_AGE_SECONDS`). Älter ODER weiter als das in der Zukunft → 403.
const MAX_MESSAGE_AGE_SECONDS: i64 = 600;

/// EventSub-Webhook-Empfänger: verifiziert Signaturen und dispatcht direkt.
pub struct WebhookReceiver {
    secret: String,
    dispatcher: Arc<EventSubDispatcher>,
    /// Optionale Revocation-Senke (`SubscriptionManager`): bei Webhook-Revocation
    /// wird die Sub untracked, damit der Reconcile-Loop sie neu anlegt. `None`
    /// → Revocation wird nur geloggt (Alt-Verhalten, kein Selbstheilen).
    revocation_sink: Option<Arc<dyn RevocationSink>>,
}

impl WebhookReceiver {
    pub fn new(secret: impl Into<String>, dispatcher: Arc<EventSubDispatcher>) -> Self {
        Self {
            secret: secret.into(),
            dispatcher,
            revocation_sink: None,
        }
    }

    /// Verdrahtet die Revocation-Senke (Port von Pythons
    /// `set_revocation_callback`). Wird beim Aufbau in `bin/tb-bot` mit dem
    /// `SubscriptionManager` gefüttert, damit widerrufene Subs zur Laufzeit
    /// untracked + beim nächsten Reconcile neu angelegt werden.
    ///
    /// WIRING-TODO(P1.17/P1.18/P1.20): In `bin/tb-bot/src/main.rs:810`
    /// (`WebhookReceiver::new`) den vorhandenen `SubscriptionManager`-`Arc`
    /// via `.with_revocation_sink(subscription_manager.clone())` durchreichen,
    /// damit Core-Sub-Revocations (stream.online/offline/channel.update) zur
    /// Laufzeit selbstheilen statt erst beim Prozess-Neustart.
    #[must_use]
    pub fn with_revocation_sink(mut self, sink: Arc<dyn RevocationSink>) -> Self {
        self.revocation_sink = Some(sink);
        self
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

/// Zweite Replay-Schranke neben dem Message-ID-Guard (B18-1/65.2): ist der
/// Twitch-Timestamp älter als [`MAX_MESSAGE_AGE_SECONDS`] oder mehr als dieses
/// Fenster in der Zukunft (Skew), wird die Nachricht abgelehnt. Unparsebare
/// Timestamps gelten als zu alt (fail-closed, wie Python `_is_message_too_old`).
fn is_timestamp_too_old(timestamp: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(timestamp) else {
        return true;
    };
    let age = now.signed_duration_since(parsed.with_timezone(&chrono::Utc));
    // Akzeptiert wird nur das Fenster [-600s, +600s]: alles außerhalb ist zu
    // alt (Replay) oder zu weit in der Zukunft (Skew).
    !(-MAX_MESSAGE_AGE_SECONDS..=MAX_MESSAGE_AGE_SECONDS).contains(&age.num_seconds())
}

/// Verarbeitet eine Webhook-Revocation (Python `_handle_eventsub_webhook_revocation`):
/// extrahiert Sub-Typ + Ziel-Broadcaster aus dem Payload und untrackt die Sub
/// über die [`RevocationSink`], damit der nächste Reconcile-Zyklus sie neu
/// anlegt (Selbstheilung statt stillem Event-Verlust). Ohne Sink bleibt es beim
/// reinen Logging (Alt-Verhalten).
fn handle_revocation(
    parsed: &Value,
    header_sub_type: &str,
    sink: Option<&dyn RevocationSink>,
) {
    let reason = parsed
        .pointer("/subscription/status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    // Sub-Typ + Ziel-Broadcaster aus dem Revocation-Payload ziehen (Header-Typ
    // als Fallback). Twitch liefert die Condition unter `subscription.condition`;
    // der Broadcaster steckt je nach Sub-Typ in `broadcaster_user_id` oder
    // `to_broadcaster_user_id` (z. B. channel.raid-Arrival).
    let sub_type = parsed
        .pointer("/subscription/type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(header_sub_type);
    let broadcaster_id = parsed
        .pointer("/subscription/condition/broadcaster_user_id")
        .and_then(Value::as_str)
        .or_else(|| {
            parsed
                .pointer("/subscription/condition/to_broadcaster_user_id")
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .trim();
    tracing::warn!(
        sub_type,
        broadcaster_id,
        reason,
        "eventsub_receiver: Subscription widerrufen"
    );
    let Some(sink) = sink else {
        return;
    };
    if broadcaster_id.is_empty() {
        tracing::warn!(
            sub_type,
            "eventsub_receiver: Revocation ohne broadcaster_user_id — kein Untrack möglich"
        );
        return;
    }
    sink.on_revocation(sub_type, broadcaster_id);
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

    // Replay-Schranke: Timestamp-Alter prüfen (zweite Linie neben dem
    // Message-ID-Dedup im Dispatcher). Vor dem Dispatch, nach der Signatur.
    if is_timestamp_too_old(timestamp, chrono::Utc::now()) {
        tracing::warn!(
            message_id,
            timestamp,
            sub_type = %header_sub_type,
            "eventsub_receiver: Nachricht zu alt/Zukunfts-Skew — abgelehnt (Replay-Schutz)"
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
            handle_revocation(&parsed, &header_sub_type, receiver.revocation_sink.as_deref());
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
            // Readiness-Gate VOR dem Dispatch (Python `_assert_dispatch_ready`,
            // 65.3): Annahme aktiv? Handler für diesen Sub-Typ registriert?
            // Beide Fehler → 503 (Twitch retryt), statt die Notification still
            // in den „unbekannt"-Zweig laufen zu lassen.
            if let Err(reason) = receiver.dispatcher.ensure_dispatch_ready(&sub_type) {
                tracing::warn!(
                    sub_type,
                    message_id,
                    %reason,
                    "eventsub_receiver: Notification vor Dispatch abgelehnt (Readiness-Gate)"
                );
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
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
    use std::sync::Mutex;

    /// Mock-Senke: zeichnet jede Revocation auf (sub_type, broadcaster_id).
    #[derive(Default)]
    struct RecordingSink {
        calls: Mutex<Vec<(String, String)>>,
    }
    impl RevocationSink for RecordingSink {
        fn on_revocation(&self, sub_type: &str, broadcaster_id: &str) -> bool {
            self.calls
                .lock()
                .unwrap()
                .push((sub_type.to_string(), broadcaster_id.to_string()));
            true
        }
    }

    #[test]
    fn revocation_untrackt_ueber_sink() {
        // P1.17/P1.18/P1.20: Revocation-Payload → Sink mit (Typ, Broadcaster).
        let sink = RecordingSink::default();
        let payload = serde_json::json!({
            "subscription": {
                "type": "stream.online",
                "status": "authorization_revoked",
                "condition": { "broadcaster_user_id": "12345" }
            }
        });
        handle_revocation(&payload, "stream.online", Some(&sink));
        assert_eq!(
            *sink.calls.lock().unwrap(),
            vec![("stream.online".to_string(), "12345".to_string())]
        );

        // Raid-Arrival: Broadcaster steckt in to_broadcaster_user_id.
        let sink2 = RecordingSink::default();
        let raid = serde_json::json!({
            "subscription": {
                "type": "channel.raid",
                "status": "user_removed",
                "condition": { "to_broadcaster_user_id": "999" }
            }
        });
        handle_revocation(&raid, "channel.raid", Some(&sink2));
        assert_eq!(
            *sink2.calls.lock().unwrap(),
            vec![("channel.raid".to_string(), "999".to_string())]
        );

        // Ohne broadcaster_user_id → kein Untrack-Versuch (still geloggt).
        let sink3 = RecordingSink::default();
        let no_bid = serde_json::json!({
            "subscription": { "type": "stream.offline", "condition": {} }
        });
        handle_revocation(&no_bid, "stream.offline", Some(&sink3));
        assert!(sink3.calls.lock().unwrap().is_empty());
    }

    fn sign(secret: &str, message_id: &str, timestamp: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(message_id.as_bytes());
        mac.update(timestamp.as_bytes());
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn timestamp_replay_fenster() {
        use chrono::{Duration, Utc};
        let now = Utc::now();
        let iso = |dt: chrono::DateTime<Utc>| dt.to_rfc3339();

        // Frisch → akzeptiert.
        assert!(!is_timestamp_too_old(&iso(now), now));
        assert!(!is_timestamp_too_old(&iso(now - Duration::seconds(599)), now));
        // Genau am Limit (600s) → noch akzeptiert (Python: nur > 600 lehnt ab).
        assert!(!is_timestamp_too_old(&iso(now - Duration::seconds(600)), now));
        // Älter als 600s → abgelehnt.
        assert!(is_timestamp_too_old(&iso(now - Duration::seconds(601)), now));
        assert!(is_timestamp_too_old(&iso(now - Duration::seconds(3600)), now));
        // Zukunfts-Skew jenseits -600s → abgelehnt.
        assert!(!is_timestamp_too_old(&iso(now + Duration::seconds(600)), now));
        assert!(is_timestamp_too_old(&iso(now + Duration::seconds(601)), now));
        // Unparsebar → abgelehnt (Fail-closed, wie Python).
        assert!(is_timestamp_too_old("", now));
        assert!(is_timestamp_too_old("nicht-ein-timestamp", now));
    }

    #[test]
    fn timestamp_twitch_format_geparst() {
        use chrono::{TimeZone, Utc};
        // Twitch-Realformat: RFC3339 mit Sub-Sekunden + Z.
        let base = Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap();
        let ts = "2026-06-12T10:00:00.7726011Z";
        assert!(!is_timestamp_too_old(ts, base + chrono::Duration::seconds(60)));
        assert!(is_timestamp_too_old(ts, base + chrono::Duration::seconds(700)));
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
