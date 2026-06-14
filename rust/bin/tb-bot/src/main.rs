//! tb-bot — Internes API-Binary auf Port 8776.
//!
//! Bindet ausschließlich auf 127.0.0.1 (Loopback). UFW blockt 8776 extern.
//! Auth: X-Internal-Token + loopback_only-Layer (Defense-in-Depth).
//!
//! Env-Variablen:
//!   TWITCH_ANALYTICS_DSN          — PostgreSQL-DSN
//!   TWITCH_INTERNAL_API_TOKEN     — Auth-Token
//!   TWITCH_CLIENT_ID              — Twitch Helix Client-ID (optional)
//!   TWITCH_CLIENT_SECRET          — Twitch Helix Client-Secret (optional)
//!   TWITCH_TARGET_GAME_NAME       — Ziel-Kategorie (default "Deadlock")
//!   TWITCH_WEBHOOK_SECRET         — EventSub-Webhook-Secret (optional)
//!   TWITCH_EVENTSUB_CALLBACK_URL  — öffentliche Callback-URL (optional;
//!                                   beide gesetzt → Subscription-Verwaltung)
//!   TB_MONITORING_POLL_ENABLED    — "1" startet den Poll-Loop (Cutover-Gate,
//!                                   default aus — Python bleibt Live-Writer)
//!   TWITCH_NOTIFY_CHANNEL_ID      — Discord-Kanal der Go-Live-Postings
//!   TWITCH_ALERT_MENTION          — optionale Alert-Mention (z. B. <@&id>)
//!   TWITCH_DISCORD_REF_CODE       — Referral-Code für Twitch-URLs
//!   TWITCH_LANGUAGE_FILTERS       — Komma-Liste (z. B. "de,en"), leer = alle
//!   DB_MASTER_KEY_V1              — AES-Master-Key (Hex); ohne ihn bleiben
//!                                   die Raid-Hooks deaktiviert (kein Token-Read)
//!   TB_INTERNAL_API_LEGACY_FALLBACK_URL — Basis-URL der Legacy-Python-API
//!                                   (z. B. http://127.0.0.1:8779); unbekannte
//!                                   interne-API-Routen werden dorthin
//!                                   geproxyt, leer = 404 wie bisher
//!   PORT                          — optional, default 8776
//!   TB_CLIP_FETCHER_ENABLED       — "1" startet den Clip-Fetch-Task (default aus;
//!                                   benötigt Helix-Client)
//!   TB_SCOUT_ENABLED              — "1" startet den Scout-Task für live Deadlock-DE-Streams
//!                                   (default aus; benötigt Helix-Client)

mod auto_raid;
mod chat_wiring;
mod streamer_link;
mod confirm_resolver;
mod eventsub_hooks;
mod eventsub_stats_adapter;
mod oauth_followups;
mod offline_side_effects;
mod partner_lookup;
mod raid_adapters;
mod raid_arrival_wiring;
mod raid_oauth_impl;
mod score_refresh;
mod wiring;

use std::net::SocketAddr;
use std::sync::Arc;
use tb_config::Settings;
use tb_crypto::FieldCipher;
use tb_internal_api::build_internal_router;
use tb_monitoring::poller::{ChannelInfoSource, PollHooks, StreamSource};
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::sessions::tracker::FollowerCountSource;
use tb_monitoring::{
    AnnounceConfigStore, AnnouncementSettings, AnnouncementSink, BrokerAnnouncementSink,
    CapacitySnapshotStore, EventSubDispatcher, EventSubHooks, ExpSessionStore, ExpSessionTracker,
    GuardStore, InboxRuntime, LiveStateStore, MonitoringEventHandler, NoFollowerSource,
    NoopAnnouncementSink, NoopEventSubHooks, PollConfig, PollEngine, PollIntervalStore,
    SessionTracker, StatsStore, SubscriptionConfig, SubscriptionManager, TelemetryStore,
    TrackedStore, VodPreviewSource,
};
use tb_raid::{
    AutoRaidPipeline, ManualRaidSuppression, OfflineEligibilityStore, OutreachBoostStore,
    PartnerRosterStore, PendingRaidStore, RaidArrivalRuntime, RaidAuthStore, RaidBlacklistStore,
    RaidExecutor, RaidHistoryStore, RaidTokenRefresher, ScoreStore, StrikesStore,
    TokenBlacklistStore, TokenProvider,
};
use tb_transport_discord::BrokerRelay;
use tb_transport_twitch::{HelixClient, HelixConfig};

use auto_raid::OfflineRaidHandler;
use eventsub_hooks::{BlacklistRaidGuard, RaidArrivalCoordinator, RaidEventSubHooks};
use offline_side_effects::OfflineSideEffects;
use raid_adapters::{
    HelixFallbackStreams, HelixRaidApi, HelixTokenClient, ManagerArrivalReadiness,
    ManualRaidAdapter,
};
use raid_arrival_wiring::RaidArrivalSinkImpl;
use score_refresh::ScoreRefreshResolver;
use wiring::{
    BrokerAnnouncementTransport, HelixFollowerSource, HelixStreamSource,
    HelixSubscriptionTransport, HelixVodPreview, SubscriptionEventSubHooks,
};

/// Go-Live-Hook des Poll-Loops → stream.offline-Subscription (wie EventSub).
struct SubscriptionPollHooks {
    manager: Arc<SubscriptionManager>,
}

#[async_trait::async_trait]
impl PollHooks for SubscriptionPollHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
    }
}

/// Sprachfilter fürs Kategorie-Sampling/Scout. Python hartkodiert
/// `TWITCH_LANGUAGE="de de-de de-at de-ch"` (core/constants.py); hier ist ein
/// Env-Override via `TWITCH_LANGUAGE_FILTERS` erlaubt, aber leer/ungesetzt fällt
/// auf den deutschen Default zurück — **nicht** auf „alle Sprachen", sonst landet
/// das Kategorie-Sample sprachgemischt in den Stats (Port-Bug bis 13.6.).
fn language_filters_from_env() -> Vec<String> {
    let parsed: Vec<String> = std::env::var("TWITCH_LANGUAGE_FILTERS")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if parsed.is_empty() {
        ["de", "de-de", "de-at", "de-ch"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        parsed
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let settings = Settings::from_env().unwrap_or_else(|e| {
        tracing::error!("Konfigurationsfehler: {e}");
        std::process::exit(1);
    });

    let pool = tb_db::connect(&settings.db).await.unwrap_or_else(|e| {
        tracing::error!("DB-Verbindungsfehler: {e}");
        std::process::exit(1);
    });

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8776);

    // HelixClient aus Env bauen — optional, Bot startet auch ohne Helix
    let helix: Arc<Option<HelixClient>> = {
        let client_id = std::env::var("TWITCH_CLIENT_ID").ok();
        let client_secret = std::env::var("TWITCH_CLIENT_SECRET").ok();
        match (client_id, client_secret) {
            (Some(id), Some(secret)) => match HelixClient::new(HelixConfig::new(id, secret)) {
                Ok(c) => {
                    tracing::info!("HelixClient initialisiert");
                    Arc::new(Some(c))
                }
                Err(e) => {
                    tracing::warn!("HelixClient-Initialisierung fehlgeschlagen: {e}");
                    Arc::new(None)
                }
            },
            _ => {
                tracing::warn!(
                    "TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET fehlen — Helix-API deaktiviert"
                );
                Arc::new(None)
            }
        }
    };

    // EventSub-Ingress: Inbox-Worker + Dispatcher. Mit Webhook-Config + Helix
    // verwaltet Rust die Core-Subscriptions selbst (Go-Live → stream.offline);
    // mit Krypto-Key sind zusätzlich alle Raid-Hooks echt (s. unten).
    let target_game =
        std::env::var("TWITCH_TARGET_GAME_NAME").unwrap_or_else(|_| "Deadlock".to_string());
    let guard = GuardStore::new(pool.clone());
    let telemetry = TelemetryStore::new(pool.clone());
    let live_state = LiveStateStore::new(pool.clone());
    let followers: Arc<dyn FollowerCountSource> = match helix.as_ref().clone() {
        Some(helix_client) => Arc::new(HelixFollowerSource {
            helix: helix_client,
        }),
        None => Arc::new(NoFollowerSource),
    };
    let tracker = Arc::new(SessionTracker::new(
        SessionStore::new(pool.clone()),
        live_state.clone(),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        followers.clone(),
        &target_game,
    ));
    tracker.rehydrate().await;

    let webhook_secret = std::env::var("TWITCH_WEBHOOK_SECRET")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let callback_url = std::env::var("TWITCH_EVENTSUB_CALLBACK_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let subscription_manager: Option<Arc<SubscriptionManager>> =
        match (webhook_secret, callback_url, helix.as_ref().clone()) {
            (Some(secret), Some(callback_url), Some(helix_client)) => {
                let manager = Arc::new(SubscriptionManager::new(
                    Arc::new(HelixSubscriptionTransport {
                        helix: helix_client,
                    }),
                    SubscriptionConfig {
                        callback_url,
                        secret,
                    },
                    CapacitySnapshotStore::new(pool.clone()),
                ));
                manager.rehydrate().await;
                tracing::info!("EventSub-Subscription-Verwaltung aktiv (Webhook-Modus)");
                // Startup-Cleanup + periodischer Core-Sub-Reconcile:
                // Python führte beim Webhook-Start _cleanup_old_eventsub_subscriptions
                // aus und stelle stream.online/offline/channel.update für alle aktiven
                // Partner sicher. Rust macht das hier als Background-Task.
                {
                    let m = manager.clone();
                    let p = pool.clone();
                    tokio::spawn(async move {
                        subscription_maintenance_loop(m, p).await;
                    });
                }
                Some(manager)
            }
            _ => {
                tracing::info!(
                    "TWITCH_WEBHOOK_SECRET/TWITCH_EVENTSUB_CALLBACK_URL nicht gesetzt — \
                     Subscription-Verwaltung deaktiviert (Hooks Noop)"
                );
                None
            }
        };
    // Welle B Phase 1: Bot-Token booten + ChatApi bauen (TB_CHAT_ENABLED=1).
    // Muss VOR der Hooks-Komposition passieren, damit die OAuth-Followup-
    // Begrüßung den nativen Send statt des Python-Umwegs (8779) nutzt.
    let chat_api_handle = chat_wiring::try_build_api(helix.as_ref().clone()).await;

    // Raid-Verdrahtung: mit Manager + Helix + Krypto-Key sind alle vier
    // Raid-Kopplungen echt (Auto-Raid, Arrival, Score-Refresh, Blacklist-Guard).
    let suppression = Arc::new(std::sync::Mutex::new(ManualRaidSuppression::new()));
    let mut manual_raid_port: Option<Arc<dyn tb_internal_api::ManualRaidPort>> = None;
    let mut raid_oauth_port: Option<Arc<dyn tb_internal_api::RaidOAuthPort>> = None;
    let eventsub_hooks: Arc<dyn EventSubHooks> = match (
        &subscription_manager,
        helix.as_ref().clone(),
        FieldCipher::from_env(),
    ) {
        (Some(manager), Some(helix_client), Ok(cipher)) => {
            let cipher = Arc::new(cipher);

            // Raid-OAuth-Strecke (Welle B): StateStore + AuthWriter +
            // Token-Client zur Composition-Root verdrahten. redirect_uri wie
            // Python (TWITCH_RAID_REDIRECT_URI mit Hardcode-Default,
            // runtime_bootstrap.py:341).
            let raid_redirect_uri = std::env::var("TWITCH_RAID_REDIRECT_URI")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| {
                    "https://deutsche-deadlock-community.de/callback/twitch".to_string()
                });
            if let Ok(client_id) = std::env::var("TWITCH_CLIENT_ID") {
                // Followup-Service: Discord via Master-Broker, Moderator via
                // Helix, Chat-Begrüßung via Legacy-Python (8779).
                let followup_relay = match BrokerRelay::new(&settings.broker) {
                    Ok(relay) => Some(relay),
                    Err(e) => {
                        tracing::warn!(
                            "BrokerRelay für OAuth-Followups nicht initialisierbar: {e} — \
                             Rollen-Sync/Display-Name entfallen"
                        );
                        None
                    }
                };
                let partner_setup = oauth_followups::build_partner_setup_service(
                    pool.clone(),
                    helix_client.clone(),
                    followup_relay,
                    chat_api_handle.as_ref().map(|h| h.api.clone()),
                );
                if partner_setup.is_none() {
                    tracing::warn!(
                        "PartnerSetupService nicht konstruierbar (TWITCH_INTERNAL_API_TOKEN fehlt) \
                         — OAuth-Followups entfallen"
                    );
                }
                raid_oauth_port = Some(Arc::new(raid_oauth_impl::TbRaidOAuthImpl::new(
                    pool.clone(),
                    tb_raid::state_store::StateStore::new(pool.clone(), raid_redirect_uri.clone()),
                    tb_raid::auth_writer::AuthWriter::new(pool.clone(), cipher.clone()),
                    Arc::new(HelixTokenClient {
                        helix: helix_client.clone(),
                        redirect_uri: raid_redirect_uri.clone(),
                    }),
                    client_id,
                    raid_redirect_uri.clone(),
                    partner_setup,
                )));
                tracing::info!(
                    "Raid-OAuth-Strecke nativ aktiv (auth-url/auth-state/block-state/go-url/oauth-callback inkl. Followups); requirements weiter via Proxy"
                );
            }

            let token_blacklist = Arc::new(TokenBlacklistStore::new(pool.clone()));
            let refresher = RaidTokenRefresher::new(
                pool.clone(),
                cipher.clone(),
                Arc::new(HelixTokenClient {
                    helix: helix_client.clone(),
                    redirect_uri: raid_redirect_uri.clone(),
                }),
                token_blacklist.clone(),
            );
            let token_provider = Arc::new(TokenProvider::new(
                RaidAuthStore::new(pool.clone(), cipher),
                refresher,
                token_blacklist,
            ));
            let pending = Arc::new(std::sync::Mutex::new(PendingRaidStore::new()));
            let executor = RaidExecutor::new(
                Arc::new(HelixRaidApi {
                    helix: helix_client.clone(),
                }),
                token_provider.clone(),
                RaidHistoryStore::new(pool.clone()),
            );
            let pipeline = AutoRaidPipeline::new(
                RaidBlacklistStore::new(pool.clone()),
                ScoreStore::new(pool.clone()),
                RaidHistoryStore::new(pool.clone()),
                StrikesStore::new(pool.clone()),
                executor,
                pending.clone(),
                Arc::new(ManagerArrivalReadiness {
                    manager: manager.clone(),
                }),
                Some(Arc::new(HelixFallbackStreams {
                    helix: helix_client.clone(),
                })),
                Some(OutreachBoostStore::new(pool.clone())),
            );
            let offline = Arc::new(OfflineRaidHandler::new(
                suppression.clone(),
                OfflineEligibilityStore::new(pool.clone()),
                live_state.clone(),
                PartnerRosterStore::new(pool.clone()),
                helix_client.clone(),
                followers.clone(),
                pipeline,
                &target_game,
            ));
            let sink = Arc::new(RaidArrivalSinkImpl::new(
                pool.clone(),
                pending.clone(),
                suppression.clone(),
                &target_game.to_lowercase(),
            ));

            // Orphan-Sweeper: promotet channel.chat.notification ohne
            // korrelierendes Raid-Event nach 15 s Grace als eigenständigen
            // Arrival (Python ruft promote_stale_* bei jedem Tracking-Tick).
            {
                let sweeper_sink = sink.clone();
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        sweeper_sink.promote_stale_orphans().await;
                    }
                });
            }

            // Periodischer Voll-Refresh aller Partner-Raid-Scores (Python
            // maybe_schedule_partner_raid_score_reconciliation, Intervall
            // 300 s): fängt Partner, deren Online/Offline-Events verpasst
            // wurden — sonst veralten deren Scores dauerhaft.
            {
                let refresh_pool = pool.clone();
                tokio::spawn(async move {
                    let resolver = ScoreRefreshResolver::new(refresh_pool.clone());
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let partners: Result<Vec<(String, String)>, _> = sqlx::query_as(
                            r#"
                            SELECT twitch_user_id, twitch_login
                            FROM twitch_partners_all_state
                            WHERE status = 'active'
                              AND COALESCE(twitch_user_id, '') <> ''
                            "#,
                        )
                        .fetch_all(&refresh_pool)
                        .await;
                        match partners {
                            Ok(pairs) if !pairs.is_empty() => {
                                match resolver.refresh_scores(&pairs, chrono::Utc::now()).await {
                                    Ok(written) => tracing::debug!(
                                        partners = pairs.len(),
                                        written,
                                        "Periodischer Partner-Score-Refresh abgeschlossen"
                                    ),
                                    Err(error) => tracing::error!(
                                        %error,
                                        "Periodischer Partner-Score-Refresh fehlgeschlagen"
                                    ),
                                }
                            }
                            Ok(_) => {}
                            Err(error) => tracing::error!(
                                %error,
                                "Partner-Liste für Score-Refresh nicht ladbar"
                            ),
                        }
                    }
                });
            }

            let arrival =
                RaidArrivalCoordinator::new(pool.clone(), pending, RaidArrivalRuntime::new(sink));
            let blacklist_guard = BlacklistRaidGuard::new(
                RaidBlacklistStore::new(pool.clone()),
                token_provider,
                helix_client,
            );
            manual_raid_port = Some(Arc::new(ManualRaidAdapter {
                handler: offline.clone(),
            }));
            tracing::info!(
                "Raid-EventSub-Hooks aktiv (Auto-Raid, Arrival, Score-Refresh, Blacklist-Guard)"
            );
            Arc::new(RaidEventSubHooks {
                manager: manager.clone(),
                score_resolver: ScoreRefreshResolver::new(pool.clone()),
                live_state: live_state.clone(),
                offline,
                side_effects: OfflineSideEffects::new(pool.clone()),
                arrival,
                guard: blacklist_guard,
            })
        }
        (Some(manager), _, cipher) => {
            if let Err(error) = cipher {
                tracing::warn!(
                    %error,
                    "DB_MASTER_KEY_V1 fehlt/ungültig — Raid-Hooks deaktiviert, nur Go-Live-Subscription aktiv"
                );
            }
            Arc::new(SubscriptionEventSubHooks {
                manager: manager.clone(),
            })
        }
        _ => Arc::new(NoopEventSubHooks),
    };
    // Welle B Phase 2: Pipeline auf der gebooteten ChatApi aufbauen. Wrappt
    // die Hooks, damit channel.chat.message in die tb-chat-Pipeline läuft;
    // startet Token-Loop, Promo-Loop, Global-Ban-Sweeper und den
    // 30-min-Subscription-Reconcile.
    let eventsub_hooks: Arc<dyn EventSubHooks> = match chat_api_handle {
        Some(handle) => {
            // !clip: Broadcaster-Token-Clip-Port, nur mit Helix + Krypto-Key.
            let clip_port = chat_wiring::build_clip_port(
                helix.as_ref().clone().map(Arc::new),
                FieldCipher::from_env().ok().map(Arc::new),
                pool.clone(),
            );
            let runtime = chat_wiring::build_runtime(
                handle,
                pool.clone(),
                manual_raid_port.clone(),
                clip_port,
                eventsub_hooks.clone(),
            )
            .await;
            runtime.start_background(subscription_manager.clone());
            runtime.hooks.clone()
        }
        None => eventsub_hooks,
    };
    // Go-Live-Enrichment: gezielter /channels-Lookup beim stream.online-Event
    // (sprachfilter-frei) — nur mit HelixClient verfügbar.
    let channel_info_source: Option<Arc<dyn ChannelInfoSource>> = helix
        .as_ref()
        .clone()
        .map(|client| Arc::new(HelixStreamSource { helix: client }) as Arc<dyn ChannelInfoSource>);
    let handler = Arc::new(MonitoringEventHandler::new(
        guard.clone(),
        live_state.clone(),
        tracker.clone(),
        telemetry.clone(),
        eventsub_hooks.clone(),
        channel_info_source,
        Arc::new(tb_monitoring::epoch_clock),
    ));
    let inbox = InboxRuntime::new(
        tb_monitoring::ProcessingInboxStore::new(pool.clone()),
        handler,
    )
    .start();
    let dispatcher = Arc::new(EventSubDispatcher::new(
        guard.clone(),
        inbox.enqueuer(),
        telemetry,
        eventsub_hooks,
        Arc::new(tb_monitoring::epoch_clock),
    ));

    // Nativer EventSub-Webhook-Empfänger (12.6.): eigener Loopback-Listener,
    // Caddy proxyt /twitch/eventsub/callback hierher — ersetzt die
    // Python-Bridge-Strecke (8765 → HTTP-Hop → 8776), die Notifications auf
    // stillen Pfaden verlor. Signatur = Auth; Dedup = persistenter Guard.
    if let Ok(secret) = std::env::var("TWITCH_WEBHOOK_SECRET") {
        let secret = secret.trim().to_string();
        if !secret.is_empty() {
            let receiver_port: u16 = std::env::var("TB_EVENTSUB_RECEIVER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8786);
            let receiver =
                Arc::new(tb_monitoring::WebhookReceiver::new(secret, dispatcher.clone()));
            let router = receiver.router();
            tokio::spawn(async move {
                let addr = SocketAddr::from(([127, 0, 0, 1], receiver_port));
                match tokio::net::TcpListener::bind(addr).await {
                    Ok(listener) => {
                        tracing::info!("EventSub-Webhook-Empfänger lauscht auf {addr}");
                        if let Err(e) = axum::serve(listener, router).await {
                            tracing::error!("EventSub-Webhook-Empfänger beendet: {e}");
                        }
                    }
                    Err(e) => {
                        tracing::error!("EventSub-Webhook-Empfänger: Bind-Fehler {addr}: {e}")
                    }
                }
            });
        }
    }

    // Poll-Loop: das Cutover-Gate. Default AUS — Python bleibt alleiniger
    // Live-Writer, bis der Flip (04-cutover-plan) explizit erfolgt.
    let poll_enabled = std::env::var("TB_MONITORING_POLL_ENABLED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false);
    let _poll_stop = if poll_enabled {
        match helix.as_ref().clone() {
            Some(helix_client) => {
                let notify_channel_id: i64 = std::env::var("TWITCH_NOTIFY_CHANNEL_ID")
                    .ok()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                let sink: Arc<dyn AnnouncementSink> = if notify_channel_id > 0 {
                    match BrokerRelay::new(&settings.broker) {
                        Ok(relay) => {
                            let vod: Arc<dyn VodPreviewSource> = Arc::new(HelixVodPreview {
                                helix: helix_client.clone(),
                            });
                            Arc::new(BrokerAnnouncementSink::new(
                                Arc::new(BrokerAnnouncementTransport { relay }),
                                AnnounceConfigStore::new(pool.clone()),
                                vod,
                                AnnouncementSettings {
                                    notify_channel_id,
                                    alert_mention: std::env::var("TWITCH_ALERT_MENTION").ok(),
                                    ref_code: std::env::var("TWITCH_DISCORD_REF_CODE").ok(),
                                    target_game: target_game.clone(),
                                },
                            ))
                        }
                        Err(e) => {
                            tracing::warn!(
                                "BrokerRelay nicht initialisierbar: {e} — Announcements aus"
                            );
                            Arc::new(NoopAnnouncementSink)
                        }
                    }
                } else {
                    tracing::info!("TWITCH_NOTIFY_CHANNEL_ID fehlt — Announcements aus");
                    Arc::new(NoopAnnouncementSink)
                };
                let poll_hooks: Arc<dyn PollHooks> = match &subscription_manager {
                    Some(manager) => Arc::new(SubscriptionPollHooks {
                        manager: manager.clone(),
                    }),
                    None => Arc::new(tb_monitoring::NoopPollHooks),
                };
                let language_filters: Vec<String> = language_filters_from_env();
                let source: Arc<dyn StreamSource> = Arc::new(HelixStreamSource {
                    helix: helix_client,
                });
                let engine = Arc::new(PollEngine::new(
                    source,
                    TrackedStore::new(pool.clone()),
                    live_state,
                    SessionStore::new(pool.clone()),
                    tracker,
                    StatsStore::new(pool.clone()),
                    guard,
                    sink,
                    poll_hooks,
                    PollIntervalStore::new(pool.clone()),
                    PollConfig {
                        target_game,
                        language_filters,
                        ..PollConfig::default()
                    },
                ));
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                tokio::spawn(engine.run(stop_rx));
                tracing::info!("Monitoring-Poll-Loop gestartet (Cutover-Gate aktiv)");
                Some(stop_tx)
            }
            None => {
                tracing::error!("TB_MONITORING_POLL_ENABLED=1, aber kein HelixClient — Poll aus");
                None
            }
        }
    } else {
        tracing::info!("Poll-Loop deaktiviert (TB_MONITORING_POLL_ENABLED != 1)");
        None
    };

    // Clip-Fetch-Task: gebaut aber standardmäßig deaktiviert (TB_CLIP_FETCHER_ENABLED≠1).
    // Setzt Helix-Client voraus — ohne ihn kein Start, auch wenn Env-Var gesetzt.
    if let Some(ref h) = *helix {
        tb_social_media::build_clip_fetch_task(pool.clone(), std::sync::Arc::new(h.clone()))
            .start_if_enabled();
    }

    // Scout-Task: entdeckt live Deadlock-Streamer und registriert sie als monitoring-only.
    // Deaktiviert bis TB_SCOUT_ENABLED=1 gesetzt ist.
    if let Some(ref h) = *helix {
        let scout_game =
            std::env::var("TWITCH_TARGET_GAME_NAME").unwrap_or_else(|_| "Deadlock".to_string());
        let scout_lang_filters: Vec<String> = language_filters_from_env();
        tb_monitoring::build_scout_task(
            pool.clone(),
            std::sync::Arc::new(h.clone()),
            scout_game,
            scout_lang_filters,
        )
        .start_if_enabled();
    }

    // Streamer-Link-Matcher: verknüpft neue Twitch-Partner mit ihrem Discord-Account.
    // Läuft alle 6h, ist still wenn keine neuen Kandidaten vorhanden.
    if let Ok(sl_relay) = BrokerRelay::new(&settings.broker) {
        let sl_config = Arc::new(streamer_link::StreamerLinkConfig::from_env());
        let sl_pool = pool.clone();
        let sl_base = format!("http://127.0.0.1:{port}");
        let sl_token = settings.internal_api.token.clone();
        tokio::spawn(streamer_link::streamer_link_task(
            sl_pool,
            sl_relay,
            sl_config,
            sl_base,
            sl_token,
        ));
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let token = settings.internal_api.token.clone();
    let legacy_proxy = std::env::var("TB_INTERNAL_API_LEGACY_FALLBACK_URL")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .map(|url| {
            tracing::info!("Legacy-Fallback aktiv: unbekannte interne-API-Routen → {url}");
            Arc::new(tb_internal_api::LegacyProxy::new(url))
        });
    // EventSub-Sektion von GET /stats: Live-`current`-Snapshot aus dem nativen
    // SubscriptionManager (Webhook-Modus). Ohne Manager (kein Helix) → None,
    // dann bleibt nur der DB-Capacity-Block (wie bisher).
    let eventsub_stats: Option<Arc<dyn tb_internal_api::EventSubStatsSource>> =
        subscription_manager.as_ref().map(|mgr| {
            Arc::new(eventsub_stats_adapter::ManagerEventSubStats::new(
                mgr.clone(),
                pool.clone(),
            )) as Arc<dyn tb_internal_api::EventSubStatsSource>
        });
    let app = build_internal_router(
        pool,
        token,
        helix,
        Some(dispatcher),
        manual_raid_port,
        raid_oauth_port,
        eventsub_stats,
        legacy_proxy,
    );

    tracing::info!("tb-bot lauscht auf {addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Bind-Fehler auf {addr}: {e}");
            std::process::exit(1);
        });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

/// Startup-Cleanup + periodischer Core-Sub-Reconcile (alle 6h).
///
/// Python-Äquivalent: `_cleanup_old_eventsub_subscriptions` (Webhook-Start)
/// + `_subscribe_core_eventsub_webhooks` (per-Kanal bei Webhook-Start).
///
/// Läuft einmal beim Start und danach jede 6h:
/// 1. Aktive Partner-User-IDs aus DB laden
/// 2. `cleanup_stale` — veraltete Twitch-Subs für entfernte Partner löschen
/// 3. `ensure_core_subscriptions` — stream.online/offline/channel.update für
///    alle aktiven Partner sicherstellen (fängt neue Kanäle + revoked Subs)
async fn subscription_maintenance_loop(
    manager: std::sync::Arc<tb_monitoring::SubscriptionManager>,
    pool: sqlx::PgPool,
) {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
    let mut tick = tokio::time::interval(INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let rows: Vec<(String, String)> = match sqlx::query_as(
            "SELECT LOWER(twitch_login), twitch_user_id \
             FROM twitch_streamers_partner_state \
             WHERE is_partner_active = 1 AND COALESCE(twitch_user_id, '') <> '' \
             UNION \
             SELECT LOWER(twitch_login), twitch_user_id \
             FROM twitch_streamers \
             WHERE COALESCE(is_monitored_only, 0) = 1 AND COALESCE(twitch_user_id, '') <> ''",
        )
        .fetch_all(&pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("sub-maintenance: Partner-Query fehlgeschlagen: {e}");
                continue;
            }
        };

        let active_ids: std::collections::HashSet<String> =
            rows.iter().map(|(_, uid)| uid.clone()).collect();

        let deleted = manager.cleanup_stale(&active_ids).await;
        tracing::debug!(deleted, "sub-maintenance: Stale-Cleanup abgeschlossen");

        let mut ensured = 0usize;
        for (login, uid) in &rows {
            manager.ensure_core_subscriptions(uid, login).await;
            ensured += 1;
        }
        tracing::info!(
            kanäle = rows.len(),
            ensured,
            deleted,
            "sub-maintenance: Core-Sub-Reconcile abgeschlossen"
        );
    }
}
