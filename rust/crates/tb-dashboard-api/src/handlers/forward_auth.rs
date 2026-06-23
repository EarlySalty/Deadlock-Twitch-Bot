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
//! Admin/Partner/None aus Cookies auf. Admin = gültige `master_dash_session`
//! (Discord-Admin) ODER ein `_TWITCH_ADMIN_LOGINS`-Login über die Twitch-Session
//! mit aktivem Admin-Mode. Admin ist „privilegiert" → 200; alles andere → 401.
//!
//! **Device-Bindung (P1.39, konditional):** Pythons `validate_admin_session`
//! prüft für die Discord-Admin-Session (`master_dash_session`) zusätzlich
//! IP-Bindung, Passive-Fingerprint und `fp_pending`. Diese Checks sind hier
//! restauriert — aber **nur konditional**: sie greifen ausschließlich, wenn die
//! geladene Admin-Session die jeweiligen Felder (`client_ip`/`passive_fp`/
//! `fp_pending`) tatsächlich trägt. Eine native Rust-Admin-Session
//! (`create_admin_session`) trägt sie nicht → die Checks werden übersprungen,
//! genau wie Pythons konditionale `if stored_ip:` / `if stored_passive_fp:`.
//!
//! **Bewusst NICHT portiert (#235-Lockout-Schutz, B3-10):** Pythons *harter*
//! js_fp-Pflicht-Zweig (`source != "discord_dashboard"` UND leerer `js_fp` → 401)
//! bleibt aus. Der Rust-Prozess hat keinen nativen Discord-Admin-Login, der
//! `source`/`js_fp`/`fp_pending=False` setzt (vertagt nach B3-10, vgl. P2.118);
//! ein 1:1-Port würde jede native Admin-Session 401en → der dokumentierte
//! #235-Login-Loop. Sobald der native Discord-Admin-Login landet, kommt der
//! js_fp-Pflicht-Zweig zusammen mit seinen Schreibpfaden hinzu.
//!
//! **Secrets:** Es werden keine Token verglichen oder geloggt; die einzige
//! Geheimnis-nahe Operation (Session-Cookie → DB-Lookup) liegt in der
//! Auth-Kaskade, die konstant-zeitlich nichts vergleicht außer dem CSRF-Token
//! (hier nicht berührt). Der Username im `X-Admin-User`-Header ist kein Secret.

use std::net::SocketAddr;

use axum::{
    extract::ConnectInfo,
    http::{request::Parts, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::auth::level::{extract_cookie, DashboardAuthLevel};
use crate::auth::session::{build_passive_fp, DashboardAuthState};

/// Cookie-Name der Discord-Admin-Session (Python `_discord_admin_cookie_name`).
const ADMIN_COOKIE_NAME: &str = "master_dash_session";

/// `GET /twitch/auth/validate` — Forward-Auth-Check für Caddy.
///
/// - Admin → **200** (autorisiert) — sofern die Discord-Admin-
///   Session-Bindung (IP/Passive-FP/fp_pending, P1.39) den Request akzeptiert.
/// - Partner ODER None    → **401** (nicht autorisiert).
///
/// Die Antwort trägt `Cache-Control: no-store` (Auth-Antworten dürfen nicht
/// gecacht werden, Python `_set_no_store_headers`) und bei Erfolg einen
/// `X-Admin-User`-Header (Python setzt ihn ebenfalls; nützlich fürs Logging
/// stromabwärts).
pub async fn validate_admin_session(auth: DashboardAuthLevel, parts: Parts) -> Response {
    if !auth.is_privileged() {
        return forward_response(StatusCode::UNAUTHORIZED, None);
    }

    // Discord-Admin-Session-Bindung (P1.39): nur wenn der Request über das
    // master_dash_session-Cookie als Admin aufgelöst wurde, die Session-Bindung
    // gegen IP + Passive-FP + fp_pending prüfen. Twitch-Admin-Login
    // (kein master_dash_session-Cookie) bleibt unberührt.
    let mut admin_user = "admin".to_string();
    if let Some(session_id) = extract_cookie(&parts, ADMIN_COOKIE_NAME) {
        let session_id = session_id.trim();
        if !session_id.is_empty() {
            if let Some(state) = parts.extensions.get::<DashboardAuthState>() {
                match state.load_admin_session_fingerprint(session_id).await {
                    Ok(Some(fp)) => {
                        let current_ip = client_ip(&parts);
                        let current_passive_fp = current_passive_fp(&parts);
                        if !fp.verify(&current_ip, &current_passive_fp) {
                            return forward_response(StatusCode::UNAUTHORIZED, None);
                        }
                        admin_user = fp.username;
                    }
                    // Cookie gesetzt, aber keine gültige discord_admin-Session: der
                    // Admin-Status kam über einen anderen Pfad (Twitch-Admin-Login)
                    // — kein Lockout, weiter mit Default-User.
                    Ok(None) => {}
                    // DB-Fehler → fail-closed (Python wirft, Caddy bekommt kein 200).
                    Err(_) => return forward_response(StatusCode::UNAUTHORIZED, None),
                }
            }
        }
    }

    forward_response(StatusCode::OK, Some(admin_user))
}

/// Baut die Forward-Auth-Antwort mit `no-store` und optionalem `X-Admin-User`.
fn forward_response(status: StatusCode, admin_user: Option<String>) -> Response {
    let mut response = status.into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    if let Some(user) = admin_user {
        if let Ok(value) = HeaderValue::from_str(&user) {
            headers.insert("X-Admin-User", value);
        } else {
            headers.insert("X-Admin-User", HeaderValue::from_static("admin"));
        }
    }
    response
}

/// Aktuelle Client-IP für die Session-Bindung. Bevorzugt die echte Peer-IP; hinter
/// dem Loopback-Reverse-Proxy (Caddy) den ersten `X-Forwarded-For`-Eintrag. Leer,
/// wenn keine Client-IP vorliegt (Caddy liefert sie auf dem Auth-Subrequest nicht
/// zuverlässig — dann wird der IP-Check übersprungen, Python-Parität).
fn client_ip(parts: &Parts) -> String {
    if let Some(ci) = parts.extensions.get::<ConnectInfo<SocketAddr>>() {
        let ip = ci.0.ip();
        if !ip.is_loopback() {
            return ip.to_string();
        }
    }
    if let Some(xff) = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            let first = first.trim();
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    String::new()
}

/// Berechnet den Passive-Fingerprint des aktuellen Requests aus `User-Agent`,
/// erstem `Accept-Language`-Eintrag und `Sec-CH-UA-Platform` (Python-Parität).
fn current_passive_fp(parts: &Parts) -> String {
    let header = |name: axum::http::HeaderName| -> &str {
        parts.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
    };
    let ua = header(axum::http::header::USER_AGENT);
    let lang = header(axum::http::header::ACCEPT_LANGUAGE);
    let platform = parts
        .headers
        .get("sec-ch-ua-platform")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    build_passive_fp(ua, lang, platform)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_parts() -> Parts {
        axum::http::Request::builder().body(()).unwrap().into_parts().0
    }

    #[tokio::test]
    async fn admin_session_gibt_200() {
        // Admin ohne master_dash_session-Cookie (z. B. Twitch-Admin-Login) → 200.
        let resp = validate_admin_session(DashboardAuthLevel::admin(), empty_parts()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("X-Admin-User").unwrap(), "admin");
        assert_eq!(
            resp.headers().get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-store, max-age=0"
        );
    }

    #[tokio::test]
    async fn admin_ohne_cookie_gibt_200() {
        let resp = validate_admin_session(DashboardAuthLevel::admin(), empty_parts()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get("X-Admin-User").is_some());
    }

    #[tokio::test]
    async fn keine_session_gibt_401() {
        let resp = validate_admin_session(DashboardAuthLevel::None, empty_parts()).await;
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
        let resp = validate_admin_session(
            DashboardAuthLevel::Partner {
                twitch_login: "somepartner".into(),
                twitch_user_id: "12345".into(),
                display_name: String::new(),
            },
            empty_parts(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get("X-Admin-User").is_none());
    }

    #[test]
    fn client_ip_aus_x_forwarded_for() {
        let parts = axum::http::Request::builder()
            .header("x-forwarded-for", "203.0.113.7, 10.0.0.1")
            .body(())
            .unwrap()
            .into_parts()
            .0;
        assert_eq!(client_ip(&parts), "203.0.113.7");
    }

    #[test]
    fn client_ip_leer_ohne_quelle() {
        assert_eq!(client_ip(&empty_parts()), "");
    }

    #[test]
    fn passive_fp_aus_request_headern() {
        let parts = axum::http::Request::builder()
            .header(axum::http::header::USER_AGENT, "ua")
            .header(axum::http::header::ACCEPT_LANGUAGE, "de,en;q=0.9")
            .header("sec-ch-ua-platform", "\"Windows\"")
            .body(())
            .unwrap()
            .into_parts()
            .0;
        assert_eq!(current_passive_fp(&parts), build_passive_fp("ua", "de", "Windows"));
    }
}
