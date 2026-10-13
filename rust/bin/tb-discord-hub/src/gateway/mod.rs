//! Gateway-Verbindung des Discord-Hubs (Discord-Gateway v10, JSON, ohne
//! Kompression).
//!
//! Aufgaben:
//! - Guild hinzugefügt/entfernt (`GUILD_CREATE`/`GUILD_DELETE`) und
//!   Mitglied verlassen (`GUILD_MEMBER_REMOVE`, privilegierter
//!   Members-Intent) an eine [`GuildEventSink`] melden.
//! - Slash-Befehl `/setup` global registrieren und beantworten
//!   (Rechteprüfung des Hubs auf fremden Servern).
//!
//! Aufbau:
//! - [`protocol`]: Opcodes, Payloads, Close-Codes (reine Logik)
//! - [`session`]: Zustandsmaschine HELLO/IDENTIFY/RESUME/Heartbeat (reine Logik)
//! - [`events`]: Dispatch → Hub-Ereignis (reine Logik)
//! - [`permissions`]: Einladungslink, Rechteberechnung, `/setup`-Texte (reine Logik)
//! - [`rest`]: schmaler Discord-REST-Client
//! - `runner`: WebSocket-Loop mit Reconnect und Backoff

pub mod events;
pub mod permissions;
pub mod protocol;
pub mod rest;
mod runner;
pub mod session;

use std::sync::Arc;

use crate::GuildEventSink;

pub use permissions::{invite_url, REQUIRED_PERMISSIONS};

/// Konfiguration der Gateway-Verbindung.
#[derive(Clone)]
pub struct GatewayConfig {
    /// Bot-Token der Hub-Anwendung.
    pub bot_token: String,
    /// Application-ID; ohne sie wird `/setup` nicht registriert.
    pub application_id: Option<u64>,
    /// Deutscher Community-Server (wird vom Haupt-Bot betreut).
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

/// Startet die Gateway-Verbindung als Hintergrund-Task. Der Task verbindet
/// bei Störungen selbst neu und endet nur bei nicht behebbaren Fehlern
/// (ungültiges Token, nicht freigeschaltete Intents …); diese werden als
/// Fehler protokolliert.
pub fn spawn(config: GatewayConfig, sink: Arc<dyn GuildEventSink>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(?config, "Discord-Gateway startet");
        if let Err(msg) = runner::run(config, sink, rest::DISCORD_API_BASE).await {
            tracing::error!(fehler = %msg, "Discord-Gateway endgültig beendet");
        }
    })
}

/// Fallback ohne `DATABASE_URL`: nur Logs. READY-Replay bleibt idempotent.
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
