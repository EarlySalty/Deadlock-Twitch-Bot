//! tb-category-collector — Dauerdienst, der die gesamte Twitch-Kategorie
//! Deadlock weltweit und sprachunabhängig beobachtet (Helix nur lesend,
//! Chat ausschließlich anonym justinfan).
//!
//! Konfigurationsherkunft:
//! - **DB-Zugang + Twitch-Credentials** kommen aus der bestehenden Infrastruktur
//!   (Infisical-Loader bzw. `--config`-Datei im KEY=VALUE-Format für manuelle
//!   Läufe). Gelesen werden nur `TWITCH_ANALYTICS_DSN`, `TWITCH_CLIENT_ID`,
//!   `TWITCH_CLIENT_SECRET`.
//! - **Sammelverhalten** (Intervalle, Retention, Toggles, game_id) lebt
//!   ausschließlich in `category_collector_config` (Postgres, Zeile 1) —
//!   bewusst keine ENV-Optionen.
//!
//! Der Dienst migriert niemals selbst (`TB_DB_MIGRATE=0` bleibt außen vor:
//! Migrationen laufen als postgres über `deadlock-twitch-migrate.service`).

use std::path::PathBuf;

use tb_category_collector::collector::SammlerDienst;
use tb_config::DbConfig;
use tb_transport_twitch::client::{HelixClient, HelixConfig};

const DSN_ENV: &str = "TWITCH_ANALYTICS_DSN";
const CLIENT_ID_ENV: &str = "TWITCH_CLIENT_ID";
const CLIENT_SECRET_ENV: &str = "TWITCH_CLIENT_SECRET";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Some(config_datei) = cli_config_pfad() {
        lade_config_datei(&config_datei);
    }

    let Some(dsn) = nicht_leer(DSN_ENV) else {
        tracing::error!("{DSN_ENV} fehlt — Datenbankzugang nicht konfiguriert");
        std::process::exit(2);
    };
    let (Some(client_id), Some(client_secret)) = (nicht_leer(CLIENT_ID_ENV), nicht_leer(CLIENT_SECRET_ENV))
    else {
        tracing::error!("{CLIENT_ID_ENV}/{CLIENT_SECRET_ENV} fehlen — Helix-Zugang nicht konfiguriert");
        std::process::exit(2);
    };

    let db_config = DbConfig {
        dsn,
        pool_max: 6,
        acquire_timeout: std::time::Duration::from_secs(5),
        connect_timeout: std::time::Duration::from_secs(5),
    };
    let pool = match tb_db::connect(&db_config).await {
        Ok(pool) => pool,
        Err(error) => {
            tracing::error!(%error, "Datenbankverbindung fehlgeschlagen");
            std::process::exit(3);
        }
    };
    let helix = match HelixClient::new(HelixConfig::new(client_id, client_secret)) {
        Ok(helix) => helix,
        Err(error) => {
            tracing::error!(%error, "Helix-Client nicht erstellbar");
            std::process::exit(4);
        }
    };

    let dienst = match SammlerDienst::new(pool, helix).await {
        Ok(dienst) => dienst,
        Err(error) => {
            tracing::error!(%error, "Sammler nicht startbar (Schema/Migration vorhanden?)");
            std::process::exit(5);
        }
    };
    tracing::info!("Kategoriesammler startet (Deadlock-Kategorie, anonym, nur lesend)");
    dienst.run().await;
}

fn cli_config_pfad() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let index = args.iter().position(|arg| arg == "--config")?;
    let pfad = args.get(index + 1).cloned();
    if pfad.is_none() {
        tracing::error!("--config braucht einen Pfad");
        std::process::exit(2);
    }
    pfad.map(PathBuf::from)
}

/// Liest KEY=VALUE-Zeilen in die Prozessumgebung (nur Lücken füllen, nie
/// bestehende Werte überschreiben). Für manuelle Läufe ohne Infisical-Loader.
fn lade_config_datei(pfad: &std::path::Path) {
    let inhalt = match std::fs::read_to_string(pfad) {
        Ok(inhalt) => inhalt,
        Err(error) => {
            tracing::error!(pfad = %pfad.display(), %error, "Config-Datei nicht lesbar");
            std::process::exit(2);
        }
    };
    for zeile in inhalt.lines() {
        let zeile = zeile.trim();
        if zeile.is_empty() || zeile.starts_with('#') {
            continue;
        }
        let Some((key, value)) = zeile.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

fn nicht_leer(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}
