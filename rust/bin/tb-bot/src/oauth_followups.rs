use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use tb_internal_api::RoleRevokeOutcome;
use tb_raid::partner_setup::{
    ChatGreeterPort, DiscordDirectoryPort, ModeratorInstallPort, PartnerSetupService,
    SignupTagEnforcePort,
};
use tb_transport_discord::BrokerRelay;
use tb_transport_twitch::{AddModeratorOutcome, HelixClient};

pub struct BrokerDiscordDirectory {
    relay: Option<BrokerRelay>,
    targets: crate::streamer_role_sync::Targets,
    pool: PgPool,
}

impl BrokerDiscordDirectory {
    pub fn from_config(
        relay: Option<BrokerRelay>,
        config: &tb_config::discord::OAuthFollowup,
        pool: PgPool,
    ) -> Self {
        Self {
            relay,
            targets: crate::streamer_role_sync::Targets::from_config(config),
            pool,
        }
    }

    async fn sync(&self, discord_user_id: &str) -> Result<Option<bool>, String> {
        let relay = self.relay.as_ref().ok_or("no_relay")?;
        let user_id = discord_user_id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or("invalid_user_id")?;
        crate::streamer_role_sync::reconcile(&self.pool, relay, &self.targets, Some(user_id)).await
    }

    async fn revoke_streamer_role_detailed(
        &self,
        discord_user_id: &str,
        _reason: &str,
    ) -> RoleRevokeOutcome {
        match self.sync(discord_user_id).await {
            Ok(Some(false)) => RoleRevokeOutcome::Revoked,
            Ok(Some(true)) => RoleRevokeOutcome::Skipped {
                reason: "active_channel".into(),
            },
            Ok(None) => RoleRevokeOutcome::Skipped {
                reason: "unknown_state".into(),
            },
            Err(reason) if matches!(reason.as_str(), "no_relay" | "invalid_user_id") => {
                RoleRevokeOutcome::Skipped { reason }
            }
            Err(detail) => RoleRevokeOutcome::Failed { detail },
        }
    }
}

#[async_trait]
impl DiscordDirectoryPort for BrokerDiscordDirectory {
    async fn resolve_display_name(&self, discord_user_id: &str) -> Option<String> {
        let relay = self.relay.as_ref()?;
        let user_id = discord_user_id.parse().ok()?;
        match relay.resolve_user(user_id).await {
            Ok(Some(user)) => user.preferred_display_name(),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(%error, "Discord-Anzeigename nicht verfügbar");
                None
            }
        }
    }

    async fn grant_streamer_role(&self, discord_user_id: &str, _reason: &str) {
        if let Err(error) = self.sync(discord_user_id).await {
            tracing::warn!(%error, discord_user_id, "Streamer-Rollenabgleich wird später wiederholt");
        }
    }

    async fn revoke_streamer_role(&self, discord_user_id: &str, reason: &str) {
        let outcome = self
            .revoke_streamer_role_detailed(discord_user_id, reason)
            .await;
        tracing::info!(
            discord_user_id,
            ?outcome,
            "Streamer-Rollenabgleich nach Zustandswechsel"
        );
    }
}

#[async_trait]
impl tb_internal_api::DiscordRolePort for BrokerDiscordDirectory {
    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str) {
        <Self as DiscordDirectoryPort>::grant_streamer_role(self, discord_user_id, reason).await
    }

    async fn revoke_streamer_role(&self, discord_user_id: &str, reason: &str) -> RoleRevokeOutcome {
        self.revoke_streamer_role_detailed(discord_user_id, reason)
            .await
    }
}

pub struct HelixModeratorInstaller {
    helix: HelixClient,
}

impl HelixModeratorInstaller {
    pub fn new(helix: HelixClient) -> Self {
        Self { helix }
    }
}

#[async_trait]
impl ModeratorInstallPort for HelixModeratorInstaller {
    async fn add_channel_moderator(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        streamer_access_token: &str,
    ) -> Result<(), String> {
        match self
            .helix
            .add_channel_moderator(broadcaster_id, bot_user_id, streamer_access_token)
            .await
        {
            Ok(AddModeratorOutcome::Added) => {
                tracing::info!(
                    "Bot (ID: {bot_user_id}) is now moderator in channel {broadcaster_id}"
                );
                Ok(())
            }
            Ok(AddModeratorOutcome::AlreadyModerator) => {
                tracing::info!(
                    "Bot (ID: {bot_user_id}) is already moderator in channel {broadcaster_id}"
                );
                Ok(())
            }
            Ok(AddModeratorOutcome::BotBanned { status, body }) => {
                let error = format!(
                    "Bot (ID: {bot_user_id}) is banned in channel {broadcaster_id} (HTTP {status}: {body}); moderator setup skipped"
                );
                tracing::warn!("{error}");
                Err(error)
            }
            Ok(AddModeratorOutcome::AuthError { status, body }) => {
                let error = format!(
                    "Streamer-Autorisierung für Kanal {broadcaster_id} trägt nicht (HTTP {status}: {body}); Moderator-Einsetzung übersprungen"
                );
                tracing::warn!("{error}");
                Err(error)
            }
            Ok(AddModeratorOutcome::Failed { status, body }) => {
                let error = format!(
                    "Failed to add bot as moderator in channel {broadcaster_id}: HTTP {status}: {body}"
                );
                tracing::warn!("{error}");
                Err(error)
            }
            Err(e) => {
                let error =
                    format!("Error adding bot as moderator in channel {broadcaster_id}: {e}");
                tracing::error!("{error}");
                Err(error)
            }
        }
    }
}

pub struct LegacyChatGreeter {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

pub struct NoopChatGreeter;

#[async_trait]
impl ChatGreeterPort for NoopChatGreeter {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        _message: &str,
    ) -> Result<bool, String> {
        tracing::warn!(
            "Chat-Begrüßung übersprungen: Kein Chat-Greeter verfügbar für {twitch_login}"
        );
        Ok(false)
    }
}

impl LegacyChatGreeter {
    pub fn from_runtime() -> Option<Self> {
        let token = std::env::var("TWITCH_INTERNAL_API_TOKEN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())?;
        let base_url = tb_config::runtime::settings()
            .ok()?
            .bot
            .legacy_greeter_base_url
            .trim_end_matches('/')
            .to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self {
            client,
            base_url,
            token,
        })
    }
}

pub struct NativeChatGreeter {
    api: Arc<dyn tb_chat::ChatApi>,
}

impl NativeChatGreeter {
    pub fn new(api: Arc<dyn tb_chat::ChatApi>) -> Self {
        Self { api }
    }
}

#[async_trait]
impl ChatGreeterPort for NativeChatGreeter {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        message: &str,
    ) -> Result<bool, String> {
        let Some(broadcaster_id) = self.api.resolve_user_id(twitch_login).await? else {
            return Err(format!("Login {twitch_login} nicht auflösbar"));
        };
        match self.api.send_message(&broadcaster_id, message).await? {
            tb_chat::SendOutcome::Sent => Ok(true),
            other => {
                tracing::warn!(login = %twitch_login, ?other, "Begrüßung nicht zugestellt");
                Ok(false)
            }
        }
    }
}

#[async_trait]
impl ChatGreeterPort for LegacyChatGreeter {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        message: &str,
    ) -> Result<bool, String> {
        let url = format!(
            "{}/internal/twitch/v1/streamers/{}/chat-action",
            self.base_url, twitch_login
        );
        let resp = self
            .client
            .post(&url)
            .header("X-Internal-Token", &self.token)
            .json(&serde_json::json!({ "message": message }))
            .send()
            .await
            .map_err(|e| format!("chat-action request failed: {e}"))?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = match resp.text().await {
                Ok(body) => body,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        status,
                        login = %twitch_login,
                        "Chat-Action-Fehlerbody nicht lesbar"
                    );
                    String::new()
                }
            };
            let snippet: String = body.chars().take(200).collect();
            return Err(format!("chat-action HTTP {status}: {snippet}"));
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("chat-action response invalid: {e}"))?;
        Ok(body
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false))
    }
}

struct AnalyticsSignupTagBlock {
    pool: PgPool,
}

#[async_trait]
impl SignupTagEnforcePort for AnalyticsSignupTagBlock {
    async fn enforce_session_tags(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        tags: &[String],
    ) -> Result<(), sqlx::Error> {
        tb_analytics::partner_signup_tag_block::enforce(
            &self.pool,
            twitch_user_id,
            twitch_login,
            tags,
        )
        .await?;
        Ok(())
    }
}

pub fn build_partner_setup_service(
    pool: PgPool,
    helix: HelixClient,
    relay: Option<BrokerRelay>,
    native_chat: Option<Arc<dyn tb_chat::ChatApi>>,
) -> Option<Arc<PartnerSetupService>> {
    let greeter: Arc<dyn ChatGreeterPort> = match native_chat {
        Some(api) => Arc::new(NativeChatGreeter::new(api)),
        None => LegacyChatGreeter::from_runtime()
            .map(|g| Arc::new(g) as Arc<dyn ChatGreeterPort>)
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Kein Chat-Greeter konfiguriert (TWITCH_INTERNAL_API_TOKEN fehlt); \
                     Chat-Begrüßung wird übersprungen"
                );
                Arc::new(NoopChatGreeter) as Arc<dyn ChatGreeterPort>
            }),
    };
    let config = match tb_config::runtime::settings() {
        Ok(config) => config,
        Err(_) => {
            tracing::error!("OAuth-Followups benötigen die geladene Betriebskonfiguration");
            return None;
        }
    };
    let bot_user_id = Some(config.twitch.bot_user_id.clone());
    Some(Arc::new(
        PartnerSetupService::new(
            pool.clone(),
            Arc::new(BrokerDiscordDirectory::from_config(
                relay,
                &config.discord.oauth_followup,
                pool.clone(),
            )),
            Arc::new(HelixModeratorInstaller::new(helix)),
            greeter,
            bot_user_id,
        )
        .with_signup_tag_block(Arc::new(AnalyticsSignupTagBlock { pool })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn from_config_hat_immer_eine_guild() {
        let directory = BrokerDiscordDirectory::from_config(
            None,
            &tb_config::discord::OAuthFollowup::default(),
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/test")
                .unwrap(),
        );
        assert!(
            directory.targets.guild_id != 0,
            "ohne Guild-Kandidat wird jeder Rollen-Entzug stumm übersprungen"
        );
    }

    #[tokio::test]
    async fn ohne_relay_wird_der_entzug_als_uebersprungen_gemeldet() {
        let directory = BrokerDiscordDirectory::from_config(
            None,
            &tb_config::discord::OAuthFollowup::default(),
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/test")
                .unwrap(),
        );
        let outcome = directory
            .revoke_streamer_role_detailed("12345", "test")
            .await;
        assert_eq!(
            outcome,
            RoleRevokeOutcome::Skipped {
                reason: "no_relay".to_string()
            }
        );
    }

    #[tokio::test]
    async fn trait_weg_liefert_denselben_ausgang() {
        let directory = BrokerDiscordDirectory::from_config(
            None,
            &tb_config::discord::OAuthFollowup::default(),
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/test")
                .unwrap(),
        );
        let via_trait =
            tb_internal_api::DiscordRolePort::revoke_streamer_role(&directory, "12345", "test")
                .await;
        let direkt = directory
            .revoke_streamer_role_detailed("12345", "test")
            .await;
        assert_eq!(via_trait, direkt);
    }
}
