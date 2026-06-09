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
//!   PORT                          — optional, default 8776

mod score_refresh;
mod wiring;

use std::net::SocketAddr;
use std::sync::Arc;
use tb_config::Settings;
use tb_internal_api::build_internal_router;
use tb_monitoring::poller::{PollHooks, StreamSource};
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
use tb_transport_discord::BrokerRelay;
use tb_transport_twitch::{HelixClient, HelixConfig};
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
    // Raid-/Score-Hooks bleiben bis zum Cutover (4f) Noop.
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
        followers,
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
    let eventsub_hooks: Arc<dyn EventSubHooks> = match &subscription_manager {
        Some(manager) => Arc::new(SubscriptionEventSubHooks {
            manager: manager.clone(),
        }),
        None => Arc::new(NoopEventSubHooks),
    };
    let handler = Arc::new(MonitoringEventHandler::new(
        guard.clone(),
        live_state.clone(),
        tracker.clone(),
        telemetry.clone(),
        eventsub_hooks.clone(),
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
                let language_filters: Vec<String> = std::env::var("TWITCH_LANGUAGE_FILTERS")
                    .map(|v| v.split(',').map(str::to_string).collect())
                    .unwrap_or_default();
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

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let token = settings.internal_api.token.clone();
    let app = build_internal_router(pool, token, helix, Some(dispatcher));

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
