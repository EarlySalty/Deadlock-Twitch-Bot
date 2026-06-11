//! Handler für `GET /twitch/api/v2/auth-status`.
//!
//! Antwortet immer 200 — auch ohne Auth. Dient dem Frontend zur Erkennung des
//! aktuellen Auth-Levels (z. B. für Conditional Rendering).

use axum::{response::IntoResponse, Json};
use serde::Serialize;
use tb_http_core::AuthLevel;

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub auth_level: &'static str,
    pub logged_in: bool,
}

/// `GET /twitch/api/v2/auth-status`
///
/// Kein Auth-Gate — immer 200.
pub async fn auth_status_handler(auth: AuthLevel) -> impl IntoResponse {
    Json(AuthStatusResponse {
        logged_in: auth.is_privileged(),
        auth_level: auth.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    fn make_router(token: &str) -> Router {
        Router::new()
            .route("/twitch/api/v2/auth-status", get(auth_status_handler))
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn req_with(ip: &str, host: &str, token: Option<&str>) -> Request<Body> {
        let addr: SocketAddr = format!("{}:9999", ip).parse().unwrap();
        let mut b = Request::builder()
            .uri("/twitch/api/v2/auth-status")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, host);
        if let Some(t) = token {
            b = b.header("x-internal-token", t);
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn admin_token_gives_admin() {
        let app = make_router("tok");
        let res = app
            .oneshot(req_with("1.2.3.4", "example.com", Some("tok")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 256).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["auth_level"], "admin");
        assert_eq!(v["logged_in"], true);
    }

    #[tokio::test]
    async fn no_token_gives_none() {
        let app = make_router("tok");
        let res = app
            .oneshot(req_with("1.2.3.4", "example.com", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 256).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["auth_level"], "none");
        assert_eq!(v["logged_in"], false);
    }

    #[tokio::test]
    async fn loopback_without_token_gives_none() {
        // Kein Localhost-Bypass mehr: Loopback ohne Token ist nicht eingeloggt.
        let app = make_router("tok");
        let addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let req = Request::builder()
            .uri("/twitch/api/v2/auth-status")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, "localhost")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 256).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["auth_level"], "none");
        assert_eq!(v["logged_in"], false);
    }
}
