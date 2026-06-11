//! GET /internal/twitch/v1/streamer/:login/discord-invite

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use sqlx::PgPool;
use tb_http_core::ApiError;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordInviteResponse {
    pub login: String,
    pub invite_url: String,
}

/// `GET /internal/twitch/v1/streamer/:login/discord-invite`
///
/// Gibt die Discord-Invite-URL zurück, die für den Streamer generiert wurde.
/// 404 wenn kein Invite-Eintrag in `twitch_streamer_invites` vorhanden.
pub async fn handler(
    State(pool): State<PgPool>,
    Path(login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let login_lower = login.to_lowercase();

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT invite_url FROM twitch_streamer_invites WHERE LOWER(streamer_login) = $1",
    )
    .bind(&login_lower)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("discord_invite DB-Fehler für {login_lower}: {e}");
        ApiError::internal()
    })?;

    match row {
        Some((invite_url,)) => Ok(Json(DiscordInviteResponse {
            login: login_lower,
            invite_url,
        })),
        None => Err(ApiError::not_found()),
    }
}
