//! `POST /internal/twitch/v1/scam-guard/revoke` — eine Scam-Guard-Entscheidung
//! zurücknehmen.
//!
//! Aufgerufen vom Discord-Revoke-Button (über den Master-Broker) und perspek-
//! tivisch vom Dashboard-Override. Der Body trägt nur die `verdictId`; die
//! gesamte Logik (Urteil laden → ggf. Twitch-Unban → als `overturned` markieren,
//! was das Self-Learning füttert) liegt hinter dem [`ScamRevokePort`] in der
//! tb-bot-Composition-Root. Die Antwort ist das Status-JSON des Ports.

use std::sync::Arc;

use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use tb_http_core::{ApiError, AuthLevel};

/// Port zum Scam-Guard-Revoke (Implementierung in tb-bot: lädt das Urteil,
/// entbannt bei Bedarf auf Twitch und markiert es als `overturned`). Liefert das
/// fertige Status-JSON (`status`, `channel_login`, `chatter_login`,
/// `was_banned`, `unbanned`).
#[async_trait::async_trait]
pub trait ScamRevokePort: Send + Sync {
    async fn revoke(&self, verdict_id: i64) -> serde_json::Value;
}

/// Extension-Wrapper (None = Scam-Guard-Revoke nicht verdrahtet → 503).
#[derive(Clone)]
pub struct ScamRevokeExt(pub Option<Arc<dyn ScamRevokePort>>);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScamRevokeRequest {
    pub verdict_id: i64,
}

pub async fn scam_revoke_handler(
    auth: AuthLevel,
    Extension(port): Extension<ScamRevokeExt>,
    Json(body): Json<ScamRevokeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        tracing::error!("scam-guard revoke ohne verdrahteten Port aufgerufen");
        return Err(ApiError::unavailable());
    };
    if body.verdict_id <= 0 {
        tracing::warn!("scam-guard revoke mit ungueltiger verdict_id abgelehnt");
        return Err(ApiError::bad_request("verdict_id must be a positive integer"));
    }

    let result = port.revoke(body.verdict_id).await;
    Ok(Json(result))
}

/// Port zum Promoten eines Scam-Guard-Vorschlags zu einem Twitch-Ban.
#[async_trait::async_trait]
pub trait ScamEnforcePort: Send + Sync {
    async fn enforce(&self, verdict_id: i64) -> serde_json::Value;
}

/// Extension-Wrapper (None = Scam-Guard-Enforce nicht verdrahtet → 503).
#[derive(Clone)]
pub struct ScamEnforceExt(pub Option<Arc<dyn ScamEnforcePort>>);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScamEnforceRequest {
    pub verdict_id: i64,
}

pub async fn scam_enforce_handler(
    auth: AuthLevel,
    Extension(port): Extension<ScamEnforceExt>,
    Json(body): Json<ScamEnforceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        tracing::error!("scam-guard enforce ohne verdrahteten Port aufgerufen");
        return Err(ApiError::unavailable());
    };
    if body.verdict_id <= 0 {
        tracing::warn!("scam-guard enforce mit ungueltiger verdict_id abgelehnt");
        return Err(ApiError::bad_request("verdict_id must be a positive integer"));
    }

    let result = port.enforce(body.verdict_id).await;
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
    use std::sync::atomic::{AtomicI64, Ordering};
    use tower::ServiceExt;

    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};

    #[derive(Default)]
    struct StubPort {
        seen: AtomicI64,
    }
    #[async_trait::async_trait]
    impl ScamRevokePort for StubPort {
        async fn revoke(&self, verdict_id: i64) -> serde_json::Value {
            self.seen.store(verdict_id, Ordering::SeqCst);
            serde_json::json!({"status": "revoked", "chatter_login": "scammer"})
        }
    }

    fn router(port: Option<Arc<dyn ScamRevokePort>>) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(
                &format!("{base}/scam-guard/revoke"),
                post(scam_revoke_handler),
            )
            .layer(Extension(ScamRevokeExt(port)))
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
            .uri(format!("{INTERNAL_API_BASE_PATH}/scam-guard/revoke"))
            .header("content-type", "application/json")
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn ohne_token_401() {
        let resp = router(Some(Arc::new(StubPort::default())))
            .oneshot(req(None, r#"{"verdictId":42}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ohne_port_503() {
        let resp = router(None)
            .oneshot(req(Some("tok"), r#"{"verdictId":42}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn ungueltige_id_400() {
        let resp = router(Some(Arc::new(StubPort::default())))
            .oneshot(req(Some("tok"), r#"{"verdictId":0}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delegiert_an_port_mit_verdict_id() {
        let port = Arc::new(StubPort::default());
        let resp = router(Some(port.clone()))
            .oneshot(req(Some("tok"), r#"{"verdictId":42}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(port.seen.load(Ordering::SeqCst), 42);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["status"], "revoked");
        assert_eq!(v["chatter_login"], "scammer");
    }

    #[derive(Default)]
    struct StubEnforcePort {
        seen: AtomicI64,
    }

    #[async_trait::async_trait]
    impl ScamEnforcePort for StubEnforcePort {
        async fn enforce(&self, verdict_id: i64) -> serde_json::Value {
            self.seen.store(verdict_id, Ordering::SeqCst);
            serde_json::json!({"status": "enforced", "chatter_login": "scammer"})
        }
    }

    fn enforce_router(port: Option<Arc<dyn ScamEnforcePort>>) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(
                &format!("{base}/scam-guard/enforce"),
                post(scam_enforce_handler),
            )
            .layer(Extension(ScamEnforceExt(port)))
            .layer(Extension(ExpectedToken("tok".to_string())))
            .layer(middleware::from_fn_with_state(
                "tok".to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn enforce_req(token: Option<&str>, body: &str) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("{INTERNAL_API_BASE_PATH}/scam-guard/enforce"))
            .header("content-type", "application/json")
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn enforce_ohne_token_401() {
        let resp = enforce_router(Some(Arc::new(StubEnforcePort::default())))
            .oneshot(enforce_req(None, r#"{"verdictId":42}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn enforce_ohne_port_503() {
        let resp = enforce_router(None)
            .oneshot(enforce_req(Some("tok"), r#"{"verdictId":42}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn enforce_ungueltige_id_400() {
        let resp = enforce_router(Some(Arc::new(StubEnforcePort::default())))
            .oneshot(enforce_req(Some("tok"), r#"{"verdictId":0}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn enforce_delegiert_an_port_mit_verdict_id() {
        let port = Arc::new(StubEnforcePort::default());
        let resp = enforce_router(Some(port.clone()))
            .oneshot(enforce_req(Some("tok"), r#"{"verdictId":42}"#))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(port.seen.load(Ordering::SeqCst), 42);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["status"], "enforced");
        assert_eq!(v["chatter_login"], "scammer");
    }
}
