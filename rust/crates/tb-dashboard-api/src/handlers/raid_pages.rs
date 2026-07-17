//! Native Raid-Dashboard-Routen (P1.51 / P1.52).
//!
//! - `GET /twitch/raid/auth` — startet den Raid-OAuth-Flow (302 → Twitch).
//! - `GET /twitch/raid/go`   — Kurz-Redirect für Discord-Buttons (302 → Twitch).
//!
//! Der eigentliche OAuth-Stack (Token-Store, State-Store, Authorize-URL) lebt
//! im Bot-/Internal-API-Prozess. Diese Handler bridgen über die Internal-API
//! (`/internal/twitch/v1/raid/auth-url` bzw. `/raid/go-url`, Header
//! `X-Internal-Token`) und leiten den Nutzer dann zur Twitch-Authorize-URL um.
//!
//! Zugriffspolitik (Python `raid_auth_start`):
//! - Eine Streamer-Dashboard-Session darf nur den eigenen Login autorisieren.
//! - Ein expliziter `?login=`-Override braucht eine Admin-Session.
//! - Öffentliches Website-Onboarding startet den reduzierten Base-Scope-Flow
//!   ohne Session (`public:website_onboarding`).

use std::time::Duration;

use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::Value;

use crate::auth::level::DashboardAuthLevel;

const PUBLIC_ONBOARDING_LOGIN: &str = "public:website_onboarding";
const BASE_SCOPE_PROFILE: &str = "base";

#[derive(Deserialize, Default)]
pub struct RaidAuthQuery {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub scope_profile: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct RaidGoQuery {
    #[serde(default)]
    pub state: Option<String>,
}

// ── Internal-API-Bridge ────────────────────────────────────────────────────────

fn internal_base_url() -> String {
    if let Some(explicit) = nonempty_env("TWITCH_INTERNAL_API_BASE_URL") {
        return explicit.trim_end_matches('/').to_string();
    }
    let host = nonempty_env("TWITCH_INTERNAL_API_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = nonempty_env("TWITCH_INTERNAL_API_PORT").unwrap_or_else(|| "8776".to_string());
    format!("http://{host}:{port}")
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Ergebnis eines Internal-API-Aufrufs.
enum BridgeResult {
    /// Erfolgreiche Authorize-URL.
    Url(String),
    /// State unbekannt/abgelaufen (Internal-API 404).
    Expired,
    /// Internal-API nicht konfiguriert/erreichbar.
    Unavailable,
    /// Eingabe ungültig (Internal-API 400/403).
    Bad,
}

/// Ruft einen Raid-OAuth-Bridge-Endpoint auf und extrahiert `auth_url`.
async fn bridge_auth_url(path_and_query: &str) -> BridgeResult {
    let Some(token) = nonempty_env("TWITCH_INTERNAL_API_TOKEN") else {
        return BridgeResult::Unavailable;
    };
    let url = format!("{}/internal/twitch/v1{path_and_query}", internal_base_url());
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return BridgeResult::Unavailable,
    };
    let resp = match client
        .get(&url)
        .header("X-Internal-Token", token)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("raid bridge transport error: {e}");
            return BridgeResult::Unavailable;
        }
    };
    match resp.status().as_u16() {
        404 => return BridgeResult::Expired,
        400 | 403 => return BridgeResult::Bad,
        503 => return BridgeResult::Unavailable,
        s if !(200..300).contains(&s) => return BridgeResult::Unavailable,
        _ => {}
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return BridgeResult::Unavailable,
    };
    match body.get("auth_url").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => BridgeResult::Url(u.to_string()),
        _ => BridgeResult::Unavailable,
    }
}

// ── Handler ─────────────────────────────────────────────────────────────────────

/// Session-Login des Dashboard-Nutzers (lowercased), falls vorhanden.
fn session_login(auth: &DashboardAuthLevel) -> Option<String> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            Some(twitch_login.trim().to_lowercase())
        }
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            Some(actor.twitch_login.trim().to_lowercase())
        }
        _ => None,
    }
    .filter(|s| !s.is_empty())
}

fn is_admin(auth: &DashboardAuthLevel) -> bool {
    matches!(auth, DashboardAuthLevel::Admin { .. })
}

/// `GET /twitch/raid/auth` — startet den Raid-OAuth-Flow.
pub async fn raid_auth_handler(
    auth: DashboardAuthLevel,
    Query(q): Query<RaidAuthQuery>,
) -> Response {
    let requested_login = q.login.unwrap_or_default().trim().to_lowercase();
    let scope_profile = q.scope_profile.unwrap_or_default().trim().to_lowercase();
    let source = q.source.unwrap_or_default().trim().to_lowercase();
    let own_login = session_login(&auth);

    // Login-Auflösung + Zugriffspolitik.
    let login = if !requested_login.is_empty() {
        // Expliziter Override: nur erlaubt wenn eigener Login ODER Admin.
        let is_own = own_login.as_deref() == Some(requested_login.as_str());
        if !is_own && !is_admin(&auth) {
            return (
                StatusCode::UNAUTHORIZED,
                "Du darfst diesen Login nicht autorisieren.",
            )
                .into_response();
        }
        requested_login
    } else if let Some(login) = own_login {
        login
    } else {
        // Kein Login + keine Session → öffentliches Onboarding nur im
        // Base-Scope + erlaubter Quelle.
        let effective_profile = if scope_profile.is_empty() {
            BASE_SCOPE_PROFILE.to_string()
        } else {
            scope_profile.clone()
        };
        let allow_public = effective_profile == BASE_SCOPE_PROFILE
            && (source.is_empty() || source == "website_onboarding");
        if allow_public {
            PUBLIC_ONBOARDING_LOGIN.to_string()
        } else {
            return (
                StatusCode::UNAUTHORIZED,
                "Für diesen Kanal ist kein öffentlicher Onboarding-Link freigeschaltet.",
            )
                .into_response();
        }
    };

    // Bridge-Query bauen.
    let mut path = format!("/raid/auth-url?login={}", urlencode(&login));
    if !scope_profile.is_empty() {
        path.push_str(&format!("&scope_profile={}", urlencode(&scope_profile)));
    }

    match bridge_auth_url(&path).await {
        BridgeResult::Url(url) => Redirect::to(&url).into_response(),
        BridgeResult::Bad => (StatusCode::BAD_REQUEST, "Ungültige Anfrage.").into_response(),
        BridgeResult::Expired | BridgeResult::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Der Anmeldedienst ist gerade nicht erreichbar. Bitte versuche es später erneut.",
        )
            .into_response(),
    }
}

/// `GET /twitch/raid/go` — Kurz-Redirect für Discord-Buttons.
///
/// Der `state` ist das Geheimnis (kurze TTL); kein Token nötig. Abgelaufener
/// oder unbekannter State → 410 mit deutscher Hinweis-Seite.
pub async fn raid_go_handler(Query(q): Query<RaidGoQuery>) -> Response {
    let state = q.state.unwrap_or_default().trim().to_string();
    if state.is_empty() {
        return (StatusCode::BAD_REQUEST, "Fehlender Link-Parameter.").into_response();
    }

    let path = format!("/raid/go-url?state={}", urlencode(&state));
    match bridge_auth_url(&path).await {
        BridgeResult::Url(url) => Redirect::to(&url).into_response(),
        BridgeResult::Expired => expired_link_page(),
        BridgeResult::Bad => (StatusCode::BAD_REQUEST, "Ungültiger Link.").into_response(),
        BridgeResult::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Der Dienst ist gerade nicht erreichbar. Bitte versuche es später erneut.",
        )
            .into_response(),
    }
}

/// 410-Seite für abgelaufene/ungültige Discord-Button-Links (Python: 410 HTML).
fn expired_link_page() -> Response {
    let html = "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Link abgelaufen</title></head>\
<body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem;text-align:center\">\
<h1>Link abgelaufen</h1>\
<p>Dieser Anmelde-Link ist abgelaufen oder ungültig. Fordere über den Bot einen neuen Link an und versuche es erneut.</p>\
</body></html>";
    (
        StatusCode::GONE,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

/// Minimales URL-Encoding für Query-Werte.
fn urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;

    /// Stellt sicher, dass ohne gesetztes Internal-Token kein echter Netzaufruf
    /// passiert (Bridge → Unavailable). Tests laufen offline. Die Handler werden
    /// direkt aufgerufen (der DashboardAuthLevel-Extractor löst sonst aus dem
    /// Request auf und ist hier nicht das zu testende Verhalten).
    fn clear_internal_env() {
        std::env::remove_var("TWITCH_INTERNAL_API_TOKEN");
        std::env::remove_var("TWITCH_INTERNAL_API_BASE_URL");
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: "1".into(),
            display_name: "x".into(),
        }
    }

    fn admin() -> DashboardAuthLevel {
        DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_login: "earlysalty".into(),
                twitch_user_id: "2".into(),
            }),
        }
    }

    #[test]
    fn session_login_lowercased() {
        assert_eq!(
            session_login(&partner("EarlySalty")).as_deref(),
            Some("earlysalty")
        );
        assert!(session_login(&DashboardAuthLevel::None).is_none());
    }

    #[test]
    fn admin_und_localhost_sind_admin() {
        assert!(is_admin(&DashboardAuthLevel::admin()));
        assert!(is_admin(&DashboardAuthLevel::admin()));
        assert!(!is_admin(&DashboardAuthLevel::None));
    }

    #[tokio::test]
    async fn fremder_login_ohne_admin_401() {
        clear_internal_env();
        let res = raid_auth_handler(
            partner("self"),
            Query(RaidAuthQuery {
                login: Some("other".into()),
                ..Default::default()
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn go_ohne_state_400() {
        clear_internal_env();
        let res = raid_go_handler(Query(RaidGoQuery::default())).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn auth_eigener_login_ohne_internal_token_503() {
        clear_internal_env();
        // Eigene/Admin-Session, eigener Login → Zugriff erlaubt, aber Bridge
        // ohne Internal-Token → 503 (kein Netzaufruf, kein 404-Fallthrough).
        let res = raid_auth_handler(
            admin(),
            Query(RaidAuthQuery {
                login: Some("earlysalty".into()),
                ..Default::default()
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn auth_public_onboarding_ohne_session_503_nicht_401() {
        clear_internal_env();
        // Kein Login, keine Session, base-scope → öffentliches Onboarding ist
        // erlaubt; Bridge ohne Token → 503 (nicht 401).
        let res =
            raid_auth_handler(DashboardAuthLevel::None, Query(RaidAuthQuery::default())).await;
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn go_mit_state_ohne_internal_token_503() {
        clear_internal_env();
        let res = raid_go_handler(Query(RaidGoQuery {
            state: Some("abc".into()),
        }))
        .await;
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn expired_page_ist_410_html() {
        let res = expired_link_page();
        assert_eq!(res.status(), StatusCode::GONE);
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
    }

    #[test]
    fn urlencode_kodiert_sonderzeichen() {
        assert_eq!(urlencode("a b:c"), "a+b%3Ac");
    }
}
