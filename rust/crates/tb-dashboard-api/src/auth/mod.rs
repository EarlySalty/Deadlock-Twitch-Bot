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
pub mod fernet;
pub mod level;
pub mod oauth_login;
pub mod partner_access;
pub mod partner_gate;
pub mod partner_login;
pub mod security;
pub mod session;

// ---------------------------------------------------------------------------
// Migriert aus dem früheren auth.rs (IDOR-Guard + Plan-Gating)
// ---------------------------------------------------------------------------

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use sqlx::PgPool;
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

/// Prüft ob `login` das konsolidierte `analytics`-Entitlement hat — über
/// [`tb_analytics::plan::plan_has_analytics`], das die echten Plan-IDs aus
/// `catalog.py` kennt (analysis_dashboard, die Analyse-Bundles, analytics_trial).
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

/// `true` wenn der Streamer einen Plan mit dem konsolidierten `analytics`-Flag,
/// ein aktives Stripe-Abo ODER einen laufenden 30-Tage-Trial hat. Nutzt denselben
/// Resolver wie das Dashboard (`resolve_plan_snapshot`), damit Manual-Override UND
/// Stripe-Abo (und der `user_id`-Match) berücksichtigt werden — die frühere
/// verkürzte streamer_plans-Login-Query sperrte zahlende Stripe-Kunden ohne
/// Manual-Eintrag fälschlich mit 403 aus. Ablauf wird im Resolver geprüft. Bei
/// DB-Fehler `false` (fail-closed — kein versehentlicher Gratis-Zugang).
pub async fn has_analytics_entitlement(pool: &PgPool, login: &str, user_id: &str) -> bool {
    match tb_analytics::plan::resolve_plan_snapshot(pool, login, user_id).await {
        Ok(snapshot) => snapshot.entitlements.contains(&"analytics"),
        Err(_) => false,
    }
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
    match auth {
        DashboardAuthLevel::Admin { .. } => None,
        DashboardAuthLevel::None => Some(unauthorized_v2_response()),
        DashboardAuthLevel::Partner { twitch_login, twitch_user_id, .. } => {
            if has_analytics_entitlement(pool, twitch_login, twitch_user_id).await {
                None
            } else {
                Some(plan_required_response())
            }
        }
    }
}

/// 401-Body wie Python `_require_v2_auth` (api_v2.py:1258-1262).
pub fn unauthorized_v2_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "Authentication required. Use Twitch login, an admin token, or access from localhost.",
            "loginUrl": "/twitch/auth/login?next=%2Fanalyse",
        })),
    )
        .into_response()
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
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "plan_required",
            "required_entitlements": ["analytics"],
            "required_plans": extended_required_plans(),
        })),
    )
        .into_response()
}

/// Bekannte Billing-Pläne (Python `KNOWN_PLAN_IDS`), die das konsolidierte
/// `analytics`-Entitlement tragen — alphabetisch sortiert wie Pythons `sorted(...)`.
///
/// Spiegelt `_require_extended_plan`s `required_plans`-Generator
/// (`sorted(p for p in _KNOWN_BILLING_PLAN_IDS if _plan_has_entitlement(p, …))`).
/// `tb_analytics::plan::plan_has_analytics` ist die kanonische Quelle des
/// Analytics-Flags; die Plan-ID-Liste ist hier festgehalten (der Katalog ist in
/// `tb-analytics` privat) und gegen Drift testgesichert.
fn extended_required_plans() -> Vec<&'static str> {
    const KNOWN_BILLING_PLAN_IDS: [&str; 9] = [
        "raid_free",
        "chat_quiet",
        "raid_boost",
        "bundle_chat_quiet_raid_boost",
        "analysis_dashboard",
        "bundle_analysis_raid_boost",
        "bundle_werbefrei_analyse",
        "bundle_komplett",
        "analytics_trial",
    ];
    let mut plans: Vec<&'static str> = KNOWN_BILLING_PLAN_IDS
        .into_iter()
        .filter(|id| tb_analytics::plan::plan_has_analytics(id))
        .collect();
    plans.sort_unstable();
    plans
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// B16-VERIFY-V2AUTH-SHAPE: 403-Body trägt error + required_entitlements UND
    /// die sortierte required_plans-Liste (vorher gefehlt → Python-Divergenz).
    #[tokio::test]
    async fn plan_required_response_traegt_required_plans() {
        let resp = plan_required_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "plan_required");
        assert_eq!(body["required_entitlements"], json!(["analytics"]));
        // Exakt die Analyse-Pläne aus KNOWN_PLAN_IDS, alphabetisch sortiert.
        assert_eq!(
            body["required_plans"],
            json!([
                "analysis_dashboard",
                "analytics_trial",
                "bundle_analysis_raid_boost",
                "bundle_komplett",
                "bundle_werbefrei_analyse",
            ])
        );
    }

    /// Drift-Gate: jeder gelistete required_plan trägt tatsächlich das
    /// `analytics`-Flag, free (raid_free) ist nie dabei. Genau die 5 Analyse-Pläne.
    #[test]
    fn extended_required_plans_sind_alle_extended() {
        let plans = extended_required_plans();
        assert_eq!(
            plans,
            vec![
                "analysis_dashboard",
                "analytics_trial",
                "bundle_analysis_raid_boost",
                "bundle_komplett",
                "bundle_werbefrei_analyse",
            ]
        );
        assert!(!plans.contains(&"raid_free"));
        assert!(plans.iter().all(|p| tb_analytics::plan::plan_has_analytics(p)));
        // sortiert
        let mut sorted = plans.clone();
        sorted.sort_unstable();
        assert_eq!(plans, sorted);
    }

    /// 401-Body-Shape (Python `_require_v2_auth`): error-Text + loginUrl.
    #[tokio::test]
    async fn unauthorized_response_traegt_loginurl() {
        let resp = unauthorized_v2_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["error"].as_str().unwrap().contains("Authentication required"));
        assert_eq!(body["loginUrl"], "/twitch/auth/login?next=%2Fanalyse");
    }
}
