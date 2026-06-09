//! sqlx-native Migrationen. Baseline = bestehendes Prod-Schema (in `rust/migrations/`
//! liegen vorerst KEINE .sql-Dateien). `run_migrations` legt nur `_sqlx_migrations`
//! an und wendet nichts an, solange es keine Migration gibt — bestehende Tabellen
//! bleiben unangetastet. Getrennt von Python-`schema_version`.

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
