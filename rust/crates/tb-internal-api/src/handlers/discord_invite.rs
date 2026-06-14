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

/// Eine Zeile aus `twitch_streamer_invites` (für den Deadlock-Bot-Sync).
#[derive(Serialize)]
pub struct StreamerInviteEntry {
    pub streamer_login: String,
    pub guild_id: i64,
    pub invite_code: String,
    pub invite_url: String,
    pub created_at: Option<String>,
    pub last_sent_at: Option<String>,
}

/// `GET /internal/twitch/v1/streamer-invites`
///
/// Listet ALLE Streamer→Discord-Invite-Zuordnungen. Der Deadlock-Bot spiegelt
/// das in seine sqlite, damit die Join-Quellen-Klassifikation Streamer-Invites
/// erkennt (die Zuordnung liegt sonst nur in dieser Postgres-DB).
pub async fn list_all_handler(State(pool): State<PgPool>) -> Result<impl IntoResponse, ApiError> {
    let rows: Vec<(String, i64, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT streamer_login, guild_id, invite_code, invite_url, created_at, last_sent_at
         FROM twitch_streamer_invites ORDER BY streamer_login",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("streamer-invites list DB-Fehler: {e}");
        ApiError::internal()
    })?;

    let out: Vec<StreamerInviteEntry> = rows
        .into_iter()
        .map(
            |(streamer_login, guild_id, invite_code, invite_url, created_at, last_sent_at)| {
                StreamerInviteEntry {
                    streamer_login,
                    guild_id,
                    invite_code,
                    invite_url,
                    created_at,
                    last_sent_at,
                }
            },
        )
        .collect();
    Ok(Json(out))
}
