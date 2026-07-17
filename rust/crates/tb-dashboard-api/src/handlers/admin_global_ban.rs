use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::global_ban as db;
use tb_domain::normalize_twitch_login;
use tb_http_core::ApiError;

#[derive(Deserialize)]
pub struct AddRequest {
    pub login: String,
    #[serde(default)]
    pub chatter_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    pub login: String,
}

#[derive(Deserialize)]
pub struct SetChannelRequest {
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct EntryResponse {
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub reason: Option<String>,
    pub added_by: Option<String>,
    pub added_at: Option<String>,
}

#[derive(Serialize)]
pub struct ChannelResponse {
    pub twitch_login: String,
    pub global_ban_enforcement_enabled: bool,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub entries: Vec<EntryResponse>,
    pub channels: Vec<ChannelResponse>,
}

fn require_login(login: &str) -> Result<String, ApiError> {
    normalize_twitch_login(login).ok_or_else(|| ApiError::bad_request("invalid login"))
}

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "admin_global_ban DB-Fehler");
    ApiError::internal()
}

pub async fn list_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let entries = db::list_bans(&pool)
        .await
        .map_err(db_error)?
        .into_iter()
        .map(|entry| EntryResponse {
            chatter_login: entry.chatter_login,
            chatter_id: entry.chatter_id,
            reason: entry.reason,
            added_by: entry.added_by,
            added_at: entry.added_at.map(|value| value.to_rfc3339()),
        })
        .collect();
    let channels = db::list_channel_enforcement(&pool)
        .await
        .map_err(db_error)?
        .into_iter()
        .map(|channel| ChannelResponse {
            twitch_login: channel.twitch_login,
            global_ban_enforcement_enabled: channel.global_ban_enforcement_enabled,
        })
        .collect();

    Ok(Json(ListResponse { entries, channels }))
}

pub async fn add_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<AddRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let login = require_login(&body.login)?;
    let chatter_id = body
        .chatter_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let reason = body
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual_ban:absolut");
    db::add_ban(
        &pool,
        &login,
        chatter_id,
        Some(reason),
        Some("admin_dashboard"),
    )
    .await
    .map_err(db_error)?;

    Ok(Json(serde_json::json!({ "ok": true, "login": login })))
}

pub async fn remove_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<RemoveRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let login = require_login(&body.login)?;
    let removed = db::remove_ban(&pool, &login).await.map_err(db_error)?;
    Ok(Json(
        serde_json::json!({ "ok": true, "login": login, "removed": removed }),
    ))
}

pub async fn set_channel_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    Json(body): Json<SetChannelRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let login = require_login(&login)?;
    let channel = db::set_channel_enforcement(&pool, &login, body.enabled)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::not_found)?;
    tracing::info!(
        channel = channel.twitch_login,
        urteil = if channel.global_ban_enforcement_enabled {
            "anwenden"
        } else {
            "übersprungen"
        },
        grund = "admin_dashboard_toggle",
        "GlobalBan-Enforcement-Einstellung geändert"
    );

    Ok(Json(ChannelResponse {
        twitch_login: channel.twitch_login,
        global_ban_enforcement_enabled: channel.global_ban_enforcement_enabled,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::StatusCode, response::IntoResponse};
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn list_handler_verlangt_admin_ohne_db_zugriff() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1/unused")
            .expect("lazy pool");

        let response = list_handler(DashboardAuthLevel::None, State(pool))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
