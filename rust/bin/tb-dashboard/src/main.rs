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
    let mut app = build_router(pool.clone(), token);

    // Welle D: Strangler-Fallback-Proxy → Python (8765) für noch nicht
    // portierte Dashboard-Routen. Ohne konfigurierte URL bleibt der Proxy
    // aus und unbekannte Pfade antworten wie bisher mit 404.
    let fallback_url = std::env::var("TB_DASHBOARD_LEGACY_FALLBACK_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let proxy_ext = match &fallback_url {
        Some(url) => {
            app = app.fallback(tb_dashboard_api::proxy::dashboard_fallback_handler);
            tracing::info!("Strangler-Fallback-Proxy aktiv → {url}");
            tb_dashboard_api::proxy::DashboardProxyExt(Some(std::sync::Arc::new(
                tb_dashboard_api::proxy::DashboardLegacyProxy::new(url.clone()),
            )))
        }
        None => tb_dashboard_api::proxy::DashboardProxyExt(None),
    };
    app = app.layer(axum::Extension(proxy_ext));

    // Welle D: Session-Auth (Fernet) — Partner-/Admin-Level für native
    // v2-Routen. Ohne Key bleibt der Extractor fail-closed (Localhost/None).
    match tb_dashboard_api::DashboardAuthState::fernet_key_from_env() {
        Some(key) => {
            let auth_state = tb_dashboard_api::DashboardAuthState::new(pool.clone(), key);
            app = app.layer(axum::Extension(auth_state));
            tracing::info!("Dashboard-Session-Auth aktiv (Fernet-Key geladen)");
        }
        None => {
            tracing::warn!(
                "SESSIONS_ENCRYPTION_KEY fehlt — Partner-/Admin-Session-Auth deaktiviert"
            );
        }
    }

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
