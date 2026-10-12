use async_trait::async_trait;
use sqlx::PgPool;
use tb_raid::token_lifecycle::{discord_user_id_for, TokenLifecycleNotifier};
use tb_social_media::autopilot_publish::{AutopilotNotifier, DiscordDelivery};

use crate::token_lifecycle_wiring::BrokerTokenLifecycleNotifier;

pub(crate) struct AutopilotDiscordNotifier {
    pool: PgPool,
    dm: BrokerTokenLifecycleNotifier,
}

impl AutopilotDiscordNotifier {
    pub(crate) fn new(pool: PgPool, dm: BrokerTokenLifecycleNotifier) -> Self {
        Self { pool, dm }
    }
}

#[async_trait]
impl AutopilotNotifier for AutopilotDiscordNotifier {
    async fn send(&self, twitch_user_id: &str, content: &str) -> DiscordDelivery {
        let Some(discord_user_id) = discord_user_id_for(&self.pool, twitch_user_id, "").await
        else {
            return DiscordDelivery::Unavailable;
        };
        if self.dm.send_user_dm(&discord_user_id, content).await {
            DiscordDelivery::Sent
        } else {
            DiscordDelivery::Failed
        }
    }
}
