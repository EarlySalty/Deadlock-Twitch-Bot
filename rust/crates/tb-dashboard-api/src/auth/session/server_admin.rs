//! Sessions des Server-Admin-Dashboards (Multi-Server Paket A).
//!
//! Eigener Session-Typ, der von keinem Partner-, Admin- oder CSRF-Gate des
//! bestehenden Dashboards akzeptiert wird. Ein Server-Admin bekommt dadurch nie
//! den globalen Admin-Zugang. Die Liste der verwalteten Guilds ist ein beim Login
//! von Discord geprüfter Snapshot (Recht „Server verwalten“); der Discord-Token
//! selbst wird nie gespeichert.
use super::*;
use serde::{Deserialize, Serialize};

pub const SERVER_ADMIN_COOKIE_NAME: &str = "tb_server_admin_session";
/// Kurz gehalten, weil die Guild-Rechte ein Login-Snapshot sind: wer „Server
/// verwalten“ verliert, verliert den Zugang spätestens nach dieser Zeit.
pub const SERVER_ADMIN_SESSION_TTL_SECS: u64 = 2 * 3600;
pub const SERVER_ADMIN_OAUTH_STATE_TTL_SECS: u64 = 600;
const SERVER_ADMIN_SESSION_TYPE: &str = "server_admin";
const SERVER_ADMIN_OAUTH_STATE_TYPE: &str = "oauth_state:server_admin";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerAdminSession {
    pub discord_user_id: String,
    pub display_name: String,
    /// Guilds, auf denen der Nutzer beim Login „Server verwalten“ hatte und auf
    /// denen der Bot installiert war.
    pub managed_guild_ids: Vec<i64>,
    pub csrf_token: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerAdminOAuthState {
    pub redirect_uri: String,
    pub expires_at: u64,
}

impl DashboardAuthState {
    pub async fn create_server_admin_session(
        &self,
        discord_user_id: u64,
        display_name: &str,
        managed_guild_ids: &[i64],
    ) -> Result<SessionCreation, sqlx::Error> {
        let now = unix_now();
        let session_id = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let csrf_token = tb_crypto::random_urlsafe_token(SESSION_ID_BYTES);
        let expires_at = now + SERVER_ADMIN_SESSION_TTL_SECS;
        let mut guilds: Vec<i64> = managed_guild_ids
            .iter()
            .copied()
            .filter(|id| *id > 0)
            .collect();
        guilds.sort_unstable();
        guilds.dedup();
        let session = ServerAdminSession {
            discord_user_id: discord_user_id.to_string(),
            display_name: display_name.to_string(),
            managed_guild_ids: guilds,
            csrf_token: csrf_token.clone(),
            expires_at,
        };
        let value = serde_json::to_value(&session).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        self.persist_new_session(
            &session_id,
            SERVER_ADMIN_SESSION_TYPE,
            &value,
            now as f64,
            expires_at as f64,
        )
        .await?;
        Ok(SessionCreation {
            session_id,
            csrf_token,
        })
    }

    pub async fn load_server_admin_session(
        &self,
        session_id: &str,
    ) -> Result<Option<ServerAdminSession>, sqlx::Error> {
        if !(20..=128).contains(&session_id.len()) {
            return Ok(None);
        }
        let Some(value) = self
            .fetch_session_payload(session_id, SERVER_ADMIN_SESSION_TYPE, unix_now())
            .await?
        else {
            return Ok(None);
        };
        Ok(serde_json::from_value::<ServerAdminSession>(value)
            .ok()
            .filter(|s| {
                s.expires_at > unix_now()
                    && s.discord_user_id.parse::<u64>().is_ok_and(|id| id > 0)
                    && !s.csrf_token.is_empty()
            }))
    }

    pub async fn save_server_admin_oauth_state(
        &self,
        state_token: &str,
        redirect_uri: &str,
    ) -> Result<(), sqlx::Error> {
        let now = unix_now();
        let state = ServerAdminOAuthState {
            redirect_uri: redirect_uri.to_string(),
            expires_at: now + SERVER_ADMIN_OAUTH_STATE_TTL_SECS,
        };
        let value = serde_json::to_value(&state).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        self.persist_new_session(
            state_token,
            SERVER_ADMIN_OAUTH_STATE_TYPE,
            &value,
            now as f64,
            state.expires_at as f64,
        )
        .await
    }

    /// Verbraucht den State atomar und einmalig (Replay-Schutz).
    pub async fn consume_server_admin_oauth_state(
        &self,
        state_token: &str,
    ) -> Result<Option<ServerAdminOAuthState>, sqlx::Error> {
        let Some(value) = self
            .consume_affiliate_state_payload(state_token, SERVER_ADMIN_OAUTH_STATE_TYPE)
            .await?
        else {
            return Ok(None);
        };
        Ok(serde_json::from_value::<ServerAdminOAuthState>(value)
            .ok()
            .filter(|s| s.expires_at > unix_now() && !s.redirect_uri.is_empty()))
    }
}
