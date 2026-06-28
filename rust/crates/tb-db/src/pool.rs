//! sqlx-`PgPool`-Aufbau aus `DbConfig` (ersetzt den Python-Eigenbau-LIFO-Pool).

use sqlx::postgres::{PgPool, PgPoolOptions};
use tb_config::DbConfig;

use crate::error::DbError;

/// Baut einen verbundenen Pool. `pool_max`/`acquire_timeout`/`connect_timeout`
/// kommen aus der Config.
pub async fn connect(cfg: &DbConfig) -> Result<PgPool, DbError> {
    // sqlx 0.8 hat keinen separaten PgConnectOptions-Connect-Timeout; der
    // Pool nutzt `acquire_timeout` auch als Deadline für neue Verbindungen.
    let connection_deadline = cfg.acquire_timeout.min(cfg.connect_timeout);
    let pool = PgPoolOptions::new()
        .max_connections(cfg.pool_max)
        .acquire_timeout(connection_deadline)
        .connect(&cfg.dsn)
        .await?;
    Ok(pool)
}
