//! sqlx-`PgPool`-Aufbau aus `DbConfig` (ersetzt den Python-Eigenbau-LIFO-Pool).

use sqlx::postgres::{PgPool, PgPoolOptions};
use tb_config::DbConfig;

use crate::error::DbError;

/// Baut einen verbundenen Pool. `pool_max`/`acquire_timeout` aus der Config.
pub async fn connect(cfg: &DbConfig) -> Result<PgPool, DbError> {
    let pool = PgPoolOptions::new()
        .max_connections(cfg.pool_max)
        .acquire_timeout(cfg.acquire_timeout)
        .connect(&cfg.dsn)
        .await?;
    Ok(pool)
}
