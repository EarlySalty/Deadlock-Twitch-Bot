pub mod events;
pub mod permissions;
pub mod protocol;
pub mod rest;
mod runner;
pub mod session;

use std::sync::Arc;

use crate::GuildEventSink;

pub use permissions::{invite_url, REQUIRED_PERMISSIONS};

#[derive(Clone)]
pub struct GatewayConfig {
    pub bot_token: String,
    pub application_id: Option<u64>,
    pub community_guild_id: u64,
    pub pool: Option<sqlx::PgPool>,
}

impl std::fmt::Debug for GatewayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayConfig")
            .field("bot_token", &"<verborgen>")
            .field("application_id", &self.application_id)
            .field("community_guild_id", &self.community_guild_id)
            .field("pool", &self.pool.as_ref().map(|_| "PgPool"))
            .finish()
    }
}

pub fn spawn(config: GatewayConfig, sink: Arc<dyn GuildEventSink>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(?config, "Discord-Gateway startet");
        if let Err(msg) = runner::run(config, sink, rest::DISCORD_API_BASE).await {
            tracing::error!(fehler = %msg, "Discord-Gateway endgültig beendet");
        }
    })
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LoggingSink;

#[async_trait::async_trait]
impl GuildEventSink for LoggingSink {
    async fn guild_added(&self, guild_id: u64, name: &str) {
        tracing::info!(guild_id, name, "Senke: Guild hinzugefügt/verfügbar");
    }

    async fn guild_removed(&self, guild_id: u64) {
        tracing::info!(guild_id, "Senke: Guild entfernt");
    }

    async fn member_left(&self, guild_id: u64, user_id: u64) {
        tracing::info!(guild_id, user_id, "Senke: Mitglied hat Guild verlassen");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_ohne_token() {
        let cfg = GatewayConfig {
            bot_token: "sehr-geheim".into(),
            application_id: Some(1),
            community_guild_id: 2,
            pool: None,
        };
        let s = format!("{cfg:?}");
        assert!(!s.contains("sehr-geheim"));
        assert!(s.contains("community_guild_id: 2"));
    }

    #[test]
    fn einladungslink_rechte() {
        let url = invite_url(42);
        assert!(url.contains(&format!("permissions={REQUIRED_PERMISSIONS}")));
        assert_eq!(REQUIRED_PERMISSIONS & permissions::ADMINISTRATOR, 0);
        assert_eq!(REQUIRED_PERMISSIONS & permissions::MENTION_EVERYONE, 0);
    }

    #[tokio::test]
    async fn logging_sink_ist_nutzbar() {
        let sink: Arc<dyn GuildEventSink> = Arc::new(LoggingSink);
        sink.guild_added(1, "x").await;
        sink.guild_added(1, "x").await;
        sink.guild_removed(1).await;
        sink.member_left(1, 2).await;
    }
}
