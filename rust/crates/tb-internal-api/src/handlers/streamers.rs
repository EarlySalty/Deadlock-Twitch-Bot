//! Handler für die Streamer-CRUD- und Analytics-Endpoints.
//!
//! # Verdrahtungs-Status — Wahrheit ist `build_internal_router` (lib.rs)
//!
//! Sicher verdrahtbar (reiner Read, Shape im Live-Diff gegen 8779 verifiziert):
//!
//! | Methode | Pfad                                          | Handler                   |
//! |---------|-----------------------------------------------|---------------------------|
//! | GET     | /analytics/comparison                         | analytics_comparison_handler |
//!
//! NICHT verdrahten — Handler existieren, erfüllen aber den
//! Python-Vertrag noch nicht vollständig:
//! - GET /stats: Rust-Shape ist eine Eigen-Erfindung
//!   (`total_sessions/avg_viewers/…`), Python liefert
//!   `{tracked:{top,hourly,weekday}, category, avg_viewers_all,
//!   avg_viewers_tracked}` (`dashboard_metrics_mixin.py:212-259`) — der
//!   Dashboard-Split-Mode liest exakt diese Felder.
//! - GET /analytics/streamer/:login: Python delegiert an
//!   `AnalyticsBackendExtended.get_comprehensive_analytics`
//!   (`runtime_bootstrap.py:230`) — der Rust-Handler baut nur
//!   `{stats, recent_sessions}` und ist damit eine andere API.
//! - GET /sessions/:session_id: Python liefert `SELECT *` plus berechnete
//!   Felder (`retention_5m/10m/20m`, `dropoff_label`, `start/end_viewers`,
//!   `samples`, …) — dem Rust-Port fehlen ~15 Felder (Live-Diff 11.6.).
//! - GET /streamers + alle Mutationen (POST/DELETE /streamers,
//!   verify/archive/discord-flag/discord-profile): brauchen den
//!   Partner-Lifecycle (`promote_streamer_to_partner` /
//!   `departner_active_partner`) + Discord-Bridge (Rollen-Sync, DMs).
//!   verify mode=clear/failed departnern in Python KOMPLETT — der native
//!   Handler antwortet dafür ehrlich 503. GET teilt den Pfad mit POST
//!   (axum: Pfad-Match vor Methoden-Match → nativer GET würde POST mit
//!   405 statt Proxy-Fallback beantworten — kein Teil-Flip möglich).
//! - POST /streamers/:login/chat-action: braucht den live rotierten
//!   Bot-Token des Python-Chats.
//!
//! Alle Endpoints: `auth.is_privileged()` → 401.
//!
//! Request-Body-Konventionen:
//! - Bestandskonsumenten (Python-Client) senden snake_case-Bodies.
//! - Felder mit Underscores akzeptieren via `#[serde(alias)]` auch camelCase.
//! - Kein Idempotency-Caching (kommt mit dem geteilten Idempotenz-Layer).
//! - Discord-Nebeneffekte (Rollen-Sync, EventSub): deferred bis Schritt 5/6.
//!
//! archive-mode-Semantik:
//! - Python gibt NIEMALS 400 für unbekannte mode-Werte — unbekannte Werte
//!   fallen durch auf "toggle". ArchiveMode::parse ist deshalb infallibel.
//!
//! verify-mode-Semantik:
//! - permanent/temp aktualisieren aktive Partner; clear/failed → 503
//!   (Lifecycle nicht portiert); unbekannte Modi → 200 "Unbekannter Modus"
//!   (Python-Parität, KEIN Permanent-Fallback).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use tb_analytics::streamers_crud as db;
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};
use tb_transport_twitch::HelixClient;

// ── Discord-Rollen-Port ───────────────────────────────────────────────────────

/// Port für den Discord-Streamer-Rollen-Sync (Python `sync_streamer_role`).
/// Echte Impl in tb-bot über den Master-Broker; Fehler werden dort geloggt,
/// nie propagiert (Python-Parität: best-effort, kein Abbruch).
#[async_trait::async_trait]
pub trait DiscordRolePort: Send + Sync {
    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str);
}

/// Router-Extension-Wrapper für [`DiscordRolePort`] (`None` = kein Sync).
#[derive(Clone)]
pub struct DiscordRoleExt(pub Option<Arc<dyn DiscordRolePort>>);

// ── Response-Typen ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct OkLoginMessageResponse {
    pub ok: bool,
    pub login: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct StreamersListResponse {
    pub ok: bool,
    pub streamers: Vec<db::StreamerListRow>,
}

// ── Request-Typen ─────────────────────────────────────────────────────────────

/// POST /streamers/:login/verify
/// Python sendet: {"mode": "permanent"|"temp"|"clear"|"failed"}
#[derive(Deserialize, Default)]
pub struct VerifyRequest {
    #[serde(default)]
    pub mode: Option<String>,
}

/// POST /streamers/:login/archive
/// Python sendet: {"mode": "toggle"|"archive"|"unarchive"|"block"|...}
/// Default wenn leer/fehlt: "toggle" (Python-Semantik).
#[derive(Deserialize, Default)]
pub struct ArchiveRequest {
    #[serde(default)]
    pub mode: Option<String>,
}

/// POST /streamers/:login/discord-flag
/// Python sendet: {"is_on_discord": true/false}
/// Alias isOnDiscord akzeptiert auch camelCase (Tests, Frontend).
#[derive(Deserialize)]
pub struct DiscordFlagRequest {
    #[serde(default, alias = "isOnDiscord", alias = "enabled", alias = "value")]
    pub is_on_discord: Option<serde_json::Value>,
}

impl DiscordFlagRequest {
    /// Parst is_on_discord — wie Python: bool(value), Default false.
    pub fn parse_enabled(&self) -> Option<bool> {
        match &self.is_on_discord {
            None => None,
            Some(serde_json::Value::Bool(b)) => Some(*b),
            Some(serde_json::Value::Number(n)) => Some(n.as_i64().unwrap_or(0) != 0),
            Some(serde_json::Value::String(s)) => match s.to_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                _ => Some(false),
            },
            Some(serde_json::Value::Null) => Some(false),
            _ => Some(false),
        }
    }
}

/// POST /streamers/:login/discord-profile
/// Python sendet snake_case: {"discord_user_id": "...", "discord_display_name": "...", "mark_member": true}
/// Aliases ermöglichen zusätzlich camelCase (Tests, manche Frontends).
#[derive(Deserialize, Default)]
pub struct DiscordProfileRequest {
    #[serde(default, alias = "discordUserId")]
    pub discord_user_id: Option<String>,
    #[serde(default, alias = "discordDisplayName")]
    pub discord_display_name: Option<String>,
    /// Default true — wie Python: server._parse_bool(body.get("mark_member", body.get("member_flag")), default=True)
    #[serde(default = "default_mark_member", alias = "markMember", alias = "member_flag")]
    pub mark_member: bool,
}

fn default_mark_member() -> bool {
    true
}

// ── Query-Typen ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct StatsQuery {
    pub hour_from: Option<i32>,
    pub hour_to: Option<i32>,
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct AnalyticsDaysQuery {
    pub days: Option<i32>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/streamers`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let target_game = std::env::var("TWITCH_TARGET_GAME_NAME")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "Deadlock".to_string());
    let streamers = db::list_streamers(&pool, &target_game).await.map_err(|e| {
        tracing::error!("list_streamers DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(StreamersListResponse {
        ok: true,
        streamers,
    }))
}

/// `POST /internal/twitch/v1/streamers`
///
/// Body: `{"login": "...", "require_link": false}` (snake_case)
/// Wenn HelixClient nicht konfiguriert: 503.
/// Wenn Helix den Login nicht kennt: 422 `{"ok": false, "error": "unknown_login"}`.
/// Wenn bereits aktiver Partner: 200 `{"ok": true, "message": "already_active_partner"}`.
pub async fn add_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Python: login = server._normalize_login(str(body.get("login") or body.get("streamer") or body.get("twitch_login") or ""))
    let raw = body
        .get("login")
        .or_else(|| body.get("streamer"))
        .or_else(|| body.get("twitch_login"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let login = match normalize_twitch_login(&raw) {
        Some(l) => l,
        None => {
            return Err(ApiError::bad_request("invalid or missing login"));
        }
    };

    // Helix-Lookup: user_id auflösen und Login validieren
    let user_id: Option<String> = match (*helix).as_ref() {
        None => {
            return Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"ok": false, "error": "helix_unavailable"})),
            )
                .into_response());
        }
        Some(client) => {
            match client.get_users(&[login.as_str()]).await {
                Ok(map) => {
                    if map.contains_key(&login) {
                        map.get(&login).map(|u| u.id.clone())
                    } else {
                        return Ok((
                            StatusCode::UNPROCESSABLE_ENTITY,
                            Json(json!({"ok": false, "error": "unknown_login"})),
                        )
                            .into_response());
                    }
                }
                Err(e) => {
                    tracing::warn!("Helix-Lookup für {login} fehlgeschlagen: {e}");
                    None
                }
            }
        }
    };

    use db::AddStreamerResult;
    match db::add_streamer(&pool, &login, user_id.as_deref()).await {
        Ok(AddStreamerResult::AlreadyExists) => Ok((
            StatusCode::OK,
            Json(json!({"ok": true, "login": login, "message": "already_active_partner"})),
        )
            .into_response()),
        Ok(AddStreamerResult::Added) => Ok((
            StatusCode::CREATED,
            Json(json!({"ok": true, "login": login, "message": format!("{login} hinzugefügt")})),
        )
            .into_response()),
        Err(e) => {
            tracing::error!("add_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/monitoring`
///
/// Body: `{"login": "..."}`, optional `"twitch_user_id": "..."`
/// Legt einen `is_monitored_only = 1`-Eintrag an (Clip-Fetcher, Cron-Jobs).
/// Kein Helix-Lookup, kein Partner-Eintrag. Idempotent.
pub async fn add_monitored_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let raw = body
        .get("login")
        .or_else(|| body.get("twitch_login"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let login = match normalize_twitch_login(&raw) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid or missing login")),
    };

    let user_id = body
        .get("twitch_user_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    db::add_monitored_streamer(&pool, &login, user_id.as_deref())
        .await
        .map_err(|e| {
            tracing::error!("add_monitored_streamer DB-Fehler: {e}");
            ApiError::internal()
        })?;

    Ok((StatusCode::OK, Json(serde_json::json!({"ok": true, "login": login}))))
}

/// `DELETE /internal/twitch/v1/streamers/:login`
pub async fn remove_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = match normalize_twitch_login(&raw_login) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid login")),
    };

    use db::RemoveStreamerResult;
    match db::remove_streamer(&pool, &login).await {
        Ok(RemoveStreamerResult::NotFound) => Err(ApiError::not_found()),
        Ok(RemoveStreamerResult::Archived) => Ok(Json(OkLoginMessageResponse {
            ok: true,
            login,
            message: "archiviert".to_string(),
        })
        .into_response()),
        Ok(RemoveStreamerResult::Deleted) => Ok(Json(OkLoginMessageResponse {
            ok: true,
            login,
            message: "gelöscht".to_string(),
        })
        .into_response()),
        Err(e) => {
            tracing::error!("remove_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/:login/verify`
///
/// Body: `{"mode": "permanent"|"temp"|"clear"|"failed"}` — Default: "permanent".
/// Parität Python: mode-Wert kommt aus Body, Default ist "permanent".
pub async fn verify_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
    body: Option<Json<VerifyRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = match normalize_twitch_login(&raw_login) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid login")),
    };

    let mode = body
        .and_then(|b| b.mode.clone())
        .map(|m| {
            let m = m.trim().to_lowercase();
            if m.is_empty() { "permanent".to_string() } else { m }
        })
        .unwrap_or_else(|| "permanent".to_string());

    use db::VerifyStreamerResult;
    // Python (`streamers.py:169`): IMMER 200 {ok, login, message} für alle
    // Geschäftsfälle — auch "nicht gespeichert" und "Unbekannter Modus".
    let ok_message = |message: String| {
        Json(OkLoginMessageResponse {
            ok: true,
            login: login.clone(),
            message,
        })
        .into_response()
    };
    match db::verify_streamer(&pool, &login, &mode).await {
        Ok(VerifyStreamerResult::Verified) => {
            // Python base_msg (`streamer_admin_mixin.py:341-345`).
            let message = if mode == "temp" {
                format!("{login} für 30 Tage verifiziert")
            } else {
                format!("{login} dauerhaft verifiziert")
            };
            Ok(ok_message(message))
        }
        Ok(VerifyStreamerResult::NotAPartner) => {
            Ok(ok_message(format!("{login} ist nicht gespeichert")))
        }
        Ok(VerifyStreamerResult::UnknownMode) => Ok(ok_message("Unbekannter Modus".to_string())),
        // clear/failed departnern in Python komplett (Partner-Lifecycle +
        // Discord-DM) — nativ nicht portiert, ehrlicher 503 statt Halb-Aktion.
        Ok(VerifyStreamerResult::RequiresPartnerLifecycle) => Err(ApiError::unavailable()),
        Err(e) => {
            tracing::error!("verify_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/:login/archive`
///
/// Body: `{"mode": "toggle"|"archive"|"unarchive"|"block"|"unblock"|...}` — Default: "toggle".
///
/// Python gibt NIEMALS 400 für unbekannte modi. Unbekannte Werte → Toggle.
/// `ArchiveMode::parse` ist deshalb infallibel (kein `?`).
pub async fn archive_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
    body: Option<Json<ArchiveRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = match normalize_twitch_login(&raw_login) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid login")),
    };

    // mode-String extrahieren — Default "toggle" wenn fehlt/leer (Python-Semantik)
    let mode_str = body
        .and_then(|b| b.mode.clone())
        .map(|m| {
            let m = m.trim().to_lowercase();
            if m.is_empty() { "toggle".to_string() } else { m }
        })
        .unwrap_or_else(|| "toggle".to_string());

    // Infallible parse — unbekannte Werte → Toggle, kein 400
    let mode = db::ArchiveMode::parse(&mode_str);

    match db::archive_streamer(&pool, &login, mode).await {
        Ok(true) => Ok(Json(OkLoginMessageResponse {
            ok: true,
            login: login.clone(),
            message: "updated".to_string(),
        })
        .into_response()),
        Ok(false) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("archive_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// Discord-Action-Scope-Guard (Python `_enforce_discord_action_scope`, app.py:817-845).
/// discord-flag/profile tragen weder guild/channel/role im Body → bei gesetzter
/// Allowlist schlägt die Prüfung via None ∉ Allowlist als 403 durch (deny-by-default,
/// wie link-click). Die interne API ist loopback-only; das ist Defense-in-depth.
fn enforce_discord_action_scope() -> Result<(), ApiError> {
    use super::telemetry_routes::{
        enforce_scope_allowlist, parse_allowlist_ids, ENV_ALLOWED_CHANNEL_IDS,
        ENV_ALLOWED_GUILD_IDS, ENV_ALLOWED_ROLE_IDS,
    };
    for (env, key) in [
        (ENV_ALLOWED_GUILD_IDS, "guild_id"),
        (ENV_ALLOWED_CHANNEL_IDS, "channel_id"),
        (ENV_ALLOWED_ROLE_IDS, "role_id"),
    ] {
        enforce_scope_allowlist(None, &parse_allowlist_ids(env), key)
            .map_err(|_| ApiError::forbidden())?;
    }
    Ok(())
}

/// `POST /internal/twitch/v1/streamers/:login/discord-flag`
///
/// Body (Python/snake_case): `{"is_on_discord": true}`
/// Body (camelCase-Alias): `{"isOnDiscord": true}`
/// Body-Aliases für den Wert: "enabled", "value"
/// Fehlendes Feld → 400 (Python: "is_on_discord is required")
pub async fn discord_flag_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
    Json(body): Json<DiscordFlagRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = match normalize_twitch_login(&raw_login) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid login")),
    };

    enforce_discord_action_scope()?;

    let enabled = match body.parse_enabled() {
        Some(v) => v,
        None => return Err(ApiError::bad_request("is_on_discord is required")),
    };

    match db::set_discord_flag(&pool, &login, enabled).await {
        Ok(true) => Ok(Json(OkLoginMessageResponse {
            ok: true,
            login: login.clone(),
            message: "updated".to_string(),
        })
        .into_response()),
        Ok(false) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("set_discord_flag DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/:login/discord-profile`
///
/// Body (Python/snake_case): `{"discord_user_id": "...", "discord_display_name": "...", "mark_member": true}`
/// Aliases akzeptieren auch camelCase.
/// Validierung: discord_user_id muss numerisch sein wenn angegeben (Python: `isdigit()`).
pub async fn discord_profile_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    Extension(role_ext): Extension<DiscordRoleExt>,
    Path(raw_login): Path<String>,
    Json(body): Json<DiscordProfileRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = match normalize_twitch_login(&raw_login) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid login")),
    };

    enforce_discord_action_scope()?;

    // discord_user_id: trimmen, leer → None; nicht-numerisch → 400
    // Python: discord_id_clean = (discord_user_id or "").strip()
    //         if discord_id_clean and not discord_id_clean.isdigit(): raise ValueError(...)
    let discord_user_id: Option<String> = body
        .discord_user_id
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(ref did) = discord_user_id {
        if !did.chars().all(|c| c.is_ascii_digit()) {
            return Err(ApiError::bad_request("discord_user_id muss numerisch sein"));
        }
    }

    // display_name auf 120 Zeichen kürzen (Python-Vertrag)
    let discord_display_name: Option<String> = body
        .discord_display_name
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.chars().count() > 120 {
                s.chars().take(120).collect()
            } else {
                s
            }
        });

    // twitch_user_id auflösen wie Python (_dashboard_save_discord_profile):
    // erst aus twitch_raid_auth, sonst über Helix `GET /users`.
    let mut twitch_user_id = db::load_twitch_user_id_from_raid_auth(&pool, &login)
        .await
        .unwrap_or(None);
    if twitch_user_id.is_none() {
        if let Some(h) = helix.as_ref() {
            if let Ok(users) = h.get_users(&[login.as_str()]).await {
                twitch_user_id = users
                    .values()
                    .next()
                    .map(|u| u.id.clone())
                    .filter(|s| !s.is_empty());
            }
        }
    }

    match db::set_discord_profile(
        &pool,
        &login,
        discord_user_id.as_deref(),
        discord_display_name.as_deref(),
        body.mark_member,
        twitch_user_id.as_deref(),
    )
    .await
    {
        Ok(true) => {
            // Discord-Streamer-Rolle setzen (Python `sync_streamer_role`) —
            // best-effort, Fehler werden in der Port-Impl geloggt, nie propagiert.
            if let (Some(did), Some(port)) = (discord_user_id.as_deref(), role_ext.0.as_ref()) {
                port.grant_streamer_role(did, &format!("Discord-Profil für {login} gesetzt"))
                    .await;
            }
            Ok(Json(OkLoginMessageResponse {
                ok: true,
                login: login.clone(),
                message: "updated".to_string(),
            })
            .into_response())
        }
        Ok(false) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("set_discord_profile DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `GET /internal/twitch/v1/stats`
///
/// Query-Parameter: `hour_from`, `hour_to` (optional, UTC-Stunde 0–23), `streamer` (optional).
///
/// ACHTUNG, KEIN Python-Vertrag: Diese Shape (`total_sessions`/`avg_viewers`/…)
/// ist eine Eigen-Aggregation. Python `_dashboard_stats`
/// (`dashboard_metrics_mixin.py:212-259`) liefert
/// `{tracked:{top,hourly,weekday}, category, avg_viewers_all,
/// avg_viewers_tracked}` — der Dashboard-Split-Mode liest exakt diese
/// Felder. Deshalb NICHT verdrahten; vor einem Flip nach Python-Shape
/// neu portieren (s. Modul-Docblock).
pub async fn stats_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<StatsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Streamer-Login normalisieren wenn angegeben
    let streamer: Option<String> = params
        .streamer
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| match normalize_twitch_login(&s) {
            Some(l) => Ok(l),
            None => Err(ApiError::bad_request("invalid streamer login")),
        })
        .transpose()?;

    let row = db::stats(&pool, params.hour_from, params.hour_to, streamer.as_deref())
        .await
        .map_err(|e| {
            tracing::error!("stats DB-Fehler: {e}");
            ApiError::internal()
        })?;

    Ok(Json(json!({
        "ok": true,
        "total_sessions": row.total_sessions.unwrap_or(0),
        "avg_viewers": row.avg_viewers.unwrap_or(0.0),
        "peak_viewers": row.peak_viewers.unwrap_or(0),
        "total_duration_hours": row.total_duration_hours.unwrap_or(0.0),
        "total_follower_delta": row.total_follower_delta.unwrap_or(0),
    })))
}

/// `GET /internal/twitch/v1/analytics/streamer/:login`
///
/// Query: `days` (Default 30, Minimum 1).
pub async fn streamer_analytics_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(raw_login): Path<String>,
    Query(params): Query<AnalyticsDaysQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = match normalize_twitch_login(&raw_login) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid login")),
    };

    let days = params.days.unwrap_or(30).max(1);

    let (agg, sessions) = db::streamer_analytics(&pool, &login, days)
        .await
        .map_err(|e| {
            tracing::error!("streamer_analytics DB-Fehler: {e}");
            ApiError::internal()
        })?;

    Ok(Json(json!({
        "ok": true,
        "login": login,
        "days": days,
        "stats": agg,
        "recent_sessions": sessions,
    })))
}

/// `GET /internal/twitch/v1/analytics/comparison`
///
/// Query: `days` (Default 30, Minimum 1).
pub async fn analytics_comparison_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<AnalyticsDaysQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let days = params.days.unwrap_or(30).max(1);

    let (category, tracked, top) = db::analytics_comparison(&pool, days)
        .await
        .map_err(|e| {
            tracing::error!("analytics_comparison DB-Fehler: {e}");
            ApiError::internal()
        })?;

    // Python-Shape exakt: KEIN ok/days-Wrapper (Payload von `_comparison`
    // wird in streamers.py:436 unverändert durchgereicht).
    Ok(Json(json!({
        "category": category,
        "tracked_avg": tracked,
        "top_streamers": top,
    })))
}

/// `GET /internal/twitch/v1/sessions/:session_id`
///
/// session_id muss eine gültige Ganzzahl sein — sonst 400.
pub async fn session_detail_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(raw_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let session_id: i64 = raw_id
        .trim()
        .parse()
        .map_err(|_| ApiError::bad_request("invalid session id"))?;

    match db::session_detail(&pool, session_id).await {
        Ok(Some(detail)) => Ok(Json(json!({
            "ok": true,
            "session": detail.session,
            "timeline": detail.timeline,
            "top_chatters": detail.top_chatters,
        }))
        .into_response()),
        Ok(None) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("session_detail DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

// ── Handler-Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::{delete, get, post},
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
    use tower::ServiceExt;

    macro_rules! db_dsn_or_skip {
        () => {
            match std::env::var("TB_TEST_DATABASE_URL").ok() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!(
                            "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt"
                        );
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
            .expect("DB-Verbindung");

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

        // prod-treue DDL
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamers (
                twitch_login        TEXT PRIMARY KEY,
                twitch_user_id      TEXT,
                discord_user_id     TEXT,
                discord_display_name TEXT,
                is_on_discord       INTEGER DEFAULT 0,
                is_verified         INTEGER DEFAULT 0,
                is_monitored_only   INTEGER DEFAULT 0,
                created_at          TIMESTAMPTZ DEFAULT NOW(),
                archived_at         TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamer_identities (
                twitch_user_id      TEXT PRIMARY KEY,
                twitch_login        TEXT NOT NULL,
                discord_user_id     TEXT,
                discord_display_name TEXT,
                is_on_discord       INTEGER DEFAULT 0,
                created_at          TIMESTAMPTZ DEFAULT NOW(),
                updated_at          TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamer_identities");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login TEXT PRIMARY KEY,
                is_live        INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id                       SERIAL PRIMARY KEY,
                twitch_login             TEXT NOT NULL,
                twitch_user_id           TEXT,
                status                   TEXT DEFAULT 'active',
                manual_verified_permanent INTEGER DEFAULT 0,
                manual_verified_at       TIMESTAMPTZ,
                manual_verified_until    TIMESTAMPTZ,
                admin_archived_at        TIMESTAMPTZ,
                technical_pause_reason   TEXT,
                manual_partner_opt_out   INTEGER DEFAULT 0,
                raid_bot_enabled         INTEGER DEFAULT 1
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_partners");

        for ddl in [
            r#"CREATE TABLE IF NOT EXISTS twitch_partners_all_state (
                twitch_login TEXT, twitch_user_id TEXT,
                manual_verified_permanent INTEGER DEFAULT 0,
                manual_verified_until TEXT, manual_verified_at TEXT,
                manual_partner_opt_out INTEGER DEFAULT 0, archived_at TEXT,
                is_on_discord INTEGER DEFAULT 0, discord_user_id TEXT,
                discord_display_name TEXT, raid_bot_enabled INTEGER DEFAULT 1,
                status TEXT DEFAULT 'active' )"#,
            r#"CREATE TABLE IF NOT EXISTS twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
                raid_enabled BOOLEAN, needs_reauth BOOLEAN,
                authorized_at TIMESTAMPTZ, token_expires_at TIMESTAMPTZ )"#,
            r#"CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY, stream_id TEXT, streamer_login TEXT,
                game_name TEXT, had_deadlock_in_session BOOLEAN DEFAULT FALSE,
                started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ,
                duration_seconds BIGINT, avg_viewers FLOAT8, peak_viewers BIGINT,
                follower_delta BIGINT, followers_start BIGINT, followers_end BIGINT,
                stream_title TEXT, unique_chatters BIGINT )"#,
        ] {
            sqlx::query(ddl)
                .execute(&pool)
                .await
                .expect("DDL Listen-Quellen");
        }

        sqlx::query(
            "TRUNCATE twitch_streamers, twitch_streamer_identities, twitch_live_state, twitch_partners, twitch_partners_all_state, twitch_raid_auth, twitch_stream_sessions RESTART IDENTITY",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");

        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        let helix: Arc<Option<HelixClient>> = Arc::new(None);
        Router::new()
            .route(&format!("{base}/streamers"), get(list_handler))
            .route(&format!("{base}/streamers"), post(add_handler))
            .route(&format!("{base}/streamers/:login"), delete(remove_handler))
            .route(
                &format!("{base}/streamers/:login/verify"),
                post(verify_handler),
            )
            .route(
                &format!("{base}/streamers/:login/archive"),
                post(archive_handler),
            )
            .route(
                &format!("{base}/streamers/:login/discord-flag"),
                post(discord_flag_handler),
            )
            .route(
                &format!("{base}/streamers/:login/discord-profile"),
                post(discord_profile_handler),
            )
            .route(&format!("{base}/stats"), get(stats_handler))
            .route(
                &format!("{base}/analytics/streamer/:login"),
                get(streamer_analytics_handler),
            )
            .route(
                &format!("{base}/analytics/comparison"),
                get(analytics_comparison_handler),
            )
            .route(
                &format!("{base}/sessions/:session_id"),
                get(session_detail_handler),
            )
            .with_state(pool)
            .layer(Extension(helix))
            .layer(Extension(DiscordRoleExt(None)))
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(
                token.to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn loopback_req(method: &str, uri: &str, body: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .extension(ConnectInfo(
                "127.0.0.1:55555".parse::<SocketAddr>().unwrap(),
            ));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── Auth-Tests ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_401").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req("GET", &format!("{base}/streamers"), "", None);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── GET /streamers ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_returns_200() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_list").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req("GET", &format!("{base}/streamers"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert!(j["streamers"].is_array());
    }

    // ── DELETE /streamers/:login ─────────────────────────────────────────────

    #[tokio::test]
    async fn remove_returns_404_bei_unbekanntem_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_remove_404").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "DELETE",
            &format!("{base}/streamers/nichtvorhanden"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── POST /streamers/:login/archive ───────────────────────────────────────

    /// Python gibt NIEMALS 400 für unbekannte mode-Werte.
    /// "ungueltig" → Toggle-Semantik → User nicht gefunden → 404.
    #[tokio::test]
    async fn archive_unbekannter_mode_gibt_nicht_400_sondern_404() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_archive_unbekannt").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        // "testuser" existiert nicht → archive_streamer gibt false → 404
        // Vorher (falsches Draft-Verhalten): 400 wegen "ungültigem" mode
        // Jetzt (Python-parität): 404, da Toggle für nicht-existenten User
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/testuser/archive"),
            r#"{"mode":"ungueltig"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "unbekannter mode → Toggle → User nicht gefunden → 404 (kein 400)"
        );
    }

    /// Fehlender mode-Body → Default "toggle" → Python-Semantik.
    #[tokio::test]
    async fn archive_ohne_mode_body_gibt_404_fuer_unbekannten_user() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_archive_kein_body").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/niemand/archive"),
            r#"{}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── POST /streamers/:login/discord-profile ───────────────────────────────

    /// Nicht-numerische discord_user_id → 400.
    /// Test sendet camelCase "discordUserId" — wird via alias akzeptiert.
    #[tokio::test]
    async fn discord_profile_returns_400_bei_nicht_numerischer_discord_id() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_discord_val").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/testuser/discord-profile"),
            r#"{"discordUserId":"nicht-eine-zahl"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "nicht-numerische discord_user_id muss 400 ergeben"
        );
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
    }

    /// Python-Client sendet snake_case — muss ebenfalls validiert werden.
    #[tokio::test]
    async fn discord_profile_returns_400_bei_snake_case_nicht_numerischer_id() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_discord_snake_val").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/testuser/discord-profile"),
            r#"{"discord_user_id":"nicht-eine-zahl"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// Numerische discord_user_id (snake_case) → kein 400 (Validierung ok),
    /// nur 404 weil User nicht in DB.
    #[tokio::test]
    async fn discord_profile_numerische_id_gibt_404_fuer_unbekannten_user() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_discord_num").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/niemand/discord-profile"),
            r#"{"discord_user_id":"123456789"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── POST /streamers/:login/discord-flag ──────────────────────────────────

    /// Fehlendes Feld → 400.
    #[tokio::test]
    async fn discord_flag_ohne_feld_gibt_400() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_dflag_400").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/testuser/discord-flag"),
            r#"{}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
    }

    /// Python-Client sendet snake_case is_on_discord → 404 für unbekannten User.
    #[tokio::test]
    async fn discord_flag_snake_case_gibt_404_fuer_unbekannten_user() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_dflag_snake").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/niemand/discord-flag"),
            r#"{"is_on_discord":true}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── POST /streamers (add) ────────────────────────────────────────────────

    #[tokio::test]
    async fn add_returns_503_wenn_helix_nicht_konfiguriert() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_add_503").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers"),
            r#"{"login":"someuser"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn add_leerer_login_gibt_400() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_add_400").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers"),
            r#"{"login":""}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── GET /stats ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn stats_gibt_200_bei_leerer_db() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_stats_empty").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req("GET", &format!("{base}/stats"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert_eq!(j["total_sessions"], 0);
    }

    #[tokio::test]
    async fn stats_ungültiger_streamer_login_gibt_400() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_stats_bad_login").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        // Login "ab" ist zu kurz — normalize_twitch_login gibt None
        let req = loopback_req(
            "GET",
            &format!("{base}/stats?streamer=ab"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── GET /sessions/:session_id ─────────────────────────────────────────────

    #[tokio::test]
    async fn session_nicht_gefunden_gibt_404() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_session_404").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req("GET", &format!("{base}/sessions/99999"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn session_ungueltige_id_gibt_400() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_session_bad_id").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "GET",
            &format!("{base}/sessions/nichtganzzahl"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
