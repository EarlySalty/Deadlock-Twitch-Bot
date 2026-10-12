//! Einstiegspunkt des Discord-Hubs.
include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

use std::sync::Arc;

use tb_discord_hub::config::HubConfig;
use tb_discord_hub::forward::BrokerForwarder;
use tb_discord_hub::gateway::{self, GatewayConfig, LoggingSink};
use tb_discord_hub::rest::DiscordRest;
use tb_discord_hub::server::{router, AppState};
use tb_discord_hub::{BrokerForward, DirectDiscord, GuildEventSink};

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "Ctrl-C-Handler konnte nicht installiert werden");
            std::future::pending::<()>().await;
        }
    };
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "SIGTERM-Handler konnte nicht installiert werden");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("Beende Discord-Hub");
}

#[tokio::main]
async fn main() -> Result<(), String> {
    if print_build_revision() {
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = HubConfig::from_env().map_err(|e| format!("Konfiguration: {e}"))?;
    tracing::info!(?config, "Discord-Hub startet");

    let forward: Arc<dyn BrokerForward> = Arc::new(
        BrokerForwarder::new(&config.broker_base_url, &config.broker_token)
            .map_err(|e| format!("Broker-Weiterleitung: {e}"))?,
    );

    let mut gateway_task = None;
    let direct: Option<Arc<dyn DirectDiscord>> = match config.bot_token.as_deref() {
        Some(bot_token) => {
            let rest = DiscordRest::new(bot_token).map_err(|e| format!("Discord-REST: {e}"))?;
            let sink: Arc<dyn GuildEventSink> = Arc::new(LoggingSink);
            gateway_task = Some(gateway::spawn(
                GatewayConfig {
                    bot_token: bot_token.to_string(),
                    application_id: config.application_id,
                    community_guild_id: config.community_guild_id,
                },
                sink,
            ));
            Some(Arc::new(rest))
        }
        None => {
            tracing::warn!(
                "DISCORD_HUB_BOT_TOKEN fehlt: nur Weiterleitung an den Broker, kein Gateway/REST"
            );
            None
        }
    };

    let state = AppState::new(
        &config.internal_token,
        config.community_guild_id,
        forward,
        direct,
    );
    let bind = config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("Bind auf {bind} fehlgeschlagen: {e}"))?;
    tracing::info!(addr = %bind, "Discord-Hub lauscht");

    let result = axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("HTTP-Server: {e}"));

    if let Some(task) = gateway_task {
        task.abort();
        let _ = task.await;
    }
    tracing::info!("Discord-Hub beendet");
    result
}
