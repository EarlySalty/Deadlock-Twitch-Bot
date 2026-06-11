//! Time-Series-Logging in `twitch_stats_tracked` / `twitch_stats_category`
//! (PK-lose Insert-only-Tabellen, Schema-Vertrag — kein Dedup, bewusst wie
//! Python). `ts_utc` ist timestamptz, `is_partner` boolean (prod-verifiziert).

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Ein Stats-Sample (Python: `_log_stats`-Row).
#[derive(Debug, Clone)]
pub struct StatsSample {
    pub streamer: String,
    pub viewer_count: i32,
    pub is_partner: bool,
    pub game_name: Option<String>,
    pub stream_title: Option<String>,
    /// JSON-Array-Text (siehe `StreamSnapshot::tags_json`).
    pub tags: Option<String>,
    /// Helix-Stream-Sprache (ISO 639-1, z. B. "de"); Basis der DE-Markt-Sicht.
    pub language: Option<String>,
}

#[derive(Clone)]
pub struct StatsStore {
    pool: PgPool,
}

impl StatsStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Samples der Ziel-Kategorie-Streams (getrackte Streamer).
    pub async fn log_tracked(
        &self,
        ts: DateTime<Utc>,
        rows: &[StatsSample],
    ) -> Result<(), sqlx::Error> {
        self.insert_batch("twitch_stats_tracked", ts, rows).await
    }

    /// Samples aller Streams der Kategorie (Discovery-Sicht).
    pub async fn log_category(
        &self,
        ts: DateTime<Utc>,
        rows: &[StatsSample],
    ) -> Result<(), sqlx::Error> {
        self.insert_batch("twitch_stats_category", ts, rows).await
    }

    async fn insert_batch(
        &self,
        table: &str,
        ts: DateTime<Utc>,
        rows: &[StatsSample],
    ) -> Result<(), sqlx::Error> {
        if rows.is_empty() {
            return Ok(());
        }
        // Tabellenname kommt ausschließlich aus den beiden Konstanten oben.
        let sql = format!(
            "INSERT INTO {table} (ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags, language)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        );
        let mut tx = self.pool.begin().await?;
        for row in rows {
            sqlx::query(&sql)
                .bind(ts)
                .bind(&row.streamer)
                .bind(row.viewer_count)
                .bind(row.is_partner)
                .bind(&row.game_name)
                .bind(&row.stream_title)
                .bind(&row.tags)
                .bind(&row.language)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
