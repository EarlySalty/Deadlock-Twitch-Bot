//! tb-bot — Internes API-Binary auf Port 8776.
//!
//! Bindet ausschließlich auf 127.0.0.1 (Loopback). UFW blockt 8776 extern.
//! Auth: X-Internal-Token + loopback_only-Layer (Defense-in-Depth).
//!
//! Env-Variablen:
//!   TWITCH_ANALYTICS_DSN       — PostgreSQL-DSN
//!   TWITCH_INTERNAL_API_TOKEN  — Auth-Token
//!   TWITCH_CLIENT_ID           — Twitch Helix Client-ID (optional)
//!   TWITCH_CLIENT_SECRET       — Twitch Helix Client-Secret (optional)
//!   PORT                       — optional, default 8776

use std::net::SocketAddr;
use std::sync::Arc;
use tb_config::Settings;
use tb_internal_api::build_internal_router;
use tb_monitoring::{
    EventSubDispatcher, ExpSessionStore, ExpSessionTracker, GuardStore, InboxRuntime,
    LiveStateStore, MonitoringEventHandler, NoFollowerSource, NoopEventSubHooks, SessionTracker,
    TelemetryStore,
};
use tb_monitoring::sessions::store::SessionStore;
use tb_transport_twitch::{HelixClient, HelixConfig};

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
            (Some(id), Some(secret)) => {
                match HelixClient::new(HelixConfig::new(id, secret)) {
                    Ok(c) => {
                        tracing::info!("HelixClient initialisiert");
                        Arc::new(Some(c))
                    }
                    Err(e) => {
                        tracing::warn!("HelixClient-Initialisierung fehlgeschlagen: {e}");
                        Arc::new(None)
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET fehlen — Helix-API deaktiviert"
                );
                Arc::new(None)
            }
        }
    };

    // EventSub-Ingress: Inbox-Worker + Dispatcher (Hooks Noop bis 4f).
    // Der Bridge-Vertrag POST /eventsub/dispatch ist damit voll bedienbar;
    // Außenwirkung (Raid, Announcements) bleibt bis zum Cutover aus.
    let target_game =
        std::env::var("TWITCH_TARGET_GAME_NAME").unwrap_or_else(|_| "Deadlock".to_string());
    let guard = GuardStore::new(pool.clone());
    let telemetry = TelemetryStore::new(pool.clone());
    let live_state = LiveStateStore::new(pool.clone());
    let tracker = Arc::new(SessionTracker::new(
        SessionStore::new(pool.clone()),
        live_state.clone(),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        Arc::new(NoFollowerSource),
        &target_game,
    ));
    tracker.rehydrate().await;
    let eventsub_hooks = Arc::new(NoopEventSubHooks);
    let handler = Arc::new(MonitoringEventHandler::new(
        guard.clone(),
        live_state,
        tracker,
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
        guard,
        inbox.enqueuer(),
        telemetry,
        eventsub_hooks,
        Arc::new(tb_monitoring::epoch_clock),
    ));

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
