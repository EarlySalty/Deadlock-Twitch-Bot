//! Discord-Link-OAuth-Flow für Partner (B3-6 / auth-core-3).
//!
//! Port von `bot/dashboard/auth/auth_mixin.py:discord_link_auth_login` /
//! `discord_link_auth_complete`. Der eingeloggte Partner verknüpft seinen Discord-
//! Account; der eigentliche Discord-OAuth-Token-Tausch läuft NICHT hier, sondern
//! delegiert an den **gemeinsamen Discord-OAuth-Broker** (interne API des
//! Hauptbots, `http://127.0.0.1:8766`, Header `X-Internal-Token`):
//!
//! - `GET /twitch/auth/discord/link?next=…`
//!     1. Auth-Gate: eingeloggter Partner (sonst Login-Redirect).
//!     2. `POST {broker}/internal/v1/discord/initiate` → `authorize_url` + `state_id`.
//!     3. `302` auf `authorize_url` (Discord-Consent).
//! - `GET /twitch/auth/discord/link/complete?state_id=…`
//!     1. Auth-Gate: eingeloggter Partner.
//!     2. `POST {broker}/internal/v1/discord/consume-result` → `discord_id`,
//!        `discord_name`, `discord_roles`, `service_metadata`.
//!     3. Validierung: `service_metadata.twitch_login`/`twitch_user_id` müssen zur
//!        aktiven Session passen; `discord_id` muss numerisch sein.
//!     4. `set_discord_profile` (tb-analytics) schreibt `discord_user_id`/
//!        `discord_display_name`/`is_on_discord` (member-Flag = Rollen vorhanden).
//!     5. `302` auf den normalisierten `next`-Pfad mit `?ok=`/`?err=`.
//!
//! Secret: Broker-Token aus Env (`TWITCH_INTERNAL_API_TOKEN`/`MASTER_BROKER_TOKEN`/
//! `MAIN_BOT_INTERNAL_TOKEN`, via Infisical) — nie geloggt. Fehlt es → 503-Redirect.

use std::time::Duration;

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

/// Basis-URL des Discord-OAuth-Brokers (Python `DISCORD_OAUTH_INTERNAL_API_BASE_URL`).
const BROKER_BASE_URL: &str = "http://127.0.0.1:8766";
const BROKER_INITIATE_PATH: &str = "/internal/v1/discord/initiate";
const BROKER_CONSUME_PATH: &str = "/internal/v1/discord/consume-result";
const BROKER_TOKEN_HEADER: &str = "X-Internal-Token";

/// Fallback-Ziel nach dem Link-Flow (Python `TWITCH_DISCORD_LINK_FALLBACK_PATH`).
const FALLBACK_PATH: &str = "/twitch/verwaltung";

#[derive(Debug, Deserialize, Default)]
pub struct LinkQuery {
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CompleteQuery {
    #[serde(default)]
    pub state_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// `GET /twitch/auth/discord/link` — Start des Discord-Link-Flows.
pub async fn link_start_handler(auth: DashboardAuthLevel, Query(q): Query<LinkQuery>) -> Response {
    let next_path = normalize_next(q.next.as_deref());

    // Auth-Gate: nur eingeloggter Partner (Login + Twitch-Bezug).
    let Some((twitch_login, twitch_user_id)) = partner_identity(&auth) else {
        let encoded: String = url::form_urlencoded::byte_serialize(next_path.as_bytes()).collect();
        return Redirect::to(&format!("/twitch/auth/login?next={encoded}")).into_response();
    };

    let Some(token) = broker_token() else {
        return redirect_status(&next_path, None, Some("Discord-Link ist aktuell nicht verfügbar."));
    };

    let payload = json!({
        "scope": "identify",
        "redirect_after": "/twitch/auth/discord/link/complete",
        "requesting_service": "twitch-dashboard-link",
        "metadata": {
            "next_path": next_path,
            "twitch_login": twitch_login,
            "twitch_user_id": twitch_user_id,
        },
    });

    match broker_post(BROKER_INITIATE_PATH, &token, &payload).await {
        Some(data) => {
            let authorize_url = data
                .get("authorize_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let state_id = data.get("state_id").and_then(Value::as_str).unwrap_or("").trim();
            if authorize_url.is_empty() || state_id.is_empty() {
                return redirect_status(&next_path, None, Some("Discord-Link ist aktuell nicht verfügbar."));
            }
            Redirect::to(&authorize_url).into_response()
        }
        None => redirect_status(&next_path, None, Some("Discord-Link ist aktuell nicht verfügbar.")),
    }
}

/// `GET /twitch/auth/discord/link/complete` — Abschluss nach Discord-Consent.
pub async fn link_complete_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<CompleteQuery>,
) -> Response {
    let Some((twitch_login, twitch_user_id)) = partner_identity(&auth) else {
        return redirect_status(FALLBACK_PATH, None, Some("Twitch-Session fehlt. Bitte erneut anmelden."));
    };

    if let Some(err) = q.error.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return redirect_status(FALLBACK_PATH, None, Some(&format!("Discord OAuth Fehler: {err}")));
    }
    let Some(state_id) = q.state_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return redirect_status(FALLBACK_PATH, None, Some("Fehlender Discord-OAuth-State."));
    };

    let Some(token) = broker_token() else {
        return redirect_status(FALLBACK_PATH, None, Some("Discord-Link ist aktuell nicht verfügbar."));
    };

    let Some(session) = broker_post(BROKER_CONSUME_PATH, &token, &json!({ "state_id": state_id })).await
    else {
        return redirect_status(FALLBACK_PATH, None, Some("Discord-User konnte nicht geladen werden."));
    };

    let metadata = session.get("service_metadata").cloned().unwrap_or(Value::Null);
    let next_path = normalize_next(metadata.get("next_path").and_then(Value::as_str));

    // Bindung an die aktive Twitch-Session prüfen (Python 1749-1761).
    let expected_login = metadata
        .get("twitch_login")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let expected_user_id = metadata
        .get("twitch_user_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let login_mismatch = !expected_login.is_empty() && expected_login != twitch_login.to_ascii_lowercase();
    let user_mismatch = !expected_user_id.is_empty()
        && !twitch_user_id.is_empty()
        && expected_user_id != twitch_user_id;
    if login_mismatch || user_mismatch {
        return redirect_status(&next_path, None, Some("Discord-Link passt nicht zur aktiven Twitch-Session."));
    }

    let discord_id = session.get("discord_id").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if discord_id.is_empty() || !discord_id.chars().all(|c| c.is_ascii_digit()) {
        return redirect_status(&next_path, None, Some("Discord-User konnte nicht geladen werden."));
    }
    let discord_name = session.get("discord_name").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let has_roles = session
        .get("discord_roles")
        .and_then(Value::as_array)
        .map(|roles| roles.iter().any(|r| r.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false)))
        .unwrap_or(false);

    // DB-Write: Discord-Profil setzen (Python `_discord_profile`). member-Flag =
    // Rollen vorhanden. twitch_user_id wird zur Auflösung mitgegeben.
    let display = (!discord_name.is_empty()).then_some(discord_name.as_str());
    let uid = (!twitch_user_id.is_empty()).then_some(twitch_user_id.as_str());
    match tb_analytics::streamers_crud::set_discord_profile(
        &pool,
        &twitch_login,
        Some(discord_id.as_str()),
        display,
        has_roles,
        uid,
    )
    .await
    {
        Ok(true) => redirect_status(&next_path, Some("Discord-Account verknüpft."), None),
        Ok(false) => redirect_status(&next_path, None, Some("Streamer nicht gefunden.")),
        Err(error) => {
            tracing::error!(%error, "discord link completion failed");
            redirect_status(&next_path, None, Some("Discord-Daten konnten nicht gespeichert werden."))
        }
    }
}

/// Baut den Discord-Link-Router (B3-6). `next`/`state_id` aus Query.
pub fn build_discord_link_router(pool: PgPool) -> Router {
    Router::new()
        .route("/twitch/auth/discord/link", get(link_start_handler))
        .route("/twitch/auth/discord/link/complete", get(link_complete_handler))
        .with_state(pool)
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

/// `(twitch_login_lowercased, twitch_user_id)` des eingeloggten Partners, sonst
/// `None` (Admin/Localhost/None haben keinen verknüpfbaren Streamer-Bezug).
fn partner_identity(auth: &DashboardAuthLevel) -> Option<(String, String)> {
    if let DashboardAuthLevel::Partner { twitch_login, twitch_user_id, .. } = auth {
        let login = twitch_login.trim().to_ascii_lowercase();
        if !login.is_empty() {
            return Some((login, twitch_user_id.trim().to_string()));
        }
    }
    None
}

/// Broker-Token aus dem Prozess-Env (Infisical). Nie geloggt.
fn broker_token() -> Option<String> {
    for key in ["TWITCH_INTERNAL_API_TOKEN", "MASTER_BROKER_TOKEN", "MAIN_BOT_INTERNAL_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// POST an den Broker (JSON + `X-Internal-Token`). `None` bei Nicht-200/Fehler.
async fn broker_post(path: &str, token: &str, payload: &Value) -> Option<Value> {
    let url = format!("{}{}", BROKER_BASE_URL.trim_end_matches('/'), path);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?;
    let resp = client
        .post(&url)
        .header(BROKER_TOKEN_HEADER, token)
        .json(payload)
        .send()
        .await
        .map_err(|error| tracing::warn!(%error, path, "discord broker request failed"))
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), path, "discord broker non-200");
        return None;
    }
    resp.json::<Value>().await.ok()
}

/// Normalisiert `next` auf einen sicheren internen Pfad (kein offener Redirect):
/// muss mit genau einem `/` beginnen (kein `//`, kein Scheme/Host), sonst
/// Fallback. Spiegelt Pythons `_safe_internal_redirect`/`_canonical_post_login`.
fn normalize_next(raw: Option<&str>) -> String {
    let candidate = raw.unwrap_or("").trim();
    if candidate.starts_with('/') && !candidate.starts_with("//") {
        candidate.to_string()
    } else {
        FALLBACK_PATH.to_string()
    }
}

/// Redirect auf `path` mit optionalem `?ok=`/`?err=` (URL-kodiert). Hängt korrekt
/// an bestehende Query-Strings an (`?` vs. `&`).
fn redirect_status(path: &str, ok: Option<&str>, err: Option<&str>) -> Response {
    let mut url = path.to_string();
    let sep = if url.contains('?') { '&' } else { '?' };
    if let Some(msg) = ok {
        let enc: String = url::form_urlencoded::byte_serialize(msg.as_bytes()).collect();
        url = format!("{url}{sep}ok={enc}");
    } else if let Some(msg) = err {
        let enc: String = url::form_urlencoded::byte_serialize(msg.as_bytes()).collect();
        url = format!("{url}{sep}err={enc}");
    }
    Redirect::to(&url).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn partner(login: &str, uid: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: uid.into(),
            display_name: String::new(),
        }
    }

    #[test]
    fn next_normalisierung_blockt_offene_redirects() {
        assert_eq!(normalize_next(Some("/twitch/verwaltung")), "/twitch/verwaltung");
        assert_eq!(normalize_next(Some("//evil.com")), FALLBACK_PATH);
        assert_eq!(normalize_next(Some("https://evil.com")), FALLBACK_PATH);
        assert_eq!(normalize_next(Some("")), FALLBACK_PATH);
        assert_eq!(normalize_next(None), FALLBACK_PATH);
    }

    #[test]
    fn partner_identity_nur_fuer_partner() {
        assert_eq!(partner_identity(&partner("Nani", "42")), Some(("nani".into(), "42".into())));
        assert_eq!(partner_identity(&DashboardAuthLevel::admin()), None);
        assert_eq!(partner_identity(&DashboardAuthLevel::None), None);
    }

    #[test]
    fn redirect_status_haengt_query_korrekt_an() {
        let r = redirect_status("/twitch/verwaltung", Some("Verknüpft"), None);
        let loc = r.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/verwaltung?ok="));
        // Bestehender Query → mit & angehängt.
        let r2 = redirect_status("/x?a=1", None, Some("Fehler"));
        let loc2 = r2.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc2.starts_with("/x?a=1&err="));
    }

    /// Unauth → Login-Redirect (kein Broker-Call).
    #[tokio::test]
    async fn link_start_unauth_redirect_login() {
        let resp = link_start_handler(
            DashboardAuthLevel::None,
            Query(LinkQuery { next: Some("/twitch/verwaltung".into()) }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.contains("/twitch/auth/login"));
    }
}
