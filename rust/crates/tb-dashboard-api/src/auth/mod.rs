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

use sqlx::PgPool;
use tb_http_core::{ApiError, AuthLevel};

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

/// Prüft ob `login` das `analytics.extended`-Entitlement hat.
///
/// Liest `manual_plan_id` und `manual_plan_expires_at` aus `streamer_plans`.
/// Plan-IDs mit Entitlement: `analytics_pro`, `analytics_extended`
/// (muss mit Python-`_KNOWN_BILLING_PLAN_IDS` synchron gehalten werden).
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
    .map_err(|_| ApiError::internal())?;

    let has_entitlement = match row {
        None => false,
        Some((plan_id, expires_at)) => {
            let plan = plan_id.as_deref().unwrap_or("");
            if !EXTENDED_PLAN_IDS.contains(&plan) {
                false
            } else {
                match expires_at.as_deref() {
                    None => true, // kein Ablaufdatum = unbegrenzt
                    Some(s) => chrono::DateTime::parse_from_rfc3339(s)
                        .map(|dt| dt > chrono::Utc::now())
                        .unwrap_or(false),
                }
            }
        }
    };

    if has_entitlement {
        Ok(())
    } else {
        Err(ApiError::plan_required())
    }
}

/// Plan-IDs, die das `analytics.extended`-Entitlement tragen.
/// Muss mit Python-`_KNOWN_BILLING_PLAN_IDS`-Filterung synchron gehalten werden.
const EXTENDED_PLAN_IDS: &[&str] = &["analytics_pro", "analytics_extended"];
