//! tb-http-core — gemeinsame axum-Bausteine für die interne API.

pub mod constants;
pub mod error;
pub mod middleware;

pub use constants::{IDEMPOTENCY_KEY_HEADER, INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER};
pub use error::ApiError;
pub use middleware::auth::internal_auth;
pub use middleware::idempotency::IdempotencyKey;
pub use middleware::loopback::loopback_only;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::get,
        Router,
    };
    use std::net::SocketAddr;
    use tower::ServiceExt;

    /// Baut einen Test-Router mit Loopback + Auth Middleware.
    fn test_router(token: &str) -> Router {
        Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                token.to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    /// Erstellt einen Request mit gesetzter ConnectInfo.
    fn loopback_request(token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().uri("/test").extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
        if let Some(t) = token {
            builder = builder.header(INTERNAL_TOKEN_HEADER, t);
        }
        builder.body(Body::empty()).unwrap()
    }

    fn non_loopback_request(token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .uri("/test")
            .extension(ConnectInfo("10.0.0.1:12345".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header(INTERNAL_TOKEN_HEADER, t);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn loopback_mit_korrektem_token_gibt_200() {
        let app = test_router("secret");
        let req = loopback_request(Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn falscher_token_gibt_401() {
        let app = test_router("secret");
        let req = loopback_request(Some("wrong"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn fehlender_token_gibt_401() {
        let app = test_router("secret");
        let req = loopback_request(None);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn leeres_konfiguriertes_token_fail_closed_401() {
        let app = test_router("");
        let req = loopback_request(Some("irgendwas"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn nicht_loopback_gibt_403() {
        let app = test_router("secret");
        let req = non_loopback_request(Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn idempotency_key_extraktor_liest_header() {
        let app = Router::new().route(
            "/idempotency",
            get(|IdempotencyKey(key): IdempotencyKey| async move {
                key.unwrap_or_else(|| "none".to_string())
            }),
        );
        let req = Request::builder()
            .uri("/idempotency")
            .header(IDEMPOTENCY_KEY_HEADER, "test-key-123")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"test-key-123");
    }
}
