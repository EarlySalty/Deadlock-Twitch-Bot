//! `POST /internal/twitch/v1/raid/manual` — manueller Raid (Phase 6h).
//!
//! Der Python-Chat-Command `!raid` ruft diesen Endpoint als dünner Proxy;
//! die gesamte Raid-Logik (Quelle, Auswahl, Ausführung, Suppression) läuft
//! in Rust. Die Antwort ist das Status-JSON, das der Chat-Command in
//! Chat-Meldungen übersetzt.

use std::sync::Arc;

use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::Json;

use tb_http_core::{ApiError, AuthLevel};

/// Port zum Raid-Subsystem (Implementierung in der tb-bot-Composition-Root).
/// Liefert das fertige Status-JSON (`status`, optional `target_login`/
/// `reason`/`error`).
#[async_trait::async_trait]
pub trait ManualRaidPort: Send + Sync {
    async fn start_manual_raid(
        &self,
        broadcaster_id: &str,
        broadcaster_login: &str,
    ) -> serde_json::Value;
}

/// Extension-Wrapper (None = Raid-Subsystem nicht verdrahtet → 503).
#[derive(Clone)]
pub struct ManualRaidExt(pub Option<Arc<dyn ManualRaidPort>>);

fn required_string_field<'a>(
    body: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a str, ApiError> {
    body.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("invalid request body"))
}

pub async fn manual_raid_handler(
    auth: AuthLevel,
    Extension(port): Extension<ManualRaidExt>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        tracing::error!("manual raid ohne verdrahteten Port aufgerufen");
        return Err(ApiError::unavailable());
    };
    let broadcaster_id = required_string_field(&body, "broadcasterId")?;
    if !broadcaster_id.chars().all(|c| c.is_ascii_digit()) {
        tracing::warn!("manual raid mit ungueltiger broadcasterId abgelehnt");
        return Err(ApiError::bad_request("invalid broadcaster_id"));
    }
    let raw_login = required_string_field(&body, "broadcasterLogin")?;
    let Some(broadcaster_login) = tb_domain::normalize_twitch_login(raw_login) else {
        tracing::warn!("manual raid mit ungueltigem broadcasterLogin abgelehnt");
        return Err(ApiError::bad_request("invalid broadcaster_login"));
    };

    let result = port
        .start_manual_raid(broadcaster_id, &broadcaster_login)
        .await;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use axum::{middleware, Router};
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};

    #[derive(Default)]
    struct StubPort {
        calls: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl ManualRaidPort for StubPort {
        async fn start_manual_raid(&self, _id: &str, _login: &str) -> serde_json::Value {
            self.calls.fetch_add(1, Ordering::SeqCst);
            serde_json::json!({"status": "started", "target_login": "ziel"})
        }
    }

    fn router(port: Option<Arc<dyn ManualRaidPort>>) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(&format!("{base}/raid/manual"), post(manual_raid_handler))
            .layer(Extension(ManualRaidExt(port)))
            .layer(Extension(ExpectedToken("tok".to_string())))
            .layer(middleware::from_fn_with_state(
                "tok".to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(token: Option<&str>, body: &str) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("{INTERNAL_API_BASE_PATH}/raid/manual"))
            .header("content-type", "application/json")
            .extension(ConnectInfo(
                "127.0.0.1:55555".parse::<SocketAddr>().unwrap(),
            ));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn ohne_token_401() {
        let port = Arc::new(StubPort::default());
        let resp = router(Some(port.clone()))
            .oneshot(req(
                None,
                r#"{"broadcasterId":"1","broadcasterLogin":"nani"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn falscher_token_401_ohne_effekt() {
        let port = Arc::new(StubPort::default());
        let resp = router(Some(port.clone()))
            .oneshot(req(
                Some("falsch"),
                r#"{"broadcasterId":"1","broadcasterLogin":"nani"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ohne_port_503() {
        let resp = router(None)
            .oneshot(req(
                Some("tok"),
                r#"{"broadcasterId":"1","broadcasterLogin":"nani"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn delegiert_an_port_und_liefert_status_json() {
        let resp = router(Some(Arc::new(StubPort::default())))
            .oneshot(req(
                Some("tok"),
                r#"{"broadcasterId":"1","broadcasterLogin":"nani"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["status"], "started");
        assert_eq!(v["target_login"], "ziel");
    }

    #[tokio::test]
    async fn leere_felder_400() {
        let port = Arc::new(StubPort::default());
        let resp = router(Some(port.clone()))
            .oneshot(req(
                Some("tok"),
                r#"{"broadcasterId":"  ","broadcasterLogin":"nani"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn falsche_body_typen_400_json_ohne_effekt() {
        let port = Arc::new(StubPort::default());
        let resp = router(Some(port.clone()))
            .oneshot(req(
                Some("tok"),
                r#"{"broadcasterId":1,"broadcasterLogin":"nani"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "bad_request");
        assert!(v["message"].as_str().is_some());
    }
}
