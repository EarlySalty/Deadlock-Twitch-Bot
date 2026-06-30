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

    if let Err(reason) = dispatcher.ensure_dispatch_ready(&sub_type) {
        tracing::warn!(
            %reason,
            sub_type,
            "EventSub-Dispatch vor Verarbeitung abgelehnt"
        );
        return Err(ApiError::unavailable());
    }

    match dispatcher.dispatch(&sub_type, message_id, &payload).await {
        Ok(outcome) => Ok(Json(outcome)),
        Err(error) => {
            tracing::error!(%error, sub_type, "EventSub-Dispatch fehlgeschlagen");
            Err(ApiError::unavailable())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::IntoResponse};
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use std::sync::Arc;
    use tb_monitoring::{
        epoch_clock, GuardStore, HandlerError, InboxHandler, InboxRuntime, NoopEventSubHooks,
        ProcessingInboxStore, TelemetryStore,
    };

    struct NoopInboxHandler;

    #[async_trait::async_trait]
    impl InboxHandler for NoopInboxHandler {
        async fn handle(
            &self,
            _work_type: &str,
            _payload: &serde_json::Value,
        ) -> Result<(), HandlerError> {
            Ok(())
        }
    }

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("create schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");
        sqlx::query(
            "CREATE TABLE twitch_eventsub_processing_inbox (
                work_id TEXT PRIMARY KEY, work_type TEXT NOT NULL, message_id TEXT,
                payload_json TEXT NOT NULL, queued_at DOUBLE PRECISION NOT NULL,
                next_attempt_at DOUBLE PRECISION NOT NULL,
                attempt_count INTEGER NOT NULL DEFAULT 0, last_error TEXT)",
        )
        .execute(&pool)
        .await
        .expect("inbox DDL");
        pool
    }

    #[tokio::test]
    async fn dispatch_handler_lehnt_unbekannten_sub_type_vor_dispatch_ab() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "t_eventsub_unknown_ready").await;
        let store = ProcessingInboxStore::new(pool.clone());
        let runtime = InboxRuntime::new(store, Arc::new(NoopInboxHandler)).start();
        let dispatcher = Arc::new(EventSubDispatcher::new(
            GuardStore::new(pool.clone()),
            runtime.enqueuer(),
            TelemetryStore::new(pool),
            Arc::new(NoopEventSubHooks),
            Arc::new(epoch_clock),
        ));

        let resp = dispatch_handler(
            Extension(EventSubDispatcherExt(Some(dispatcher))),
            Json(DispatchRequest {
                sub_type: Some("channel.unbekannt".to_string()),
                message_id: Some("m-unknown".to_string()),
                payload: Some(serde_json::json!({"event": {}})),
            }),
        )
        .await
        .into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        runtime.shutdown().await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], serde_json::json!("upstream_unavailable"));
    }
}
