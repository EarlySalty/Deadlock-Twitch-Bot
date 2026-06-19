//! POST /internal/twitch/v1/chat/command
//!
//! Verarbeitet Chat-Befehle vom Python-Chat-Worker.
//! Gibt den fertigen Reply-Text zurück — Python sendet ihn via IRC.
//!
//! Aktuell unterstützt: `!invite`
//!   - Nur aktiv, wenn der Kanal Deadlock streamt (last_game ILIKE 'deadlock',
//!     is_live = 1).
//!   - Invite-URL: zuerst streamer-spezifisch aus `twitch_streamer_invites`,
//!     dann Env-Var `PROMO_DISCORD_INVITE` oder globaler Default.

use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_http_core::ApiError;

/// Env-Var-Name für den globalen Discord-Invite-Fallback.
const PROMO_DISCORD_INVITE_ENV: &str = "PROMO_DISCORD_INVITE";
/// Python-paritärer Default-Fallback für den globalen Discord-Invite.
const DEFAULT_PROMO_DISCORD_INVITE: &str = "https://discord.gg/z5TfVHuQq2";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRequest {
    /// Login des Kanals, in dem der Befehl getippt wurde (ohne #).
    pub channel_login: String,
    /// Login des Chatters, der den Befehl getippt hat.
    pub chatter_login: String,
    /// Roher Nachrichteninhalt (z. B. "!invite").
    pub content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResponse {
    /// Fertige Antwort zum Senden — `null` bedeutet: nicht antworten.
    pub reply: Option<String>,
}

/// `POST /internal/twitch/v1/chat/command`
pub async fn handler(
    State(pool): State<PgPool>,
    Json(body): Json<CommandRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let channel = body.channel_login.trim().to_lowercase();
    let chatter = body.chatter_login.trim().to_lowercase();
    let cmd = body.content.trim().to_lowercase();

    if channel.is_empty() || chatter.is_empty() || cmd.is_empty() {
        return Ok(Json(CommandResponse { reply: None }));
    }

    // Nur !invite ist aktuell implementiert.
    if cmd != "!invite" && !cmd.starts_with("!invite ") {
        return Ok(Json(CommandResponse { reply: None }));
    }

    // Kanal muss live Deadlock streamen.
    let live_row: Option<(i32, Option<String>)> = sqlx::query_as(
        "SELECT is_live, last_game FROM twitch_live_state WHERE LOWER(streamer_login) = $1 LIMIT 1",
    )
    .bind(&channel)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("chat_command live_state-Query fehlgeschlagen für {channel}: {e}");
        ApiError::internal()
    })?;

    let is_deadlock_live = live_row
        .map(|(is_live, last_game)| {
            is_live == 1
                && last_game
                    .as_deref()
                    .map(|g| g.to_lowercase().contains("deadlock"))
                    .unwrap_or(false)
        })
        .unwrap_or(false);

    if !is_deadlock_live {
        return Ok(Json(CommandResponse { reply: None }));
    }

    // Invite-URL: streamer-spezifisch oder globaler Fallback.
    let invite_url = get_invite_url(&pool, &channel).await?;
    let Some(invite_url) = invite_url else {
        return Ok(Json(CommandResponse { reply: None }));
    };

    let reply = format!(
        "@{chatter} Wenn du einen Zugang benötigst, schau gerne auf unserem Discord vorbei, \
         dort bekommst du eine Einladung und Hilfe beim Einstieg :) {invite_url}"
    );

    Ok(Json(CommandResponse { reply: Some(reply) }))
}

async fn get_invite_url(pool: &PgPool, channel_login: &str) -> Result<Option<String>, ApiError> {
    // 1. Streamer-spezifischer Invite (twitch_streamer_invites).
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT invite_url FROM twitch_streamer_invites WHERE LOWER(streamer_login) = $1 LIMIT 1",
    )
    .bind(channel_login)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("chat_command invite-Query fehlgeschlagen für {channel_login}: {e}");
        ApiError::internal()
    })?;

    if let Some((url,)) = row {
        if !url.trim().is_empty() {
            return Ok(Some(url));
        }
    }

    // 2. Globaler Fallback aus Env oder Python-paritärem Default.
    let configured = std::env::var(PROMO_DISCORD_INVITE_ENV).ok();
    let invite = configured
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PROMO_DISCORD_INVITE);
    Ok(Some(invite.to_string()))
}
