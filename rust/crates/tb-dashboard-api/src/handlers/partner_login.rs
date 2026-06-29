//! Partner-Einmal-Login via HMAC One-Time-Token (B3-8 / auth-core-4).
//!
//! Zwei Routen (Python `auth_partner_link`/`auth_partner_login`,
//! routes_mixin.py:630-631):
//!
//! - `POST /twitch/auth/partner/link` — **Admin/Localhost** stellt einen
//!   einmaligen Login-Link für einen Partner aus. Erzeugt eine `sid`, signiert
//!   einen HMAC-Token (Secret `TWITCH_PARTNER_TOKEN`) und persistiert den State
//!   (`save_partner_login_state`, Typ `oauth_state:partner_login`). Antwort:
//!   `{ login_path, login_method, login_token, next_path, expires_in }`.
//! - `POST /twitch/auth/partner/login` — verbraucht den Token: HMAC-Verify +
//!   atomarer Single-Use-Consume des States + Partner-Auflösung + Session-
//!   Erstellung + Cookie + 302-Redirect aufs Dashboard.
//!
//! Secret-Quelle: `TWITCH_PARTNER_TOKEN` aus dem Prozess-Env (Infisical injiziert
//! es). Fehlt es → 503 (Feature aus), niemals Secret loggen/ausgeben.

use axum::{
    extract::State,
    http::{header::SET_COOKIE, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Extension, Json,
};
use serde_json::json;
use sqlx::PgPool;
use tracing::warn;

use crate::auth::csrf::is_allowed_origin;
use crate::auth::level::DashboardAuthLevel;
use crate::auth::oauth_login::sanitize_next_path;
use crate::auth::partner_login::{PartnerLoginToken, PARTNER_LOGIN_TOKEN_TTL_SECS};
use crate::auth::session::{
    build_session_cookie, DashboardAuthState, SameSite, ADMIN_COOKIE_NAME,
    PARTNER_ACCESS_COOKIE_NAME, PARTNER_COOKIE_NAME, SESSION_CREATE_TTL_SECS,
};
use crate::handlers::auth_login::OAuthLoginConfig;

const LOGIN_PATH: &str = "/twitch/auth/partner/login";
const STATE_ID_BYTES: usize = 24;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// HMAC-Signing-Secret aus dem Prozess-Env (Infisical). Leer/fehlend → `None`.
fn partner_secret() -> Option<String> {
    std::env::var("TWITCH_PARTNER_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn no_store(mut resp: Response) -> Response {
    use axum::http::header::{HeaderValue, CACHE_CONTROL, PRAGMA};
    resp.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store, max-age=0"));
    resp.headers_mut().insert(PRAGMA, HeaderValue::from_static("no-cache"));
    resp
}

// ── POST /twitch/auth/partner/link (Admin stellt Link aus) ───────────────────

#[derive(serde::Deserialize, Default)]
pub struct LinkBody {
    /// Ziel-Partner-Login, für den der Einmal-Link gilt.
    #[serde(default)]
    pub login: Option<String>,
    /// Redirect-Pfad nach erfolgreichem Login (Default `/analyse`).
    #[serde(default)]
    pub next: Option<String>,
}

/// `POST /twitch/auth/partner/link` — Admin/Localhost erzeugt einen Einmal-Link.
pub async fn link_handler(
    auth: DashboardAuthLevel,
    state: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: Option<Json<LinkBody>>,
) -> Response {
    let _ = &pool; // State trägt den Pool; hier nur Symmetrie zu anderen Handlern.
    if !auth.is_privileged() {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "admin_required" }))).into_response();
    }
    // P2.134: Same-Origin-Guard für Browser-Admin-Caller (Cookie-Session).
    // Ein nachweislich fremder Origin auf der Link-Ausstellung → 403 (Vorfall #235:
    // kein harter X-CSRF-Header-Zwang, sondern Origin/Referer same-origin).
    if !is_allowed_origin(&headers) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "csrf_failed" }))).into_response();
    }
    let Some(Extension(state)) = state else {
        // Ohne Auth-State kein Persistenz-Pfad → Feature aus.
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "auth_unavailable" }))).into_response();
    };
    let Some(secret) = partner_secret() else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "partner_login_disabled" }))).into_response();
    };

    let body = body.map(|Json(b)| b).unwrap_or_default();
    let login = body
        .login
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);
    let Some(login) = login else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "login required" }))).into_response();
    };
    let next_path = sanitize_next_path(body.next.as_deref());

    let now = unix_now();
    let sid = tb_crypto::random_urlsafe_token(STATE_ID_BYTES);
    let token = PartnerLoginToken::new(sid.clone(), next_path.clone(), now, PARTNER_LOGIN_TOKEN_TTL_SECS);
    let wire = token.sign(secret.as_bytes());

    if let Err(error) = state
        .save_partner_login_state(&sid, &login, &next_path, PARTNER_LOGIN_TOKEN_TTL_SECS)
        .await
    {
        warn!(%error, "Partner-Login-State persistieren fehlgeschlagen");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "state_unavailable" }))).into_response();
    }

    no_store(
        Json(json!({
            "login_path": LOGIN_PATH,
            "login_method": "POST",
            "login_token": wire,
            "next_path": next_path,
            "expires_in": PARTNER_LOGIN_TOKEN_TTL_SECS,
        }))
        .into_response(),
    )
}

// ── POST /twitch/auth/partner/login (Token verbrauchen) ──────────────────────

/// `POST /twitch/auth/partner/login` — Token verbrauchen, Session anlegen.
///
/// Token aus JSON-Body (`{"token": "..."}`) ODER form-encoded (`token=...`).
pub async fn login_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let _ = &pool;
    let Some(Extension(state)) = state else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Partner-Login nicht verfügbar.").into_response();
    };
    let Some(secret) = partner_secret() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Partner-Login nicht verfügbar.").into_response();
    };

    // P3.27: Existiert bereits eine gültige Session (Admin / Partner / durable
    // Partner-Access), brechen wir VOR dem Token-Consume ab und leiten schlicht
    // aufs Dashboard um — kein neuer Token-Verbrauch, kein neues Set-Cookie, keine
    // neue Session-Row (Python `auth_partner_login`-Short-Circuit).
    if has_active_dashboard_session(&state, &headers).await {
        return no_store(Redirect::to("/analyse").into_response());
    }

    let Some(token) = extract_token(&body) else {
        return (StatusCode::BAD_REQUEST, "Partner-Login-Token fehlt.").into_response();
    };

    // 1. HMAC-Verify (Signatur + Version + Zeitfenster).
    let now = unix_now();
    let parsed = match PartnerLoginToken::verify(&token, secret.as_bytes(), now) {
        Ok(t) => t,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "Partner-Login-Token ungültig oder abgelaufen.").into_response();
        }
    };

    // 2. Atomar einmaligen State verbrauchen (Replay-Schutz).
    let (login, stored_next) = match state.consume_partner_login_state(&parsed.sid).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "Partner-Login-Token ungültig oder abgelaufen.").into_response();
        }
        Err(error) => {
            warn!(%error, "Partner-Login-State consume DB-Fehler");
            return (StatusCode::SERVICE_UNAVAILABLE, "Partner-Login konnte nicht abgeschlossen werden.").into_response();
        }
    };
    // Defensive: next-Pfad aus Token und State müssen übereinstimmen (Python-Parität).
    if parsed.next != stored_next {
        return (StatusCode::UNAUTHORIZED, "Partner-Login-Token ungültig oder abgelaufen.").into_response();
    }

    // 3. Partner auflösen (nur aktive, nicht-blockierte Partner).
    let partner = match state.find_partner_for_login(&login, "").await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (StatusCode::FORBIDDEN, "Kein aktiver Partner für diesen Login.").into_response();
        }
        Err(error) => {
            warn!(%error, "Partner-Lookup beim Einmal-Login fehlgeschlagen");
            return (StatusCode::SERVICE_UNAVAILABLE, "Partner-Login konnte nicht abgeschlossen werden.").into_response();
        }
    };

    // 4. Durable, geräte-gebundene Partner-Access-Session anlegen (P1.53/P1.54).
    //    Cookie `twitch_dash_session_partner`, Typ `partner_token`, mit
    //    User-Agent-Fingerprint-Bindung. Überdauert den Einmal-Login.
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let session = match state
        .create_partner_access_session(
            &partner.twitch_login,
            &partner.twitch_user_id,
            "",
            user_agent,
        )
        .await
    {
        Ok(s) => s,
        Err(error) => {
            warn!(%error, "Session-Erstellung beim Einmal-Login fehlgeschlagen");
            return (StatusCode::SERVICE_UNAVAILABLE, "Partner-Login konnte nicht abgeschlossen werden.").into_response();
        }
    };

    let cookie_secure = config.as_ref().map(|c| c.0.cookie_secure).unwrap_or(true);
    let cookie = build_session_cookie(
        PARTNER_ACCESS_COOKIE_NAME,
        &session.session_id,
        cookie_secure,
        SameSite::Lax,
        SESSION_CREATE_TTL_SECS,
    );
    let destination = sanitize_next_path(Some(&stored_next));
    let mut response = Redirect::to(&destination).into_response();
    if let Ok(value) = axum::http::HeaderValue::from_str(&cookie) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    no_store(response)
}

/// Liest einen Cookie-Wert direkt aus den Request-Headern.
fn cookie_from_headers(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k.trim() == name {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// `true`, wenn der Request bereits eine gültige Dashboard-Session trägt
/// (Admin, Partner ODER durable Partner-Access). Grundlage für den
/// Existing-Session-Short-Circuit (P3.27). DB-Fehler → behandeln wir als „keine
/// Session" (fail-open in Richtung normalem Login-Flow, nicht in Richtung Zugang).
async fn has_active_dashboard_session(state: &DashboardAuthState, headers: &HeaderMap) -> bool {
    if let Some(sid) = cookie_from_headers(headers, ADMIN_COOKIE_NAME) {
        if matches!(state.load_admin_session(&sid).await, Ok(Some(true))) {
            return true;
        }
    }
    if let Some(sid) = cookie_from_headers(headers, PARTNER_COOKIE_NAME) {
        if matches!(state.load_partner_session(&sid).await, Ok(Some(_))) {
            return true;
        }
    }
    if let Some(sid) = cookie_from_headers(headers, PARTNER_ACCESS_COOKIE_NAME) {
        let ua = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if matches!(state.load_partner_access_session(&sid, ua).await, Ok(Some(_))) {
            return true;
        }
    }
    false
}

/// Liest `token` aus JSON (`{"token":...}`) oder form-encoded Body (`token=...`).
fn extract_token(body: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(t) = v.get("token").and_then(|t| t.as_str()) {
            let t = t.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    // form-encoded: token=...
    for pair in body.split('&') {
        if let Some(val) = pair.strip_prefix("token=") {
            let decoded = urldecode(val);
            let decoded = decoded.trim();
            if !decoded.is_empty() {
                return Some(decoded.to_string());
            }
        }
    }
    None
}

/// Minimaler URL-Decode (nur `%XX` + `+`), reicht für den `token`-Form-Wert.
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod route_tests {
    //! DB-gestützte Route-Tests (self-contained Router) für P1.53 (durable
    //! Partner-Access-Cookie) und P3.27 (Existing-Session-Short-Circuit).
    use super::*;
    use crate::auth::session::ADMIN_COOKIE_NAME;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use axum::Router;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::Mutex;
    use tower::ServiceExt;

    /// Serialisiert Tests, die `TWITCH_PARTNER_TOKEN` setzen.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    const TEST_SECRET: &str = "test-partner-secret-xyz";

    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE dashboard_sessions (
                session_id TEXT PRIMARY KEY, session_type TEXT NOT NULL,
                payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL,
                expires_at DOUBLE PRECISION NOT NULL)"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_partners (
                twitch_login TEXT, twitch_user_id TEXT, status TEXT,
                technical_pause_reason TEXT, manual_partner_opt_out INTEGER,
                departnered_at TEXT, admin_archived_at TEXT,
                partnered_at TEXT)"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ('linkpartner', '5551', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn app(pool: PgPool, state: DashboardAuthState) -> Router {
        Router::new()
            .route("/login", post(super::login_handler))
            .layer(Extension(state))
            .with_state(pool)
    }

    /// Baut einen gültigen Login-Token + persistierten State für `linkpartner`.
    async fn mint_token(state: &DashboardAuthState) -> String {
        let now = unix_now();
        let sid = tb_crypto::random_urlsafe_token(STATE_ID_BYTES);
        let token = PartnerLoginToken::new(sid.clone(), "/analyse".to_string(), now, PARTNER_LOGIN_TOKEN_TTL_SECS);
        let wire = token.sign(TEST_SECRET.as_bytes());
        state
            .save_partner_login_state(&sid, "linkpartner", "/analyse", PARTNER_LOGIN_TOKEN_TTL_SECS)
            .await
            .unwrap();
        wire
    }

    /// P1.53: erfolgreicher Einmal-Login setzt das DURABLE Partner-Access-Cookie
    /// (`twitch_dash_session_partner`) mit einer `partner_token`-Row.
    #[tokio::test]
    async fn login_setzt_durable_partner_access_cookie() {
        let Some(pool) = make_pool("t_plogin_durable").await else { return; };
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("TWITCH_PARTNER_TOKEN", TEST_SECRET);
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let wire = mint_token(&state).await;

        let req = Request::builder()
            .method("POST")
            .uri("/login")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"token":"{wire}"}}"#)))
            .unwrap();
        let resp = app(pool.clone(), state).oneshot(req).await.unwrap();

        std::env::remove_var("TWITCH_PARTNER_TOKEN");
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let cookie = resp.headers().get(SET_COOKIE).unwrap().to_str().unwrap();
        assert!(
            cookie.starts_with(&format!("{PARTNER_ACCESS_COOKIE_NAME}=")),
            "muss durables Partner-Access-Cookie setzen, war: {cookie}"
        );

        let sid = cookie
            .strip_prefix(&format!("{PARTNER_ACCESS_COOKIE_NAME}="))
            .and_then(|s| s.split(';').next())
            .unwrap();
        let session_type: String = sqlx::query_scalar(
            "SELECT session_type FROM dashboard_sessions WHERE session_id = $1",
        )
        .bind(sid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(session_type, "partner_token");
    }

    /// P3.27: existiert bereits eine gültige Admin-Session, wird der Token NICHT
    /// verbraucht und KEIN neues Cookie/keine neue Session-Row geschrieben.
    #[tokio::test]
    async fn login_short_circuit_bei_bestehender_session() {
        let Some(pool) = make_pool("t_plogin_shortcircuit").await else { return; };
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("TWITCH_PARTNER_TOKEN", TEST_SECRET);
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let wire = mint_token(&state).await;
        let admin = state.create_admin_session("admin-1", "Admin").await.unwrap();

        let rows_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM dashboard_sessions")
                .fetch_one(&pool)
                .await
                .unwrap();

        let req = Request::builder()
            .method("POST")
            .uri("/login")
            .header("content-type", "application/json")
            .header(
                axum::http::header::COOKIE,
                format!("{ADMIN_COOKIE_NAME}={}", admin.session_id),
            )
            .body(Body::from(format!(r#"{{"token":"{wire}"}}"#)))
            .unwrap();
        let resp = app(pool.clone(), state).oneshot(req).await.unwrap();

        std::env::remove_var("TWITCH_PARTNER_TOKEN");
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        // Kein Set-Cookie (keine neue Session).
        assert!(resp.headers().get(SET_COOKIE).is_none(), "kein neues Cookie");
        // Zeilenzahl unverändert (Token-State NICHT verbraucht, keine neue Session).
        let rows_after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM dashboard_sessions")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rows_after, rows_before, "weder State verbraucht noch Session erzeugt");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_aus_json_und_form() {
        assert_eq!(extract_token(r#"{"token":"abc.def"}"#).as_deref(), Some("abc.def"));
        assert_eq!(extract_token("token=abc.def&x=1").as_deref(), Some("abc.def"));
        // url-encodierter Punkt-Token.
        assert_eq!(extract_token("token=abc%2Edef").as_deref(), Some("abc.def"));
        assert_eq!(extract_token("nothing=here").as_deref(), None);
        assert_eq!(extract_token("").as_deref(), None);
    }
}
