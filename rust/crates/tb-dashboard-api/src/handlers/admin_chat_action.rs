//! Native Admin-Partner-Chat-Aktion (P2.120, B-Welle-2-A1).
//!
//! Port von `bot/dashboard/live/live.py:1790-1966` (`admin_partner_chat_action`).
//! Der Admin-React-Client (`submitLegacyAction('/twitch/admin/chat_action', …)`)
//! sendet eine `application/x-www-form-urlencoded`-Form, mit der ein manueller
//! Chat-/Announcement-Send in einen Partner-Kanal ausgelöst wird.
//!
//! **Owner-Gate (P2.119):** Nur der freigeschaltete Discord-Owner
//! (`_DASHBOARD_OWNER_DISCORD_ID`) darf die Aktion auslösen — ein abgelehnter
//! Versuch wird als AUDIT-Warnung geloggt. Admins ohne Discord-Admin-Session
//! haben keine Owner-Identität → werden ebenfalls abgelehnt.
//!
//! **Gating (Python):** `mode` ∈ {message, action, announcement} (sonst message),
//! `color` ∈ {blue, green, orange, purple, primary} (sonst purple), 450-Zeichen-
//! Cap, Lookup in `twitch_partners_all_state` (nicht gefunden → Abbruch),
//! `manual_partner_opt_out` ODER (archiviert UND nicht admin-archiv-erlaubt) →
//! Abbruch.
//!
//! **Prozessgrenze:** Der eigentliche Send läuft NICHT in tb-dashboard (eigener
//! Prozess ohne Bot-Token/Chat). Er wird über die Bot-internal-API
//! (`POST {base}/streamers/:login/chat-action`, `X-Internal-Token`) gebrückt —
//! exakt der native Send-Pfad (`chat_action_handler` → `ChatActionAdapter`).

use std::time::Duration;

use axum::{
    extract::{Extension, RawForm, State},
    response::Response,
};
use serde_json::json;
use sqlx::PgPool;

use super::legacy_form::{form_get, parse_form, redirect_with};
use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME};
use tb_domain::login::normalize_twitch_login;

/// Freigeschalteter Discord-Owner (Python `_DASHBOARD_OWNER_DISCORD_ID`).
const DASHBOARD_OWNER_DISCORD_ID: &str = "662995601738170389";

/// Gültige Chat-Aktions-Modi (Python `_CHAT_ACTION_MODES`).
const CHAT_ACTION_MODES: &[&str] = &["message", "action", "announcement"];
/// Gültige Announcement-Farben (Python `_CHAT_ANNOUNCEMENT_COLORS`).
const CHAT_ANNOUNCEMENT_COLORS: &[&str] = &["blue", "green", "orange", "purple", "primary"];

/// Maximale Nachrichtenlänge (Python `len(message) > 450`).
const MAX_MESSAGE_LEN: usize = 450;

/// Redirect-Ziel (Python `default_path="/twitch/admin"`).
const ADMIN_PATH: &str = "/twitch/admin";

const CHAT_ACTION_PATH_PREFIX: &str = "/internal/twitch/v1/streamers/";
const CHAT_ACTION_PATH_SUFFIX: &str = "/chat-action";

fn redirect_ok(message: &str) -> Response {
    redirect_with(ADMIN_PATH, "ok", message)
}

fn redirect_err(message: &str) -> Response {
    redirect_with(ADMIN_PATH, "err", message)
}

/// `POST /twitch/admin/chat_action` — manueller Chat-/Announcement-Send.
pub async fn chat_action_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    if !auth.is_privileged() {
        return redirect_err("Nicht autorisiert.");
    }
    let form = parse_form(&body);

    // Owner-Gate (P2.119): Discord-Owner-ID aus der Admin-Session lesen.
    let owner_id = resolve_admin_discord_user_id(&auth, config.as_ref(), &headers).await;
    if owner_id.as_deref() != Some(DASHBOARD_OWNER_DISCORD_ID) {
        tracing::warn!(
            "AUDIT dashboard chat action denied: discord_user_id={} path={}",
            if owner_id.is_some() { "present" } else { "none" },
            "/twitch/admin/chat_action"
        );
        return redirect_err("Nur der freigeschaltete Discord-Owner darf diese Chat-Aktion nutzen.");
    }

    // Login (Python: login / streamer, normalisiert).
    let raw_login = {
        let l = form_get(&form, "login");
        if l.trim().is_empty() {
            form_get(&form, "streamer")
        } else {
            l
        }
    };
    let Some(login) = normalize_twitch_login(raw_login.trim()) else {
        return redirect_err("Bitte einen Partner-Login angeben");
    };

    // Mode + Color normalisieren (Fallbacks message/purple).
    let mode = normalize_choice(form_get(&form, "mode"), CHAT_ACTION_MODES, "message");
    let color = normalize_choice(form_get(&form, "color"), CHAT_ANNOUNCEMENT_COLORS, "purple");

    let message = form_get(&form, "message").trim();
    if message.is_empty() {
        return redirect_err("Bitte eine Nachricht eingeben");
    }
    if message.chars().count() > MAX_MESSAGE_LEN {
        return redirect_err("Nachricht ist zu lang (max. 450 Zeichen)");
    }

    // Partner-Gating gegen twitch_partners_all_state.
    match partner_send_allowed(&pool, &login).await {
        Ok(SendGate::NotFound) => return redirect_err(&format!("Streamer {login} nicht gefunden")),
        Ok(SendGate::Denied) => {
            return redirect_err(
                "Chat-Aktion ist nur für aktive oder admin-archivierte Partner-Streamer erlaubt",
            )
        }
        Ok(SendGate::Allowed) => {}
        Err(_) => return redirect_err(&format!("Streamer {login} nicht gefunden")),
    }

    // Send über die Bot-internal-API brücken.
    match bridge_chat_action(&login, &mode, &color, message).await {
        SendResult::Sent => {
            let label = match mode.as_str() {
                "announcement" => "Announcement",
                "action" => "Action",
                _ => "Nachricht",
            };
            redirect_ok(&format!("{label} an {login} gesendet"))
        }
        SendResult::Failed { detail } => {
            if let Some(detail) = detail {
                tracing::warn!(%detail, "chat_action bridge upstream failed");
            }
            redirect_err(&format!("Chat-Aktion für {login} konnte nicht gesendet werden"))
        }
        SendResult::Unavailable => redirect_err("Twitch Chat Bot ist aktuell nicht verfügbar"),
    }
}

// ── Owner-Gate ────────────────────────────────────────────────────────────────

/// Liest die Discord-User-ID des Admins aus der `discord_admin`-Session
/// (Python `_get_discord_admin_user_id`). Admin ohne Admin-Cookie → `None`.
async fn resolve_admin_discord_user_id(
    auth: &DashboardAuthLevel,
    config: Option<&Extension<DashboardAuthState>>,
    headers: &axum::http::HeaderMap,
) -> Option<String> {
    // Nur privilegierte Admins kommen bis hierher; die Owner-Identität hängt
    // ausschließlich an der Discord-Admin-Session.
    let _ = auth;
    let Extension(state) = config?;
    let cookie = read_cookie(headers, ADMIN_COOKIE_NAME)?;
    state
        .load_admin_session_user_id(&cookie)
        .await
        .ok()
        .flatten()
}

fn read_cookie(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())?;
    cookie_header.split(';').find_map(|pair| {
        let pair = pair.trim();
        pair.split_once('=')
            .filter(|(k, _)| k.trim() == name)
            .map(|(_, v)| v.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

// ── Partner-Gating ────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum SendGate {
    Allowed,
    Denied,
    NotFound,
}

/// Prüft die Send-Berechtigung gegen `twitch_partners_all_state`.
///
/// Python: `archived/opt_out`-Gating. `archived_at` gesetzt → erlaubt nur, wenn
/// der Streamer aktuell/zuletzt ein aktiver Partner ist (admin-archiviert);
/// `manual_partner_opt_out` ODER nicht erlaubt → Abbruch.
async fn partner_send_allowed(pool: &PgPool, login: &str) -> Result<SendGate, sqlx::Error> {
    let row: Option<(Option<String>, Option<String>, Option<i32>)> =
        sqlx::query_as(
            "SELECT twitch_user_id, archived_at, manual_partner_opt_out \
             FROM twitch_partners_all_state \
             WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
        )
        .bind(login)
        .fetch_optional(pool)
        .await?;

    let Some((_uid, archived_at, opt_out)) = row else {
        return Ok(SendGate::NotFound);
    };

    let is_archived = archived_at.as_deref().map(str::trim).is_some_and(|s| !s.is_empty());
    let manual_opt_out = opt_out.unwrap_or(0) != 0;
    let partner_allowed = if is_archived {
        is_partner_chat_action_allowed(pool, login).await?
    } else {
        true
    };

    if manual_opt_out || !partner_allowed {
        Ok(SendGate::Denied)
    } else {
        Ok(SendGate::Allowed)
    }
}

/// Python `_is_partner_chat_action_allowed`: erlaubt, wenn ein aktiver Partner
/// existiert ODER der zuletzt bekannte Partner-Status `active` ist.
async fn is_partner_chat_action_allowed(pool: &PgPool, login: &str) -> Result<bool, sqlx::Error> {
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM twitch_partners \
         WHERE LOWER(twitch_login) = LOWER($1) \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    Ok(status
        .map(|s| s.trim().eq_ignore_ascii_case("active"))
        .unwrap_or(false))
}

// ── Bridge zur Bot-internal-API ───────────────────────────────────────────────

enum SendResult {
    Sent,
    Failed { detail: Option<String> },
    Unavailable,
}

/// Brückt den Send an die Bot-internal-API (`chat_action_handler` → Bot-Token).
/// Ohne `TWITCH_INTERNAL_API_TOKEN` oder bei Transport-/Upstream-Fehler →
/// `Unavailable`; `ok=false` der Upstream-Antwort → `Failed`.
async fn bridge_chat_action(login: &str, mode: &str, color: &str, message: &str) -> SendResult {
    let Some(token) = nonempty_env("TWITCH_INTERNAL_API_TOKEN") else {
        return SendResult::Unavailable;
    };
    let url = format!(
        "{}{CHAT_ACTION_PATH_PREFIX}{login}{CHAT_ACTION_PATH_SUFFIX}",
        worker_internal_base_url()
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let resp = client
        .post(url)
        .header("X-Internal-Token", token)
        .json(&json!({ "mode": mode, "color": color, "message": message }))
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("chat_action bridge transport error: {e}");
            return SendResult::Unavailable;
        }
    };
    if resp.status().as_u16() == 503 {
        return SendResult::Unavailable;
    }
    let value: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("chat_action bridge body error: {e}");
            return SendResult::Failed { detail: None };
        }
    };
    if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        SendResult::Sent
    } else {
        let detail = value
            .get("detail")
            .or_else(|| value.get("drop_reason"))
            .or_else(|| value.get("message"))
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        SendResult::Failed { detail }
    }
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn worker_internal_base_url() -> String {
    if let Some(explicit) = nonempty_env("TWITCH_INTERNAL_API_BASE_URL") {
        return explicit.trim_end_matches('/').to_string();
    }
    let host = nonempty_env("TWITCH_INTERNAL_API_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = nonempty_env("TWITCH_INTERNAL_API_PORT").unwrap_or_else(|| "8776".to_string());
    format!("http://{host}:{port}")
}

/// Normalisiert eine Choice gegen eine Whitelist (Fallback bei Nichttreffer).
fn normalize_choice(raw: &str, allowed: &[&str], fallback: &str) -> String {
    let v = raw.trim().to_lowercase();
    if allowed.contains(&v.as_str()) {
        v
    } else {
        fallback.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_normalisierung() {
        assert_eq!(
            normalize_choice("Announcement", CHAT_ACTION_MODES, "message"),
            "announcement"
        );
        assert_eq!(normalize_choice("bogus", CHAT_ACTION_MODES, "message"), "message");
        assert_eq!(normalize_choice("GREEN", CHAT_ANNOUNCEMENT_COLORS, "purple"), "green");
        assert_eq!(normalize_choice("", CHAT_ANNOUNCEMENT_COLORS, "purple"), "purple");
    }

    #[test]
    fn redirect_helpers_kodieren() {
        let ok = redirect_ok("Nachricht an nani gesendet");
        let loc = ok.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/admin?ok="));
    }

    // ── DB-Gating (env-gated über TB_TEST_DATABASE_URL) ─────────────────────
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_partners_all_state (twitch_login TEXT, twitch_user_id TEXT, \
                 archived_at TEXT, manual_partner_opt_out INTEGER DEFAULT 0)",
            "CREATE TABLE twitch_partners (id BIGSERIAL PRIMARY KEY, twitch_login TEXT, status TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn gate_unbekannter_streamer_notfound() {
        let Some(pool) = make_pool("t_chat_notfound").await else {
            return;
        };
        assert_eq!(
            partner_send_allowed(&pool, "ghost").await.unwrap(),
            SendGate::NotFound
        );
    }

    #[tokio::test]
    async fn gate_aktiver_partner_allowed() {
        let Some(pool) = make_pool("t_chat_allowed").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, archived_at, manual_partner_opt_out) VALUES ('nani', '1', NULL, 0)")
            .execute(&pool).await.unwrap();
        assert_eq!(
            partner_send_allowed(&pool, "nani").await.unwrap(),
            SendGate::Allowed
        );
    }

    #[tokio::test]
    async fn gate_archivierter_partner_abgelehnt() {
        let Some(pool) = make_pool("t_chat_archived").await else {
            return;
        };
        // Archiviert + kein aktiver Partner-History-Eintrag → Denied.
        sqlx::query("INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, archived_at, manual_partner_opt_out) VALUES ('arch', '2', '2026-06-01T00:00:00+00:00', 0)")
            .execute(&pool).await.unwrap();
        assert_eq!(
            partner_send_allowed(&pool, "arch").await.unwrap(),
            SendGate::Denied
        );
    }

    #[tokio::test]
    async fn gate_archivierter_admin_partner_allowed() {
        let Some(pool) = make_pool("t_chat_archived_admin").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, archived_at, manual_partner_opt_out) VALUES ('aa', '3', '2026-06-01T00:00:00+00:00', 0)")
            .execute(&pool).await.unwrap();
        // Zuletzt aktiver Partner → admin-archiviert erlaubt.
        sqlx::query("INSERT INTO twitch_partners (twitch_login, status) VALUES ('aa', 'active')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            partner_send_allowed(&pool, "aa").await.unwrap(),
            SendGate::Allowed
        );
    }

    #[tokio::test]
    async fn gate_opt_out_abgelehnt() {
        let Some(pool) = make_pool("t_chat_optout").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, archived_at, manual_partner_opt_out) VALUES ('o', '4', NULL, 1)")
            .execute(&pool).await.unwrap();
        assert_eq!(
            partner_send_allowed(&pool, "o").await.unwrap(),
            SendGate::Denied
        );
    }

    // ── Route-Test: fehlende Admin-Auth → abgelehnt ─────────────────────────
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn route_lehnt_loopback_ohne_admin_auth_ab() {
        let Some(pool) = make_pool("t_chat_owner_gate").await else {
            return;
        };
        // Aktiver Partner vorhanden (damit nur das Owner-Gate greift).
        sqlx::query("INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, archived_at, manual_partner_opt_out) VALUES ('nani', '1', NULL, 0)")
            .execute(&pool).await.unwrap();
        let app = Router::new()
            .route("/twitch/admin/chat_action", post(chat_action_handler))
            .with_state(pool);
        let req = Request::builder()
            .method("POST")
            .uri("/twitch/admin/chat_action")
            .header("host", "127.0.0.1:8769")
            .header("content-type", "application/x-www-form-urlencoded")
            .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                12345,
            ))))
            .body(Body::from("login=nani&message=hi"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Loopback ohne Admin-Session ist nicht privilegiert → 302 err.
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        assert!(loc.starts_with("/twitch/admin?err="), "loc={loc}");
    }
}
