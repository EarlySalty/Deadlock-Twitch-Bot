//! `partner_status_gate` — axum-Middleware, die passive Partner von
//! active-only-Routen aussperrt. Port von Python
//! `build_partner_status_gate_middleware` (auth_mixin.py:1836-1905).
//!
//! Komplement zum Session-Gate: Ein Partner mit token_error darf sich einloggen
//! (Session gültig) und passive-allowed-Seiten (Verwaltung, Abbo, Re-Auth)
//! nutzen, wird aber von den active-only-Analytics-Routen gegated. Admins und
//! nicht-eingeloggte Requests werden durchgelassen (der Handler entscheidet
//! weiterhin selbst über Auth).

use axum::{
    body::Body,
    http::{header::ACCEPT, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde_json::json;

use crate::auth::level::extract_cookie;
use crate::auth::session::DashboardAuthState;

/// Python `_PASSIVE_ALLOWED_EXACT_PATHS`.
const PASSIVE_ALLOWED_EXACT_PATHS: &[&str] = &[
    "/twitch/verwaltung",
    "/twitch/pricing",
    "/twitch/abbo",
    "/twitch/abbo/bezahlen",
    "/twitch/abbo/kündigen",
    "/twitch/abbo/lurker-tax-settings",
    "/twitch/abbo/promo-message",
    "/twitch/abbo/promo-settings",
    "/twitch/abbo/rechnung",
    "/twitch/abbo/rechnungen",
    "/twitch/abbo/rechnungsdaten",
    "/twitch/abbo/stripe-settings",
    "/twitch/dashboard",
    "/twitch/dashboards",
    "/twitch/dashboads",
    "/twitch/affiliate/portal",
    "/twitch/affiliate/signup",
    "/twitch/affiliate/signup/complete",
    "/twitch/affiliate/claim",
    "/twitch/affiliate/connect/stripe",
    "/twitch/affiliate/connect/stripe/callback",
    "/twitch/raid/auth",
];

/// Python `_PASSIVE_ALLOWED_PREFIXES`.
const PASSIVE_ALLOWED_PREFIXES: &[&str] = &[
    "/twitch/auth/",
    "/callback/twitch",
    "/callback/discord",
    "/twitch/api/v2/internal-home",
    "/twitch/api/v2/auth-status",
    "/twitch/api/billing/",
    "/twitch/api/v2/billing/",
    "/twitch/api/affiliate/",
    "/twitch/api/v2/affiliate/",
    "/twitch/auth/discord/",
    "/twitch/auth/partner/",
    "/twitch/agb",
    "/twitch/datenschutz",
    "/twitch/impressum",
    "/twitch/legal/",
    "/health",
    "/healthz",
    "/readyz",
    "/twitch/raid/auth",
];

/// Python `_PUBLIC_PATH_PREFIXES`.
const PUBLIC_PATH_PREFIXES: &[&str] = &[
    "/health",
    "/healthz",
    "/readyz",
    "/twitch/auth/",
    "/twitch/eventsub/",
    "/twitch/api/billing/stripe/webhook",
    "/twitch/agb",
    "/twitch/datenschutz",
    "/twitch/impressum",
    "/twitch/legal/",
    "/twitch/demo",
    "/twitch/raid/callback",
    "/callback/twitch",
    "/callback/discord",
    "/twitch/api/v2/public/",
];

fn path_matches_passive_allowed(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    PASSIVE_ALLOWED_EXACT_PATHS.contains(&path)
        || PASSIVE_ALLOWED_PREFIXES.iter().any(|p| path.starts_with(p))
}

fn path_is_public(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    PUBLIC_PATH_PREFIXES.iter().any(|p| path.starts_with(p))
}

/// Middleware: lehnt active-only-Routen für passive Partner ab. Admin- und
/// nicht-eingeloggte Requests sowie passive-allowed/public-Pfade gehen durch.
pub async fn partner_status_gate(req: Request<Body>, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();

    if parts.method == Method::OPTIONS || path_is_public(&path) {
        return next.run(Request::from_parts(parts, body)).await;
    }

    let Some(state) = parts.extensions.get::<DashboardAuthState>().cloned() else {
        return next.run(Request::from_parts(parts, body)).await;
    };

    let admin_cookie = extract_cookie(&parts, "master_dash_session").map(str::to_string);
    let partner_cookie = extract_cookie(&parts, "twitch_dash_session").map(str::to_string);
    let wants_json = path.starts_with("/twitch/api/")
        || parts
            .headers
            .get(ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(|a| a.contains("application/json"))
            .unwrap_or(false);
    let passive_allowed = path_matches_passive_allowed(&path);

    // Admin-Session → durchlassen.
    if let Some(sid) = admin_cookie.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(Some(_)) = state.load_admin_session(sid).await {
            return next.run(Request::from_parts(parts, body)).await;
        }
    }

    // Partner-Session laden; ohne gültige → durchlassen (Handler-Auth greift).
    let partner = match partner_cookie.as_deref().filter(|s| !s.is_empty()) {
        Some(sid) => state.load_partner_session(sid).await.ok().flatten(),
        None => None,
    };
    let Some(partner) = partner else {
        return next.run(Request::from_parts(parts, body)).await;
    };

    // Passive-allowed Pfad → durchlassen.
    if passive_allowed {
        return next.run(Request::from_parts(parts, body)).await;
    }

    // Active-Status prüfen.
    if state
        .is_partner_active(&partner.twitch_login, &partner.twitch_user_id)
        .await
    {
        return next.run(Request::from_parts(parts, body)).await;
    }

    // Passiver Partner auf active-only-Route → ablehnen (Python 1882-1903).
    tracing::info!(login = %partner.twitch_login, path = %path, "partner_status_gate: passiver Partner abgelehnt");
    if wants_json {
        // P2.85: für `/twitch/api/*` den Python-Access-Denied-Vertrag liefern
        // (account_blocked vs dashboard_access_restricted + redirectUrl/
        // partnerStatus/technicalPauseReason/operationalState/
        // tokenErrorGraceExpiresAt), damit das Frontend korrekt verzweigt.
        if path.starts_with("/twitch/api/") {
            let access = tb_analytics::partner_access::load_partner_access_state(
                state.pool(),
                &partner.twitch_login,
                &partner.twitch_user_id,
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("partner_status_gate: Access-State-Fehler für {}: {e}", partner.twitch_login);
                tb_analytics::partner_access::AccessState {
                    partner_status: "active".into(),
                    analytics_access_allowed: true,
                    landing_access_allowed: true,
                    ..Default::default()
                }
            });
            let payload = if access.landing_access_allowed {
                crate::auth::partner_access::analytics_access_denied_payload(&access)
            } else {
                crate::auth::partner_access::landing_access_denied_payload(&access)
            };
            return crate::auth::partner_access::forbidden_json(payload);
        }
        // Nicht-`/twitch/api/`-JSON-Akzeptierer (z. B. Accept: application/json auf
        // einer Seite): schlanker Forbidden-Body wie bisher.
        (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "partner_inactive",
                "message": "Dein Streamer-Account ist aktuell nicht als aktiver Partner geführt. Bitte authentifiziere dich neu, um diesen Bereich zu nutzen.",
                "reauth_url": "/twitch/raid/auth",
            })),
        )
            .into_response()
    } else {
        Redirect::to("/twitch/verwaltung?inactive=1").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_paths_erkannt() {
        assert!(path_is_public("/health"));
        assert!(path_is_public("/twitch/eventsub/callback"));
        assert!(path_is_public("/twitch/api/v2/public/network"));
        assert!(!path_is_public("/twitch/api/v2/overview"));
        assert!(!path_is_public(""));
    }

    #[test]
    fn passive_allowed_exact_und_prefix() {
        assert!(path_matches_passive_allowed("/twitch/verwaltung"));
        assert!(path_matches_passive_allowed("/twitch/abbo/kündigen"));
        assert!(path_matches_passive_allowed("/twitch/api/billing/trial/start"));
        assert!(path_matches_passive_allowed("/twitch/api/v2/auth-status"));
        assert!(!path_matches_passive_allowed("/twitch/api/v2/overview"));
        assert!(!path_matches_passive_allowed("/analyse"));
        assert!(!path_matches_passive_allowed(""));
    }
}
