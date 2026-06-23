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
//! - Direkte Loopback-Requests → durchgelassen (kein Browser-CSRF-Vektor;
//!   loopback-only interne Tools, vor allem Changelog-Spiegelung).
//! - Write mit gültigem Token oder Same-Origin-Session → durchgelassen.
//! - Cross-Origin oder Write ohne gültige Session → `403 csrf_failed`.
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

    // Direkter Loopback-Bypass (interne loopback-only Tools).
    if is_local_request(&parts) {
        return next.run(Request::from_parts(parts, body)).await;
    }

    let Some(state) = parts.extensions.get::<DashboardAuthState>().cloned() else {
        // Auth-State fehlt → kein Validierungspfad → fail-closed.
        return csrf_failed();
    };

    if !is_allowed_origin(&parts.headers) {
        return csrf_failed();
    }

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

    let token_valid = match (&admin_cookie, &partner_cookie) {
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

    let session_valid = if token_valid {
        true
    } else {
        // F2: Der tokenlose Browser-Fallback trägt sicherheitlich auf SameSite=Lax
        // plus Origin/Referer-Gate; ohne gültige DB-Session bleibt er zu.
        match (&admin_cookie, &partner_cookie) {
            (Some(sid), _) if !sid.is_empty() => state
                .load_admin_session(sid)
                .await
                .ok()
                .flatten()
                .is_some(),
            (_, Some(sid)) if !sid.is_empty() => state
                .load_partner_session(sid)
                .await
                .ok()
                .flatten()
                .is_some(),
            _ => false,
        }
    };

    if session_valid {
        next.run(Request::from_parts(parts, body)).await
    } else {
        csrf_failed()
    }
}

/// Session-Typ-Konstanten der validate_csrf-Lookups (DB-`session_type`).
const ADMIN_COOKIE_NAME_TYPE: &str = "discord_admin";
const PARTNER_COOKIE_NAME_TYPE: &str = "twitch";

// ───────────────────────────────────────────────────────────────────────────
// Same-Origin-Guard (P1.32 / P2.134 / P1.45)
// ───────────────────────────────────────────────────────────────────────────
//
// Browser-CSRF-Schutz OHNE erzwungenen `X-CSRF-Token`-Header. Hintergrund
// (dokumentierter Prod-Vorfall #235): `auth-status` liefert `csrfToken: null`,
// daher kann das v2-Frontend keinen Header mitschicken — ein harter Header-Zwang
// erzeugte 403-Login-Loops in Prod. Stattdessen prüfen wir die HTTP-`Origin`-
// (bzw. `Referer`-)Herkunft gegen den `Host` der Anfrage. Zusammen mit
// `SameSite=Lax`-Session-Cookies deckt das den Browser-CSRF-Vektor ab, weil ein
// fremder Origin im Cross-Site-POST sichtbar abweicht.

use axum::http::header::{HeaderMap, HOST, ORIGIN, REFERER};

/// Liest den Host-Teil (ohne Port) aus einem absoluten URL-String (z. B. dem
/// `Origin`- oder `Referer`-Header). `None`, wenn kein parsebarer Host enthalten.
fn url_host(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    url.host_str().map(|h| h.to_ascii_lowercase())
}

/// Host-Teil (ohne Port) aus dem `Host`-Request-Header.
fn request_host(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(HOST).and_then(|v| v.to_str().ok())?.trim();
    if raw.is_empty() {
        return None;
    }
    // `[::1]:port` / `host:port` → Host-Teil isolieren.
    let host = if let Some(stripped) = raw.strip_prefix('[') {
        // IPv6-Literal in Brackets.
        stripped.split(']').next().unwrap_or(stripped)
    } else {
        raw.split(':').next().unwrap_or(raw)
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// Ergebnis der Same-Origin-Prüfung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginCheck {
    /// `Origin`/`Referer` stimmt mit dem Request-`Host` überein → erlaubt.
    SameOrigin,
    /// `Origin`/`Referer` zeigt auf einen fremden Host → Cross-Origin (403).
    CrossOrigin,
    /// Weder `Origin` noch `Referer` vorhanden — keine Browser-Cross-Site-
    /// Information. Der Aufrufer entscheidet (i. d. R. erlaubt, da Nicht-Browser-
    /// Clients wie curl/interne Tools keinen Origin senden; sie passieren
    /// ohnehin den Auth-Gate).
    Missing,
}

/// Prüft, ob ein zustandsänderndes Request same-origin ist.
///
/// Vergleicht den Host aus `Origin` (bevorzugt) bzw. `Referer` gegen den
/// `Host`-Header. Fehlt der `Host`-Header, gilt es als Cross-Origin (fail-closed),
/// denn ohne bekannten Ziel-Host lässt sich Herkunft nicht bestätigen.
pub fn check_same_origin(headers: &HeaderMap) -> OriginCheck {
    let Some(host) = request_host(headers) else {
        return OriginCheck::CrossOrigin;
    };

    let origin_host = headers
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(url_host);
    if let Some(origin_host) = origin_host {
        return if origin_host == host {
            OriginCheck::SameOrigin
        } else {
            OriginCheck::CrossOrigin
        };
    }

    let referer_host = headers
        .get(REFERER)
        .and_then(|v| v.to_str().ok())
        .and_then(url_host);
    if let Some(referer_host) = referer_host {
        return if referer_host == host {
            OriginCheck::SameOrigin
        } else {
            OriginCheck::CrossOrigin
        };
    }

    OriginCheck::Missing
}

/// `true`, wenn das Request NICHT als Cross-Origin-Browser-Request erkannt wird.
///
/// `SameOrigin` und `Missing` (Nicht-Browser/interne Clients ohne Origin) gelten
/// als erlaubt; nur ein nachweislich fremder Origin (`CrossOrigin`) wird
/// abgelehnt. So bleibt der Browser-CSRF-Vektor zu, ohne legitime Header-lose
/// Clients (curl, interne Tools) zu sperren (Vorfall #235).
pub fn is_allowed_origin(headers: &HeaderMap) -> bool {
    !matches!(check_same_origin(headers), OriginCheck::CrossOrigin)
}

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
        // Nicht-Loopback-Host erzwingen, sonst greift der Loopback-Bypass.
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

    // ── Same-Origin-Guard (P1.32 / P2.134 / P1.45) ─────────────────────────
    use axum::http::HeaderMap;

    fn headers_with(host: &str, origin: Option<&str>, referer: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(super::HOST, host.parse().unwrap());
        if let Some(o) = origin {
            h.insert(super::ORIGIN, o.parse().unwrap());
        }
        if let Some(r) = referer {
            h.insert(super::REFERER, r.parse().unwrap());
        }
        h
    }

    #[test]
    fn same_origin_via_origin_header() {
        let h = headers_with("dash.example.com", Some("https://dash.example.com"), None);
        assert_eq!(check_same_origin(&h), OriginCheck::SameOrigin);
        assert!(is_allowed_origin(&h));
    }

    #[test]
    fn cross_origin_via_origin_header_abgelehnt() {
        let h = headers_with("dash.example.com", Some("https://evil.example.org"), None);
        assert_eq!(check_same_origin(&h), OriginCheck::CrossOrigin);
        assert!(!is_allowed_origin(&h));
    }

    #[test]
    fn origin_mit_port_match() {
        // Host trägt Port, Origin nur Host → Host-Vergleich ignoriert Port.
        let h = headers_with("dash.example.com:8769", Some("https://dash.example.com"), None);
        assert_eq!(check_same_origin(&h), OriginCheck::SameOrigin);
    }

    #[test]
    fn referer_fallback_wenn_origin_fehlt() {
        let same = headers_with("dash.example.com", None, Some("https://dash.example.com/x"));
        assert_eq!(check_same_origin(&same), OriginCheck::SameOrigin);
        let cross = headers_with("dash.example.com", None, Some("https://evil.org/x"));
        assert_eq!(check_same_origin(&cross), OriginCheck::CrossOrigin);
    }

    #[test]
    fn fehlender_origin_und_referer_ist_missing_aber_erlaubt() {
        // Nicht-Browser-Client (curl) ohne Origin/Referer → Missing → erlaubt.
        let h = headers_with("dash.example.com", None, None);
        assert_eq!(check_same_origin(&h), OriginCheck::Missing);
        assert!(is_allowed_origin(&h));
    }

    #[test]
    fn fehlender_host_ist_cross_origin_fail_closed() {
        let h = HeaderMap::new();
        assert_eq!(check_same_origin(&h), OriginCheck::CrossOrigin);
        assert!(!is_allowed_origin(&h));
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

    async fn maybe_test_state() -> Option<(sqlx::PgPool, DashboardAuthState)> {
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = sqlx::PgPool::connect(&url).await.ok()?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .ok()?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id BIGINT PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                twitch_user_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                technical_pause_reason TEXT,
                admin_archived_at TEXT,
                departnered_at TEXT,
                partnered_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .ok()?;
        let state = DashboardAuthState::new(pool.clone(), "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string());
        Some((pool, state))
    }

    async fn ensure_partner(pool: &sqlx::PgPool, id: i64, login: &str, user_id: &str) {
        sqlx::query("DELETE FROM twitch_partners WHERE id = $1 OR twitch_login = $2 OR twitch_user_id = $3")
            .bind(id)
            .bind(login)
            .bind(user_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (id, twitch_login, twitch_user_id, status)
             VALUES ($1, $2, $3, 'active')
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(login)
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn post_with_state(state: DashboardAuthState, cookie: Option<String>, origin: Option<&str>) -> Response {
        let app = guarded_router();
        let mut builder = Request::builder()
            .method("POST")
            .uri("/write")
            .header("host", "dashboard.example.com");
        if let Some(cookie) = cookie {
            builder = builder.header("cookie", cookie);
        }
        if let Some(origin) = origin {
            builder = builder.header("origin", origin);
        }
        let mut req = builder.body(Body::empty()).unwrap();
        req.extensions_mut().insert(state);
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn same_origin_session_ohne_token_passiert() {
        let Some((pool, state)) = maybe_test_state().await else { return; };
        ensure_partner(&pool, 9062401, "csrf_fallback", "9062401").await;
        let session = state.create_partner_session("csrf_fallback", "9062401", "CSRF Fallback").await.unwrap();
        let resp = post_with_state(state.clone(), Some(format!("{}={}", PARTNER_COOKIE_NAME, session.session_id)), Some("https://dashboard.example.com")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1").bind(&session.session_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM twitch_partners WHERE id = 9062401").execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn cross_origin_session_ohne_token_403() {
        let Some((pool, state)) = maybe_test_state().await else { return; };
        ensure_partner(&pool, 9062402, "csrf_cross", "9062402").await;
        let session = state.create_partner_session("csrf_cross", "9062402", "CSRF Cross").await.unwrap();
        let resp = post_with_state(state.clone(), Some(format!("{}={}", PARTNER_COOKIE_NAME, session.session_id)), Some("https://evil.example.org")).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1").bind(&session.session_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM twitch_partners WHERE id = 9062402").execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn same_origin_ohne_session_403() {
        let Some((_pool, state)) = maybe_test_state().await else { return; };
        let resp = post_with_state(state, None, Some("https://dashboard.example.com")).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
