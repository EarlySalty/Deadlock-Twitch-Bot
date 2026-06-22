//! Access-Denied-JSON-Vertrag für `/twitch/api/*`-Endpoints (P2.85).
//!
//! Python-Referenz: `api_v2.py:1184-1220` (`_analytics_access_denied_payload`,
//! `_landing_access_denied_payload`) und `api_v2.py:1269-1289` (`_require_v2_auth`,
//! Pfad-bedingtes JSON vs. Plain-Text).
//!
//! Hintergrund (Audit P2.85): Der Block ist bereits durchgesetzt (passive/blocked
//! Partner werden gegated), aber der JSON-Fehler-Vertrag wich ab — ein Frontend,
//! das auf `error` (`account_blocked` vs `dashboard_access_restricted`) verzweigt
//! oder `redirectUrl`/`technicalPauseReason`/`operationalState`/
//! `tokenErrorGraceExpiresAt` aus dem Denial-Body liest, bekam diese Felder nicht
//! und routete falsch. Dieses Modul reproduziert den Python-Body 1:1.
//!
//! Der zugrundeliegende Access-State-Resolver liegt in
//! [`tb_analytics::partner_access`] (bereits portiert) — dieses Modul ist nur der
//! Denial-Vertrag obendrauf, kein zweiter Resolver.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use tb_analytics::partner_access::AccessState;

/// Python `_PARTNER_STATUS_BLOCKED`.
const PARTNER_STATUS_BLOCKED: &str = "blocked";
/// Python `_PARTNER_STATUS_ACTIVE`.
const PARTNER_STATUS_ACTIVE: &str = "active";

/// Baut das `/twitch/api/*`-Denial-JSON für eine **Analytics**-Sperre
/// (`_analytics_access_denied_payload`, api_v2.py:1184-1209).
///
/// - `partner_status == "blocked"` → `account_blocked`, `redirectUrl="/"`.
/// - sonst → `dashboard_access_restricted`, `redirectUrl="/twitch/dashboard"`.
pub fn analytics_access_denied_payload(state: &AccessState) -> Value {
    let partner_status = if state.partner_status.trim().is_empty() {
        PARTNER_STATUS_ACTIVE
    } else {
        state.partner_status.trim()
    };

    if partner_status == PARTNER_STATUS_BLOCKED {
        return json!({
            "error": "account_blocked",
            "message": "This account is blocked from all dashboard surfaces.",
            "partnerStatus": partner_status,
            "redirectUrl": "/",
            "technicalPauseReason": optional_str(&state.technical_pause_reason),
            "operationalState": optional_str(&state.operational_state),
            "tokenErrorGraceExpiresAt": optional_str(&state.token_error_grace_expires_at),
        });
    }

    json!({
        "error": "dashboard_access_restricted",
        "message": "Analytics dashboard access is temporarily restricted. \
                    Use /twitch/dashboard or /twitch/verwaltung to manage the account.",
        "partnerStatus": partner_status,
        "redirectUrl": "/twitch/dashboard",
        "technicalPauseReason": optional_str(&state.technical_pause_reason),
        "operationalState": optional_str(&state.operational_state),
        "tokenErrorGraceExpiresAt": optional_str(&state.token_error_grace_expires_at),
    })
}

/// Baut das `/twitch/api/*`-Denial-JSON für eine **Landing**-Sperre
/// (`_landing_access_denied_payload`, api_v2.py:1211-1220). Immer `account_blocked`.
///
/// Anders als das Analytics-Payload trägt das Landing-Payload **kein**
/// `tokenErrorGraceExpiresAt` (Python-Parität) und defaulted `partnerStatus` bei
/// Leere auf `"blocked"`.
pub fn landing_access_denied_payload(state: &AccessState) -> Value {
    let partner_status = if state.partner_status.trim().is_empty() {
        PARTNER_STATUS_BLOCKED
    } else {
        state.partner_status.trim()
    };
    json!({
        "error": "account_blocked",
        "message": "This account is blocked from all dashboard surfaces.",
        "partnerStatus": partner_status,
        "redirectUrl": "/",
        "technicalPauseReason": optional_str(&state.technical_pause_reason),
        "operationalState": optional_str(&state.operational_state),
    })
}

/// 403-JSON-Response aus einem Denial-Payload (für `/twitch/api/*`).
pub fn forbidden_json(payload: Value) -> Response {
    (StatusCode::FORBIDDEN, Json(payload)).into_response()
}

/// `Option<String>` → JSON-`null` bei `None`/leer, sonst der getrimmte String —
/// exakt wie Pythons `state.get(...)` (None bleibt JSON-null).
fn optional_str(value: &Option<String>) -> Value {
    match value {
        Some(s) if !s.trim().is_empty() => json!(s.trim()),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked_state() -> AccessState {
        AccessState {
            partner_status: "blocked".into(),
            technical_pause_reason: Some("blocked".into()),
            operational_state: Some("blocked".into()),
            token_error_grace_expires_at: None,
            analytics_access_allowed: false,
            landing_access_allowed: false,
        }
    }

    fn token_error_state() -> AccessState {
        AccessState {
            partner_status: "token_error".into(),
            technical_pause_reason: Some("token_error".into()),
            operational_state: Some("token_error".into()),
            token_error_grace_expires_at: Some("2026-07-01T00:00:00+00:00".into()),
            analytics_access_allowed: false,
            landing_access_allowed: true,
        }
    }

    #[test]
    fn analytics_blocked_payload_shape() {
        let p = analytics_access_denied_payload(&blocked_state());
        assert_eq!(p["error"], "account_blocked");
        assert_eq!(p["partnerStatus"], "blocked");
        assert_eq!(p["redirectUrl"], "/");
        assert_eq!(p["technicalPauseReason"], "blocked");
        assert_eq!(p["operationalState"], "blocked");
        assert!(p["tokenErrorGraceExpiresAt"].is_null());
    }

    #[test]
    fn analytics_restricted_payload_shape() {
        let p = analytics_access_denied_payload(&token_error_state());
        assert_eq!(p["error"], "dashboard_access_restricted");
        assert_eq!(p["partnerStatus"], "token_error");
        assert_eq!(p["redirectUrl"], "/twitch/dashboard");
        assert_eq!(p["technicalPauseReason"], "token_error");
        assert_eq!(p["tokenErrorGraceExpiresAt"], "2026-07-01T00:00:00+00:00");
    }

    #[test]
    fn landing_payload_immer_account_blocked_ohne_grace() {
        let p = landing_access_denied_payload(&token_error_state());
        assert_eq!(p["error"], "account_blocked");
        assert_eq!(p["redirectUrl"], "/");
        // Landing-Payload trägt kein tokenErrorGraceExpiresAt.
        assert!(p.get("tokenErrorGraceExpiresAt").is_none());
    }

    #[test]
    fn landing_payload_default_blocked_bei_leerem_status() {
        let mut s = blocked_state();
        s.partner_status = "".into();
        let p = landing_access_denied_payload(&s);
        assert_eq!(p["partnerStatus"], "blocked");
    }

    #[test]
    fn forbidden_json_ist_403() {
        let resp = forbidden_json(analytics_access_denied_payload(&blocked_state()));
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
