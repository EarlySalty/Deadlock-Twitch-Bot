//! POST /internal/twitch/v1/chat/command
//!
//! Verarbeitet Chat-Befehle vom Python-Chat-Worker.
//! Gibt den fertigen Reply-Text zurück — Python sendet ihn via IRC.
//!
//! Aktuell unterstützt: `!invite`
//!   - Unabhängig vom Streamstatus und der Kategorie verfügbar.
//!   - Invite-URL vom Heimat-Server des Kanals (Paket R): zuerst
//!     streamer-spezifisch aus `twitch_streamer_invites`, dann beim deutschen
//!     Server Konfiguration `discord.chat.promo_invite` oder die feste
//!     Community-Einladung, bei anderen Servern deren Server-Einladung.

use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_chat::{server_invite, HomeInviteResolver, InvitePurpose, StreamerKey};
use tb_http_core::ApiError;

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
    let key = StreamerKey::Login(channel_login);
    let lookup = HomeInviteResolver::new(pool.clone(), None);
    let Some(home) = lookup.home_guild(key).await else {
        return Ok(None);
    };

    // 1. Streamer-spezifischer Invite (twitch_streamer_invites), passend zum Heimat-Server.
    let url = lookup
        .fitting_personal_invite(&home, key)
        .await
        .map_err(|e| {
            tracing::error!("chat_command invite-Query fehlgeschlagen für {channel_login}: {e}");
            ApiError::internal()
        })?;
    if url.is_some() {
        return Ok(url);
    }

    // 2. Rückfall des Heimat-Servers. Die Konfiguration gilt nur für den
    //    deutschen Server und wird wie bisher erst hier gelesen.
    let community_override = if home.is_community() {
        let config = tb_config::runtime::settings().map_err(|_| ApiError::internal())?;
        config.discord.chat.promo_invite.clone()
    } else {
        None
    };
    Ok(server_invite(
        &home,
        InvitePurpose::Promo,
        community_override.as_deref(),
    ))
}

#[cfg(test)]
use crate::test_postgres;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echter_command_handler_antwortet_offline_ohne_live_tabelle() {
        let database = test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        sqlx::query("CREATE TABLE twitch_streamer_invites (streamer_login TEXT, invite_url TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamer_invites VALUES ('testchannel', 'https://discord.gg/test')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let response = handler(
            State(pool),
            Json(CommandRequest {
                channel_login: "testchannel".into(),
                chatter_login: "viewer".into(),
                content: "!invite".into(),
            }),
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["reply"]
            .as_str()
            .unwrap()
            .contains("https://discord.gg/test"));
    }
}
