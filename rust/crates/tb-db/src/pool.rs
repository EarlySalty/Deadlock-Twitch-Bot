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
    // Zwei Verbindungen bleiben offen stehen, statt nach zehn Minuten Ruhe
    // abgeraeumt zu werden. Eine wiederverwendete Verbindung behaelt ihre
    // vorbereiteten Abfragen, eine neue faengt bei null an und laesst Postgres
    // jede Abfrage neu planen; ueber den Zeitreihen-Tabellen ist allein das
    // Planen teuer. Eine Zusicherung, dass die offenen Verbindungen immer warm
    // sind, ist das nicht: nach `max_lifetime` ersetzt der Pool eine Verbindung
    // durch eine frische.
    //
    // Zwei und nicht mehr, weil `connect` diese Verbindungen beim Start
    // nacheinander innerhalb von `acquire_timeout` aufbauen muss.
    let dauerhaft_offen = cfg.pool_max.min(2);
    let pool = PgPoolOptions::new()
        .max_connections(cfg.pool_max)
        .min_connections(dauerhaft_offen)
        .idle_timeout(Some(Duration::from_secs(30 * 60)))
        .acquire_timeout(connection_deadline)
        .connect(&cfg.dsn)
        .await?;
    Ok(pool)
}
