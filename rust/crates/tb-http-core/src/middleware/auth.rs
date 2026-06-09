//! Interne Auth-Middleware: prüft den X-Internal-Token-Header (constant-time).

use crate::constants::INTERNAL_TOKEN_HEADER;
use crate::error::ApiError;
use axum::{
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Axum-Middleware: prüft `X-Internal-Token` gegen den konfigurierten Token.
///
/// Leeres konfiguriertes Token → fail-closed (immer 401).
/// Vergleich via constant-time-Funktion (`subtle`-frei: direkte Byte-Iteration).
pub async fn internal_auth(
    State(expected_token): State<String>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Leeres konfiguriertes Token → immer 401 (fail-closed)
    if expected_token.is_empty() {
        return ApiError::unauthorized().into_response();
    }

    let provided = req
        .headers()
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !constant_time_eq(provided.as_bytes(), expected_token.as_bytes()) {
        return ApiError::unauthorized().into_response();
    }

    next.run(req).await
}

/// Auth-Level eines eingehenden Requests.
///
/// Auswertungsreihenfolge:
/// 1. Loopback-IP (127.x.x.x) **und** Host-Header ist `localhost` oder `127.0.0.1`
///    → `Localhost` (impliziert admin-Level, kein Token nötig)
/// 2. `X-Internal-Token`-Header stimmt constant-time mit konfiguriertem Token überein
///    → `Admin`
/// 3. Sonst → `None`
///
/// Partner-Session-Auth (Fernet-Cookie) ist deferred (ADR 0003) und wird hier nicht
/// implementiert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthLevel {
    /// Loopback-Verbindung (127.x.x.x) mit localhost-Host-Header.
    Localhost,
    /// Gültiger X-Internal-Token.
    Admin,
    /// Nicht authentifiziert.
    None,
}

impl AuthLevel {
    /// Gibt `true` zurück wenn Admin oder Localhost.
    pub fn is_privileged(&self) -> bool {
        matches!(self, AuthLevel::Admin | AuthLevel::Localhost)
    }

    /// Serialisiert den Level als JSON-String-Wert.
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthLevel::Localhost => "localhost",
            AuthLevel::Admin => "admin",
            AuthLevel::None => "none",
        }
    }
}

/// Newtype um den erwarteten Token als axum-Extension einzufügen.
///
/// Wird vom Router via `.layer(Extension(ExpectedToken(token)))` gesetzt.
#[derive(Clone)]
pub struct ExpectedToken(pub String);

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for AuthLevel
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // 1. Loopback: ConnectInfo + Host-Check
        let is_loopback = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.0.ip().is_loopback())
            .unwrap_or(false);

        if is_loopback {
            let host = parts
                .headers
                .get(axum::http::header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            // Host-Header: "localhost", "localhost:<port>", "127.0.0.1", "127.0.0.1:<port>"
            let host_base = host.split(':').next().unwrap_or(host);
            if host_base == "localhost" || host_base == "127.0.0.1" {
                return Ok(AuthLevel::Localhost);
            }
        }

        // 2. Admin: X-Internal-Token aus Extension
        if let Some(expected) = parts.extensions.get::<ExpectedToken>() {
            let provided = parts
                .headers
                .get(INTERNAL_TOKEN_HEADER)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !expected.0.is_empty()
                && constant_time_eq(provided.as_bytes(), expected.0.as_bytes())
            {
                return Ok(AuthLevel::Admin);
            }
        }

        Ok(AuthLevel::None)
    }
}

/// Constant-time Byte-Vergleich (verhindert Timing-Angriffe).
///
/// Gibt `true` zurück, wenn beide Slices identisch sind.
/// Laufzeit ist proportional zur Länge von `expected`, unabhängig vom Mismatch.
fn constant_time_eq(provided: &[u8], expected: &[u8]) -> bool {
    if provided.len() != expected.len() {
        // Längenunterschied ist selbst keine Information, die Timing enthüllt —
        // early-return hier ist akzeptabel, da der Angreifer die Tokenlänge kennt.
        return false;
    }
    let mut diff: u8 = 0;
    for (a, b) in provided.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod auth_level_tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use std::net::SocketAddr;
    use tower::ServiceExt;

    async fn echo_auth(auth: AuthLevel) -> axum::response::Response {
        axum::response::Response::builder()
            .status(200)
            .body(Body::from(auth.as_str()))
            .unwrap()
    }

    fn make_router(token: &str) -> Router {
        Router::new()
            .route("/", get(echo_auth))
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn req(ip: &str, host: &str, token: Option<&str>) -> Request<Body> {
        let addr: SocketAddr = format!("{}:12345", ip).parse().unwrap();
        let mut b = Request::builder()
            .uri("/")
            .extension(ConnectInfo(addr))
            .header(axum::http::header::HOST, host);
        if let Some(t) = token {
            b = b.header(INTERNAL_TOKEN_HEADER, t);
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn localhost_loopback_with_localhost_host() {
        // ConnectInfo wird direkt als Extension injiziert — kein into_make_service nötig
        let app = make_router("secret");
        let res = app
            .oneshot(req("127.0.0.1", "localhost", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 64).await.unwrap();
        assert_eq!(&body[..], b"localhost");
    }

    #[tokio::test]
    async fn admin_with_correct_token() {
        let app = make_router("secret");
        let res = app
            .oneshot(req("192.168.1.1", "example.com", Some("secret")))
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), 64).await.unwrap();
        assert_eq!(&body[..], b"admin");
    }

    #[tokio::test]
    async fn no_token_gives_none() {
        let app = make_router("secret");
        let res = app
            .oneshot(req("192.168.1.1", "example.com", None))
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), 64).await.unwrap();
        assert_eq!(&body[..], b"none");
    }

    #[tokio::test]
    async fn loopback_ip_but_non_localhost_host_falls_through_to_none() {
        let app = make_router("secret");
        let res = app
            .oneshot(req("127.0.0.1", "myservice.internal", None))
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), 64).await.unwrap();
        assert_eq!(&body[..], b"none");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gleiche_werte_sind_gleich() {
        assert!(constant_time_eq(b"abc", b"abc"));
    }

    #[test]
    fn unterschiedliche_werte_sind_ungleich() {
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    #[test]
    fn unterschiedliche_laengen_sind_ungleich() {
        assert!(!constant_time_eq(b"ab", b"abc"));
    }

    #[test]
    fn leere_strings_sind_gleich() {
        assert!(constant_time_eq(b"", b""));
    }
}
