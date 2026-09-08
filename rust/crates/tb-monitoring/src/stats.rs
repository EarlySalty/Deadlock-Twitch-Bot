//! Time-Series-Logging in `twitch_stats_tracked` / `twitch_stats_category`
//! (PK-lose Insert-only-Tabellen, Schema-Vertrag — kein Dedup, bewusst wie
//! Python). `ts_utc` ist timestamptz, `is_partner` ist BOOLEAN (Prod-Schema).

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Ein Stats-Sample (Python: `_log_stats`-Row).
#[derive(Debug, Clone)]
pub struct StatsSample {
    pub twitch_user_id: String,
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
            "INSERT INTO {table} (ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags, language, twitch_user_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
        );
        let mut tx = self.pool.begin().await?;
        for row in rows {
            // dyn: Tabellenname ist der ausgewählte Stats-Sink (`tracked` oder `category`).
            sqlx::query(&sql)
                .bind(ts)
                .bind(&row.streamer)
                .bind(row.viewer_count)
                .bind(row.is_partner)
                .bind(&row.game_name)
                .bind(&row.stream_title)
                .bind(&row.tags)
                .bind(&row.language)
                .bind(&row.twitch_user_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stats_speichern_plattform_id_und_sprache_in_beiden_tabellen() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-database-url.txt");
        let Ok(dsn) = std::fs::read_to_string(path) else {
            eprintln!("SKIP: Testdatenbank fehlt ({path})");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn.trim())
            .await
            .unwrap();
        sqlx::query("DROP SCHEMA IF EXISTS onboarding_stats_ids CASCADE")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE SCHEMA onboarding_stats_ids")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("SET search_path TO onboarding_stats_ids")
            .execute(&pool)
            .await
            .unwrap();
        for table in ["twitch_stats_tracked", "twitch_stats_category"] {
            sqlx::query(&format!("CREATE TABLE {table} (ts_utc timestamptz, streamer text, viewer_count int, is_partner boolean, game_name text, stream_title text, tags text, language text, twitch_user_id text)"))
                .execute(&pool).await.unwrap();
        }
        let store = StatsStore::new(pool.clone());
        let sample = StatsSample {
            twitch_user_id: "123456".into(),
            streamer: "umbenannt".into(),
            viewer_count: 7,
            is_partner: false,
            game_name: Some("Deadlock".into()),
            stream_title: None,
            tags: None,
            language: Some("de".into()),
        };
        store
            .log_category(Utc::now(), std::slice::from_ref(&sample))
            .await
            .unwrap();
        store.log_tracked(Utc::now(), &[sample]).await.unwrap();
        for table in ["twitch_stats_tracked", "twitch_stats_category"] {
            let row: (String, String) =
                sqlx::query_as(&format!("SELECT twitch_user_id, language FROM {table}"))
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(row, ("123456".into(), "de".into()));
        }
    }
}
