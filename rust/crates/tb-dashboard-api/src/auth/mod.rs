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

pub mod fernet;
pub mod level;
pub mod partner_gate;
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

/// Prüft ob `login` das `analytics.extended`-Entitlement hat — über
/// [`tb_analytics::plan::plan_is_extended`], das die echten Plan-IDs aus
/// `catalog.py` kennt (analysis_dashboard, die Bundles, analytics_trial).
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
    if has_extended_entitlement(pool, login, "").await {
        Ok(())
    } else {
        Err(ApiError::plan_required())
    }
}

/// `true` wenn der Streamer einen aktiven Extended-Plan, ein aktives Stripe-Abo
/// ODER einen laufenden 30-Tage-Trial hat. Nutzt denselben Resolver wie das
/// Dashboard (`resolve_plan_snapshot`), damit Manual-Override UND Stripe-Abo
/// (und der `user_id`-Match) berücksichtigt werden — die frühere verkürzte
/// streamer_plans-Login-Query sperrte zahlende Stripe-Kunden ohne Manual-Eintrag
/// fälschlich mit 403 aus. Ablauf wird im Resolver geprüft. Bei DB-Fehler
/// `false` (fail-closed — kein versehentlicher Gratis-Zugang).
pub async fn has_extended_entitlement(pool: &PgPool, login: &str, user_id: &str) -> bool {
    match tb_analytics::plan::resolve_plan_snapshot(pool, login, user_id).await {
        Ok(snapshot) => tb_analytics::plan::plan_is_extended(snapshot.plan_id),
        Err(_) => false,
    }
}

/// Plan-Gate für die DashboardAuthLevel-Handler. Gibt `Some(response)` zurück,
/// wenn der Request blockiert ist (401 unauth / 403 plan_required), sonst `None`.
///
/// - Localhost/Admin → durchgelassen (Bypass, wie Python `_require_extended_plan`)
/// - Partner → braucht aktiven Extended-Plan oder laufenden Trial
/// - None → 401
pub async fn extended_gate(pool: &PgPool, auth: &DashboardAuthLevel) -> Option<Response> {
    match auth {
        DashboardAuthLevel::Localhost | DashboardAuthLevel::Admin => None,
        DashboardAuthLevel::None => Some(
            (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response(),
        ),
        DashboardAuthLevel::Partner { twitch_login, twitch_user_id } => {
            if has_extended_entitlement(pool, twitch_login, twitch_user_id).await {
                None
            } else {
                Some(
                    (
                        StatusCode::FORBIDDEN,
                        Json(json!({
                            "error": "plan_required",
                            "message": "Erweiterte Analytics erfordern einen Plan oder den 30-Tage-Trial.",
                        })),
                    )
                        .into_response(),
                )
            }
        }
    }
}

