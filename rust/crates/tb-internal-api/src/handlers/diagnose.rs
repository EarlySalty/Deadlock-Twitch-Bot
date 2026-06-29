//! Read-only Diagnose-Endpoint: Twitch-Auth/Scope-Status zu einer Discord-User-ID.
//!
//! `GET /internal/twitch/v1/diagnose?discord_id=...`
//!
//! Liefert einen sanitisierten Status für den Self-Service-Support (FAQ-/Ticket-Bot):
//! ist ein Twitch-Streamer-Account verknüpft, ist OAuth verbunden, welche Pflicht-
//! Scopes fehlen, ist eine Neu-Autorisierung nötig, Partner-/Live-Status und ob die
//! Discord-Verknüpfung steht. **Nur lesend, keine Tokens/Secrets in der Antwort.**
//!
//! Die Status-Logik wird 1:1 aus `tb_analytics::admin_streamers` wiederverwendet
//! (`scope_snapshot`, `partner_status`, `streamer_detail`) — Einstieg ist hier die
//! Discord-ID statt des Logins.

use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::admin_streamers;
use tb_http_core::{ApiError, AuthLevel};

#[derive(Deserialize)]
pub struct DiagnoseQuery {
    #[serde(default)]
    pub discord_id: Option<String>,
}

#[derive(Serialize)]
pub struct DiagnoseResponse {
    pub ok: bool,
    /// true, wenn zur Discord-ID ein Twitch-Streamer-Account mit Partner-State existiert
    pub found: bool,
    pub twitch_login: Option<String>,
    pub discord_linked: bool,
    pub oauth_connected: bool,
    pub needs_reauth: bool,
    /// "reauth" | "missing" | "partial" | "connected"
    pub oauth_status: String,
    pub missing_scopes: Vec<String>,
    pub granted_scope_count: usize,
    pub required_scope_count: usize,
    /// Zeitpunkt der letzten Autorisierung (kein Token, kein Ablaufdatum verfügbar).
    pub authorized_at: Option<String>,
    /// "active" | "departnered" | "archived" | "blocked" | "token_error" | "non_partner"
    pub partner_status: String,
    pub is_partner_active: bool,
    pub is_verified: bool,
    pub is_monitored_only: bool,
    pub is_live: bool,
    pub raid_bot_enabled: bool,
    pub technical_pause_reason: Option<String>,
    pub operational_state: Option<String>,
}

/// Antwort, wenn kein verknüpfter/auswertbarer Streamer-Account existiert.
/// `twitch_login` kann gesetzt sein (verknüpft, aber kein Partner-State).
fn empty_response(twitch_login: Option<String>) -> DiagnoseResponse {
    DiagnoseResponse {
        ok: true,
        found: false,
        twitch_login,
        discord_linked: false,
        oauth_connected: false,
        needs_reauth: false,
        oauth_status: "missing".to_string(),
        missing_scopes: admin_streamers::REQUIRED_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect(),
        granted_scope_count: 0,
        required_scope_count: admin_streamers::REQUIRED_SCOPES.len(),
        authorized_at: None,
        partner_status: "non_partner".to_string(),
        is_partner_active: false,
        is_verified: false,
        is_monitored_only: false,
        is_live: false,
        raid_bot_enabled: false,
        technical_pause_reason: None,
        operational_state: None,
    }
}

pub async fn handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(q): Query<DiagnoseQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let discord_id = q
        .discord_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("missing discord_id"))?;

    let login = admin_streamers::login_for_discord_user(&pool, &discord_id)
        .await
        .map_err(|e| {
            tracing::error!("diagnose: login lookup failed: {e}");
            ApiError::internal()
        })?;

    let Some(login) = login else {
        // Keine Twitch-Streamer-Verknüpfung zu dieser Discord-ID.
        return Ok(Json(empty_response(None)));
    };

    let detail = admin_streamers::streamer_detail(&pool, &login)
        .await
        .map_err(|e| {
            tracing::error!("diagnose: streamer_detail failed: {e}");
            ApiError::internal()
        })?;

    let Some(row) = detail else {
        // Verknüpft, aber kein Partner-State vorhanden.
        return Ok(Json(empty_response(Some(login))));
    };

    let snap =
        admin_streamers::scope_snapshot(row.scopes.as_deref(), row.needs_reauth.unwrap_or(false));
    let pstatus = admin_streamers::partner_status(
        row.status.as_deref(),
        row.archived_at.as_deref(),
        row.manual_partner_opt_out.unwrap_or(0),
        row.technical_pause_reason.as_deref(),
    );

    let resp = DiagnoseResponse {
        ok: true,
        found: true,
        twitch_login: Some(row.twitch_login.clone()),
        discord_linked: row.is_on_discord.unwrap_or(0) != 0,
        oauth_connected: snap.connected,
        needs_reauth: snap.needs_reauth,
        oauth_status: snap.status.to_string(),
        missing_scopes: snap.missing_scopes,
        granted_scope_count: snap.granted_scopes.len(),
        required_scope_count: admin_streamers::REQUIRED_SCOPES.len(),
        authorized_at: row.authorized_at.map(crate::security::datetime_to_iso),
        partner_status: pstatus.to_string(),
        is_partner_active: row.is_partner_active != 0,
        is_verified: row.is_verified != 0,
        is_monitored_only: row.is_monitored_only.unwrap_or(0) != 0,
        is_live: row.is_live != 0,
        raid_bot_enabled: row.raid_bot_enabled.unwrap_or(0) != 0,
        technical_pause_reason: row.technical_pause_reason.clone(),
        operational_state: row.operational_state.clone(),
    };
    Ok(Json(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_response_form_ohne_login() {
        let resp = empty_response(None);
        assert!(resp.ok);
        assert!(!resp.found);
        assert_eq!(resp.twitch_login, None);
        assert_eq!(resp.oauth_status, "missing");
        assert!(!resp.oauth_connected);
        assert!(!resp.needs_reauth);
        assert_eq!(resp.granted_scope_count, 0);
        assert_eq!(
            resp.required_scope_count,
            admin_streamers::REQUIRED_SCOPES.len()
        );
        // missing_scopes spiegelt vollständig die Pflicht-Scopes wider
        assert_eq!(
            resp.missing_scopes.len(),
            admin_streamers::REQUIRED_SCOPES.len()
        );
        assert_eq!(resp.partner_status, "non_partner");
    }

    #[test]
    fn empty_response_uebernimmt_login() {
        let resp = empty_response(Some("dragscope".to_string()));
        assert!(!resp.found);
        assert_eq!(resp.twitch_login.as_deref(), Some("dragscope"));
    }

    #[test]
    fn diagnose_response_serialisiert_zu_json() {
        let resp = empty_response(None);
        let v = serde_json::to_value(&resp).expect("serde-Serialisierung muss klappen");
        assert_eq!(v["ok"], true);
        assert_eq!(v["found"], false);
        assert_eq!(v["oauth_status"], "missing");
        assert_eq!(v["granted_scope_count"], 0);
        assert_eq!(
            v["required_scope_count"],
            admin_streamers::REQUIRED_SCOPES.len()
        );
        assert!(v["twitch_login"].is_null());
        assert!(v["missing_scopes"].is_array());
    }
}
