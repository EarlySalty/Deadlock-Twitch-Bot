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

    let row = sqlx::query!(
        r#"SELECT invite_url AS "invite_url!"
           FROM twitch_streamer_invites
          WHERE LOWER(streamer_login) = $1"#,
        &login_lower
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("discord_invite DB-Fehler für {login_lower}: {e}");
        ApiError::internal()
    })?;

    match row {
        Some(row) => Ok(Json(DiscordInviteResponse {
            login: login_lower,
            invite_url: row.invite_url,
        })),
        None => Err(ApiError::not_found()),
    }
}

/// Eine Zeile aus `twitch_streamer_invites` (für den Deadlock-Bot-Sync).
#[derive(Serialize, sqlx::FromRow)]
pub struct StreamerInviteEntry {
    pub streamer_login: String,
    pub guild_id: i64,
    pub channel_id: Option<i64>,
    pub twitch_user_id: Option<String>,
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
    let rows: Vec<StreamerInviteEntry> = sqlx::query_as(
        "SELECT i.streamer_login, i.guild_id, i.channel_id,
                COALESCE(NULLIF(i.twitch_user_id, ''), p.twitch_user_id) AS twitch_user_id,
                i.invite_code, i.invite_url, i.created_at, i.last_sent_at
         FROM twitch_streamer_invites i
         LEFT JOIN LATERAL (
             SELECT twitch_user_id FROM twitch_streamers_partner_state
             WHERE LOWER(twitch_login) = LOWER(i.streamer_login)
                 AND COALESCE(is_partner_active, 0) <> 0
             ORDER BY twitch_user_id LIMIT 1
         ) p ON TRUE
         ORDER BY i.streamer_login",
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "Streamer-Einladungen konnten nicht exportiert werden");
        ApiError::internal()
    })?;
    Ok(Json(rows))
}
