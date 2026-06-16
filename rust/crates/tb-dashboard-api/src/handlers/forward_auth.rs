//! Forward-Auth-Endpoint für Caddys `forward_auth` auf dem Admin-Host (B1-ADMIN-FORWARD-AUTH).
//!
//! Route: `GET /twitch/auth/validate` (vor dem Strangler-Proxy registriert,
//! damit der Admin-Host nicht mehr 502 vom toten Python-Service bekommt).
//!
//! Python-Referenz: `bot/dashboard/server_v2.py:415-485` (`validate_admin_session`)
//! und `routes_entry.py:49` (Route-Registrierung).
//!
//! **Caddy-Vertrag:** Caddy schickt vor jedem Request an den Admin-Host einen
//! Auth-Subrequest hierher. Antwort **200** → autorisiert (Caddy lässt den
//! Original-Request durch), **401** → nicht autorisiert (Caddy leitet auf den
//! Login um / verweigert). Der Endpoint liest KEINEN Request-Body und antwortet
//! ohne Inhalt — nur Status + optionaler `X-Admin-User`-Header.
//!
//! **Auth-Quelle:** Die [`DashboardAuthLevel`]-Kaskade (`auth/level.rs`) löst
//! Localhost/Admin/Partner/None aus Cookie + Loopback-Check auf. Admin =
//! gültige `master_dash_session` (Discord-Admin) ODER ein `_TWITCH_ADMIN_LOGINS`-
//! Login über die Twitch-Session. Localhost = Loopback-Peer + Loopback-Host.
//! Beides ist „privilegiert" → 200; alles andere → 401.
//!
//! **Bewusste Abgrenzung (B3-10, separates Ticket):** Pythons
//! `validate_admin_session` prüft zusätzlich IP-Bindung, Passive-/JS-Fingerprint
//! und `fp_pending` der Admin-Session. Dieses Device-/Canvas-Fingerprinting nach
//! Admin-Login ist Ticket **B3-10** (Phase 1, `dependsOn: B3-2, B3-1`) und wird
//! mit dem Fingerprint-Flow zusammen gebaut — NICHT hier. Dieser Endpoint deckt
//! die Session-Ebene ab (gültige Admin-/Localhost-Session → 200). Sobald B3-10
//! landet, ergänzt es die FP-Checks an dieser Stelle.
//!
//! **Secrets:** Es werden keine Token verglichen oder geloggt; die einzige
//! Geheimnis-nahe Operation (Session-Cookie → DB-Lookup) liegt in der
//! Auth-Kaskade, die konstant-zeitlich nichts vergleicht außer dem CSRF-Token
//! (hier nicht berührt). Der Username im `X-Admin-User`-Header ist kein Secret.

use axum::{
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::auth::level::DashboardAuthLevel;

/// `GET /twitch/auth/validate` — Forward-Auth-Check für Caddy.
///
/// - Localhost ODER Admin → **200** (autorisiert).
/// - Partner ODER None    → **401** (nicht autorisiert).
///
/// Die Antwort trägt `Cache-Control: no-store` (Auth-Antworten dürfen nicht
/// gecacht werden, Python `_set_no_store_headers`) und bei Erfolg einen
/// `X-Admin-User`-Header (Python setzt ihn ebenfalls; nützlich fürs Logging
/// stromabwärts).
pub async fn validate_admin_session(auth: DashboardAuthLevel) -> Response {
    if auth.is_privileged() {
        let mut response = StatusCode::OK.into_response();
        let headers = response.headers_mut();
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, max-age=0"),
        );
        // Python setzt X-Admin-User auf username/display_name der Session, sonst
        // "admin". Die Auth-Kaskade hält den Discord-Admin-Username derzeit nicht
        // vor (load_admin_session liefert nur bool); wir setzen den stabilen
        // Wert "admin". Ein feinerer Username folgt mit B3-10/Session-Payload.
        headers.insert(
            "X-Admin-User",
            HeaderValue::from_static("admin"),
        );
        response
    } else {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, max-age=0"),
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn admin_session_gibt_200() {
        let resp = validate_admin_session(DashboardAuthLevel::admin()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("X-Admin-User").unwrap(),
            "admin"
        );
        assert_eq!(
            resp.headers().get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-store, max-age=0"
        );
    }

    #[tokio::test]
    async fn localhost_gibt_200() {
        let resp = validate_admin_session(DashboardAuthLevel::Localhost).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get("X-Admin-User").is_some());
    }

    #[tokio::test]
    async fn keine_session_gibt_401() {
        let resp = validate_admin_session(DashboardAuthLevel::None).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get("X-Admin-User").is_none());
        assert_eq!(
            resp.headers().get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-store, max-age=0"
        );
    }

    #[tokio::test]
    async fn partner_session_gibt_401() {
        // Ein reiner Partner (kein Admin-Login) ist NICHT autorisiert für den
        // Admin-Host — forward_auth muss 401 liefern.
        let resp = validate_admin_session(DashboardAuthLevel::Partner {
            twitch_login: "somepartner".into(),
            twitch_user_id: "12345".into(),
            display_name: String::new(),
        })
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get("X-Admin-User").is_none());
    }
}
