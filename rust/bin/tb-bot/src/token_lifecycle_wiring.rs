//! Composition-Root für die Token-Lifecycle-Reaktionen (Block 4).
//!
//! Verdrahtet den [`tb_raid::TokenLifecycleReactor`]-Port mit dem F4-Master-Broker
//! (`send-rich-message` als Alert-Embed, `send-dm` als Text-DM, `member/remove-role`)
//! und spawnt die periodischen Sweeps:
//!
//! - **Token-Fehler-Reaktion + Grace-Sweep + Bot-Ban-Restore** — stündlich (Python
//!   `check_grace_periods`, plus native `notify_token_error`-Nachholung):
//!   benachrichtigt neu blacklistete Streamer einmalig (Admin-Embed + User-DM)
//!   entzieht nach 7 Tagen abgelaufener Grace die Streamer-Rolle und hebt
//!   technische `bot_banned`-Pausen nach Health-Restore wieder auf.
//! - **Blacklist-Cleanup** — alle 3,5 h (Python `cleanup_old_entries`, >30 Tage).
//!
//! Discord-Reaktionen laufen ausschließlich über den Broker (der Twitch-Bot hat
//! keinen Discord-Zugang); ohne erreichbaren Broker laufen DB-Cleanup und
//! Bot-Ban-Restore weiter, Discord-lastige Sweeps bleiben aus.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use tb_chat::timeout_tracking::{BotBannedChannelHandler, BotBannedChannelSignal};
use tb_raid::token_lifecycle::TokenLifecycleNotifier;
use tb_raid::TokenLifecycleReactor;
use tb_transport_discord::{BrokerRelay, DiscordBackend, SendAlertEmbed, SendUserDm};

/// Stündlicher Sweep (Token-Fehler-Reaktion + Grace-Period).
const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Cleanup-Intervall: 3,5 h (Python-Scheduler).
const CLEANUP_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60 + 30 * 60);
/// Blacklist-Aufbewahrung in Tagen (Python `cleanup_old_entries` Default).
const CLEANUP_DAYS: i64 = 30;
/// Embed-Farbe für Token-Fehler-Alerts (rot).
const ALERT_COLOR: i64 = 0xE7_4C_3C;

/// Streamer-Guild/Rolle (gleiche Defaults wie [`crate::oauth_followups`] /
/// `streamer_link`): Env `STREAMER_GUILD_ID`/`MAIN_GUILD_ID` und `STREAMER_ROLE_ID`.
const DEFAULT_STREAMER_ROLE_ID: u64 = 1313624729466441769;

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
}

/// Broker-gestützte Umsetzung des Discord-Reaktions-Ports. Alle Methoden sind
/// best-effort: Fehler werden geloggt, nie propagiert (Python-Parität).
struct BrokerTokenLifecycleNotifier {
    relay: Option<BrokerRelay>,
    guild_id: Option<u64>,
    role_id: u64,
}

impl BrokerTokenLifecycleNotifier {
    fn from_env(relay: BrokerRelay) -> Self {
        Self::from_optional_env(Some(relay))
    }

    fn disabled() -> Self {
        Self::from_optional_env(None)
    }

    fn from_optional_env(relay: Option<BrokerRelay>) -> Self {
        Self {
            relay,
            guild_id: env_u64("STREAMER_GUILD_ID").or_else(|| env_u64("MAIN_GUILD_ID")),
            role_id: env_u64("STREAMER_ROLE_ID").unwrap_or(DEFAULT_STREAMER_ROLE_ID),
        }
    }
}

#[async_trait]
impl TokenLifecycleNotifier for BrokerTokenLifecycleNotifier {
    async fn send_admin_embed(&self, channel_id: i64, title: &str, description: &str) -> bool {
        let Some(relay) = &self.relay else {
            return false;
        };
        let payload = SendAlertEmbed {
            channel_id,
            content: None,
            embed: serde_json::json!({
                "title": title,
                "description": description,
                "color": ALERT_COLOR,
            }),
            allowed_role_ids: vec![],
        };
        match relay.send_alert_embed(payload).await {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!("Token-Lifecycle: Admin-Embed fehlgeschlagen: {e}");
                false
            }
        }
    }

    async fn send_user_dm(&self, discord_user_id: &str, content: &str) -> bool {
        let Some(relay) = &self.relay else {
            return false;
        };
        let Ok(user_id) = discord_user_id.parse::<u64>() else {
            return false;
        };
        let payload = SendUserDm {
            user_id,
            content: content.to_string(),
        };
        match relay.send_user_dm(payload).await {
            Ok(_) => true,
            Err(e) => {
                // DMs geschlossen / User unbekannt etc. → nur Debug, kein Alarm.
                tracing::debug!("Token-Lifecycle: User-DM nicht zustellbar: {e}");
                false
            }
        }
    }

    async fn revoke_streamer_role(&self, discord_user_id: &str, reason: &str) -> bool {
        let Some(relay) = &self.relay else {
            return false;
        };
        let Some(guild_id) = self.guild_id else {
            tracing::warn!(
                "Token-Lifecycle: Rollen-Entzug übersprungen — STREAMER_GUILD_ID/MAIN_GUILD_ID nicht gesetzt"
            );
            return false;
        };
        let Ok(user_id) = discord_user_id.parse::<u64>() else {
            return false;
        };
        match DiscordBackend::remove_member_role(relay, guild_id, user_id, self.role_id, reason)
            .await
        {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("Token-Lifecycle: Streamer-Rollen-Entzug fehlgeschlagen: {e}");
                false
            }
        }
    }
}

struct BrokerBotBanLifecycleHandler {
    reactor: Arc<TokenLifecycleReactor<BrokerTokenLifecycleNotifier>>,
}

#[async_trait]
impl BotBannedChannelHandler for BrokerBotBanLifecycleHandler {
    async fn on_bot_banned_channel(&self, signal: BotBannedChannelSignal) {
        let outcome = self
            .reactor
            .handle_bot_banned_channel(
                &signal.broadcaster_id,
                &signal.broadcaster_login,
                &signal.reason,
            )
            .await;
        if outcome.already_flagged {
            tracing::debug!(
                login = %signal.broadcaster_login,
                "Bot-Ban-Lifecycle bereits verarbeitet"
            );
        } else {
            tracing::info!(
                login = %signal.broadcaster_login,
                dm = outcome.user_dm_sent,
                "Bot-Ban-Lifecycle aus Chat-Signal verarbeitet"
            );
        }
    }
}

pub(crate) fn build_bot_ban_handler(
    pool: PgPool,
    broker: &tb_config::BrokerConfig,
) -> Arc<dyn BotBannedChannelHandler> {
    let notifier = match BrokerRelay::new(broker) {
        Ok(relay) => BrokerTokenLifecycleNotifier::from_env(relay),
        Err(error) => {
            tracing::warn!(
                "Bot-Ban-Lifecycle: BrokerRelay nicht initialisierbar, Recovery-DM deaktiviert: {error}"
            );
            BrokerTokenLifecycleNotifier::disabled()
        }
    };
    Arc::new(BrokerBotBanLifecycleHandler {
        reactor: Arc::new(TokenLifecycleReactor::new(pool, notifier)),
    })
}

/// Spawnt die Token-Lifecycle-Scheduler. Wenn kein BrokerRelay konstruierbar
/// ist, laufen DB-Cleanup und Bot-Ban-Restore weiter; Discord-lastige Sweeps
/// bleiben aus.
pub fn spawn_token_lifecycle_schedulers(pool: PgPool, broker: &tb_config::BrokerConfig) {
    let (notifier, discord_enabled) = match BrokerRelay::new(broker) {
        Ok(relay) => (BrokerTokenLifecycleNotifier::from_env(relay), true),
        Err(e) => {
            tracing::warn!(
                "Token-Lifecycle-Scheduler ohne Discord-Broker gestartet: BrokerRelay nicht initialisierbar: {e}"
            );
            (BrokerTokenLifecycleNotifier::disabled(), false)
        }
    };
    let reactor = Arc::new(TokenLifecycleReactor::new(pool, notifier));
    spawn_token_lifecycle_tasks(reactor, discord_enabled);
}

fn spawn_token_lifecycle_tasks(
    reactor: Arc<TokenLifecycleReactor<BrokerTokenLifecycleNotifier>>,
    discord_enabled: bool,
) {
    // Stündlicher Sweep: erst neu blacklistete Streamer benachrichtigen
    // (notify_token_error, 1×/Streamer), dann abgelaufene Grace-Periods abräumen.
    {
        let reactor = reactor.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SWEEP_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let (notified, expired) = if discord_enabled {
                    (
                        reactor.notify_pending_errors().await,
                        reactor.check_grace_periods().await,
                    )
                } else {
                    (0, 0)
                };
                let restored = reactor.restore_ready_bot_banned_channels().await;
                let reconciled = reactor.reconcile_healthy_raid_toggles().await;
                if notified > 0 || expired > 0 || restored > 0 || reconciled > 0 {
                    tracing::info!(
                        notified,
                        grace_expired = expired,
                        bot_ban_restored = restored,
                        raid_toggle_reconciled = reconciled,
                        "Token-Lifecycle-Sweep abgeschlossen"
                    );
                }
            }
        });
    }

    // Blacklist-Cleanup alle 3,5 h.
    {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(CLEANUP_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                reactor.cleanup_old_entries(CLEANUP_DAYS).await;
            }
        });
    }

    tracing::info!(
        "Token-Lifecycle-Scheduler aktiv (Fehler-Reaktion + Grace + Bot-Ban-Restore stündlich, Cleanup 3,5 h)"
    );
}
