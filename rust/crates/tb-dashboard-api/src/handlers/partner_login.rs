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
    http::{header::SET_COOKIE, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Extension, Json,
};
use serde_json::json;
use sqlx::PgPool;
use tracing::warn;

use crate::auth::level::DashboardAuthLevel;
use crate::auth::oauth_login::sanitize_next_path;
use crate::auth::partner_login::{PartnerLoginToken, PARTNER_LOGIN_TOKEN_TTL_SECS};
use crate::auth::session::{
    build_session_cookie, DashboardAuthState, SameSite, PARTNER_COOKIE_NAME,
    SESSION_CREATE_TTL_SECS,
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
    body: Option<Json<LinkBody>>,
) -> Response {
    let _ = &pool; // State trägt den Pool; hier nur Symmetrie zu anderen Handlern.
    if !auth.is_privileged() {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "admin_required" }))).into_response();
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
    body: String,
) -> Response {
    let _ = &pool;
    let Some(Extension(state)) = state else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Partner-Login nicht verfügbar.").into_response();
    };
    let Some(secret) = partner_secret() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Partner-Login nicht verfügbar.").into_response();
    };

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

    // 4. Session anlegen + Cookie setzen + Redirect.
    let session = match state
        .create_partner_session(&partner.twitch_login, &partner.twitch_user_id, "")
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
        PARTNER_COOKIE_NAME,
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
