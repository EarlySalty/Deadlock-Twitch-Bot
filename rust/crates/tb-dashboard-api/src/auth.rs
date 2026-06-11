//! Auth-Hilfsfunktionen: IDOR-Guard + Plan-Gating.
//!
//! Partner-Session-Auth ist deferred (ADR 0003). Für jetzt wird `Partner`-Level
//! nie erzeugt — die Funktionen sind trotzdem korrekt implementiert, damit der
//! Umbau später ohne Behaviour-Änderung einsetzbar ist.

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
