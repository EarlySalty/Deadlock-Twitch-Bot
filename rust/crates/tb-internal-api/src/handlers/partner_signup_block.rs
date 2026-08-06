//! Handler für den Signup-Block (`twitch_partner_signup_denylist`).
//!
//! Vertrag:
//! - `POST /partner/signup-block/add`    → `{ok, login, twitch_user_id, reason, inserted,
//!                                           raid_blacklisted, credentials_deleted,
//!                                           active_partner_paused}`
//! - `POST /partner/signup-block/remove` → `{ok, login, removed, raid_entries_removed,
//!                                           partner_pause_cleared}`
//! - `GET  /partner/signup-block/check`  → `{ok, login, blocked[, twitch_user_id, reason,
//!                                           public_message, added_by, added_at]}`
//! - `GET  /partner/signup-block`        → `{ok, entries: [...]}`
//!
//! `add` braucht eine stabile `twitch_user_id`. Steht sie nicht im Request, wird
//! sie aus dem Bestand aufgeloest ([`tb_analytics::partner_signup_block::resolve_user_id`]);
//! scheitert das, gibt es 400 statt eines Eintrags, der nur am Login haengt —
//! eine Umbenennung wuerde den Block sonst aushebeln.
//!
//! `reason` ist interner Klartext und wird nie an den Streamer ausgeliefert.
//! `public_message` ueberschreibt den Standard-Absagetext aus
//! [`tb_domain::SIGNUP_BLOCK_BODY`].

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::partner_signup_block as db;
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};

use super::common::pick_first_truthy;

/// Fallback-Grund, wenn der Aufrufer keinen mitgibt. Bewusst sprechend, weil er
/// als `signup_block:<reason>` auch in der Raid-Blacklist landet.
const DEFAULT_REASON: &str = "owner_decision";

// ── Request-Typen ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddRequest {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
    #[serde(default)]
    pub twitch_user_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub public_message: Option<String>,
    #[serde(default)]
    pub added_by: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
    #[serde(default)]
    pub twitch_user_id: Option<String>,
}

#[derive(Deserialize)]
pub struct CheckQuery {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub twitch_user_id: Option<String>,
}

// ── Response-Typen ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AddResponse {
    pub ok: bool,
    pub login: String,
    pub twitch_user_id: String,
    pub reason: String,
    pub inserted: bool,
    pub raid_blacklisted: bool,
    pub credentials_deleted: bool,
    pub active_partner_paused: bool,
}

#[derive(Serialize)]
pub struct RemoveResponse {
    pub ok: bool,
    pub login: String,
    pub removed: bool,
    pub raid_entries_removed: u64,
    pub partner_pause_cleared: bool,
}

#[derive(Serialize)]
pub struct Entry {
    pub twitch_user_id: String,
    pub login: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_message: Option<String>,
    pub added_by: String,
    pub added_at: String,
}

impl From<db::SignupBlockEntry> for Entry {
    fn from(e: db::SignupBlockEntry) -> Self {
        Self {
            twitch_user_id: e.twitch_user_id,
            login: e.twitch_login,
            reason: e.reason,
            public_message: e.public_message,
            added_by: e.added_by,
            added_at: e.added_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub ok: bool,
    pub login: String,
    pub blocked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<Entry>,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub ok: bool,
    pub entries: Vec<Entry>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /internal/twitch/v1/partner/signup-block/add`
pub async fn add_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<AddRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let raw = pick_first_truthy(body.login, body.twitch_login);
    let Some(login) = normalize_twitch_login(&raw) else {
        return Err(ApiError::bad_request("invalid or missing login"));
    };

    let twitch_user_id = match body
        .twitch_user_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => id.to_string(),
        None => match db::resolve_user_id(&pool, &login).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                tracing::warn!(
                    %login,
                    "signup-block add abgelehnt: twitch_user_id unbekannt und nicht mitgeliefert"
                );
                return Err(ApiError::bad_request(
                    "twitch_user_id unbekannt — bitte explizit mitschicken",
                ));
            }
            Err(e) => {
                tracing::error!(%login, "signup-block resolve_user_id DB-Fehler: {e}");
                return Err(ApiError::internal());
            }
        },
    };

    let reason = body
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_REASON)
        .to_string();
    let added_by = body
        .added_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("internal_api")
        .to_string();

    let outcome = db::add(
        &pool,
        &twitch_user_id,
        &login,
        &reason,
        body.public_message.as_deref(),
        &added_by,
    )
    .await
    .map_err(|e| {
        tracing::error!(%login, "signup-block add DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(AddResponse {
        ok: true,
        login,
        twitch_user_id,
        reason,
        inserted: outcome.inserted,
        raid_blacklisted: outcome.raid_blacklisted,
        credentials_deleted: outcome.credentials_deleted,
        active_partner_paused: outcome.active_partner_paused,
    }))
}

/// `POST /internal/twitch/v1/partner/signup-block/remove`
pub async fn remove_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<RemoveRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let raw = pick_first_truthy(body.login, body.twitch_login);
    let user_id = body
        .twitch_user_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    // Ohne Login reicht die ID allein; sonst muss der Login gueltig sein.
    let login = match normalize_twitch_login(&raw) {
        Some(login) => login,
        None if user_id.is_some() => String::new(),
        None => return Err(ApiError::bad_request("invalid or missing login")),
    };

    let outcome = db::remove(&pool, user_id, &login).await.map_err(|e| {
        tracing::error!(%login, "signup-block remove DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(RemoveResponse {
        ok: true,
        login,
        removed: outcome.removed,
        raid_entries_removed: outcome.raid_entries_removed,
        partner_pause_cleared: outcome.partner_pause_cleared,
    }))
}

/// `GET /internal/twitch/v1/partner/signup-block/check?login=<login>`
pub async fn check_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<CheckQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let raw = params.login.unwrap_or_default();
    let user_id = params
        .twitch_user_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let login = match normalize_twitch_login(&raw) {
        Some(login) => login,
        None if user_id.is_some() => String::new(),
        None => return Err(ApiError::bad_request("invalid or missing login")),
    };

    let entry = db::check(&pool, user_id, &login).await.map_err(|e| {
        tracing::error!(%login, "signup-block check DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(CheckResponse {
        ok: true,
        login,
        blocked: entry.is_some(),
        entry: entry.map(Entry::from),
    }))
}

/// `GET /internal/twitch/v1/partner/signup-block`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let rows = db::list_entries(&pool).await.map_err(|e| {
        tracing::error!("signup-block list DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(ListResponse {
        ok: true,
        entries: rows.into_iter().map(Entry::from).collect(),
    }))
}
