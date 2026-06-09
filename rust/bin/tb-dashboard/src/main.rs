//! Minimaler HTTP-Server für das Analytics-Dashboard.
//!
//! Port: `DASHBOARD_PORT` Env-Variable, Default 8767.
//! DSN:  `TWITCH_ANALYTICS_DSN` (via tb-config).
//!
//! **Nicht automatisch starten** — Start ist user-gated (erfordert echtes DSN).

use std::net::SocketAddr;
use tb_config::Settings;
use tb_dashboard_api::build_router;

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

    // Startzeit-Timestamp so früh wie möglich setzen
    let _ = tb_dashboard_api::process_info::uptime_secs();

    let port: u16 = std::env::var("DASHBOARD_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8767);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let token = settings.internal_api.token.clone();
    let app = build_router(pool, token);

    tracing::info!("tb-dashboard lauscht auf {addr}");
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
