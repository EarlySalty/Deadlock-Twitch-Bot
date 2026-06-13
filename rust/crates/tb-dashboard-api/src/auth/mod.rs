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
    if has_extended_entitlement(pool, login).await {
        Ok(())
    } else {
        Err(ApiError::plan_required())
    }
}

/// `true` wenn `login` einen aktiven Extended-Plan ODER einen laufenden 30-Tage-
/// Trial (`analytics_trial`) hat. Liest `manual_plan_id` + `manual_plan_expires_at`
/// aus `streamer_plans`; abgelaufene Pläne/Trials zählen nicht. Bei DB-Fehler
/// `false` (fail-closed — kein versehentlicher Gratis-Zugang).
pub async fn has_extended_entitlement(pool: &PgPool, login: &str) -> bool {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        r#"
        SELECT manual_plan_id, manual_plan_expires_at
        FROM streamer_plans
        WHERE LOWER(twitch_login) = LOWER($1)
        LIMIT 1
        "#,
    )
    .bind(login)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    match row {
        None => false,
        Some((plan_id, expires_at)) => {
            let plan = plan_id.as_deref().unwrap_or("").trim();
            // Echte Entitlement-Prüfung (Python `_plan_has_entitlement`) statt einer
            // hartkodierten Liste — bleibt automatisch mit catalog.py synchron und
            // erkennt die real verkauften Pläne (analysis_dashboard, Bundles, Trial).
            if !tb_analytics::plan::plan_is_extended(plan) {
                false
            } else {
                match expires_at.as_deref() {
                    None => true, // kein Ablaufdatum = unbegrenzt
                    Some(s) => expiry_in_future(s),
                }
            }
        }
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
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            if has_extended_entitlement(pool, twitch_login).await {
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

/// Parst ein Ablaufdatum (TEXT) und prüft, ob es in der Zukunft liegt. Wie Python
/// (`_parse_plan_override_datetime`): reines Datum `YYYY-MM-DD` → Tagesende UTC,
/// `Z`→`+00:00`, offset-lose Zeit → als UTC interpretiert. Nicht parsbar → false.
fn expiry_in_future(raw: &str) -> bool {
    let text = raw.trim();
    if text.is_empty() {
        return false;
    }
    // Reines Datum (YYYY-MM-DD) → Ende des Tages UTC (Python: + "T23:59:59+00:00").
    let normalized = if text.len() == 10 && text.as_bytes().get(4) == Some(&b'-') {
        format!("{text}T23:59:59+00:00")
    } else {
        text.replace('Z', "+00:00")
    };
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&normalized) {
        return dt > chrono::Utc::now();
    }
    // Offset-lose ISO-Zeit → als UTC interpretieren.
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, fmt) {
            return naive.and_utc() > chrono::Utc::now();
        }
    }
    false
}
