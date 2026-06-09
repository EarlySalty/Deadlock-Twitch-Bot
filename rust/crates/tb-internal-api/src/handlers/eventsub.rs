//! `POST /eventsub/dispatch` — Ingress der EventSub-Bridge.
//!
//! Vertrag identisch zum Python-Endpoint (`internal_api/routes/telemetry.py`):
//! Body `{"sub_type", "message_id", "payload"}`; Antwort ist das
//! Dispatch-Ergebnis (`ok`/`duplicate`/`queued`/`processed`). Annahmefehler →
//! 503, die Bridge puffert durable in ihrer Outbox und retryt.

use std::sync::Arc;

use axum::{response::IntoResponse, Extension, Json};
use serde::Deserialize;
use serde_json::Value;
use tb_http_core::ApiError;
use tb_monitoring::EventSubDispatcher;

/// Router-Extension: Dispatcher ist optional — ohne Monitoring-Wiring
/// antwortet der Endpoint 503 (Bridge puffert).
#[derive(Clone)]
pub struct EventSubDispatcherExt(pub Option<Arc<EventSubDispatcher>>);

#[derive(Deserialize)]
pub struct DispatchRequest {
    pub sub_type: Option<String>,
    pub message_id: Option<String>,
    pub payload: Option<Value>,
}

pub async fn dispatch_handler(
    Extension(dispatcher): Extension<EventSubDispatcherExt>,
    Json(body): Json<DispatchRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let sub_type = body
        .sub_type
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if sub_type.is_empty() {
        return Err(ApiError::bad_request_with_body(serde_json::json!({
            "error": "bad_request",
            "message": "invalid or missing sub_type"
        })));
    }
    let Some(payload) = body.payload.filter(Value::is_object) else {
        return Err(ApiError::bad_request_with_body(serde_json::json!({
            "error": "bad_request",
            "message": "invalid payload"
        })));
    };
    let Some(dispatcher) = dispatcher.0 else {
        return Err(ApiError::unavailable());
    };
    let message_id = body
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty());

    match dispatcher.dispatch(&sub_type, message_id, &payload).await {
        Ok(outcome) => Ok(Json(outcome)),
        Err(error) => {
            tracing::error!(%error, sub_type, "EventSub-Dispatch fehlgeschlagen");
            Err(ApiError::unavailable())
        }
    }
}
