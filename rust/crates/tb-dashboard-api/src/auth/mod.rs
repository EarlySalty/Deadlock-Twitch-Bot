//! Dashboard-Auth-Modul.
//!
//! Struktur:
//! - `fernet` — Fernet-kompatible Entschlüsselung (Python `cryptography.fernet`)
//! - `session` — DB-Session-Lookup + 5s-In-Memory-Cache
//! - `level` — `DashboardAuthLevel`-Kaskade als axum-Extractor
//!
//! Routing-Integration: Keine Route in main.rs — dieses Modul ist ein Draft
//! für Review. Integration erfolgt nach Abnahme in Welle D.
//!
//! Verwendung:
//! ```rust,ignore
//! use tb_dashboard_api::auth::session::DashboardAuthState;
//! use tb_dashboard_api::auth::level::DashboardAuthLevel;
//!
//! // Im Router:
//! let auth_state = DashboardAuthState::new(pool, fernet_key);
//! router.layer(Extension(auth_state))
//!
//! // In einem Handler:
//! async fn handler(auth: DashboardAuthLevel) -> impl IntoResponse { ... }
//! ```

pub mod csrf;
pub mod discord_admin_login;
pub mod fernet;
pub mod level;
pub mod oauth_login;
pub mod partner_access;
pub mod partner_gate;
pub mod partner_login;
pub mod security;
pub mod session;
pub(crate) mod streamer_scope;

#[cfg(test)]
mod idor_e2e_tests;

pub(crate) use streamer_scope::resolve_streamer_scope;

// ---------------------------------------------------------------------------
// Migriert aus dem früheren auth.rs (IDOR-Guard + Plan-Gating)
// ---------------------------------------------------------------------------

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use sqlx::PgPool;
use tb_analytics::stufe::Stufe;
use tb_http_core::{ApiError, AuthLevel};

use crate::auth::level::DashboardAuthLevel;

/// Prüft ob der anfragende User die angegebene Login abfragen darf.
///
/// - `Admin` → immer OK
/// - `None` → immer 401
///
/// `session_login` und `queried_login` werden für das deferred Partner-Level gebraucht.
pub fn require_owner(
    auth: &AuthLevel,
    _queried_login: &str,
    _session_login: Option<&str>,
) -> Result<(), ApiError> {
    match auth {
        AuthLevel::Admin => Ok(()),
        AuthLevel::None => Err(ApiError::unauthorized()),
    }
}

pub fn admin_required_error() -> ApiError {
    ApiError::forbidden_with_body(json!({
        "error": "admin_required",
        "required": "admin",
    }))
}

pub fn auth_required_error() -> ApiError {
    ApiError::unauthorized_with_body(json!({
        "error": "auth_required",
        "required": "admin",
    }))
}

pub fn require_admin(auth: &DashboardAuthLevel) -> Option<ApiError> {
    if auth.is_privileged() {
        None
    } else if auth.is_authenticated() {
        Some(admin_required_error())
    } else {
        Some(auth_required_error())
    }
}

fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

pub async fn require_admin_before_csrf(
    auth: DashboardAuthLevel,
    request: Request<Body>,
    next: Next,
) -> Response {
    if is_write_method(request.method()) {
        if let Some(error) = require_admin(&auth) {
            return error.into_response();
        }
    }
    next.run(request).await
}

pub fn admin_required_response() -> Response {
    admin_required_error().into_response()
}

pub fn auth_required_response() -> Response {
    auth_required_error().into_response()
}

/// Prüft ob `login` mindestens die Stufe Netzwerk Plus hat.
///
/// Admin überspringt die Prüfung (bypass).
pub async fn require_extended_plan(
    pool: &PgPool,
    login: &str,
    auth: &AuthLevel,
) -> Result<(), ApiError> {
    if auth.is_privileged() {
        return Ok(());
    }
    // AuthLevel-Pfad kennt keine twitch_user_id → login-only (leerer user_id;
    // der Resolver-Guard `$2 <> ''` verhindert Falsch-Matches).
    if has_analytics_entitlement(pool, login, "").await {
        Ok(())
    } else {
        Err(ApiError::plan_required())
    }
}

/// Die Stufe eines Streamers, fail-closed.
///
/// Ein einziges Praedikat fuer alle Sperren: [`tb_analytics::stufe::plan_stufe`]
/// nutzt denselben Resolver wie das Dashboard (Manual-Override, Stripe-Abo,
/// Trial, Ablaufpruefung). Bei DB-Fehler `Free`, damit ein Ausfall keinen
/// Gratis-Zugang oeffnet.
pub async fn plan_stufe(pool: &PgPool, login: &str, user_id: &str) -> Stufe {
    tb_analytics::stufe::plan_stufe_mit_user_id(pool, login, user_id)
        .await
        .unwrap_or(Stufe::Free)
}

/// Stufe der aufrufenden Session.
///
/// Admin und Localhost zaehlen als `Pro` (Bypass, wie bisher beim Plan-Gate);
/// eine Session ohne Auth ist `Free`. Handler, die zwischen "gekuerzt" und
/// "voll" unterscheiden, fragen nur diesen Wert.
pub async fn stufe_fuer_auth(pool: &PgPool, auth: &DashboardAuthLevel) -> Stufe {
    match auth {
        DashboardAuthLevel::Admin { .. } => Stufe::Pro,
        DashboardAuthLevel::None => Stufe::Free,
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => plan_stufe(pool, twitch_login, twitch_user_id).await,
    }
}

/// `true` ab Netzwerk Plus. Abgeleitet aus [`plan_stufe`], damit es nur eine
/// Entscheidungsstelle gibt; das Entitlement `"analytics"` ist die Anzeige-
/// Ableitung, nicht mehr die Grundlage.
pub async fn has_analytics_entitlement(pool: &PgPool, login: &str, user_id: &str) -> bool {
    plan_stufe(pool, login, user_id).await.hat_plus()
}

/// Plan-Gate für die DashboardAuthLevel-Handler. Gibt `Some(response)` zurück,
/// wenn der Request blockiert ist (401 unauth / 403 plan_required), sonst `None`.
///
/// - Admin → durchgelassen (Bypass, wie Python `_require_extended_plan`)
/// - Partner → braucht aktiven Extended-Plan oder laufenden Trial
/// - None → 401
///
/// Response-Shapes B16-FIX-SHAPE-PARITY (Python `_require_v2_auth` /
/// `_require_extended_plan`): 401 trägt den vollen Message-Text + `loginUrl`,
/// 403 trägt `error=plan_required` + `required_entitlements`.
pub async fn extended_gate(pool: &PgPool, auth: &DashboardAuthLevel) -> Option<Response> {
    stufen_gate(pool, auth, Stufe::Plus).await
}

/// Gate fuer die Pro-Grenze (Clips ohne Limit, automatisches Posten).
pub async fn pro_gate(pool: &PgPool, auth: &DashboardAuthLevel) -> Option<Response> {
    stufen_gate(pool, auth, Stufe::Pro).await
}

/// Gemeinsamer Kern beider Gates: `None` heisst durchgelassen.
///
/// Ohne Auth 401, unterhalb der geforderten Stufe 403 mit der noetigen Stufe im
/// Body. Admin/Localhost sind Pro und passieren immer.
pub async fn stufen_gate(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    benoetigt: Stufe,
) -> Option<Response> {
    if matches!(auth, DashboardAuthLevel::None) {
        return Some(unauthorized_v2_response());
    }
    if stufe_fuer_auth(pool, auth).await >= benoetigt {
        None
    } else {
        Some(stufe_required_response(benoetigt))
    }
}

/// Verlaufsfenster fuer diese Session.
///
/// Kernstueck der Free-Regel: **keine 403-Wand auf Verlaufs- und Vergleichs-
/// endpunkten.** Ohne Plus wird die angefragte Tageszahl auf das letzte
/// Stream-Fenster geklemmt statt die Antwort zu verweigern; Free sieht damit
/// eine vollwertige, nur kuerzere Seite. Ab Plus bleibt die Nachfrage
/// unveraendert.
///
/// Liefert `(effektive_tage, stufe, gekuerzt)`.
pub async fn verlauf_fenster(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    streamer: &str,
    angefragt: i64,
) -> (i64, Stufe, bool) {
    let stufe = stufe_fuer_auth(pool, auth).await;
    let (tage, gekuerzt) =
        tb_analytics::stufe::verlauf_tage_fuer(pool, stufe, streamer, angefragt).await;
    (tage, stufe, gekuerzt)
}

/// Haengt den Stufen-Hinweis als Header an eine Antwort.
///
/// Header statt Body, weil ein Teil der Verlaufsendpunkte ein nacktes Array
/// liefert und die Form nicht brechen soll. `x-plan-stufe` steht immer drin,
/// `x-plan-gekuerzt`/`x-plan-fenster-tage` nur bei gekuerzter Antwort.
pub fn mit_plan_hinweis(
    mut response: Response,
    stufe: Stufe,
    fenster_tage: i64,
    gekuerzt: bool,
) -> Response {
    use axum::http::HeaderValue;
    let headers = response.headers_mut();
    headers.insert("x-plan-stufe", HeaderValue::from_static(match stufe {
        Stufe::Free => "free",
        Stufe::Plus => "plus",
        Stufe::Pro => "pro",
    }));
    if gekuerzt {
        headers.insert("x-plan-gekuerzt", HeaderValue::from_static("1"));
        if let Ok(value) = HeaderValue::from_str(&fenster_tage.to_string()) {
            headers.insert("x-plan-fenster-tage", value);
        }
    }
    response
}

/// 401-Body wie Python `_require_v2_auth` (api_v2.py:1258-1262).
pub fn unauthorized_v2_response() -> Response {
    unauthorized_v2_json().into_response()
}

pub fn unauthorized_v2_json() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "Authentication required. Use Twitch login, an admin token, or access from localhost.",
            "loginUrl": "/twitch/auth/login?next=%2Fanalyse",
        })),
    )
}

pub fn analytics_request_failed_json() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "internal_error",
            "code": "analytics_request_failed",
        })),
    )
}

/// 403-Body wie Python `_require_extended_plan` (api_v2.py:656-664).
///
/// B16-VERIFY-V2AUTH-SHAPE: Python sendet im 403-Body NICHT nur `error` +
/// `required_entitlements`, sondern zusätzlich `required_plans` — die sortierte
/// Liste aller bekannten Billing-Pläne (`KNOWN_PLAN_IDS`), die das konsolidierte
/// `analytics`-Entitlement tragen. Das Frontend nutzt diese Liste, um dem Nutzer
/// die buchbaren Upgrade-Pläne anzuzeigen. Die frühere Rust-Antwort ließ das Feld
/// weg (Shape-Divergenz) — hier korrigiert (Test `plan_required_response_*`).
pub fn plan_required_response() -> Response {
    stufe_required_response(Stufe::Plus)
}

/// 403 mit der Stufe, die fehlt. `required_plans` listet die Stufen, die den
/// Zugang freischalten (Pro schaltet alles frei, was Plus schaltet).
pub fn stufe_required_response(benoetigt: Stufe) -> Response {
    let required_plans: Vec<&'static str> = [Stufe::Plus, Stufe::Pro]
        .into_iter()
        .filter(|stufe| *stufe >= benoetigt)
        .map(Stufe::as_str)
        .collect();
    let hinweis = match benoetigt {
        Stufe::Pro => "Dafür brauchst du Creator Pro.",
        _ => "Dafür brauchst du Netzwerk Plus.",
    };
    // Das Entitlement muss zur fehlenden Stufe passen. Vorher stand hier fest
    // `analytics`, auch im 403 des Pro-Gates — dort fehlt aber `social.auto_post`,
    // und das Frontend bekam damit die falsche Begruendung fuer die Sperre.
    let required_entitlements: &[&str] = match benoetigt {
        Stufe::Pro => &["social.auto_post"],
        _ => &["analytics"],
    };
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "plan_required",
            "required_entitlements": required_entitlements,
            "required_stufe": benoetigt.as_str(),
            "required_plans": required_plans,
            "message": hinweis,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// 403-Body: error, die fehlende Stufe und die Stufen, die freischalten.
    #[tokio::test]
    async fn plan_required_response_nennt_die_fehlende_stufe() {
        let resp = plan_required_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "plan_required");
        assert_eq!(body["required_entitlements"], json!(["analytics"]));
        assert_eq!(body["required_stufe"], "plus");
        // Pro schaltet alles frei, was Plus freischaltet.
        assert_eq!(body["required_plans"], json!(["plus", "pro"]));
        assert!(body["message"].as_str().unwrap().contains("Netzwerk Plus"));
    }

    /// Pro-Grenze: nur Pro steht in der Liste, und der Text nennt Creator Pro.
    #[tokio::test]
    async fn stufe_required_response_fuer_pro() {
        let resp = stufe_required_response(Stufe::Pro);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["required_stufe"], "pro");
        assert_eq!(body["required_plans"], json!(["pro"]));
        assert!(body["message"].as_str().unwrap().contains("Creator Pro"));
    }

    /// 401-Body-Shape (Python `_require_v2_auth`): error-Text + loginUrl.
    #[tokio::test]
    async fn unauthorized_response_traegt_loginurl() {
        let resp = unauthorized_v2_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("Authentication required"));
        assert_eq!(body["loginUrl"], "/twitch/auth/login?next=%2Fanalyse");
    }
}
