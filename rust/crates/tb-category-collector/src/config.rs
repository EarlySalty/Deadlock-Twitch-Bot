//! Sammler-Konfiguration aus Postgres, ohne ENV-Verhaltensoptionen.
//! Rohchat und Snapshots haben keine automatische Aufbewahrungsfrist.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SammlerKonfig {
    pub enabled: bool,
    pub deadlock_game_id: String,
    pub discovery_interval: Duration,
    pub chat_enabled: bool,
    pub roster_decay: Duration,
    /// Immer None: Zeitfenster im Dashboard begrenzen nicht die Speicherung.
    pub retention_days: Option<i32>,
    pub rollup_enabled: bool,
    pub vod_metadata_enabled: bool,
    pub snapshot_retention_days: Option<i32>,
}

impl SammlerKonfig {
    /// Fehlende Konfiguration darf keinen neuen Sammellauf aktivieren.
    pub fn notfall_default() -> Self {
        Self {
            enabled: false,
            deadlock_game_id: String::new(),
            discovery_interval: Duration::from_secs(60),
            chat_enabled: false,
            roster_decay: Duration::from_secs(900),
            retention_days: None,
            rollup_enabled: true,
            vod_metadata_enabled: true,
            snapshot_retention_days: None,
        }
    }
}

pub async fn lade(pool: &sqlx::PgPool) -> Result<SammlerKonfig, sqlx::Error> {
    let (enabled, game_id, interval, chat, decay, rollup, vod): (
        bool,
        String,
        i32,
        bool,
        i32,
        bool,
        bool,
    ) = sqlx::query_as(
        "SELECT enabled, deadlock_game_id, discovery_interval_seconds, chat_enabled, \
                    roster_decay_seconds, rollup_enabled, vod_metadata_enabled \
               FROM category_collector_config WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    if game_id.trim().is_empty() {
        return Err(sqlx::Error::Protocol(
            "category_collector_config: game_id fehlt".into(),
        ));
    }
    Ok(SammlerKonfig {
        enabled,
        deadlock_game_id: game_id.trim().to_string(),
        discovery_interval: Duration::from_secs(interval.clamp(30, 3600) as u64),
        chat_enabled: chat,
        roster_decay: Duration::from_secs(decay.clamp(60, 86400) as u64),
        // Auch alte Konfigurationswerte dürfen keine Löschung reaktivieren.
        retention_days: None,
        rollup_enabled: rollup,
        vod_metadata_enabled: vod,
        snapshot_retention_days: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notfall_default_erhaelt_daten_und_aktiviert_keine_sammlung() {
        let cfg = SammlerKonfig::notfall_default();
        assert_eq!(cfg.retention_days, None);
        assert_eq!(cfg.snapshot_retention_days, None);
        assert!(!cfg.enabled && !cfg.chat_enabled);
        assert_eq!(cfg.discovery_interval, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn konfiguration_ohne_altersloeschung_auch_bei_altwerten() {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = &database.pool;
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918123000_category_collector.sql"
        ))
        .execute(pool)
        .await
        .unwrap();
        let cfg = lade(pool).await.unwrap();
        assert_eq!(
            cfg.retention_days, None,
            "auch ein alter 90-Tage-Wert ist wirkungslos"
        );
        assert_eq!(cfg.snapshot_retention_days, None);
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918161000_category_preserve_data.sql"
        ))
        .execute(pool)
        .await
        .unwrap();
        let preserved: bool = sqlx::query_scalar(
            "SELECT preserve_raw_data FROM category_collector_config WHERE id=1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(preserved);
        assert!(
            sqlx::query("UPDATE category_collector_config SET preserve_raw_data=FALSE WHERE id=1")
                .execute(pool)
                .await
                .is_err(),
            "Erhaltungsmodus ist verpflichtend"
        );
        sqlx::query("UPDATE category_collector_config SET snapshot_retention_days=30 WHERE id=1")
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            lade(pool).await.unwrap().snapshot_retention_days,
            None,
            "historische Konfigurationsfelder können keine Löschung aktivieren"
        );
    }
}
