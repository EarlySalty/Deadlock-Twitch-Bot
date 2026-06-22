//! Composition-Root für den Chatters-Poller (#11, P2.64/P2.61/P1.23/P1.24).
//!
//! Verdrahtet die [`tb_monitoring`]-Ports ([`BotChatterAuth`],
//! [`StreamerTokenSource`], [`ChattersFetcher`], [`ModeratorProvisioner`]) mit
//! den konkreten Token-Stores des Binaries und spawnt die beiden periodischen
//! Loops:
//!
//! - **Chatters-Collect** — alle 30 s: pollt alle live Streamer über Helix
//!   `GET /chat/chatters` und spiegelt alle Anwesenden (inkl. stiller Lurker)
//!   nach `twitch_session_chatters`/`twitch_chatter_rollup`/
//!   `twitch_viewer_presence_ticks`. Wird nur gestartet, wenn alle vier Ports
//!   verfügbar sind (sonst ist kein Poll möglich → Log + kein Spawn).
//! - **Raid-Retention** — stündlich, unabhängig vom Token-Plumbing: berechnet
//!   die 5/15/30-Minuten-Retention der letzten 7 Tage. Wird immer gestartet.
//!
//! Der [`ChattersCollector`] wird **einmal** vor dem Loop gebaut, damit
//! Self-Heal-Cooldowns und der gemeinsame State über alle 30s-Ticks hinweg
//! geteilt werden.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use tb_chat::token::BotTokenManager;
use tb_monitoring::{
    BotChatterAuth, ChattersCollector, ChattersFetcher, ModeratorProvisioner, StreamerTokenSource,
};
use tb_raid::TokenProvider;
use tb_transport_twitch::{HelixClient, HelixError};

/// Intervall des Chatters-Collect-Loops (Python `collect_chatters_data`, 30 s).
const COLLECT_INTERVAL: Duration = Duration::from_secs(30);
/// Intervall des Raid-Retention-Loops (1 h).
const RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);

// ---------------------------------------------------------------------------
// Port-Adapter
// ---------------------------------------------------------------------------

/// [`BotChatterAuth`] über den live rotierten [`BotTokenManager`].
struct BotTokenManagerAuth {
    manager: Arc<BotTokenManager>,
}

#[async_trait]
impl BotChatterAuth for BotTokenManagerAuth {
    async fn bot_token(&self) -> Option<String> {
        self.manager.access_token().await.ok()
    }

    async fn bot_user_id(&self) -> Option<String> {
        let id = self.manager.bot_user_id().await;
        (!id.is_empty()).then_some(id)
    }

    async fn bot_login(&self) -> Option<String> {
        let login = self.manager.bot_login().await;
        (!login.is_empty()).then_some(login.to_lowercase())
    }

    async fn has_chatters_scope(&self) -> bool {
        // Wie Python: leere Scope-Liste = „noch nicht geladen → erlaubt".
        let scopes = self.manager.scopes().await;
        scopes.is_empty() || scopes.iter().any(|s| s == "moderator:read:chatters")
    }
}

/// [`StreamerTokenSource`] über den raid-gegateten [`TokenProvider`].
/// `get_valid_token` liefert nur bei `raid_enabled IS TRUE` einen Token.
struct TokenProviderStreamerSource {
    token_provider: Arc<TokenProvider>,
}

#[async_trait]
impl StreamerTokenSource for TokenProviderStreamerSource {
    async fn streamer_token(&self, twitch_user_id: &str) -> Option<String> {
        self.token_provider
            .get_valid_token(twitch_user_id, Utc::now())
            .await
            .ok()
            .flatten()
    }
}

/// Realer [`ChattersFetcher`] über `HelixClient::get_chatters`.
/// Reicht die `user_id` durch (leer → vom Aufrufer zu `None` normalisiert);
/// 403 propagiert als [`HelixError::NotModerator`].
struct HelixChattersFetcher {
    helix: HelixClient,
}

#[async_trait]
impl ChattersFetcher for HelixChattersFetcher {
    async fn fetch_chatters(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        token: &str,
    ) -> Result<Vec<(String, Option<String>)>, HelixError> {
        self.helix
            .get_chatters(broadcaster_id, moderator_id, token)
            .await
            .map(|chatters| {
                chatters
                    .into_iter()
                    .map(|c| {
                        let user_id = (!c.user_id.is_empty()).then_some(c.user_id);
                        (c.user_login, user_id)
                    })
                    .collect()
            })
    }
}

// ---------------------------------------------------------------------------
// Adapter-Konstruktion (aus den Binary-Stores)
// ---------------------------------------------------------------------------

/// Baut den Bot-Auth-Port aus dem [`BotTokenManager`]-Clone des ChatApi-Handles.
pub fn build_bot_chatter_auth(
    bot_token_manager: Option<Arc<BotTokenManager>>,
) -> Option<Arc<dyn BotChatterAuth>> {
    bot_token_manager.map(|manager| Arc::new(BotTokenManagerAuth { manager }) as Arc<dyn BotChatterAuth>)
}

/// Baut den Streamer-Token-Port aus dem Mod-`TokenProvider`.
pub fn build_streamer_token_source(
    token_provider: Option<Arc<TokenProvider>>,
) -> Option<Arc<dyn StreamerTokenSource>> {
    token_provider
        .map(|tp| Arc::new(TokenProviderStreamerSource { token_provider: tp }) as Arc<dyn StreamerTokenSource>)
}

/// Baut den Helix-Chatters-Fetcher (nur mit aktivem `HelixClient`).
pub fn build_chatters_fetcher(helix: Option<HelixClient>) -> Option<Arc<dyn ChattersFetcher>> {
    helix.map(|helix| Arc::new(HelixChattersFetcher { helix }) as Arc<dyn ChattersFetcher>)
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Spawnt die beiden Chatters-Loops.
///
/// Der **Collect-Loop** wird nur gestartet, wenn alle vier Ports vorhanden sind
/// (sonst ist kein Poll möglich). Der **Retention-Loop** braucht kein
/// Token-Plumbing und wird immer gestartet. Fehler werden geloggt; beide Loops
/// laufen weiter.
pub fn spawn_chatters_schedulers(
    pool: PgPool,
    auth: Option<Arc<dyn BotChatterAuth>>,
    streamer_tokens: Option<Arc<dyn StreamerTokenSource>>,
    fetcher: Option<Arc<dyn ChattersFetcher>>,
    provisioner: Option<Arc<dyn ModeratorProvisioner>>,
) {
    spawn_collect_loop(pool.clone(), auth, streamer_tokens, fetcher, provisioner);
    spawn_retention_loop(pool);
}

fn spawn_collect_loop(
    pool: PgPool,
    auth: Option<Arc<dyn BotChatterAuth>>,
    streamer_tokens: Option<Arc<dyn StreamerTokenSource>>,
    fetcher: Option<Arc<dyn ChattersFetcher>>,
    provisioner: Option<Arc<dyn ModeratorProvisioner>>,
) {
    let (auth, streamer_tokens, fetcher, provisioner) =
        match (auth, streamer_tokens, fetcher, provisioner) {
            (Some(auth), Some(streamer_tokens), Some(fetcher), Some(provisioner)) => {
                (auth, streamer_tokens, fetcher, provisioner)
            }
            (auth, streamer_tokens, fetcher, provisioner) => {
                let mut missing: Vec<&str> = Vec::new();
                if auth.is_none() {
                    missing.push("Bot-Token-Manager");
                }
                if fetcher.is_none() {
                    missing.push("HelixClient");
                }
                if streamer_tokens.is_none() || provisioner.is_none() {
                    missing.push("DB_MASTER_KEY_V1/TokenProvider");
                }
                tracing::info!(
                    missing = %missing.join(", "),
                    "Chatters-Poll inaktiv: fehlende Ports"
                );
                return;
            }
        };

    // Collector EINMAL bauen → Self-Heal- + Bot-nicht-Mod-Backoff-Cooldowns
    // werden über alle 30s-Ticks geteilt.
    let collector = ChattersCollector::new(pool, auth, streamer_tokens, fetcher, provisioner);

    tokio::spawn(async move {
        let mut tick = tokio::time::interval(COLLECT_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let stats = collector.run_cycle().await;
            tracing::info!(
                live_streamers = stats.live_streamers,
                bot_path_success = stats.bot_path_success,
                bot_path_failure = stats.bot_path_failure,
                bot_path_skipped_backoff = stats.bot_path_skipped_backoff,
                fallback_to_streamer_token = stats.fallback_to_streamer_token,
                self_heal_success = stats.self_heal_success,
                self_heal_failure = stats.self_heal_failure,
                chatters_written = stats.chatters_written,
                lurkers_new = stats.lurkers_new,
                "chatters: Collect-Zyklus abgeschlossen"
            );
        }
    });
}

fn spawn_retention_loop(pool: PgPool) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(RETENTION_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match tb_monitoring::compute_raid_retention(&pool).await {
                Ok(stats) => tracing::info!(
                    raids_scanned = stats.raids_scanned,
                    raids_computed = stats.raids_computed,
                    raids_skipped_existing = stats.raids_skipped_existing,
                    raids_skipped_no_session = stats.raids_skipped_no_session,
                    "raid_retention: Lauf abgeschlossen"
                ),
                Err(error) => tracing::error!(%error, "raid_retention: Lauf fehlgeschlagen"),
            }
        }
    });
}
