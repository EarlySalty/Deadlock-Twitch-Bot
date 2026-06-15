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
//!
//! Streamer-CRUD + Partner-Lifecycle (Block 10, nativ):
//! - DELETE /streamers/:login departnert aktive Partner voll
//!   (`departner_active_partner`: Status→departnered, Identity-Upsert,
//!   Raid-Auth-Disable) und entfernt die Discord-Streamer-Rolle.
//! - POST /streamers/:login/verify (permanent/temp) promotet via
//!   `promote_streamer_to_partner`, backfillt Kategorie-Stats und vergibt die
//!   Rolle; clear/failed departnern (clear_verification) und entziehen die Rolle.
//! - POST /streamers/:login/archive liefert kontextspezifische Meldungen
//!   (archiviert/ent-archiviert/blockiert/entsperrt/…).
//! - POST /streamers trägt require_link + next_link_check_at (now+30d) nach und
//!   backfillt Kategorie-Stats.
//!
//! Discord-DMs sind per B10-Direktive ("keine Discord-DMs mehr") GEDROPPT
//! (Verify-Erfolgs-/Fehler-DM): die Meldungen reduzieren sich auf Rollen-Sync.
//! Lifecycle-DB-Logik liegt in [`crate::streamer_lifecycle`].
//!
//! POST /streamers/:login/chat-action ist seit der Bot-Token-Bridge (F3) nativ:
//! der Handler (python_stubs::chat_action_handler) sendet über den
//! [`ChatActionPort`] mit dem live rotierten Bot-User-Token; ohne Port (Chat aus)
//! antwortet er 503.
//!
//! Alle Endpoints: `auth.is_privileged()` → 401.
//!
//! Request-Body-Konventionen:
//! - Bestandskonsumenten (Python-Client) senden snake_case-Bodies.
//! - Felder mit Underscores akzeptieren via `#[serde(alias)]` auch camelCase.
//! - Kein Idempotency-Caching (kommt mit dem geteilten Idempotenz-Layer).
//! - Discord-Rollen-Sync: best-effort über [`DiscordRolePort`] (grant/revoke);
//!   EventSub-Supervisor-Trigger: Handoff (kein Port → tb-bot-Wiring nötig).
//!
//! archive-mode-Semantik:
//! - Python gibt NIEMALS 400 für unbekannte mode-Werte — unbekannte Werte
//!   fallen durch auf "toggle". Die Meldung ist kontextabhängig (s.
//!   `lifecycle::archive_with_message`).
//!
//! verify-mode-Semantik:
//! - permanent/temp promoten + backfillen + Rolle vergeben; clear/failed
//!   departnern (clear_verification) + Rolle entziehen; unbekannte Modi → 200
//!   "Unbekannter Modus" (Python-Parität, KEIN Permanent-Fallback).

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
use crate::streamer_lifecycle as lifecycle;
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
    /// Vergibt die Streamer-Rolle (Python `sync_streamer_role(should_have_role=True)`).
    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str);

    /// Entfernt die Streamer-Rolle (Python `sync_streamer_role(should_have_role=False)`).
    /// Default-Impl no-op, damit bestehende Test-Doubles nicht brechen; die
    /// echte tb-bot-Impl entzieht die Rolle über den Master-Broker.
    async fn revoke_streamer_role(&self, _discord_user_id: &str, _reason: &str) {}
}

/// Router-Extension-Wrapper für [`DiscordRolePort`] (`None` = kein Sync).
#[derive(Clone)]
pub struct DiscordRoleExt(pub Option<Arc<dyn DiscordRolePort>>);

// ── Chat-Action-Port (Bot-Token-Bridge) ────────────────────────────────────────

/// Ergebnis eines `POST /streamers/:login/chat-action`.
///
/// Trennt sauber zwischen zugestellt, von Twitch verworfen (`is_sent=false`,
/// z. B. Stummschaltung/Channel-Settings) und Fehler. Der Drop-Fall wird NIE
/// als Erfolg gefälscht — Python-Parität (`mixin.py:_dashboard_partner_chat_action`
/// liefert in dem Fall die „konnte nicht gesendet werden"-Meldung).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatActionResult {
    /// Nachricht/Ankündigung zugestellt. `label` = menschenlesbare Bestätigung.
    Sent { label: String },
    /// Twitch hat verworfen (z. B. `channel_settings`, `sender_banned`).
    Dropped { code: String, message: String },
    /// Broadcaster-User-ID zu diesem Login nicht auflösbar.
    UnknownChannel,
    /// Senden schlug fehl (HTTP-Fehler/Token nicht verfügbar). `reason` ist
    /// log-tauglich (enthält NIE den Token).
    Failed { reason: String },
}

/// Port für die Owner-Chat-Action: sendet über den live rotierten Bot-User-Token.
///
/// Echte Impl in tb-bot (Composition-Root), die den Broadcaster-Login zur
/// User-ID auflöst und je nach `mode` (`message`/`action`/`announcement`) via
/// `ChatApi::send_message`/`send_announcement` mit dem aktuellen Bot-Token sendet.
/// `None` als Router-Extension (Chat aus / Token nicht gebootet) → Handler
/// antwortet 503 statt stumm zu scheitern.
#[async_trait::async_trait]
pub trait ChatActionPort: Send + Sync {
    /// `mode` ∈ {`message`, `action`, `announcement`} (unbekannt → `message`),
    /// `color` für Announcements (unbekannt → `purple`), `message` ist nicht leer.
    async fn send_chat_action(
        &self,
        login: &str,
        mode: &str,
        color: &str,
        message: &str,
    ) -> ChatActionResult;
}

/// Router-Extension-Wrapper für [`ChatActionPort`] (`None` = Chat-Send aus → 503).
#[derive(Clone)]
pub struct ChatActionExt(pub Option<Arc<dyn ChatActionPort>>);

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

    // require_link aus dem Body (Python `_cmd_add(login, require_link)`,
    // Default false). Akzeptiert bool/Zahl/String wie der übrige Body.
    let require_link = parse_bool_loose(
        body.get("require_link")
            .or_else(|| body.get("require_discord_link")),
    );

    use db::AddStreamerResult;
    match db::add_streamer(&pool, &login, user_id.as_deref()).await {
        Ok(AddStreamerResult::AlreadyExists) => Ok((
            StatusCode::OK,
            Json(json!({"ok": true, "login": login, "message": "already_active_partner"})),
        )
            .into_response()),
        Ok(AddStreamerResult::Added) => {
            // require_link + next_link_check_at (now+30d) nachtragen und
            // Kategorie-Stats backfillen (Python `_cmd_add`: upsert_non_partner_streamer
            // + backfill_tracked_stats_from_category). Beide best-effort über
            // die native Lifecycle-Schicht; Fehler werden geloggt, nicht propagiert.
            if let Err(e) = lifecycle::backfill_require_link(&pool, &login, require_link).await {
                tracing::warn!("backfill_require_link für {login} fehlgeschlagen: {e}");
            }
            let copied = lifecycle::backfill_tracked_stats_from_category(&pool, &login)
                .await
                .unwrap_or(0);
            let suffix = if copied > 0 {
                format!(" ({copied} historische Datenpunkte übernommen)")
            } else {
                String::new()
            };
            Ok((
                StatusCode::CREATED,
                Json(json!({"ok": true, "login": login, "message": format!("{login} hinzugefügt{suffix}")})),
            )
                .into_response())
        }
        Err(e) => {
            tracing::error!("add_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// Parst einen losen Bool-Wert (bool/Zahl/String) wie Pythons `_parse_bool`;
/// fehlend/null/unbekannt → `false`.
fn parse_bool_loose(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(serde_json::Value::String(s)) => {
            matches!(s.trim().to_lowercase().as_str(), "true" | "1" | "yes" | "on")
        }
        _ => false,
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
///
/// Voller Departner-Lifecycle (Parität Python `_cmd_remove`, `admin.py:256`):
/// 1. Aktiven Partner departnern (`departner_active_partner`): Status →
///    `departnered`, Identity-Upsert, Raid-Auth-Disable.
/// 2. War kein aktiver Partner da: Streamer-Zeile archivieren/löschen wie bisher.
/// 3. `twitch_live_state`-Zeile löschen (idempotent).
/// 4. Bei departnertem Partner mit Discord-ID: Streamer-Rolle entfernen
///    (best-effort über [`DiscordRolePort`]) und Meldung
///    `"{login} operativ deaktiviert (Streamer-Rolle entfernt)"`.
///
/// Migrations-Bug-Fix (vorher 1:1 mitgeschleppt): der alte Pfad setzte nur
/// `twitch_streamers.archived_at` und ließ `twitch_partners` aktiv — ein
/// entfernter Partner blieb in der Partner-Wahrheit aktiv (Raid-Bot lief weiter,
/// Discord-Rolle blieb). Test `remove_departnert_aktiven_partner` lockt das ein.
pub async fn remove_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Extension(role_ext): Extension<DiscordRoleExt>,
    Path(raw_login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = match normalize_twitch_login(&raw_login) {
        Some(l) => l,
        None => return Err(ApiError::bad_request("invalid login")),
    };

    // Schritt 1: aktiven Partner departnern (Status-Wechsel + Raid-Auth-Disable).
    let departnered = lifecycle::departner_active_partner(&pool, &login, false)
        .await
        .map_err(|e| {
            tracing::error!("departner_active_partner DB-Fehler: {e}");
            ApiError::internal()
        })?;

    // Schritt 2: ohne aktiven Partner → Streamer-Zeile archivieren/löschen.
    let removed_streamer = if departnered.is_none() {
        use db::RemoveStreamerResult;
        match db::remove_streamer(&pool, &login).await {
            Ok(RemoveStreamerResult::NotFound) => false,
            Ok(_) => true,
            Err(e) => {
                tracing::error!("remove_streamer DB-Fehler: {e}");
                return Err(ApiError::internal());
            }
        }
    } else {
        // remove_streamer löscht twitch_live_state mit; im Departner-Pfad
        // holen wir das separat nach (Python: explizites DELETE in _cmd_remove).
        lifecycle::clear_live_state(&pool, &login).await.map_err(|e| {
            tracing::error!("clear_live_state DB-Fehler: {e}");
            ApiError::internal()
        })?;
        true
    };

    // Schritt 3: Antwort nach Python `_cmd_remove`.
    if let Some(outcome) = departnered {
        let mut role_note = String::new();
        if let (Some(did), Some(port)) = (outcome.discord_user_id.as_deref(), role_ext.0.as_ref()) {
            port.revoke_streamer_role(did, "Streamer als Partner deaktiviert")
                .await;
            role_note = " (Streamer-Rolle entfernt)".to_string();
        }
        return Ok(Json(OkLoginMessageResponse {
            ok: true,
            login: login.clone(),
            message: format!("{login} operativ deaktiviert{role_note}"),
        })
        .into_response());
    }

    if removed_streamer {
        return Ok(Json(OkLoginMessageResponse {
            ok: true,
            login: login.clone(),
            message: format!("{login} entfernt"),
        })
        .into_response());
    }

    // Python: "{login} war nicht gespeichert" → 404 (interne API-Konvention).
    Err(ApiError::not_found())
}

/// `POST /internal/twitch/v1/streamers/:login/verify`
///
/// Body: `{"mode": "permanent"|"temp"|"clear"|"failed"}` — Default: "permanent".
/// Voller nativer Verify-Lifecycle (Parität Python `_dashboard_verify` /
/// `_dashboard_verify_storage_step`, `streamer_admin_mixin.py:291/475`):
///
/// - `permanent`/`temp`: Streamer (oder aktiven Partner) zum Partner promoten
///   (`promote_streamer_to_partner`), Kategorie-Stats backfillen, Streamer-Rolle
///   vergeben. Ohne auflösbare `twitch_user_id` → "{login} ist nicht gespeichert".
///   Meldung: `"{login} dauerhaft verifiziert"` bzw. `"… für 30 Tage verifiziert"`
///   + `"(N historische Datenpunkte übernommen)"` + `"(Streamer-Rolle vergeben)"`.
/// - `clear`: departnern (`clear_verification=True`), Rolle entfernen, KEINE DM.
///   Meldung: `"Verifizierung für {login} zurückgesetzt (keine DM versendet) …"`.
/// - `failed`: departnern, Rolle entfernen. Die Python-Fehler-DM ist per
///   B10-Direktive ("keine Discord-DMs mehr") bewusst gedroppt — die Meldung
///   reduziert sich auf den Rollen-Entzug.
/// - unbekannter Modus → "Unbekannter Modus" (200, keine Mutation).
///
/// Alle Geschäftsfälle antworten 200 `{ok, login, message}` (Python-Parität).
/// DMs (`_notify_verification_success`, Fehler-DM) sind per B10-Direktive raus.
pub async fn verify_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    Extension(role_ext): Extension<DiscordRoleExt>,
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

    let ok_message = |message: String| {
        Json(OkLoginMessageResponse {
            ok: true,
            login: login.clone(),
            message,
        })
        .into_response()
    };

    let internal = |e: sqlx::Error, ctx: &str| {
        tracing::error!("verify_handler {ctx} DB-Fehler: {e}");
        ApiError::internal()
    };

    match mode.as_str() {
        "permanent" | "temp" => {
            let payload = lifecycle::VerificationPayload::for_mode(&mode)
                .ok_or_else(ApiError::internal)?; // permanent/temp sind immer Some
            let source = lifecycle::load_verify_source(&pool, &login, helix.as_ref().as_ref())
                .await
                .map_err(|e| internal(e, "load_verify_source"))?;
            let Some(source) = source.filter(|s| s.twitch_user_id_present()) else {
                return Ok(ok_message(format!("{login} ist nicht gespeichert")));
            };

            lifecycle::promote_streamer_to_partner(
                &pool,
                &login,
                &source.twitch_user_id,
                source.discord_user_id.as_deref(),
                source.discord_display_name.as_deref(),
                if source.discord_user_id.is_some() { 1 } else { 0 },
                &payload,
            )
            .await
            .map_err(|e| internal(e, "promote"))?;

            let copied = lifecycle::backfill_tracked_stats_from_category(&pool, &login)
                .await
                .map_err(|e| internal(e, "backfill"))?;

            let base = if mode == "temp" {
                format!("{login} für 30 Tage verifiziert")
            } else {
                format!("{login} dauerhaft verifiziert")
            };
            let mut notes: Vec<String> = Vec::new();
            if copied > 0 {
                notes.push(format!("({copied} historische Datenpunkte übernommen)"));
            }
            // Streamer-Rolle vergeben (best-effort).
            if let (Some(did), Some(port)) =
                (source.discord_user_id.as_deref(), role_ext.0.as_ref())
            {
                port.grant_streamer_role(
                    did,
                    "Streamer-Verifizierung über Dashboard bestätigt",
                )
                .await;
                notes.push("(Streamer-Rolle vergeben)".to_string());
            }
            let merged = notes.join(" ");
            Ok(ok_message(format!("{base} {merged}").trim().to_string()))
        }
        "clear" => {
            let outcome = lifecycle::departner_active_partner(&pool, &login, true)
                .await
                .map_err(|e| internal(e, "departner_clear"))?;
            let Some(outcome) = outcome else {
                return Ok(ok_message(format!("{login} ist nicht gespeichert")));
            };
            let role_note = revoke_role_note(
                &role_ext,
                outcome.discord_user_id.as_deref(),
                "Streamer-Verifizierung über Dashboard entfernt",
            )
            .await;
            let msg = format!(
                "Verifizierung für {login} zurückgesetzt (keine DM versendet) {role_note}"
            );
            Ok(ok_message(msg.trim().to_string()))
        }
        "failed" => {
            let outcome = lifecycle::departner_active_partner(&pool, &login, true)
                .await
                .map_err(|e| internal(e, "departner_failed"))?;
            let Some(outcome) = outcome else {
                return Ok(ok_message(format!("{login} ist nicht gespeichert")));
            };
            // Python sendet hier zusätzlich eine Fehler-DM — per B10-Direktive
            // ("keine Discord-DMs mehr") gedroppt. Nur der Rollen-Entzug bleibt.
            let role_note = revoke_role_note(
                &role_ext,
                outcome.discord_user_id.as_deref(),
                "Streamer-Verifizierung über Dashboard fehlgeschlagen",
            )
            .await;
            Ok(ok_message(
                format!("{login}: Verifizierung fehlgeschlagen {role_note}")
                    .trim()
                    .to_string(),
            ))
        }
        _ => Ok(ok_message("Unbekannter Modus".to_string())),
    }
}

/// Entfernt best-effort die Streamer-Rolle und gibt die Python-Notiz zurück.
async fn revoke_role_note(
    role_ext: &DiscordRoleExt,
    discord_user_id: Option<&str>,
    reason: &str,
) -> String {
    if let (Some(did), Some(port)) = (discord_user_id, role_ext.0.as_ref()) {
        port.revoke_streamer_role(did, reason).await;
        "(Streamer-Rolle entfernt)".to_string()
    } else {
        String::new()
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

    // Kontextspezifische Meldung + Mutation in einem (Python `_dashboard_archive_sync`):
    // 'X archiviert' / 'X ent-archiviert' / 'X dauerhaft blockiert' / 'X entsperrt' /
    // 'X ist bereits archiviert (seit …)' / 'X reaktiviert' usw.
    use lifecycle::ArchiveOutcome;
    match lifecycle::archive_with_message(&pool, &login, &mode_str).await {
        Ok(ArchiveOutcome::Done(message)) => Ok(Json(OkLoginMessageResponse {
            ok: true,
            login: login.clone(),
            message,
        })
        .into_response()),
        // Python: nicht gespeichert → ValueError → 4xx. Interne API: 404.
        Ok(ArchiveOutcome::NotStored) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("archive_with_message DB-Fehler: {e}");
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
                require_discord_link INTEGER DEFAULT 0,
                next_link_check_at  TEXT,
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
                -- TEXT wie Prod (Python schreibt ISO-Strings; der Lifecycle bindet
                -- created_at/updated_at als ISO-String, nicht NOW()).
                created_at          TEXT,
                updated_at          TEXT
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

        // Timestamp-Spalten TEXT wie Prod (Python schreibt ISO-Strings) —
        // der Lifecycle dekodiert admin_archived_at/departnered_at als String.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id                       BIGINT PRIMARY KEY,
                twitch_login             TEXT NOT NULL,
                twitch_user_id           TEXT,
                status                   TEXT DEFAULT 'active',
                require_discord_link     INTEGER DEFAULT 0,
                next_link_check_at       TEXT,
                manual_verified_permanent INTEGER DEFAULT 0,
                manual_verified_at       TEXT,
                manual_verified_until    TEXT,
                partnered_at             TEXT DEFAULT CURRENT_TIMESTAMP,
                admin_archived_at        TEXT,
                departnered_at           TEXT,
                technical_pause_reason   TEXT,
                manual_partner_opt_out   INTEGER DEFAULT 0,
                raid_bot_enabled         INTEGER DEFAULT 1
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_partners");

        // Stats-Tabellen für den Kategorie-Backfill (Verify/Add-Pfad).
        for ddl in [
            r#"CREATE TABLE IF NOT EXISTS twitch_stats_category (
                ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
                is_partner INTEGER, game_name TEXT, stream_title TEXT, tags TEXT )"#,
            r#"CREATE TABLE IF NOT EXISTS twitch_stats_tracked (
                ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
                is_partner INTEGER, game_name TEXT, stream_title TEXT, tags TEXT )"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("DDL stats");
        }

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

    /// Aktiver Partner anlegen (mit Identity für Discord-Daten + Raid-Auth).
    async fn seed_active_partner(pool: &PgPool, login: &str, uid: &str) {
        sqlx::query(
            "INSERT INTO twitch_partners (id, twitch_login, twitch_user_id, status, manual_verified_permanent)
             VALUES (1, $1, $2, 'active', 1)",
        )
        .bind(login)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord)
             VALUES ($1, $2, '555', 'Name', 1)",
        )
        .bind(uid)
        .bind(login)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled) VALUES ($1, $2, TRUE)")
            .bind(uid)
            .bind(login)
            .execute(pool)
            .await
            .unwrap();
    }

    /// DELETE departnert einen aktiven Partner (Status→departnered, Raid-Auth aus)
    /// statt nur twitch_streamers zu archivieren — der Block-10-Lifecycle-Fix.
    #[tokio::test]
    async fn remove_departnert_aktiven_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_remove_departner").await;
        seed_active_partner(&pool, "drag", "42").await;
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req("DELETE", &format!("{base}/streamers/drag"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert!(
            j["message"].as_str().unwrap().contains("operativ deaktiviert"),
            "Python-Meldung _cmd_remove, war='{}'",
            j["message"]
        );

        let (status, raid): (Option<String>, Option<bool>) = sqlx::query_as(
            "SELECT p.status, a.raid_enabled FROM twitch_partners p
             LEFT JOIN twitch_raid_auth a ON a.twitch_user_id = p.twitch_user_id
             WHERE p.id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status.as_deref(), Some("departnered"));
        assert_eq!(raid, Some(false), "raid-auth disabled beim Departnern");
    }

    /// verify mode=clear departnert nativ (vorher: 503). Antwortet 200 mit Meldung.
    #[tokio::test]
    async fn verify_clear_departnert_statt_503() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_verify_clear").await;
        seed_active_partner(&pool, "drag", "42").await;
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/drag/verify"),
            r#"{"mode":"clear"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "clear ist nativ, kein 503 mehr");
        let j = json_body(resp).await;
        assert!(
            j["message"].as_str().unwrap().contains("zurückgesetzt"),
            "war='{}'",
            j["message"]
        );

        let (status, mvp): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT status, manual_verified_permanent FROM twitch_partners WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status.as_deref(), Some("departnered"));
        assert_eq!(mvp, Some(0), "clear_verification → permanent zurückgesetzt");
    }

    /// verify mode=permanent promotet einen Nicht-Partner zum aktiven Partner.
    #[tokio::test]
    async fn verify_permanent_promotet_nicht_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_verify_promote").await;
        // Nicht-Partner in twitch_streamers mit twitch_user_id.
        sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('newbie', '321')")
            .execute(&pool)
            .await
            .unwrap();
        let app = make_router(pool.clone(), "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/newbie/verify"),
            r#"{"mode":"permanent"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert!(
            j["message"].as_str().unwrap().contains("dauerhaft verifiziert"),
            "war='{}'",
            j["message"]
        );

        let (status, mvp): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT status, manual_verified_permanent FROM twitch_partners WHERE LOWER(twitch_login) = 'newbie'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status.as_deref(), Some("active"), "promoted");
        assert_eq!(mvp, Some(1));
    }

    /// verify mode=permanent für unbekannten Login (keine user_id auflösbar) →
    /// "nicht gespeichert" (200), keine Promotion.
    #[tokio::test]
    async fn verify_permanent_unbekannt_gibt_nicht_gespeichert() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_verify_unknown_login").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/niemand/verify"),
            r#"{"mode":"permanent"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert!(j["message"].as_str().unwrap().contains("nicht gespeichert"));
    }

    /// verify unbekannter Modus → 200 "Unbekannter Modus", keine Mutation.
    #[tokio::test]
    async fn verify_unbekannter_modus_gibt_200() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_verify_badmode").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/drag/verify"),
            r#"{"mode":"quatsch"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["message"], "Unbekannter Modus");
    }

    /// archive eines aktiven Partners liefert die kontextspezifische Meldung
    /// "{login} archiviert" statt generisch "updated".
    #[tokio::test]
    async fn archive_aktiver_partner_liefert_kontext_meldung() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sh_archive_msg").await;
        seed_active_partner(&pool, "drag", "42").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/drag/archive"),
            r#"{"mode":"archive"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["message"], "drag archiviert");
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
