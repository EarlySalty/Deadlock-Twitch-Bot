//! CSRF-Schutz-Layer für Write-Actions (F6 / B3-4-Vorstufe).
//!
//! Stellt die Middleware [`csrf_protect`] bereit, die Schreib-Requests
//! (POST/PUT/PATCH/DELETE) gegen das sessiongebundene CSRF-Token absichert
//! ([`DashboardAuthState::validate_csrf`], session.rs). Das Token kommt aus dem
//! Header `X-CSRF-Token`, der Session-Cookie aus `twitch_dash_session`
//! (Partner) bzw. `master_dash_session` (Admin).
//!
//! **Scope dieses Tickets (B3-2):** Der Layer wird *bereitgestellt und getestet*,
//! aber NICHT bereits auf alle Write-Routen gelegt — das erzwingende Verdrahten
//! auf konkrete Routen ist B3-5 (siehe build-plan-dag). Die Login-/Callback-/
//! Logout-Routen sind GET und damit vom CSRF-Gate ausgenommen (sicher).
//!
//! Verhalten:
//! - Safe-Methoden (GET/HEAD/OPTIONS/TRACE) → immer durchgelassen.
//! - Localhost-Requests → durchgelassen (kein Browser-CSRF-Vektor; loopback-only
//!   interne Tools, Python-Parität für den Localhost-Bypass).
//! - Write ohne gültiges Token/Session → `403 csrf_failed`.
//! - Ohne `DashboardAuthState`-Extension (Auth aus) → fail-closed `403`.

use axum::{
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use super::level::{extract_cookie, is_local_request};
use super::session::{DashboardAuthState, ADMIN_COOKIE_NAME, PARTNER_COOKIE_NAME};

/// Header, in dem der Client das sessiongebundene CSRF-Token präsentiert
/// (Python: `X-CSRF-Token`).
pub const CSRF_HEADER: &str = "x-csrf-token";

/// `true` für CSRF-relevante (zustandsändernde) Methoden.
fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn csrf_failed() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "csrf_failed",
            "message": "Ungültiges oder fehlendes CSRF-Token.",
        })),
    )
        .into_response()
}

/// axum-Middleware: erzwingt das CSRF-Token auf Write-Requests.
///
/// Liest das Session-Cookie (Partner bzw. Admin) und das `X-CSRF-Token`-Header,
/// validiert beide konstant-zeitlich gegen den verschlüsselten Session-Payload.
pub async fn csrf_protect(request: Request, next: Next) -> Response {
    if !is_write_method(request.method()) {
        return next.run(request).await;
    }

    let (parts, body) = request.into_parts();

    // Localhost-Bypass (interne loopback-only Tools; Python-Parität).
    if is_local_request(&parts) {
        return next.run(Request::from_parts(parts, body)).await;
    }

    let Some(state) = parts.extensions.get::<DashboardAuthState>().cloned() else {
        // Auth-State fehlt → kein Validierungspfad → fail-closed.
        return csrf_failed();
    };

    let presented = parts
        .headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string();

    // Admin- vor Partner-Session prüfen (Admin-Cookie hat Vorrang).
    let admin_cookie = extract_cookie(&parts, ADMIN_COOKIE_NAME).map(str::to_string);
    let partner_cookie = extract_cookie(&parts, PARTNER_COOKIE_NAME).map(str::to_string);

    let valid = match (&admin_cookie, &partner_cookie) {
        (Some(sid), _) if !sid.is_empty() => state
            .validate_csrf(sid, ADMIN_COOKIE_NAME_TYPE, &presented)
            .await
            .unwrap_or(false),
        (_, Some(sid)) if !sid.is_empty() => state
            .validate_csrf(sid, PARTNER_COOKIE_NAME_TYPE, &presented)
            .await
            .unwrap_or(false),
        _ => false,
    };

    if valid {
        next.run(Request::from_parts(parts, body)).await
    } else {
        csrf_failed()
    }
}

/// Session-Typ-Konstanten der validate_csrf-Lookups (DB-`session_type`).
const ADMIN_COOKIE_NAME_TYPE: &str = "discord_admin";
const PARTNER_COOKIE_NAME_TYPE: &str = "twitch";

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request},
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;

    #[test]
    fn write_methoden_erkannt() {
        assert!(is_write_method(&Method::POST));
        assert!(is_write_method(&Method::PUT));
        assert!(is_write_method(&Method::PATCH));
        assert!(is_write_method(&Method::DELETE));
        assert!(!is_write_method(&Method::GET));
        assert!(!is_write_method(&Method::HEAD));
        assert!(!is_write_method(&Method::OPTIONS));
    }

    /// Router mit aufgelegtem CSRF-Layer; KEINE DashboardAuthState-Extension
    /// (Auth aus) → Write muss fail-closed mit 403 abgelehnt werden, GET passiert.
    fn guarded_router() -> Router {
        Router::new()
            .route("/read", get(|| async { "ok" }))
            .route("/write", post(|| async { "written" }))
            .layer(axum::middleware::from_fn(csrf_protect))
    }

    #[tokio::test]
    async fn write_ohne_gueltiges_csrf_token_403() {
        let app = guarded_router();
        // Nicht-Loopback-Host erzwingen, sonst greift der Localhost-Bypass.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/write")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn safe_get_passiert_csrf_layer() {
        let app = guarded_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/read")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn write_von_localhost_passiert_bypass() {
        use axum::extract::ConnectInfo;
        use std::net::SocketAddr;

        let app = guarded_router();
        // Loopback-Host UND Loopback-Peer-IP → Bypass (interne Tools); kein 403
        // obwohl kein Token (is_local_request verlangt beide Bedingungen).
        let mut req = Request::builder()
            .method("POST")
            .uri("/write")
            .header("host", "127.0.0.1:8767")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo("127.0.0.1:5555".parse::<SocketAddr>().unwrap()));

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
