//! sqlx-`PgPool`-Aufbau aus `DbConfig` (ersetzt den Python-Eigenbau-LIFO-Pool).

use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};
use tb_config::DbConfig;

use crate::error::DbError;

/// Baut einen verbundenen Pool. `pool_max`/`acquire_timeout`/`connect_timeout`
/// kommen aus der Config.
pub async fn connect(cfg: &DbConfig) -> Result<PgPool, DbError> {
    // sqlx 0.8 hat keinen separaten PgConnectOptions-Connect-Timeout; der
    // Pool nutzt `acquire_timeout` auch als Deadline für neue Verbindungen.
    let connection_deadline = cfg.acquire_timeout.min(cfg.connect_timeout);
    // Ein Teil des Pools bleibt dauerhaft offen. Eine frisch aufgebaute
    // Verbindung hat einen leeren Anweisungs-Zwischenspeicher, deshalb muss
    // Postgres jede Abfrage neu planen; bei den Analytics-Abfragen ueber die
    // Zeitreihen-Tabellen kostet allein das Planen mehrere Sekunden, sobald
    // eine Verbindung zwischendurch geschlossen wurde.
    let warm = cfg.pool_max.min(4);
    let pool = PgPoolOptions::new()
        .max_connections(cfg.pool_max)
        .min_connections(warm)
        .idle_timeout(Some(Duration::from_secs(30 * 60)))
        .max_lifetime(Some(Duration::from_secs(4 * 60 * 60)))
        .acquire_timeout(connection_deadline)
        .connect(&cfg.dsn)
        .await?;
    Ok(pool)
}
