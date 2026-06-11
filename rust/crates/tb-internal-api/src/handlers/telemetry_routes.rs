//! Handler für die nativ portierten Telemetry-Routen.
//!
//! Portiert aus `bot/internal_api/routes/telemetry.py`:
//!
//! - `GET /live/active-announcements` → `[{streamer_login, message_id, tracking_token,
//!                                         referral_url, button_label, channel_id}]`
//! - `POST /live/link-click`          → `{ok: true}` | 400 | 403 | 500
//!
//! NICHT portiert (bleiben proxied, weil sie Python-In-Process-Bot-State brauchen):
//! - `GET /debug/observability`    → ruft `_observability_snapshot()` auf → bleibt Proxy
//! - `GET /debug/chatters/{login}` → ruft `_chatters_debug(login)` auf   → bleibt Proxy
//!
//! `/healthz` und `/eventsub/dispatch` sind bereits nativ (handlers/healthz.rs,
//! handlers/eventsub.rs) — kein Überschneidungsrisiko.
//!
//! Auth: alle Endpoints verlangen `is_privileged()` (X-Internal-Token).
//!
//! Idempotenz für `POST /live/link-click`: läuft über den geteilten
//! [`crate::idempotency`]-Layer (voller Python-Vertrag: Scope-Key,
//! Fingerprint+409, Inflight-Dedup, Replay-Header) — als
//! `Extension<IdempotencyState>` eingehängt.

use axum::{
    extract::{OriginalUri, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_analytics::telemetry_routes as db;
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};

use crate::idempotency::{IdempotencyState, Prepared, IDEMPOTENCY_KEY_HEADER};

/// Referral-Code identisch zu `TWITCH_DISCORD_REF_CODE` in `bot/core/constants.py`.
const DISCORD_REF_CODE: &str = "DE-Deadlock-Discord";

/// Fallback-Label, identisch zu `TWITCH_BUTTON_LABEL` in `bot/core/constants.py`.
const TWITCH_BUTTON_LABEL: &str = "Auf Twitch ansehen";

/// Env-Variable für den Discord-Benachrichtigungskanal.
const ENV_NOTIFY_CHANNEL_ID: &str = "TWITCH_NOTIFY_CHANNEL_ID";

/// Env-Variable für die Guild-ID-Allowlist (Parität zu `_allowed_guild_ids`).
const ENV_ALLOWED_GUILD_IDS: &str = "TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS";
/// Env-Variable für die Channel-ID-Allowlist (Parität zu `_allowed_channel_ids`).
const ENV_ALLOWED_CHANNEL_IDS: &str = "TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS";
/// Env-Variable für die Role-ID-Allowlist (Parität zu `_allowed_role_ids`).
const ENV_ALLOWED_ROLE_IDS: &str = "TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS";

// ── Hilfs-Typen ───────────────────────────────────────────────────────────────

/// Normalisiertes Announcement-Item.
/// Felder spiegeln `normalize_live_announcement_item` in `bot/internal_api/policy.py`.
#[derive(Debug, Serialize)]
pub struct AnnouncementItem {
    pub streamer_login: String,
    pub message_id: i64,
    pub tracking_token: String,
    pub referral_url: String,
    pub button_label: String,
    pub channel_id: i64,
}

/// POST /live/link-click Request-Body
#[derive(Debug, Deserialize)]
pub struct LinkClickRequest {
    #[serde(default)]
    pub streamer_login: Option<String>,
    #[serde(default)]
    pub tracking_token: Option<String>,
    #[serde(default)]
    pub discord_user_id: Option<String>,
    #[serde(default)]
    pub discord_username: Option<String>,
    #[serde(default)]
    pub guild_id: Option<Value>,
    #[serde(default)]
    pub channel_id: Option<Value>,
    #[serde(default)]
    pub message_id: Option<Value>,
    #[serde(default)]
    pub source_hint: Option<String>,
    /// Nur für Discord-Action-Scope-Prüfung; link-click-Bodies senden dieses
    /// Feld nie — wenn `TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS` gesetzt ist,
    /// schlägt die Prüfung wie im Python-Original durch (None ∉ allowlist → 403).
    #[serde(default)]
    pub role_id: Option<Value>,
}

// ── Hilfsfunktionen ───────────────────────────────────────────────────────────

/// Parsed einen positiven Integer aus einem JSON-Value.
/// Parität zu `_coerce_optional_positive_int` in `bot/internal_api/policy.py`.
fn coerce_positive_int(value: &Value, key: &str) -> Result<Option<i64>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(_) => Err(format!("{key} must be a positive integer")),
        Value::Number(n) => {
            let v = n.as_i64().ok_or_else(|| format!("{key} must be a positive integer"))?;
            if v <= 0 {
                return Err(format!("{key} must be a positive integer"));
            }
            Ok(Some(v))
        }
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None);
            }
            // Python: `item.isdigit()` — nur Ziffern
            if !s.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("{key} must be a positive integer"));
            }
            let v: i64 = s.parse().map_err(|_| format!("{key} must be a positive integer"))?;
            if v <= 0 {
                return Err(format!("{key} must be a positive integer"));
            }
            Ok(Some(v))
        }
        _ => Err(format!("{key} must be a positive integer")),
    }
}

/// Normalisiert einen Freitext-Wert.
/// Parität zu `normalize_text_field` in `bot/internal_api/policy.py`.
fn normalize_text_field(
    value: &Option<String>,
    field_name: &str,
    required: bool,
    max_length: usize,
) -> Result<Option<String>, String> {
    let text = value
        .as_deref()
        .unwrap_or("")
        .replace(['\r', '\n'], " ");
    let text = text.trim().to_string();
    if text.is_empty() {
        if required {
            return Err(format!("invalid {field_name}"));
        }
        return Ok(None);
    }
    if text.len() > max_length {
        return Err(format!("invalid {field_name}"));
    }
    Ok(Some(text))
}

/// Normalisiert eine Discord-User-ID.
/// Parität zu `normalize_discord_user_id` in `bot/internal_api/policy.py`.
fn normalize_discord_user_id(
    value: &Option<String>,
    required: bool,
) -> Result<Option<String>, String> {
    let raw = value.as_deref().unwrap_or("").trim().to_string();
    if raw.is_empty() {
        if required {
            return Err("invalid discord_user_id".to_string());
        }
        return Ok(None);
    }
    if !raw.chars().all(|c| c.is_ascii_digit()) {
        return Err("invalid discord_user_id".to_string());
    }
    Ok(Some(raw))
}

/// Normalisiert einen Tracking-Token.
/// Parität zu `normalize_tracking_token` in `bot/internal_api/policy.py`.
fn normalize_tracking_token(
    value: &Option<String>,
    required: bool,
) -> Result<Option<String>, String> {
    let text = value.as_deref().unwrap_or("").trim().to_string();
    if text.is_empty() {
        if required {
            return Err("invalid tracking_token".to_string());
        }
        return Ok(None);
    }
    if text.len() > 128 {
        return Err("invalid tracking_token".to_string());
    }
    Ok(Some(text))
}

/// Parst eine kommagetrennte Liste positiver Integer-IDs aus einer Env-Variable.
/// Parität zu `parse_allowlist_ids` in `bot/internal_api/policy.py`.
/// Gibt `None` zurück wenn die Env-Variable nicht gesetzt ist (= kein Filter).
fn parse_allowlist_ids(env_name: &str) -> Option<std::collections::HashSet<i64>> {
    let raw = std::env::var(env_name).ok()?;
    let raw = raw.trim().to_string();
    // Env-Variable gesetzt (auch wenn leer) → fail-closed deny-all
    let mut allowed = std::collections::HashSet::new();
    for token in raw.replace(';', ",").split(',') {
        let item = token.trim();
        if item.is_empty() {
            continue;
        }
        if let Ok(v) = item.parse::<i64>() {
            if v > 0 {
                allowed.insert(v);
            }
        }
    }
    Some(allowed)
}

/// Prüft ob ein Integer-Wert in einer optionalen Allowlist enthalten ist.
/// `None` Allowlist = kein Filter (wie Python: `if allowed is None: return`).
fn enforce_scope_allowlist(
    value_opt: Option<i64>,
    allowed: &Option<std::collections::HashSet<i64>>,
    key: &str,
) -> Result<(), String> {
    let Some(allowed_set) = allowed else {
        return Ok(());
    };
    // Python: wenn value None ist UND allowed ist gesetzt → not in allowed → PermissionError
    let Some(value) = value_opt else {
        return Err(format!("{key} is not allowed"));
    };
    if !allowed_set.contains(&value) {
        return Err(format!("{key} is not allowed"));
    }
    Ok(())
}

/// Baut die Referral-URL für einen Streamer.
/// Parität zu `_dashboard_build_referral_url` in `bot/dashboard/mixin.py`.
fn build_referral_url(streamer_login: &str) -> String {
    if streamer_login.is_empty() {
        return "https://www.twitch.tv/".to_string();
    }
    format!(
        "https://www.twitch.tv/{}?ref={}",
        streamer_login, DISCORD_REF_CODE
    )
}

/// Extrahiert den Button-Label aus config_json.
///
/// Parität zu `_dashboard_live_button_label_from_config` in `bot/dashboard/mixin.py`:
/// 1. `raw_json` leer/None → `TWITCH_BUTTON_LABEL`
/// 2. JSON-Parse schlägt fehl oder kein dict → `TWITCH_BUTTON_LABEL`
/// 3. `parsed["button"]` muss ein dict sein; wenn nicht vorhanden → leeres dict
/// 4. `button_cfg["label"]` ODER `button_cfg["label_template"]` (label hat Vorrang),
///    getrimmt
/// 5. Wenn leer → `TWITCH_BUTTON_LABEL`; sonst auf 80 Zeichen kürzen
fn button_label_from_config(config_json: Option<&str>) -> String {
    let raw = config_json.unwrap_or("").trim();
    if raw.is_empty() {
        return TWITCH_BUTTON_LABEL.to_string();
    }
    let Ok(val) = serde_json::from_str::<Value>(raw) else {
        return TWITCH_BUTTON_LABEL.to_string();
    };
    if !val.is_object() {
        return TWITCH_BUTTON_LABEL.to_string();
    }
    // button_cfg = parsed.get("button") if isinstance(parsed.get("button"), dict) else {}
    let button_cfg = val.get("button").and_then(|v| v.as_object());
    let label = button_cfg.and_then(|cfg| {
        // label hat Vorrang; label_template als Fallback
        let l = cfg.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let t = cfg.get("label_template").and_then(|v| v.as_str()).unwrap_or("");
        let chosen = if !l.is_empty() { l } else { t };
        let trimmed = chosen.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    match label {
        // auf 80 Zeichen kürzen (Python: label[:80])
        Some(s) => s.chars().take(80).collect(),
        None => TWITCH_BUTTON_LABEL.to_string(),
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/live/active-announcements`
///
/// Parität zu `live_active_announcements` in `telemetry.py` +
/// `_dashboard_live_active_announcements` in `bot/dashboard/mixin.py`.
///
/// Liefert eine JSON-Liste (kein Wrapper-Objekt) von normalisierten
/// Announcement-Items. Parität: `server._json_response(normalized)` wo
/// `normalized` eine Liste ist.
pub async fn live_active_announcements_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // channel_id kommt aus dem Request-Kontext in Python (_notify_channel_id).
    // Im Rust-Kern wird channel_id aus der Env gelesen.
    let channel_id_raw = std::env::var(ENV_NOTIFY_CHANNEL_ID).unwrap_or_default();
    let channel_id: i64 = channel_id_raw.trim().parse().unwrap_or(0);
    if channel_id <= 0 {
        // Kein channel_id konfiguriert → leere Liste (Parität: Python gibt [] zurück)
        return Ok(Json(serde_json::json!([])));
    }

    let rows = db::list_active_announcements(&pool).await.map_err(|e| {
        tracing::error!("live_active_announcements DB-Fehler: {e}");
        ApiError::internal()
    })?;

    let mut items: Vec<AnnouncementItem> = Vec::new();
    for row in rows {
        // streamer_login normalisieren
        let Some(streamer_login) = normalize_twitch_login(&row.streamer_login) else {
            continue;
        };
        // tracking_token
        let tracking_token = match row.last_tracking_token.as_deref() {
            Some(t) => {
                let t = t.trim().to_string();
                if t.is_empty() {
                    continue;
                }
                t
            }
            None => continue,
        };
        // message_id als i64 (Python: int(str(message_id_raw).strip()))
        let message_id: i64 = match row.last_discord_message_id.as_deref() {
            Some(s) => match s.trim().parse::<i64>() {
                Ok(v) if v > 0 => v,
                _ => continue,
            },
            None => continue,
        };
        let referral_url = build_referral_url(&streamer_login);
        let button_label = button_label_from_config(row.config_json.as_deref());

        items.push(AnnouncementItem {
            streamer_login,
            message_id,
            tracking_token,
            referral_url,
            button_label,
            channel_id,
        });
    }

    Ok(Json(serde_json::json!(items)))
}

/// `POST /internal/twitch/v1/live/link-click`
///
/// Parität zu `live_link_click` in `telemetry.py` +
/// `_dashboard_live_link_click` in `bot/dashboard/mixin.py`.
///
/// Idempotenz (geteilter Layer, voller Python-Vertrag) → Validierung →
/// Discord-Action-Scope → INSERT in `twitch_link_clicks`. Wie in Python
/// laufen Validierungs-/Scope-Fehler als Owner durch und werden den Waitern
/// zurückgespielt, aber NICHT gecacht (`cacheable` erst nach Erfolg).
pub async fn live_link_click_handler(
    auth: AuthLevel,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    State(pool): State<PgPool>,
    Extension(idem): Extension<IdempotencyState>,
    Json(raw_payload): Json<Value>,
) -> Result<Response, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Fingerprint über den ROHEN Body (wie Python canonical_json(payload) —
    // unbekannte Felder zählen mit); erst danach typisiert deserialisieren.
    let body: LinkClickRequest = serde_json::from_value(raw_payload.clone())
        .map_err(|_| ApiError::bad_request("invalid request body"))?;

    let raw_key = headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok());
    let path = uri.path().to_string();
    let path_qs = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| path.clone());

    match idem.prepare(raw_key, "POST", &path, &path_qs, &raw_payload).await {
        Prepared::Immediate(resp) => Ok(resp),
        Prepared::Skip => {
            let result = process_link_click(&pool, body).await?;
            Ok(Json(result).into_response())
        }
        Prepared::Owner(slot) => match process_link_click(&pool, body).await {
            Ok(result) => {
                // Python: owner_cacheable erst NACH erfolgreichem Write.
                slot.complete(200, &result, true);
                Ok(Json(result).into_response())
            }
            Err(e) => {
                // Fehler an Waiter zurückspielen, aber nicht cachen — Retry
                // mit gleichem Key führt neu aus.
                slot.complete(e.status.as_u16(), &e.payload_json(), false);
                Err(e)
            }
        },
    }
}

/// Geschäftslogik von `POST /live/link-click` — Validierung, Scope-Guard,
/// INSERT. Gibt den Erfolgs-Body `{"ok": true}` zurück.
async fn process_link_click(pool: &PgPool, body: LinkClickRequest) -> Result<Value, ApiError> {
    // ── Validation (Parität zu telemetry.py + policy.py) ─────────────────────

    let streamer_login =
        normalize_twitch_login(body.streamer_login.as_deref().unwrap_or(""))
            .ok_or_else(|| ApiError::bad_request("invalid streamer_login"))?;

    let tracking_token = normalize_tracking_token(&body.tracking_token, true)
        .map_err(|_| ApiError::bad_request("invalid request body"))?
        .ok_or_else(|| ApiError::bad_request("invalid tracking_token"))?;

    let discord_user_id = normalize_discord_user_id(&body.discord_user_id, true)
        .map_err(|_| ApiError::bad_request("invalid request body"))?
        .ok_or_else(|| ApiError::bad_request("invalid discord_user_id"))?;

    let discord_username =
        normalize_text_field(&body.discord_username, "discord_username", true, 200)
            .map_err(|_| ApiError::bad_request("invalid request body"))?
            .ok_or_else(|| ApiError::bad_request("invalid discord_username"))?;

    // guild_id: optional, aber wenn vorhanden muss es ein positiver Integer sein
    let guild_id_opt: Option<i64> = match &body.guild_id {
        Some(v) => coerce_positive_int(v, "guild_id")
            .map_err(|_| ApiError::bad_request("invalid request body"))?,
        None => None,
    };

    let channel_id_val: i64 = match &body.channel_id {
        Some(v) => coerce_positive_int(v, "channel_id")
            .map_err(|_| ApiError::bad_request("invalid request body"))?
            .ok_or_else(|| ApiError::bad_request("invalid channel_id"))?,
        None => return Err(ApiError::bad_request("invalid channel_id")),
    };

    let message_id_val: i64 = match &body.message_id {
        Some(v) => coerce_positive_int(v, "message_id")
            .map_err(|_| ApiError::bad_request("invalid request body"))?
            .ok_or_else(|| ApiError::bad_request("invalid message_id"))?,
        None => return Err(ApiError::bad_request("invalid message_id")),
    };

    let source_hint = normalize_text_field(&body.source_hint, "source_hint", true, 100)
        .map_err(|_| ApiError::bad_request("invalid request body"))?
        .ok_or_else(|| ApiError::bad_request("invalid source_hint"))?;

    // ── Discord-Action-Scope-Prüfung (Parität zu _enforce_discord_action_scope) ──

    let allowed_guilds = parse_allowlist_ids(ENV_ALLOWED_GUILD_IDS);
    let allowed_channels = parse_allowlist_ids(ENV_ALLOWED_CHANNEL_IDS);
    let allowed_roles = parse_allowlist_ids(ENV_ALLOWED_ROLE_IDS);

    enforce_scope_allowlist(guild_id_opt, &allowed_guilds, "guild_id")
        .map_err(|_| ApiError::forbidden())?;
    enforce_scope_allowlist(Some(channel_id_val), &allowed_channels, "channel_id")
        .map_err(|_| ApiError::forbidden())?;
    // role_id: nie im Link-Click-Body gesendet → None; wenn allowed_roles gesetzt ist,
    // schlägt die Prüfung durch (None ∉ allowlist → 403), wie im Python-Original.
    let role_id_opt: Option<i64> = match &body.role_id {
        Some(v) => coerce_positive_int(v, "role_id")
            .map_err(|_| ApiError::bad_request("invalid request body"))?,
        None => None,
    };
    enforce_scope_allowlist(role_id_opt, &allowed_roles, "role_id")
        .map_err(|_| ApiError::forbidden())?;

    // ── Persistenz ───────────────────────────────────────────────────────────

    let ref_code = Some(DISCORD_REF_CODE);
    let guild_id_str = guild_id_opt.map(|v| v.to_string());
    let channel_id_str = channel_id_val.to_string();
    let message_id_str = message_id_val.to_string();

    db::insert_link_click(
        pool,
        Utc::now(),
        &streamer_login,
        &tracking_token,
        &discord_user_id,
        &discord_username,
        guild_id_str.as_deref(),
        &channel_id_str,
        &message_id_str,
        ref_code,
        &source_hint,
    )
    .await
    .map_err(|e| {
        tracing::error!("live_link_click DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(json!({ "ok": true }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::{get, post},
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        sqlx::query(
            r#"
            CREATE TABLE twitch_live_state (
                twitch_user_id              TEXT PRIMARY KEY,
                streamer_login              TEXT NOT NULL,
                last_stream_id              TEXT,
                last_started_at             TEXT,
                last_title                  TEXT,
                last_game_id                TEXT,
                last_discord_message_id     TEXT,
                last_notified_at            TEXT,
                is_live                     INTEGER DEFAULT 0,
                last_seen_at                TEXT,
                last_game                   TEXT,
                last_viewer_count           INTEGER DEFAULT 0,
                last_tracking_token         TEXT,
                active_session_id           INTEGER,
                had_deadlock_in_session     INTEGER DEFAULT 0,
                last_deadlock_seen_at       TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");

        sqlx::query(
            r#"
            CREATE TABLE twitch_live_announcement_configs (
                streamer_login          TEXT PRIMARY KEY,
                config_json             TEXT NOT NULL,
                allowed_editor_role_ids TEXT,
                updated_at              TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_by              TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_announcement_configs");

        sqlx::query(
            r#"
            CREATE TABLE twitch_link_clicks (
                id               SERIAL PRIMARY KEY,
                clicked_at       TIMESTAMPTZ DEFAULT NOW(),
                streamer_login   TEXT NOT NULL,
                tracking_token   TEXT,
                discord_user_id  TEXT,
                discord_username TEXT,
                guild_id         TEXT,
                channel_id       TEXT,
                message_id       TEXT,
                ref_code         TEXT,
                source_hint      TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_link_clicks");

        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(
                &format!("{base}/live/active-announcements"),
                get(live_active_announcements_handler),
            )
            .route(
                &format!("{base}/live/link-click"),
                post(live_link_click_handler),
            )
            .with_state(pool)
            .layer(Extension(IdempotencyState::new()))
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(token.to_string(), internal_auth))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(method: &str, uri: &str, body: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    fn req_with_idem_key(
        method: &str,
        uri: &str,
        body: &str,
        token: &str,
        idem_key: &str,
    ) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("x-internal-token", token)
            .header("Idempotency-Key", idem_key)
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── Auth ──────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn ohne_token_401() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_tr_401").await, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/live/active-announcements"),
                "",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Live Active Announcements ─────────────────────────────────────────────

    #[tokio::test]
    async fn live_active_announcements_ohne_channel_id_leer() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_ann_nochid").await, "secret");
        let base = INTERNAL_API_BASE_PATH;

        // TWITCH_NOTIFY_CHANNEL_ID nicht gesetzt → leere Liste
        std::env::remove_var("TWITCH_NOTIFY_CHANNEL_ID");

        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/live/active-announcements"),
                "",
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert!(
            j.as_array().map(|a| a.is_empty()).unwrap_or(false),
            "ohne TWITCH_NOTIFY_CHANNEL_ID muss leere Liste kommen"
        );
    }

    #[tokio::test]
    async fn live_active_announcements_mit_button_label() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_ann_label").await;
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;

        std::env::set_var("TWITCH_NOTIFY_CHANNEL_ID", "123456789");

        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_discord_message_id, last_tracking_token) VALUES ($1,$2,$3,$4)"
        )
        .bind("uid_lbl").bind("label_streamer").bind("999").bind("tok_lbl")
        .execute(&pool).await.expect("insert");

        sqlx::query(
            "INSERT INTO twitch_live_announcement_configs (streamer_login, config_json) VALUES ($1,$2)"
        )
        .bind("label_streamer").bind(r#"{"button":{"label":"Jetzt Live!"}}"#)
        .execute(&pool).await.expect("insert config");

        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/live/active-announcements"),
                "",
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        let arr = j.as_array().expect("Liste erwartet");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["button_label"], "Jetzt Live!");
        assert_eq!(arr[0]["channel_id"], 123456789_i64);

        std::env::remove_var("TWITCH_NOTIFY_CHANNEL_ID");
    }

    #[tokio::test]
    async fn live_active_announcements_fallback_label_wenn_kein_button() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_ann_fallback").await;
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;

        std::env::set_var("TWITCH_NOTIFY_CHANNEL_ID", "111222333");

        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_discord_message_id, last_tracking_token) VALUES ($1,$2,$3,$4)"
        )
        .bind("uid_fb").bind("fallback_str").bind("1").bind("tok_fb")
        .execute(&pool).await.expect("insert");

        // config_json ohne button-Schlüssel → Fallback
        sqlx::query(
            "INSERT INTO twitch_live_announcement_configs (streamer_login, config_json) VALUES ($1,$2)"
        )
        .bind("fallback_str").bind(r#"{"other":"x"}"#)
        .execute(&pool).await.expect("insert config");

        let resp = app
            .oneshot(req(
                "GET",
                &format!("{base}/live/active-announcements"),
                "",
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        let arr = j.as_array().expect("Liste erwartet");
        assert_eq!(arr[0]["button_label"], "Auf Twitch ansehen");

        std::env::remove_var("TWITCH_NOTIFY_CHANNEL_ID");
    }

    // ── Live Link Click ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn link_click_valide_daten_200() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_lc_200").await, "secret");
        let base = INTERNAL_API_BASE_PATH;

        // Allowlists deaktivieren (kein Env gesetzt)
        std::env::remove_var(ENV_ALLOWED_GUILD_IDS);
        std::env::remove_var(ENV_ALLOWED_CHANNEL_IDS);
        std::env::remove_var(ENV_ALLOWED_ROLE_IDS);

        let body = r#"{
            "streamer_login": "dragscope",
            "tracking_token": "tok_abc123",
            "discord_user_id": "123456789",
            "discord_username": "CoolUser",
            "guild_id": 987654321,
            "channel_id": 111111111,
            "message_id": 222222222,
            "source_hint": "discord_button"
        }"#;

        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/live/link-click"),
                body,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
    }

    #[tokio::test]
    async fn link_click_fehlende_pflichtfelder_400() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_lc_400").await, "secret");
        let base = INTERNAL_API_BASE_PATH;

        // Kein streamer_login
        let body = r#"{
            "tracking_token": "tok",
            "discord_user_id": "123",
            "discord_username": "user",
            "channel_id": 1,
            "message_id": 2,
            "source_hint": "x"
        }"#;

        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/live/link-click"),
                body,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn link_click_ungueltige_channel_id_400() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_lc_cid_400").await, "secret");
        let base = INTERNAL_API_BASE_PATH;

        let body = r#"{
            "streamer_login": "dragscope",
            "tracking_token": "tok",
            "discord_user_id": "123",
            "discord_username": "user",
            "channel_id": "not_a_number",
            "message_id": 1,
            "source_hint": "x"
        }"#;

        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/live/link-click"),
                body,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn link_click_guild_id_ausserhalb_allowlist_403() {
        let dsn = db_dsn_or_skip!();
        let app = make_router(make_pool(&dsn, "test_h_lc_403").await, "secret");
        let base = INTERNAL_API_BASE_PATH;

        // Allowlist: nur guild 999, aber Request sendet 111
        std::env::set_var(ENV_ALLOWED_GUILD_IDS, "999");
        std::env::remove_var(ENV_ALLOWED_CHANNEL_IDS);

        let body = r#"{
            "streamer_login": "dragscope",
            "tracking_token": "tok",
            "discord_user_id": "123",
            "discord_username": "user",
            "guild_id": 111,
            "channel_id": 222,
            "message_id": 333,
            "source_hint": "x"
        }"#;

        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/live/link-click"),
                body,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        std::env::remove_var(ENV_ALLOWED_GUILD_IDS);
    }

    #[tokio::test]
    async fn link_click_ref_code_ist_konstant() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_lc_refcode").await;
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;

        std::env::remove_var(ENV_ALLOWED_GUILD_IDS);
        std::env::remove_var(ENV_ALLOWED_CHANNEL_IDS);

        let body = r#"{
            "streamer_login": "streamer_x",
            "tracking_token": "tok_ref",
            "discord_user_id": "999",
            "discord_username": "ref_user",
            "channel_id": 555,
            "message_id": 666,
            "source_hint": "test_src"
        }"#;

        let resp = app
            .oneshot(req(
                "POST",
                &format!("{base}/live/link-click"),
                body,
                Some("secret"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let ref_code: Option<String> =
            sqlx::query_scalar("SELECT ref_code FROM twitch_link_clicks LIMIT 1")
                .fetch_one(&pool)
                .await
                .expect("select ref_code");

        assert_eq!(
            ref_code.as_deref(),
            Some("DE-Deadlock-Discord"),
            "ref_code muss DISCORD_REF_CODE sein"
        );
    }

    #[tokio::test]
    async fn link_click_idempotenz_replay() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_h_lc_idem").await;
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;

        std::env::remove_var(ENV_ALLOWED_GUILD_IDS);
        std::env::remove_var(ENV_ALLOWED_CHANNEL_IDS);
        std::env::remove_var(ENV_ALLOWED_ROLE_IDS);

        let body = r#"{
            "streamer_login": "idem_streamer",
            "tracking_token": "tok_idem",
            "discord_user_id": "100",
            "discord_username": "IdemUser",
            "channel_id": 1,
            "message_id": 2,
            "source_hint": "idem_src"
        }"#;

        // Erster Request mit Idempotency-Key → schreibt in DB
        let resp1 = app
            .clone()
            .oneshot(req_with_idem_key(
                "POST",
                &format!("{base}/live/link-click"),
                body,
                "secret",
                "unique-key-abc",
            ))
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let count1: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_link_clicks WHERE streamer_login='idem_streamer'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count1, 1, "Erster Request muss in DB schreiben");

        // Zweiter Request mit demselben Key → Replay, KEIN zweiter DB-Write
        let resp2 = app
            .oneshot(req_with_idem_key(
                "POST",
                &format!("{base}/live/link-click"),
                body,
                "secret",
                "unique-key-abc",
            ))
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);

        // X-Idempotency-Replayed muss gesetzt sein
        assert_eq!(
            resp2.headers().get("X-Idempotency-Replayed").and_then(|v| v.to_str().ok()),
            Some("1"),
            "Zweiter Request muss als Replay markiert sein"
        );

        let count2: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_link_clicks WHERE streamer_login='idem_streamer'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count2, 1, "Idempotenz: kein zweiter DB-Write");
    }

    // ── Hilfs-Unit-Tests ──────────────────────────────────────────────────────

    #[test]
    fn coerce_positive_int_parses_number() {
        assert_eq!(coerce_positive_int(&json!(42), "x").unwrap(), Some(42));
        assert_eq!(coerce_positive_int(&json!("123"), "x").unwrap(), Some(123));
        assert_eq!(coerce_positive_int(&json!(null), "x").unwrap(), None);
        assert_eq!(coerce_positive_int(&json!(""), "x").unwrap(), None);
    }

    #[test]
    fn coerce_positive_int_rejected_fuer_bool_und_negativ() {
        assert!(coerce_positive_int(&json!(true), "x").is_err());
        assert!(coerce_positive_int(&json!(-1), "x").is_err());
        assert!(coerce_positive_int(&json!(0), "x").is_err());
        assert!(coerce_positive_int(&json!("abc"), "x").is_err());
    }

    #[test]
    fn normalize_text_field_trimmt_und_prueft() {
        assert_eq!(
            normalize_text_field(&Some("  hello  ".to_string()), "f", false, 100).unwrap(),
            Some("hello".to_string())
        );
        assert!(normalize_text_field(&None, "f", true, 100).is_err());
        assert!(normalize_text_field(&Some("a".repeat(200)), "f", false, 100).is_err());
    }

    #[test]
    fn enforce_scope_allowlist_none_bedeutet_kein_filter() {
        // None = nicht konfiguriert → immer erlaubt
        assert!(enforce_scope_allowlist(Some(42), &None, "k").is_ok());
        assert!(enforce_scope_allowlist(None, &None, "k").is_ok());
    }

    #[test]
    fn enforce_scope_allowlist_gesetzt_prueft() {
        use std::collections::HashSet;
        let allowed: Option<HashSet<i64>> = Some([42_i64].into_iter().collect());
        assert!(enforce_scope_allowlist(Some(42), &allowed, "k").is_ok());
        assert!(enforce_scope_allowlist(Some(99), &allowed, "k").is_err());
        assert!(enforce_scope_allowlist(None, &allowed, "k").is_err());
    }

    #[test]
    fn build_referral_url_enthaelt_ref_code() {
        let url = build_referral_url("dragscope");
        assert!(url.contains("dragscope"));
        assert!(url.contains("DE-Deadlock-Discord"));
    }

    #[test]
    fn button_label_from_config_korrekte_struktur() {
        // None → TWITCH_BUTTON_LABEL
        assert_eq!(button_label_from_config(None), "Auf Twitch ansehen");
        // Ungültiges JSON → TWITCH_BUTTON_LABEL
        assert_eq!(button_label_from_config(Some("invalid json")), "Auf Twitch ansehen");
        // JSON ohne button-Schlüssel → TWITCH_BUTTON_LABEL
        assert_eq!(
            button_label_from_config(Some(r#"{"other":"x"}"#)),
            "Auf Twitch ansehen"
        );
        // button.label vorhanden → wird genommen
        assert_eq!(
            button_label_from_config(Some(r#"{"button":{"label":"Watch now"}}"#)),
            "Watch now"
        );
        // button.label_template (kein label) → wird genommen
        assert_eq!(
            button_label_from_config(Some(r#"{"button":{"label_template":"Jetzt live!"}}"#)),
            "Jetzt live!"
        );
        // button.label hat Vorrang vor label_template
        assert_eq!(
            button_label_from_config(Some(
                r#"{"button":{"label":"Haupt","label_template":"Neben"}}"#
            )),
            "Haupt"
        );
        // button ist kein Objekt → leeres dict → TWITCH_BUTTON_LABEL
        assert_eq!(
            button_label_from_config(Some(r#"{"button":"string_statt_objekt"}"#)),
            "Auf Twitch ansehen"
        );
    }

    #[test]
    fn button_label_wird_auf_80_zeichen_gekuerzt() {
        let long_label = "x".repeat(100);
        let config = format!(r#"{{"button":{{"label":"{long_label}"}}}}"#);
        let result = button_label_from_config(Some(&config));
        assert_eq!(result.len(), 80, "Label muss auf 80 Zeichen gekürzt werden");
    }

}
