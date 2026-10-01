//! Viewer-only sessions: intentionally never accepted as dashboard/partner/admin auth.
use super::*;
use serde::{Deserialize, Serialize};

pub const PLAYER_COOKIE_NAME: &str = "twitch_player_session";
pub const PLAYER_SESSION_TTL: u64 = 3600;
const PLAYER_SESSION_TYPE: &str = "twitch_player";
const STEAM_FLOW_TYPE: &str = "twitch_player_steam_flow";

#[derive(Clone, Deserialize, Serialize)]
pub struct PlayerSession {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub csrf_token: String,
    pub expires_at: u64,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct PlayerSteamFlow {
    pub session_key: String,
    pub twitch_user_id: String,
    pub revision: i64,
    pub return_to: String,
    pub expires_at: u64,
}

impl DashboardAuthState {
    pub async fn create_player_session(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now();
        let session_id = tb_crypto::random_urlsafe_token(32);
        let csrf_token = tb_crypto::random_urlsafe_token(32);
        let payload = serde_json::json!({
            "twitch_user_id": twitch_user_id,
            "twitch_login": twitch_login,
            "csrf_token": csrf_token,
            "expires_at": now + PLAYER_SESSION_TTL,
        });
        self.persist_new_session(
            &session_id,
            PLAYER_SESSION_TYPE,
            &payload,
            now as f64,
            (now + PLAYER_SESSION_TTL) as f64,
        )
        .await?;
        Ok(SessionCreation {
            session_id,
            csrf_token,
        })
    }

    pub async fn load_player_session(
        &self,
        session_id: &str,
    ) -> Result<Option<PlayerSession>, sqlx::Error> {
        let Some(value) = self
            .fetch_session_payload(session_id, PLAYER_SESSION_TYPE, unix_now())
            .await?
        else {
            return Ok(None);
        };
        Ok(serde_json::from_value::<PlayerSession>(value)
            .ok()
            .filter(|s| {
                s.expires_at > unix_now()
                    && !s.twitch_user_id.is_empty()
                    && !s.csrf_token.is_empty()
            }))
    }

    pub async fn save_player_steam_flow(
        &self,
        token: &str,
        flow: &PlayerSteamFlow,
    ) -> Result<(), sqlx::Error> {
        let value = serde_json::to_value(flow).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        self.persist_new_session(
            token,
            STEAM_FLOW_TYPE,
            &value,
            unix_now() as f64,
            flow.expires_at as f64,
        )
        .await
    }

    pub async fn consume_player_steam_flow(
        &self,
        token: &str,
    ) -> Result<Option<PlayerSteamFlow>, sqlx::Error> {
        let Some(value) = self
            .consume_affiliate_state_payload(token, STEAM_FLOW_TYPE)
            .await?
        else {
            return Ok(None);
        };
        Ok(serde_json::from_value::<PlayerSteamFlow>(value)
            .ok()
            .filter(|f| f.expires_at > unix_now()))
    }
}
