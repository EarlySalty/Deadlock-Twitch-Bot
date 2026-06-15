//! sqlx-native Migrationen mit Tracking-Tabelle `_sqlx_migrations` (getrennt von Pythons `schema_version`).
//!
//! Seit F1 enthält `rust/migrations/` das vollständige Ziel-Schema als clean, idempotente Baseline.
//! Eine frische, leere DB wird allein durch `run_migrations` vollständig aufgesetzt.
//!
//! Alle Statements sind `IF NOT EXISTS` / `CREATE OR REPLACE` / guarded — gegen das bestehende Prod-Schema ein No-op (kein Schema-Bruch im Parallelbetrieb, ADR-0002).
//! Timescale-DDL läuft als raw SQL; die `timescaledb`-Extension muss im geteilten Schema vorinstalliert sein.

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPool;

use crate::error::DbError;

/// Eingebettete Migrationen aus dem Workspace-Verzeichnis `rust/migrations/`.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Führt ausstehende Migrationen aus (Phase 0b: keine → no-op außer Tracking-Tabelle).
pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    MIGRATOR.run(pool).await?;
    Ok(())
}
