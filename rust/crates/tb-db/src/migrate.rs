//! sqlx-native Migrationen mit Tracking-Tabelle `_sqlx_migrations` (getrennt von Pythons `schema_version`).
//!
//! Seit F1 enthält `rust/migrations/` das vollständige Ziel-Schema als clean, idempotente Baseline.
//! Eine frische, leere DB wird allein durch `run_migrations` vollständig aufgesetzt.
//!
//! Alle Statements sind `IF NOT EXISTS` / `CREATE OR REPLACE` / guarded — gegen das bestehende Prod-Schema ein No-op (kein Schema-Bruch im Parallelbetrieb, ADR-0002).
//! Timescale-DDL läuft als raw SQL; die `timescaledb`-Extension muss im geteilten Schema vorinstalliert sein.
//!
//! # Schema-Architektur: Streamer vs. Partner (NICHT mischen)
//!
//! ```text
//! twitch_streamers          ← reine Identitäts-Tabelle
//!   twitch_login, twitch_user_id
//!   → KEIN is_partner-Flag hier
//!
//! twitch_partners           ← einzige Wahrheitsquelle für Partner-Status
//!   status (active/archived), admin_archived_at, raid_bot_enabled, …
//!
//! twitch_streamer_identities ← Discord-Verknüpfungen
//!   discord_user_id, discord_display_name, is_on_discord
//!
//! monitored-only             ← abgeleitet: Streamer ohne twitch_partners-Eintrag
//! ```
//!
//! Regel: Partner-Zugehörigkeit IMMER über `twitch_partners` oder die View prüfen,
//! nie über ein `is_partner`-Flag in `twitch_streamers`.

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPool;
use sqlx::Row;

use crate::error::DbError;

/// Eingebettete Migrationen aus dem Workspace-Verzeichnis `rust/migrations/`.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

pub const SCHEMA_OWNER_COMPONENT: &str = "analytics_schema";
pub const SCHEMA_OWNER_VALUE: &str = "rust";
pub const SCHEMA_OWNER_MARKER_VERSION: i32 = 1;

/// Führt ausstehende Migrationen aus (Phase 0b: keine → no-op außer Tracking-Tabelle).
pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    MIGRATOR.run(pool).await?;
    ensure_schema_owner_marker(pool).await?;
    Ok(())
}

pub async fn ensure_schema_owner_marker(pool: &PgPool) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO public.tb_schema_ownership (
            component,
            schema_owner,
            marker_version,
            updated_at,
            details_json
        )
        VALUES ($1, $2, $3, now(), '{"set_by":"tb-db"}'::jsonb)
        ON CONFLICT (component) DO NOTHING
        "#,
    )
    .bind(SCHEMA_OWNER_COMPONENT)
    .bind(SCHEMA_OWNER_VALUE)
    .bind(SCHEMA_OWNER_MARKER_VERSION)
    .execute(pool)
    .await?;

    let row = sqlx::query(
        r#"
        SELECT schema_owner, marker_version
        FROM public.tb_schema_ownership
        WHERE component = $1
        "#,
    )
    .bind(SCHEMA_OWNER_COMPONENT)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(DbError::Integrity(
            "Rust-Schema-Ownership-Marker fehlt nach Migration".to_string(),
        ));
    };

    let owner: String = row.try_get("schema_owner")?;
    let version: i32 = row.try_get("marker_version")?;
    if owner != SCHEMA_OWNER_VALUE || version != SCHEMA_OWNER_MARKER_VERSION {
        return Err(DbError::Integrity(format!(
            "Rust-Schema-Ownership-Marker unerwartet: owner={owner}, version={version}"
        )));
    }

    Ok(())
}
