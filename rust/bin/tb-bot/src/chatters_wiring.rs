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
/// Twitch-Helix-Scope fuer `GET /chat/chatters`.
const CHATTERS_SCOPE: &str = "moderator:read:chatters";

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

/// [`StreamerTokenSource`] über den Broadcaster-Tokenpfad.
/// Der Chatters-Fallback ist bewusst NICHT `raid_enabled`-gegated: Twitch erlaubt
/// `moderator_id == broadcaster_id`, sofern der Streamer-Token den Chatters-Scope
/// hat. Tokens ohne `moderator:read:chatters` werden nicht geliefert.
struct TokenProviderStreamerSource {
    token_provider: Arc<TokenProvider>,
}

#[async_trait]
impl StreamerTokenSource for TokenProviderStreamerSource {
    async fn streamer_token(&self, twitch_user_id: &str) -> Option<String> {
        self.token_provider
            .get_valid_token_unrestricted_with_scope(twitch_user_id, Utc::now(), CHATTERS_SCOPE)
            .await
            .ok()
            .flatten()
    }
}

struct MissingBotChatterAuth;

#[async_trait]
impl BotChatterAuth for MissingBotChatterAuth {
    async fn bot_token(&self) -> Option<String> {
        None
    }

    async fn bot_user_id(&self) -> Option<String> {
        None
    }

    async fn bot_login(&self) -> Option<String> {
        None
    }

    async fn has_chatters_scope(&self) -> bool {
        false
    }
}

struct MissingStreamerTokenSource;

#[async_trait]
impl StreamerTokenSource for MissingStreamerTokenSource {
    async fn streamer_token(&self, _twitch_user_id: &str) -> Option<String> {
        None
    }
}

struct MissingModeratorProvisioner;

#[async_trait]
impl ModeratorProvisioner for MissingModeratorProvisioner {
    async fn ensure_bot_is_mod(&self, _broadcaster_id: &str, login: &str) -> bool {
        tracing::warn!(
            channel = login,
            "chatters: Mod-Self-Heal uebersprungen, Provisioner fehlt"
        );
        false
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
    bot_token_manager
        .map(|manager| Arc::new(BotTokenManagerAuth { manager }) as Arc<dyn BotChatterAuth>)
}

/// Baut den Streamer-Token-Port aus dem Mod-`TokenProvider`.
pub fn build_streamer_token_source(
    token_provider: Option<Arc<TokenProvider>>,
) -> Option<Arc<dyn StreamerTokenSource>> {
    token_provider.map(|tp| {
        Arc::new(TokenProviderStreamerSource { token_provider: tp }) as Arc<dyn StreamerTokenSource>
    })
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
    let Some(fetcher) = fetcher else {
        tracing::warn!("Chatters-Poll inaktiv: HelixClient fehlt");
        return;
    };

    if auth.is_none() && streamer_tokens.is_none() {
        tracing::warn!(
            "Chatters-Poll inaktiv: weder Bot-Token-Manager noch Streamer-TokenProvider verfuegbar"
        );
        return;
    }

    let auth = match auth {
        Some(auth) => auth,
        None => {
            tracing::warn!("chatters: Bot-Token-Manager fehlt, nutze nur Streamer-Token-Fallback");
            Arc::new(MissingBotChatterAuth) as Arc<dyn BotChatterAuth>
        }
    };
    let streamer_tokens = match streamer_tokens {
        Some(streamer_tokens) => streamer_tokens,
        None => {
            tracing::warn!(
                "chatters: TokenProvider fehlt, Streamer-Token-Fallback wird uebersprungen"
            );
            Arc::new(MissingStreamerTokenSource) as Arc<dyn StreamerTokenSource>
        }
    };
    let provisioner = match provisioner {
        Some(provisioner) => provisioner,
        None => {
            tracing::warn!("chatters: Mod-Self-Heal-Port fehlt, Self-Heal wird uebersprungen");
            Arc::new(MissingModeratorProvisioner) as Arc<dyn ModeratorProvisioner>
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
