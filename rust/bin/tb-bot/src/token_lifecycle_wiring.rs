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
//! - **Aktive Ban-Prüfung** — im selben stündlichen Sweep: fragt für jeden
//!   gesunden Partner-Kanal bei Twitch nach, ob der Bot dort gebannt ist. Ohne
//!   sie fällt ein Ban nur auf, wenn der Bot in dem Kanal gerade sendet.
//! - **Blacklist-Cleanup** — alle 3,5 h (Python `cleanup_old_entries`, >30 Tage).
//!
//! Discord-Reaktionen laufen ausschließlich über den Broker (der Twitch-Bot hat
//! keinen Discord-Zugang); ohne erreichbaren Broker laufen Grace-Expiry,
//! DB-Cleanup und Bot-Ban-Restore weiter, Discord-Benachrichtigungen bleiben aus.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use tb_chat::timeout_tracking::{BotBannedChannelHandler, BotBannedChannelSignal};
use tb_raid::token_lifecycle::TokenLifecycleNotifier;
use tb_raid::{BotBanStatusProbe, TokenLifecycleReactor};
use tb_transport_discord::{BrokerRelay, DiscordBackend, SendAlertEmbed, SendUserDm};

use crate::task_supervisor::TaskSupervisor;

/// Stündlicher Sweep (Token-Fehler-Reaktion + Grace-Period).
const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Cleanup-Intervall: 3,5 h (Python-Scheduler).
const CLEANUP_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60 + 30 * 60);
/// Blacklist-Aufbewahrung in Tagen (Python `cleanup_old_entries` Default).
const CLEANUP_DAYS: i64 = 30;
/// Deadlock-Pause-Sweep: 15 Minuten. Der Unmod-Teil hat es nicht eilig (er greift
/// erst nach zwei Monaten), aber ein Comeback soll nicht bis zum nächsten
/// Stundenschlag warten. Ohne Treffer kostet der Lauf zwei DB-Queries.
const DEADLOCK_PAUSE_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// Embed-Farbe für Token-Fehler-Alerts (rot).
const ALERT_COLOR: i64 = 0xE7_4C_3C;

/// Streamer-Guild/Rolle (gleiche Defaults wie [`crate::oauth_followups`] /
/// `streamer_link`): Env `STREAMER_GUILD_ID`/`MAIN_GUILD_ID` und `STREAMER_ROLE_ID`.
const DEFAULT_STREAMER_ROLE_ID: u64 = 1313624729466441769;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenLifecycleSweepPolicy {
    notify_pending_errors: bool,
    check_grace_periods: bool,
}

fn token_lifecycle_sweep_policy(discord_enabled: bool) -> TokenLifecycleSweepPolicy {
    TokenLifecycleSweepPolicy {
        notify_pending_errors: discord_enabled,
        check_grace_periods: true,
    }
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
}

/// Broker-gestützte Umsetzung des Discord-Reaktions-Ports. Alle Methoden sind
/// best-effort: Fehler werden geloggt, nie propagiert (Python-Parität).
pub(crate) struct BrokerTokenLifecycleNotifier {
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
/// ist, laufen Grace-Expiry, DB-Cleanup und Bot-Ban-Restore weiter; nur
/// Discord-Benachrichtigungen bleiben aus.
pub fn spawn_token_lifecycle_schedulers(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &tb_config::BrokerConfig,
    bot_ban_status_probe: Option<Arc<dyn BotBanStatusProbe>>,
) {
    let (notifier, discord_enabled) = match BrokerRelay::new(broker) {
        Ok(relay) => (BrokerTokenLifecycleNotifier::from_env(relay), true),
        Err(e) => {
            tracing::warn!(
                "Token-Lifecycle-Scheduler ohne Discord-Broker gestartet: BrokerRelay nicht initialisierbar: {e}"
            );
            (BrokerTokenLifecycleNotifier::disabled(), false)
        }
    };
    let mut reactor = TokenLifecycleReactor::new(pool, notifier);
    if let Some(probe) = bot_ban_status_probe {
        reactor = reactor.with_bot_ban_status_probe(probe);
    }
    let reactor = Arc::new(reactor);
    spawn_token_lifecycle_tasks(supervisor, reactor, discord_enabled);
}

fn spawn_token_lifecycle_tasks(
    supervisor: &TaskSupervisor,
    reactor: Arc<TokenLifecycleReactor<BrokerTokenLifecycleNotifier>>,
    discord_enabled: bool,
) {
    // Stündlicher Sweep: erst neu blacklistete Streamer benachrichtigen
    // (notify_token_error, 1×/Streamer), dann abgelaufene Grace-Periods abräumen.
    {
        let reactor = reactor.clone();
        supervisor.spawn("token_lifecycle_sweep", async move {
            let mut tick = tokio::time::interval(SWEEP_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let policy = token_lifecycle_sweep_policy(discord_enabled);
                let notified = if policy.notify_pending_errors {
                    reactor.notify_pending_errors().await
                } else {
                    0
                };
                let expired = if policy.check_grace_periods {
                    reactor.check_grace_periods().await
                } else {
                    0
                };
                // Zuerst die eigenen Fehlurteile zurücknehmen: die aktive
                // Prüfung durfte einmal selbst pausieren und hat dabei kaputte
                // Tokens für Banns gehalten. Was danach läuft, soll auf einem
                // sauberen Zustand arbeiten.
                let ban_probe_geheilt = reactor.clear_unverified_ban_probe_marks().await;
                let restored = reactor.restore_ready_bot_banned_channels().await;
                let token_reactivated = reactor
                    .reactivate_token_error_partners_with_valid_auth()
                    .await;
                let reconciled = reactor.reconcile_healthy_raid_toggles().await;
                // Aktive Ban-Prüfung zuletzt: sie sendet Helix-Requests und soll
                // erst laufen, wenn die reinen DB-Sweeps ihren Zustand gesetzt
                // haben (sonst probt sie Kanäle, die gerade restauriert wurden).
                let banned_detected = reactor.detect_bot_bans().await;
                if notified > 0
                    || expired > 0
                    || restored > 0
                    || token_reactivated > 0
                    || reconciled > 0
                    || banned_detected > 0
                    || ban_probe_geheilt > 0
                {
                    tracing::info!(
                        notified,
                        grace_expired = expired,
                        bot_ban_restored = restored,
                        bot_ban_detected = banned_detected,
                        ban_probe_geheilt,
                        token_error_reactivated = token_reactivated,
                        raid_toggle_reconciled = reconciled,
                        "Token-Lifecycle-Sweep abgeschlossen"
                    );
                }
            }
        });
    }

    // Blacklist-Cleanup alle 3,5 h.
    {
        supervisor.spawn("token_lifecycle_cleanup", async move {
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

/// Der Reactor des Deadlock-Pause-Sweeps, geteilt zwischen Timer-Task und
/// MCP-Connector. Ein manuell ausgelöster Sweep soll denselben Reactor fahren
/// wie der Timer, nicht einen zweiten mit eigener Verdrahtung.
pub(crate) type SharedDeadlockPauseReactor =
    Arc<tb_raid::DeadlockPauseReactor<BrokerTokenLifecycleNotifier>>;

/// Spawnt den Deadlock-Pause-Sweep: Mod-Rechte abgeben, wenn ein Partner
/// [`tb_raid::DEADLOCK_PAUSE_DAYS`] lang kein Deadlock gestreamt hat, und beim
/// Comeback zurückholen.
///
/// Eigener Task statt eines weiteren Schritts im Token-Lifecycle-Sweep: der läuft
/// stündlich, hier soll ein Comeback schneller beantwortet werden. Ohne Unmod-Port
/// oder Ban-Probe (fehlender `DB_MASTER_KEY_V1`, keine Bot-ID) startet er nicht,
/// statt halb zu arbeiten und Kanäle unmarkiert zu lassen.
///
/// Gibt den Reactor zurück, damit ein Aufrufer denselben Lauf auch außer der
/// Reihe auslösen kann (MCP-Connector). `None` heißt: es gibt keinen Sweep.
pub(crate) fn spawn_deadlock_pause_scheduler(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &tb_config::BrokerConfig,
    unmod: Option<Arc<dyn tb_raid::DeadlockPauseUnmodPort>>,
    remod: Option<Arc<dyn BotBanStatusProbe>>,
) -> Option<SharedDeadlockPauseReactor> {
    let (Some(unmod), Some(remod)) = (unmod, remod) else {
        tracing::info!(
            "Deadlock-Pause-Sweep nicht gestartet — Mod-Entzug oder Ban-Probe nicht verdrahtet"
        );
        return None;
    };
    let notifier = match BrokerRelay::new(broker) {
        Ok(relay) => BrokerTokenLifecycleNotifier::from_env(relay),
        Err(error) => {
            tracing::warn!(
                "Deadlock-Pause-Sweep ohne Discord-Broker gestartet: BrokerRelay nicht initialisierbar: {error}"
            );
            BrokerTokenLifecycleNotifier::disabled()
        }
    };
    let reactor = Arc::new(tb_raid::DeadlockPauseReactor::new(
        pool, notifier, unmod, remod,
    ));
    let shared = reactor.clone();
    supervisor.spawn("deadlock_pause_sweep", async move {
        let mut tick = tokio::time::interval(DEADLOCK_PAUSE_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Der erste Tick von `interval` feuert sofort. Ohne dieses Vorab-Warten
        // gingen Sekunden nach jedem Deploy die ersten Unmods samt echter
        // Streamer-DMs raus, bevor irgendjemand die Vorschau gesehen hat.
        tick.tick().await;
        loop {
            tick.tick().await;
            let outcome = reactor.sweep().await;
            if outcome.any() {
                tracing::info!(
                    unmodded = outcome.unmodded,
                    remodded = outcome.remodded,
                    "Deadlock-Pause-Sweep abgeschlossen"
                );
            }
        }
    });
    tracing::info!(
        pause_days = tb_raid::DEADLOCK_PAUSE_DAYS,
        "Deadlock-Pause-Sweep aktiv (Unmod nach Deadlock-Pause, Remod beim Comeback)"
    );
    Some(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_absent_deaktiviert_nur_discord_notify_nicht_grace_expiry() {
        let policy = token_lifecycle_sweep_policy(false);

        assert!(!policy.notify_pending_errors);
        assert!(policy.check_grace_periods);
    }

    #[test]
    fn broker_present_aktiviert_notify_und_grace_expiry() {
        let policy = token_lifecycle_sweep_policy(true);

        assert!(policy.notify_pending_errors);
        assert!(policy.check_grace_periods);
    }
}
