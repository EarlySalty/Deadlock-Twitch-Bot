//! Handler für die Admin-Streamer-Endpoints.
//!
//! Lesend: `GET /twitch/api/admin/streamers`, `GET …/{login}`.
//! Schreibend (B11-PR-4): `POST …/{login}/verify`, `…/{login}/archive`,
//! `…/{login}/block`, `…/{login}/discord-flag`. Die reine DB-Logik liegt in
//! [`tb_analytics::streamers_crud`]; hier nur Routen-/Handler-Verdrahtung
//! (Body-Parsing, AuthLevel, Status-Mapping) — Parität zu Pythons
//! `partner_registry`-Dashboard-Routen.

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_analytics::admin_streamers::{
    list_streamers, partner_status, scope_snapshot, streamer_detail, streamer_stats_and_sessions,
    StreamerView,
};
use tb_analytics::streamers_crud::{
    archive_streamer, departner_streamer, set_discord_flag, verify_streamer, ArchiveMode,
    VerifyStreamerResult,
};
use tb_http_core::{ApiError, AuthLevel};

// ── Query-Parameter ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListQuery {
    pub view: Option<String>,
}

// ── List-Response ────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStreamersResponse {
    pub items: Vec<AdminStreamerItem>,
    pub count: usize,
    pub view: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStreamerItem {
    pub login: String,
    pub display_name: String,
    pub twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub verified: bool,
    pub archived: bool,
    pub archived_at: Option<String>,
    pub created_at: Option<String>,
    pub is_live: bool,
    pub is_on_discord: bool,
    pub manual_partner_opt_out: bool,
    pub partner_status: String,
    pub viewer_count: i64,
    pub active_session_id: Option<i64>,
    pub last_seen_at: Option<String>,
    pub last_game: Option<String>,
    pub last_stream_at: Option<String>,
    pub plan_id: Option<String>,
    pub billing_status: Option<String>,
    pub oauth_connected: bool,
    pub oauth_needs_reauth: bool,
    pub oauth_status: String,
    pub granted_scopes: Vec<String>,
    pub missing_scopes: Vec<String>,
    pub oauth_authorized_at: Option<String>,
    pub promo_disabled: bool,
    pub notes: Option<String>,
    pub technical_pause_reason: Option<String>,
    pub operational_state: Option<String>,
    /// Abgeleiteter Anzeige-Status: "live" | "verified" | "offline" | "archived" |
    /// "departnered" | "blocked" | "non_partner" | "token_error"
    pub status: String,
}

// ── Detail-Response ──────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStreamerDetailResponse {
    pub login: String,
    pub display_name: String,
    pub twitch_user_id: Option<String>,
    pub verified: bool,
    pub archived: bool,
    pub archived_at: Option<String>,
    pub created_at: Option<String>,
    pub is_live: bool,
    pub partner_status: String,
    pub plan_id: Option<String>,
    pub stats: StreamerStats,
    pub sessions: Vec<StreamerSession>,
    pub settings: StreamerSettings,
    pub oauth: StreamerOAuth,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerStats {
    pub total_sessions: i64,
    /// `round(SUM(duration_seconds)/3600, 2)` — Python `totalWatchHours`
    /// (gerundete Stunden, nicht rohe Sekunden).
    pub total_watch_hours: f64,
    pub avg_viewers: f64,
    pub peak_viewers: i32,
    pub follower_delta: i64,
    /// Aus dem Live-State der Streamer-Row (nicht dem Session-Aggregat).
    pub viewer_count: i64,
    pub last_seen_at: Option<String>,
    pub last_started_at: Option<String>,
    pub last_game: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerSession {
    pub id: i64,
    /// P1.34: Frontend-Alias zu `id` (admin_dashboard StreamerDetail).
    pub session_id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub stream_title: Option<String>,
    /// P1.34: Frontend-Alias zu `stream_title`.
    pub title: Option<String>,
    pub game_name: Option<String>,
    /// P1.34: Frontend-Alias zu `game_name`.
    pub category: Option<String>,
    pub avg_viewers: Option<f64>,
    /// P1.34: Frontend-Alias zu `avg_viewers`.
    pub average_viewers: Option<f64>,
    pub peak_viewers: Option<i32>,
    pub duration_seconds: Option<i32>,
    /// P1.34: gerundete Stunden aus `duration_seconds` (Python `watchTimeHours`).
    pub watch_time_hours: f64,
    pub follower_delta: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerSettings {
    pub raid_bot_enabled: bool,
    pub silent_ban: bool,
    pub silent_raid: bool,
    pub is_monitored_only: bool,
    pub live_ping_enabled: bool,
    pub promo_disabled: bool,
    pub promo_message: Option<String>,
    pub raid_boost_enabled: bool,
    pub notes: Option<String>,
    pub plan_name: Option<String>,
    pub manual_plan_id: Option<String>,
    pub manual_plan_expires_at: Option<String>,
    pub manual_plan_notes: Option<String>,
    pub billing_plan_id: Option<String>,
    pub billing_status: Option<String>,
    pub is_on_discord: bool,
    pub require_discord_link: bool,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub created_at: Option<String>,
    pub archived_at: Option<String>,
    pub operational_state: Option<String>,
    pub technical_pause_reason: Option<String>,
    /// P3.20: manueller Partner-Opt-Out (Python settings.manualPartnerOptOut).
    pub manual_partner_opt_out: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerOAuth {
    pub connected: bool,
    pub needs_reauth: bool,
    pub status: String,
    pub granted_scopes: Vec<String>,
    pub missing_scopes: Vec<String>,
    pub authorized_at: Option<String>,
    pub raid_enabled: bool,
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

fn fmt_dt(dt: DateTime<Utc>) -> String {
    let micros = dt.timestamp_subsec_micros();
    if micros == 0 {
        dt.format("%Y-%m-%dT%H:%M:%S+00").to_string()
    } else {
        format!("{}.{micros:06}+00", dt.format("%Y-%m-%dT%H:%M:%S"))
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /twitch/api/admin/streamers?view=<view>`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // P2.80: Default-View ist "active" (Python _admin_parse_streamer_view),
    // nicht "all". Fehlender/leerer Param → Active.
    let view = StreamerView::parse_or_default(params.view.as_deref()).ok_or_else(|| {
        ApiError::bad_request_with_body(serde_json::json!({
            "error": "invalid_view",
            "supported": StreamerView::all_names(),
        }))
    })?;
    let view_str = view.canonical_name();

    let rows = list_streamers(&pool, view).await.map_err(|e| {
        tracing::error!("list_streamers Fehler: {e}");
        ApiError::internal()
    })?;

    let items: Vec<AdminStreamerItem> = rows
        .into_iter()
        .map(|r| {
            let snap = scope_snapshot(r.scopes.as_deref(), r.needs_reauth.unwrap_or(false));
            let ps = partner_status(
                r.status.as_deref(),
                r.archived_at.as_deref(),
                r.manual_partner_opt_out.unwrap_or(0),
                r.technical_pause_reason.as_deref(),
            );
            // Abgeleiteter Anzeige-Status — Python admin_streamer_queries.py:358-375:
            // Lifecycle-Status hat Vorrang vor live/verified, Endfallback "offline".
            let display_status = match ps {
                "blocked" | "non_partner" | "departnered" | "archived" | "token_error" => ps,
                _ if r.is_live != 0 => "live",
                _ if r.is_verified != 0 => "verified",
                _ => "offline",
            };

            AdminStreamerItem {
                login: r.twitch_login.clone(),
                // Python: discord_display_name bevorzugt, sonst Login.
                display_name: r
                    .discord_display_name
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| r.twitch_login.clone()),
                twitch_user_id: r.twitch_user_id,
                discord_user_id: r.discord_user_id,
                discord_display_name: r.discord_display_name,
                verified: r.is_verified != 0,
                // P3.21: leerer archived_at-String gilt NICHT als archiviert
                // (konsistent mit detail_handler + partner_status; Python
                // admin_streamer_queries.py:385 `bool(archived_at)`).
                archived: r
                    .archived_at
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty()),
                archived_at: r.archived_at,
                created_at: r.created_at,
                is_live: r.is_live != 0,
                is_on_discord: r.is_on_discord.unwrap_or(0) != 0,
                manual_partner_opt_out: r.manual_partner_opt_out.unwrap_or(0) != 0,
                partner_status: ps.to_string(),
                viewer_count: r.last_viewer_count.unwrap_or(0) as i64,
                active_session_id: r.active_session_id,
                last_seen_at: r.last_seen_at,
                last_game: r.last_game,
                last_stream_at: r.last_stream_at.map(fmt_dt),
                // Python admin_streamer_queries.py:397-402: manual_plan_id ZUERST
                // (Admin-Override), dann billing; leere/Whitespace-Werte = None.
                plan_id: r
                    .manual_plan_id
                    .filter(|s| !s.trim().is_empty())
                    .or(r.billing_plan_id.filter(|s| !s.trim().is_empty()))
                    .map(|s| s.trim().to_string()),
                billing_status: r.billing_status,
                oauth_connected: snap.connected,
                oauth_needs_reauth: snap.needs_reauth,
                oauth_status: snap.status.to_string(),
                granted_scopes: snap.granted_scopes,
                missing_scopes: snap.missing_scopes,
                oauth_authorized_at: r.authorized_at.map(fmt_dt),
                promo_disabled: r.promo_disabled.unwrap_or(0) != 0,
                notes: r.manual_plan_notes, // Python: manual_plan_notes als notes
                technical_pause_reason: r.technical_pause_reason,
                operational_state: r.operational_state,
                status: display_status.to_string(),
            }
        })
        .collect();

    let count = items.len();
    Ok(Json(AdminStreamersResponse {
        items,
        count,
        view: view_str.to_string(),
    }))
}

/// `GET /twitch/api/admin/streamers/{login}`
pub async fn detail_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let row = streamer_detail(&pool, &login)
        .await
        .map_err(|e| {
            tracing::error!("streamer_detail Fehler für {login}: {e}");
            ApiError::internal()
        })?
        .ok_or_else(ApiError::not_found)?;

    let (stats_row, session_rows) =
        streamer_stats_and_sessions(&pool, &login)
            .await
            .map_err(|e| {
                tracing::error!("streamer_stats_and_sessions Fehler für {login}: {e}");
                ApiError::internal()
            })?;

    let snap = scope_snapshot(row.scopes.as_deref(), row.needs_reauth.unwrap_or(false));
    let ps = partner_status(
        row.status.as_deref(),
        row.archived_at.as_deref(),
        row.manual_partner_opt_out.unwrap_or(0),
        row.technical_pause_reason.as_deref(),
    );

    let sessions = session_rows
        .into_iter()
        .map(|s| {
            // P1.34: watchTimeHours = round(duration_seconds/3600, 2), 0.0 ohne Dauer.
            let watch_time_hours =
                ((s.duration_seconds.unwrap_or(0) as f64 / 3600.0) * 100.0).round() / 100.0;
            StreamerSession {
                id: s.id,
                session_id: s.id,
                started_at: fmt_dt(s.started_at),
                ended_at: s.ended_at.map(fmt_dt),
                title: s.stream_title.clone(),
                stream_title: s.stream_title,
                category: s.game_name.clone(),
                game_name: s.game_name,
                average_viewers: s.avg_viewers,
                avg_viewers: s.avg_viewers,
                peak_viewers: s.peak_viewers,
                duration_seconds: s.duration_seconds,
                watch_time_hours,
                follower_delta: s.follower_delta,
            }
        })
        .collect();

    // Abgeleitete Top-Level-Felder VOR dem Response-Bau berechnen — `settings`
    // moved unten dieselben row-Felder, daher hier klonen/borgen (Python
    // _admin_streamer_detail_payload Z. 458-494).
    let display_name = row
        .discord_display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| row.twitch_login.clone());
    let verified = row.is_verified != 0;
    let is_live = row.is_live != 0;
    let archived = row
        .archived_at
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let archived_at = row.archived_at.clone();
    let created_at = row.created_at.clone();
    // planId = manual_plan_id || billing_plan_id || plan_name (erster nicht-leerer).
    let plan_id = [&row.manual_plan_id, &row.billing_plan_id, &row.plan_name]
        .into_iter()
        .filter_map(|f| f.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .next()
        .map(str::to_string);
    // totalWatchHours: gerundete Stunden aus dem Sekunden-Aggregat (round 2).
    let total_watch_hours =
        ((stats_row.total_duration_seconds as f64 / 3600.0) * 100.0).round() / 100.0;
    // viewerCount/lastSeenAt/lastStartedAt/lastGame stammen aus dem Live-State der
    // Streamer-Row (nicht dem Session-Aggregat) — settings nutzt sie nicht, daher move.
    let viewer_count = row.last_viewer_count.unwrap_or(0) as i64;
    let last_seen_at = row.last_seen_at.clone();
    let last_started_at = row.last_started_at.clone();
    let last_game = row.last_game.clone();

    Ok(Json(AdminStreamerDetailResponse {
        login: row.twitch_login.clone(),
        display_name,
        twitch_user_id: row.twitch_user_id.clone(),
        verified,
        archived,
        archived_at,
        created_at,
        is_live,
        partner_status: ps.to_string(),
        plan_id,
        stats: StreamerStats {
            total_sessions: stats_row.total_sessions,
            total_watch_hours,
            avg_viewers: stats_row.avg_viewers,
            peak_viewers: stats_row.peak_viewers,
            follower_delta: stats_row.follower_delta,
            viewer_count,
            last_seen_at,
            last_started_at,
            last_game,
        },
        sessions,
        settings: StreamerSettings {
            raid_bot_enabled: row.raid_bot_enabled.unwrap_or(1) != 0,
            silent_ban: row.silent_ban.unwrap_or(0) != 0,
            silent_raid: row.silent_raid.unwrap_or(0) != 0,
            is_monitored_only: row.is_monitored_only.unwrap_or(0) != 0,
            live_ping_enabled: row.live_ping_enabled != 0,
            promo_disabled: row.promo_disabled.unwrap_or(0) != 0,
            promo_message: row.promo_message,
            raid_boost_enabled: row.raid_boost_enabled.unwrap_or(0) != 0,
            notes: row.notes,
            plan_name: row.plan_name,
            manual_plan_id: row.manual_plan_id,
            manual_plan_expires_at: row.manual_plan_expires_at,
            manual_plan_notes: row.manual_plan_notes,
            billing_plan_id: row.billing_plan_id,
            billing_status: row.billing_status,
            is_on_discord: row.is_on_discord.unwrap_or(0) != 0,
            require_discord_link: row.require_discord_link.unwrap_or(0) != 0,
            discord_user_id: row.discord_user_id,
            discord_display_name: row.discord_display_name,
            created_at: row.created_at,
            archived_at: row.archived_at,
            operational_state: row.operational_state,
            technical_pause_reason: row.technical_pause_reason,
            manual_partner_opt_out: row.manual_partner_opt_out.unwrap_or(0) != 0,
        },
        oauth: StreamerOAuth {
            connected: snap.connected,
            needs_reauth: snap.needs_reauth,
            status: snap.status.to_string(),
            granted_scopes: snap.granted_scopes,
            missing_scopes: snap.missing_scopes,
            authorized_at: row.authorized_at.map(fmt_dt),
            raid_enabled: row.oauth_raid_enabled.unwrap_or(false),
        },
    }))
}

// ── Mutationen (B11-PR-4) ──────────────────────────────────────────────────────

/// Liest einen optionalen String aus einem JSON-Body. Leerer/kein Body und
/// fehlender Key → `None`. Whitespace wird getrimmt; nur-Whitespace → `None`.
fn body_str(body: &[u8], key: &str) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Liest einen Bool aus dem JSON-Body (akzeptiert `true/false`, `1/0`,
/// `"true"/"false"/"on"/"off"`). Fehlt der Key → `default`.
fn body_bool(body: &[u8], key: &str, default: bool) -> bool {
    let Some(value) = serde_json::from_slice::<Value>(body)
        .ok()
        .as_ref()
        .and_then(|v| v.get(key).cloned())
    else {
        return default;
    };
    match value {
        Value::Bool(b) => b,
        Value::Number(n) => n.as_i64().map(|i| i != 0).unwrap_or(default),
        Value::String(s) => matches!(
            s.trim().to_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        _ => default,
    }
}

/// `POST /twitch/api/admin/streamers/{login}/verify`
///
/// Body: `{ "mode": "permanent" | "temp" | "clear" | "failed" }` (Default
/// `permanent`). Parität Python `_dashboard_verify`:
/// - `permanent`/`temp`: bestätigt einen aktiven Partner ohne eigene Verify-Spalten.
/// - `clear`/`failed`: departnert den aktiven Partner via [`departner_streamer`]
///   (Status→`departnered`, Raid-Auth-Disable, Identity-Upsert,
///   Engagement-Disable). Beide Modi sind im Python-Orakel DB-identisch
///   (`departner_active_partner(clear_verification=True)`); der Unterschied war
///   nur die Antwort-Meldung bzw. eine Fehler-DM (per B10-Direktive gedroppt).
///   Das **Discord-Rollen-Removal** läuft in Prod über den Master-Broker, den
///   `tb-dashboard-api` nicht hat — bewusster Handoff (siehe `departner_streamer`).
/// - unbekannte Modi → 200 `unknown_mode` (ohne Mutation).
pub async fn verify_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let mode = body_str(&body, "mode").unwrap_or_else(|| "permanent".to_string());

    // Departner-Modi (clear/failed) laufen über die native Departnerung — nicht
    // über verify_streamer. Beide Modi sind DB-identisch (Python-Parität).
    if matches!(mode.trim().to_lowercase().as_str(), "clear" | "failed") {
        let outcome = departner_streamer(&pool, &login).await.map_err(|e| {
            tracing::error!("departner_streamer Fehler für {login}: {e}");
            ApiError::internal()
        })?;
        return match outcome {
            // Kein aktiver Partner — Python: "{login} ist nicht gespeichert".
            None => Err(ApiError::not_found()),
            Some(_) => Ok(Json(json!({
                "ok": true,
                "login": login,
                "mode": mode,
                "status": "departnered",
            }))),
        };
    }

    match verify_streamer(&pool, &login, &mode).await.map_err(|e| {
        tracing::error!("verify_streamer Fehler für {login}: {e}");
        ApiError::internal()
    })? {
        VerifyStreamerResult::Verified => Ok(Json(
            json!({ "ok": true, "login": login, "mode": mode, "status": "verified" }),
        )),
        VerifyStreamerResult::NotAPartner => Err(ApiError::not_found()),
        // clear/failed werden oben abgefangen; dieser Marker erreicht den Handler
        // hier nie. Defensiv auf den Departner-Pfad mappen (kein toter no-op).
        VerifyStreamerResult::RequiresPartnerLifecycle => Err(ApiError::internal()),
        // Python antwortet bei unbekanntem Modus 200 ohne Mutation.
        VerifyStreamerResult::UnknownMode => Ok(Json(
            json!({ "ok": false, "login": login, "mode": mode, "status": "unknown_mode" }),
        )),
    }
}

/// Gemeinsamer Pfad für Archive/Block-Mutationen (beide laufen über
/// [`archive_streamer`], unterscheiden sich nur im Default-Modus).
async fn run_archive(
    pool: &PgPool,
    login: &str,
    mode: ArchiveMode,
) -> Result<impl IntoResponse, ApiError> {
    let changed = archive_streamer(pool, login, mode).await.map_err(|e| {
        tracing::error!("archive_streamer Fehler für {login}: {e}");
        ApiError::internal()
    })?;
    if changed {
        Ok(Json(json!({ "ok": true, "login": login })))
    } else {
        Err(ApiError::not_found())
    }
}

/// `POST /twitch/api/admin/streamers/{login}/archive`
///
/// Body: `{ "mode": "archive"|"unarchive"|"toggle"|… }` (Default `toggle`).
/// Parität Python `_dashboard_archive`: unbekannte Modi fallen auf Toggle
/// (kein 400). 404, wenn kein Partner-Eintrag betroffen war.
pub async fn archive_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let mode = body_str(&body, "mode")
        .map(|m| ArchiveMode::parse(&m))
        .unwrap_or(ArchiveMode::Toggle);
    run_archive(&pool, &login, mode).await
}

/// `POST /twitch/api/admin/streamers/{login}/block`
///
/// Body: `{ "mode": "block"|"unblock"|"toggle" }` (Default `toggle` → Block-
/// Toggle). Parität Python `_dashboard_archive` Block-Pfad: setzt/löst
/// `technical_pause_reason='blocked'`. 404, wenn kein Eintrag betroffen war.
pub async fn block_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let mode = match body_str(&body, "mode").as_deref() {
        Some("block") | Some("blocked") | Some("ban") => ArchiveMode::Block,
        Some("unblock") | Some("allow") => ArchiveMode::Unblock,
        // Default + alles andere: Block-Toggle (nicht der Archive-Toggle!).
        _ => ArchiveMode::ToggleBlock,
    };
    run_archive(&pool, &login, mode).await
}

/// `POST /twitch/api/admin/streamers/{login}/discord-flag`
///
/// Body: `{ "is_on_discord": bool }` (Default `true`). Parität Python
/// `_dashboard_set_discord_flag`: setzt das `is_on_discord`-Flag (Partner- oder
/// Non-Partner-Pfad). 404, wenn der Login unbekannt ist.
pub async fn discord_flag_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let is_on_discord = body_bool(&body, "is_on_discord", true);

    let changed = set_discord_flag(&pool, &login, is_on_discord)
        .await
        .map_err(|e| {
            tracing::error!("set_discord_flag Fehler für {login}: {e}");
            ApiError::internal()
        })?;
    if changed {
        Ok(Json(
            json!({ "ok": true, "login": login, "isOnDiscord": is_on_discord }),
        ))
    } else {
        Err(ApiError::not_found())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME};
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    /// Gibt die DSN zurück oder bricht den Test ab.
    /// Mit `TB_TEST_REQUIRE_DB=1` wird statt des stillen Skips ein panic ausgelöst.
    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
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

        // Gleiche DDL wie in tb-analytics-Tests (kopiert, damit Handler-Tests standalone laufen)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners_all_state (
                id BIGSERIAL PRIMARY KEY, twitch_login TEXT NOT NULL, twitch_user_id TEXT,
                discord_user_id TEXT, discord_display_name TEXT, created_at TEXT,
                archived_at TEXT, require_discord_link INTEGER NOT NULL DEFAULT 0,
                is_on_discord INTEGER NOT NULL DEFAULT 0, manual_partner_opt_out INTEGER NOT NULL DEFAULT 0,
                status TEXT, raid_bot_enabled INTEGER NOT NULL DEFAULT 1, silent_ban INTEGER NOT NULL DEFAULT 0,
                silent_raid INTEGER NOT NULL DEFAULT 0, is_monitored_only INTEGER NOT NULL DEFAULT 0,
                is_verified INTEGER NOT NULL DEFAULT 0, is_partner_active INTEGER NOT NULL DEFAULT 1,
                live_ping_enabled INTEGER NOT NULL DEFAULT 1,
                technical_pause_reason TEXT, operational_state TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login TEXT PRIMARY KEY, twitch_user_id TEXT,
                is_live INTEGER NOT NULL DEFAULT 0, last_seen_at TEXT,
                last_started_at TEXT, last_viewer_count INTEGER,
                active_session_id BIGINT, last_game TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_auth (
                id BIGSERIAL PRIMARY KEY, twitch_login TEXT, twitch_user_id TEXT,
                scopes TEXT, needs_reauth BOOLEAN NOT NULL DEFAULT FALSE,
                raid_enabled BOOLEAN NOT NULL DEFAULT TRUE, authorized_at TIMESTAMPTZ
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_billing_subscriptions (
                id BIGSERIAL PRIMARY KEY, customer_reference TEXT NOT NULL,
                plan_id TEXT, status TEXT, updated_at TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS streamer_plans (
                twitch_user_id TEXT, twitch_login TEXT, plan_name TEXT,
                promo_disabled INTEGER NOT NULL DEFAULT 0, promo_message TEXT,
                raid_boost_enabled INTEGER NOT NULL DEFAULT 0, notes TEXT,
                manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id TEXT PRIMARY KEY,
                session_type TEXT NOT NULL DEFAULT 'twitch',
                payload_enc BYTEA NOT NULL,
                created_at DOUBLE PRECISION NOT NULL,
                expires_at DOUBLE PRECISION NOT NULL
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL dashboard_sessions");

        sqlx::query("DROP TABLE IF EXISTS twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("DROP stream_sessions");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL,
                started_at TIMESTAMPTZ NOT NULL, ended_at TIMESTAMPTZ,
                stream_title TEXT, game_name TEXT, avg_viewers DOUBLE PRECISION,
                peak_viewers INTEGER, duration_seconds INTEGER, follower_delta INTEGER
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        // twitch_partners — Basistabelle: list_streamers/streamer_detail prüfen per
        // NOT EXISTS, ob ein Streamer hier steht (is_monitored_only). Volle Spalten,
        // damit die Mutations-Tests (make_write_pool: verify/archive) sie mitnutzen.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id SERIAL PRIMARY KEY, twitch_login TEXT NOT NULL, twitch_user_id TEXT,
                status TEXT DEFAULT 'active', admin_archived_at TEXT,
                departnered_at TEXT,
                technical_pause_reason TEXT, manual_partner_opt_out INTEGER DEFAULT 0,
                raid_bot_enabled INTEGER DEFAULT 1
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_partners");
        sqlx::query(
            "TRUNCATE twitch_partners_all_state, twitch_partners, twitch_live_state, twitch_raid_auth, \
             twitch_billing_subscriptions, streamer_plans, twitch_stream_sessions",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");
        pool
    }

    fn make_list_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/streamers", get(list_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn make_detail_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/streamers/:login", get(detail_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn addr() -> SocketAddr {
        "1.2.3.4:9999".parse().unwrap()
    }

    // ── List-Tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_list_unauth").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=all")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_returns_400_bei_invalidem_view() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_list_400").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=ungueltig")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["error"], "invalid_view");
        assert!(v["supported"].is_array());
    }

    #[tokio::test]
    async fn list_returns_200_mit_leerer_liste() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_list_200_leer").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=all")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["count"], 0);
        assert!(v["items"].as_array().unwrap().is_empty());
        assert_eq!(v["view"], "all");
    }

    #[tokio::test]
    async fn list_accepts_discord_admin_session_cookie() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_list_discord_admin_cookie").await;
        let auth_state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let session = auth_state
            .create_admin_session("discord-admin-1", "Discord Admin")
            .await
            .expect("admin session");
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=all")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header(
                axum::http::header::COOKIE,
                format!("{ADMIN_COOKIE_NAME}={}", session.session_id),
            )
            .body(Body::empty())
            .unwrap();
        let res = crate::build_admin_streamers_router(pool, "tok".into())
            .layer(Extension(auth_state))
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["count"], 0);
        assert_eq!(v["view"], "all");
    }

    #[tokio::test]
    async fn list_default_view_ist_active() {
        // P2.80: ohne ?view= ist der Default 'active' (nicht 'all'); nur aktive
        // Partner erscheinen, archivierte/departnered nicht.
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_list_default_active").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, archived_at) VALUES \
             ('aktiv1', 'active', NULL), \
             ('archiviert1', 'archived', '2024-01-01T00:00:00Z'), \
             ('departnered1', 'departnered', NULL)",
        )
        .execute(&pool)
        .await
        .expect("insert");
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers") // KEIN view-Param
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["view"], "active");
        let logins: Vec<&str> = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["login"].as_str().unwrap())
            .collect();
        assert_eq!(logins, vec!["aktiv1"]);
    }

    #[tokio::test]
    async fn list_archived_leerer_string_ist_false() {
        // P3.21: archived_at='' (leerer TEXT) → archived=false, konsistent mit Detail.
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_list_archived_empty").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, archived_at) \
             VALUES ('leerarchiv', 'active', '')",
        )
        .execute(&pool)
        .await
        .expect("insert");
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=all")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        let item = &v["items"][0];
        assert_eq!(item["login"], "leerarchiv");
        assert_eq!(item["archived"], false);
    }

    #[tokio::test]
    async fn list_dekodiert_bool_needs_reauth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_list_bool_reauth").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, status, created_at) \
             VALUES ('boolreauth', '42', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert partner");
        sqlx::query(
            "INSERT INTO twitch_raid_auth \
             (twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at) \
             VALUES ('boolreauth', '42', 'bits:read', TRUE, FALSE, '2026-06-29T12:10:00+00')",
        )
        .execute(&pool)
        .await
        .expect("insert auth");
        sqlx::query(
            "INSERT INTO streamer_plans \
             (twitch_login, manual_plan_id, manual_plan_expires_at, manual_plan_notes) \
             VALUES ('boolreauth', 'manual-list', '2026-07-01T12:10:00+00', 'list fixture')",
        )
        .execute(&pool)
        .await
        .expect("insert plan");

        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=active")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["items"][0]["login"], "boolreauth");
        assert_eq!(v["items"][0]["oauthNeedsReauth"], true);
        assert_eq!(v["items"][0]["oauthStatus"], "reauth");
        assert_eq!(v["items"][0]["oauthAuthorizedAt"], "2026-06-29T12:10:00+00");
        assert_eq!(v["items"][0]["planId"], "manual-list");
    }

    // ── Detail-Tests ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detail_returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_detail_unauth").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/teststreamer")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn detail_returns_404_fuer_unbekannten_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_detail_404").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/gibts_nicht")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn detail_returns_200_fuer_bekannten_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_detail_200").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, created_at) \
             VALUES ('bekannter', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert");
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/Bekannter") // case-insensitive
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["login"], "bekannter");
        // Top-Level-Felder (Python-Parität _admin_streamer_detail_payload):
        assert_eq!(v["displayName"], "bekannter"); // kein discord_display_name → Login
        assert_eq!(v["verified"], false);
        assert_eq!(v["archived"], false);
        assert_eq!(v["isLive"], false);
        assert!(v["planId"].is_null()); // kein Plan gesetzt
        // created_at = NOW()
        assert!(!v["createdAt"].is_null());
        // Stats: totalWatchHours statt totalDurationSeconds; Live-State-Felder vorhanden.
        assert_eq!(v["stats"]["totalWatchHours"], 0.0);
        assert!(v["stats"].get("totalDurationSeconds").is_none());
        assert_eq!(v["stats"]["viewerCount"], 0);
        assert!(v["stats"].as_object().unwrap().contains_key("lastSeenAt"));
        assert!(v["stats"].as_object().unwrap().contains_key("lastGame"));
        assert!(v["sessions"].is_array());
        assert!(v["settings"].is_object());
        assert!(v["oauth"].is_object());
    }

    #[tokio::test]
    async fn detail_session_keys_und_settings_opt_out() {
        // P1.34: sessions[] trägt sessionId/title/category/averageViewers/watchTimeHours
        // (zusätzlich zu id/streamTitle/...). P3.20: settings.manualPartnerOptOut.
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_detail_session_keys").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state \
             (twitch_login, status, created_at, manual_partner_opt_out) \
             VALUES ('keystreamer', 'active', NOW()::TEXT, 1)",
        )
        .execute(&pool)
        .await
        .expect("insert partner");
        sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (streamer_login, started_at, ended_at, stream_title, game_name, avg_viewers, \
              peak_viewers, duration_seconds, follower_delta) \
             VALUES ('keystreamer', NOW(), NOW(), 'Mein Titel', 'Deadlock', 123.5, 400, 7200, 12)",
        )
        .execute(&pool)
        .await
        .expect("insert session");
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/keystreamer")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 16384).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        let s = &v["sessions"][0];
        // P1.34 Frontend-Keys vorhanden + korrekt.
        assert_eq!(s["sessionId"], s["id"]);
        assert_eq!(s["title"], "Mein Titel");
        assert_eq!(s["category"], "Deadlock");
        assert_eq!(s["averageViewers"], 123.5);
        assert_eq!(s["peakViewers"], 400);
        assert_eq!(s["durationSeconds"], 7200);
        assert_eq!(s["followerDelta"], 12);
        // 7200s = 2.0h
        assert_eq!(s["watchTimeHours"], 2.0);
        // Bestehende Keys bleiben (additiv).
        assert_eq!(s["streamTitle"], "Mein Titel");
        assert_eq!(s["gameName"], "Deadlock");
        // P3.20: settings.manualPartnerOptOut.
        assert_eq!(v["settings"]["manualPartnerOptOut"], true);
    }

    #[tokio::test]
    async fn detail_dekodiert_bool_oauth_flags() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_admin_h_detail_bool_oauth").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, twitch_user_id, status, created_at) \
             VALUES ('detailbool', '43', 'active', NOW()::TEXT)",
        )
        .execute(&pool)
        .await
        .expect("insert partner");
        sqlx::query(
            "INSERT INTO twitch_raid_auth \
             (twitch_login, twitch_user_id, scopes, needs_reauth, raid_enabled, authorized_at) \
             VALUES ('detailbool', '43', 'bits:read', TRUE, FALSE, '2026-06-29T13:10:00+00')",
        )
        .execute(&pool)
        .await
        .expect("insert auth");
        sqlx::query(
            "INSERT INTO streamer_plans \
             (twitch_login, manual_plan_id, manual_plan_expires_at, manual_plan_notes) \
             VALUES ('detailbool', 'manual-detail', '2026-07-02T13:10:00+00', 'detail fixture')",
        )
        .execute(&pool)
        .await
        .expect("insert plan");

        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/detailbool")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 16384).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["oauth"]["needsReauth"], true);
        assert_eq!(v["oauth"]["status"], "reauth");
        assert_eq!(v["oauth"]["raidEnabled"], false);
        assert_eq!(v["oauth"]["authorizedAt"], "2026-06-29T13:10:00+00");
        assert_eq!(
            v["settings"]["manualPlanExpiresAt"],
            "2026-07-02T13:10:00+00"
        );
    }

    // ── Mutations-Tests (B11-PR-4) ──────────────────────────────────────────

    /// Pool mit den Write-Seiten-Tabellen (verify/archive nutzen
    /// `twitch_partners`, discord-flag `twitch_streamers`/-`_identities`).
    async fn make_write_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = make_pool(dsn, schema).await;
        // twitch_partners kommt jetzt aus make_pool (volle Definition dort).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamers (
                twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT,
                created_at TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT NOT NULL,
                discord_user_id TEXT, discord_display_name TEXT, is_on_discord INTEGER DEFAULT 0,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP, updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamer_identities");
        // make_pool legt twitch_raid_auth bereits prod-treu mit BOOLEAN-Flags an.
        // Fuer die Write-Tests wird sie mit der schmaleren Mutations-Fixture neu
        // angelegt, plus engagement_settings.
        sqlx::query("DROP TABLE IF EXISTS twitch_raid_auth")
            .execute(&pool)
            .await
            .expect("drop twitch_raid_auth");
        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
                raid_enabled BOOLEAN DEFAULT TRUE, needs_reauth BOOLEAN DEFAULT FALSE
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_raid_auth (bool)");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_engagement_settings (
                channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT FALSE
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_engagement_settings");
        pool
    }

    async fn status_of(r: Result<impl IntoResponse, ApiError>) -> StatusCode {
        r.into_response().status()
    }

    async fn json_of(r: Result<impl IntoResponse, ApiError>) -> serde_json::Value {
        let body = r.into_response().into_body();
        let b = axum::body::to_bytes(body, 8192).await.unwrap();
        serde_json::from_slice(&b).unwrap()
    }

    #[tokio::test]
    async fn verify_unauth_ist_401() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_verify_unauth").await;
        let r = verify_handler(
            AuthLevel::None,
            State(pool),
            Path("x".into()),
            axum::body::Bytes::new(),
        )
        .await;
        assert_eq!(status_of(r).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn verify_permanent_bestaetigt_aktiven_partner_und_200() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_verify_ok").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_login, status) VALUES ('vp', 'active')")
            .execute(&pool)
            .await
            .unwrap();
        let r = verify_handler(
            AuthLevel::Admin,
            State(pool.clone()),
            Path("vp".into()),
            axum::body::Bytes::from_static(br#"{"mode":"permanent"}"#),
        )
        .await;
        let v = json_of(r).await;
        assert_eq!(v["status"], "verified");
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM twitch_partners WHERE twitch_login='vp'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.as_deref(), Some("active"));
    }

    #[tokio::test]
    async fn verify_unbekannter_login_ist_404() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_verify_404").await;
        let r = verify_handler(
            AuthLevel::Admin,
            State(pool),
            Path("niemand".into()),
            axum::body::Bytes::from_static(br#"{"mode":"permanent"}"#),
        )
        .await;
        assert_eq!(status_of(r).await, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn verify_clear_departnert_aktiven_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_verify_clear").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ('lc', '42', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled) VALUES ('42', 'lc', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let r = verify_handler(
            AuthLevel::Admin,
            State(pool.clone()),
            Path("lc".into()),
            axum::body::Bytes::from_static(br#"{"mode":"clear"}"#),
        )
        .await;
        let v = json_of(r).await;
        assert_eq!(v["status"], "departnered");
        assert_eq!(v["ok"], true);

        // Echte Departnerung: Status + Raid-Auth disabled.
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM twitch_partners WHERE twitch_login='lc'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.as_deref(), Some("departnered"));
        let raid: Option<bool> = sqlx::query_scalar(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id='42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(raid, Some(false), "raid-auth disabled");
    }

    #[tokio::test]
    async fn verify_failed_departnert_wie_clear() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_verify_failed").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status) VALUES ('lf', '7', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let r = verify_handler(
            AuthLevel::Admin,
            State(pool.clone()),
            Path("lf".into()),
            axum::body::Bytes::from_static(br#"{"mode":"failed"}"#),
        )
        .await;
        let v = json_of(r).await;
        assert_eq!(v["status"], "departnered", "failed = DB-identisch zu clear");
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM twitch_partners WHERE twitch_login='lf'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.as_deref(), Some("departnered"));
    }

    #[tokio::test]
    async fn verify_clear_ohne_aktiven_partner_ist_404() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_verify_clear_404").await;
        let r = verify_handler(
            AuthLevel::Admin,
            State(pool),
            Path("gibtsnicht".into()),
            axum::body::Bytes::from_static(br#"{"mode":"clear"}"#),
        )
        .await;
        assert_eq!(status_of(r).await, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn archive_toggle_archiviert_und_200() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_archive").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_login, status) VALUES ('arc', 'active')")
            .execute(&pool)
            .await
            .unwrap();
        let r = archive_handler(
            AuthLevel::Admin,
            State(pool.clone()),
            Path("arc".into()),
            axum::body::Bytes::from_static(br#"{}"#), // Default = toggle
        )
        .await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        let archived: Option<String> = sqlx::query_scalar(
            "SELECT admin_archived_at FROM twitch_partners WHERE twitch_login='arc'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(archived.is_some());
    }

    #[tokio::test]
    async fn archive_unbekannt_ist_404() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_archive_404").await;
        let r = archive_handler(
            AuthLevel::Admin,
            State(pool),
            Path("weg".into()),
            axum::body::Bytes::from_static(br#"{"mode":"archive"}"#),
        )
        .await;
        assert_eq!(status_of(r).await, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn block_setzt_technical_pause_reason() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_block").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_login, status) VALUES ('blk', 'active')")
            .execute(&pool)
            .await
            .unwrap();
        let r = block_handler(
            AuthLevel::Admin,
            State(pool.clone()),
            Path("blk".into()),
            axum::body::Bytes::from_static(br#"{"mode":"block"}"#),
        )
        .await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        let reason: (Option<String>,) = sqlx::query_as(
            "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_login='blk'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reason.0.as_deref(), Some("blocked"));
    }

    #[tokio::test]
    async fn discord_flag_setzt_is_on_discord() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_discord_flag").await;
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('df', 'uid_df')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let r = discord_flag_handler(
            AuthLevel::Admin,
            State(pool.clone()),
            Path("df".into()),
            axum::body::Bytes::from_static(br#"{"is_on_discord":true}"#),
        )
        .await;
        let v = json_of(r).await;
        assert_eq!(v["isOnDiscord"], true);
        let flag: (Option<i32>,) = sqlx::query_as(
            "SELECT is_on_discord FROM twitch_streamer_identities WHERE twitch_user_id='uid_df'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(flag.0, Some(1));
    }

    #[tokio::test]
    async fn discord_flag_unbekannt_ist_404() {
        let dsn = db_dsn_or_skip!();
        let pool = make_write_pool(&dsn, "test_admin_h_discord_flag_404").await;
        let r = discord_flag_handler(
            AuthLevel::Admin,
            State(pool),
            Path("nichtda".into()),
            axum::body::Bytes::from_static(br#"{"is_on_discord":true}"#),
        )
        .await;
        assert_eq!(status_of(r).await, StatusCode::NOT_FOUND);
    }

    #[test]
    fn body_bool_und_body_str_parsen_robust() {
        assert!(body_bool(br#"{"x":true}"#, "x", false));
        assert!(body_bool(br#"{"x":"on"}"#, "x", false));
        assert!(!body_bool(br#"{"x":0}"#, "x", true));
        assert!(body_bool(br#"{}"#, "x", true)); // fehlt → default
        assert_eq!(
            body_str(br#"{"mode":" temp "}"#, "mode").as_deref(),
            Some("temp")
        );
        assert!(body_str(br#"{"mode":"  "}"#, "mode").is_none()); // nur-Whitespace
        assert!(body_str(br#"not json"#, "mode").is_none());
    }
}
