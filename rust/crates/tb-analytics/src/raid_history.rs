//! Daten-Layer für das Admin-Raid-History-Listing (`GET /twitch/raid/history`).
//!
//! Port von `bot/raid/mixin.py:_dashboard_raid_history_sync` (Z.28-61): die volle
//! 11-Spalten-Raid-Historie mit optionalem Broadcaster-Filter und Limit. Im
//! Gegensatz zum öffentlichen `recent_raids` (4 Spalten, nur Erfolge, Limit 10)
//! liefert dies auch fehlgeschlagene Raids samt `error_message`,
//! `candidates_count`, `stream_duration_sec` und `target_stream_started_at`.

use serde_json::{json, Value};
use sqlx::PgPool;

/// Eine Zeile aus `twitch_raid_history` (11 Spalten).
///
/// Zeitstempel werden als `::text` gelesen (clean-SQL, kein Mischvergleich).
#[derive(Debug, sqlx::FromRow)]
struct RaidHistoryFullRow {
    from_broadcaster_id: Option<String>,
    from_broadcaster_login: Option<String>,
    to_broadcaster_id: Option<String>,
    to_broadcaster_login: Option<String>,
    viewer_count: Option<i32>,
    stream_duration_sec: Option<i32>,
    executed_at: Option<String>,
    success: Option<bool>,
    error_message: Option<String>,
    target_stream_started_at: Option<String>,
    candidates_count: Option<i32>,
}

impl RaidHistoryFullRow {
    fn into_json(self) -> Value {
        json!({
            "fromBroadcasterId": self.from_broadcaster_id,
            "fromBroadcasterLogin": self.from_broadcaster_login,
            "toBroadcasterId": self.to_broadcaster_id,
            "toBroadcasterLogin": self.to_broadcaster_login,
            "viewerCount": self.viewer_count.unwrap_or(0),
            "streamDurationSec": self.stream_duration_sec,
            "executedAt": self.executed_at,
            "success": self.success.unwrap_or(false),
            "errorMessage": self.error_message,
            "targetStreamStartedAt": self.target_stream_started_at,
            "candidatesCount": self.candidates_count.unwrap_or(0),
        })
    }
}

/// Normalisiert das `limit` auf den Python-Default 50, geklemmt auf 1..=500.
fn normalize_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 500)
}

/// Lädt die Raid-Historie (optional gefiltert nach `from_broadcaster_login`,
/// case-insensitiv) mit Limit. `from_broadcaster` leer/None → keine Filterung.
pub async fn load_raid_history(
    pool: &PgPool,
    from_broadcaster: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<Value>, sqlx::Error> {
    let limit = normalize_limit(limit);
    let filter = from_broadcaster
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());

    let rows: Vec<RaidHistoryFullRow> = if let Some(login) = filter {
        sqlx::query_as(
            r#"
            SELECT from_broadcaster_id, from_broadcaster_login,
                   to_broadcaster_id, to_broadcaster_login,
                   viewer_count, stream_duration_sec, executed_at::text AS executed_at,
                   success, error_message,
                   target_stream_started_at::text AS target_stream_started_at,
                   candidates_count
            FROM twitch_raid_history
            WHERE LOWER(from_broadcaster_login) = $1
            ORDER BY executed_at DESC
            LIMIT $2
            "#,
        )
        .bind(login)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT from_broadcaster_id, from_broadcaster_login,
                   to_broadcaster_id, to_broadcaster_login,
                   viewer_count, stream_duration_sec, executed_at::text AS executed_at,
                   success, error_message,
                   target_stream_started_at::text AS target_stream_started_at,
                   candidates_count
            FROM twitch_raid_history
            ORDER BY executed_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.into_iter().map(RaidHistoryFullRow::into_json).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("create schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");
        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_history (
                id                       BIGSERIAL PRIMARY KEY,
                from_broadcaster_id      TEXT,
                from_broadcaster_login   TEXT,
                to_broadcaster_id        TEXT,
                to_broadcaster_login     TEXT,
                viewer_count             INTEGER DEFAULT 0,
                stream_duration_sec      INTEGER,
                reason                   TEXT,
                executed_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success                  BOOLEAN DEFAULT TRUE,
                error_message            TEXT,
                target_stream_started_at TIMESTAMPTZ,
                candidates_count         INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_raid_history");
        pool
    }

    async fn seed(pool: &PgPool) {
        sqlx::query(
            r#"
            INSERT INTO twitch_raid_history
                (from_broadcaster_id, from_broadcaster_login, to_broadcaster_id,
                 to_broadcaster_login, viewer_count, stream_duration_sec, executed_at,
                 success, error_message, target_stream_started_at, candidates_count)
            VALUES
                ('1', 'alice', '2', 'bob',   100, 3600, NOW() - INTERVAL '3 hours',
                 TRUE,  NULL,           NOW() - INTERVAL '5 hours', 7),
                ('1', 'ALICE', '3', 'carol', 50,  1800, NOW() - INTERVAL '1 hour',
                 FALSE, 'no target',    NULL,                        3),
                ('9', 'dave',  '4', 'erin',  20,  900,  NOW(),
                 TRUE,  NULL,           NOW() - INTERVAL '2 hours', 1)
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn liefert_alle_spalten_und_neueste_zuerst() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_raidhist_all").await;
        seed(&pool).await;

        let rows = load_raid_history(&pool, None, None).await.unwrap();
        assert_eq!(rows.len(), 3);
        // Neuester zuerst: dave
        assert_eq!(rows[0]["fromBroadcasterLogin"], "dave");
        // 11 Felder vorhanden, inkl. der 4 vom öffentlichen Endpoint fehlenden.
        let failed = rows.iter().find(|r| r["success"] == false).unwrap();
        assert_eq!(failed["fromBroadcasterLogin"], "ALICE");
        assert_eq!(failed["errorMessage"], "no target");
        assert_eq!(failed["candidatesCount"], 3);
        assert!(failed["targetStreamStartedAt"].is_null());
        // success-Zeile hat target_stream_started_at gesetzt + candidates_count
        let ok = rows.iter().find(|r| r["toBroadcasterLogin"] == "bob").unwrap();
        assert_eq!(ok["candidatesCount"], 7);
        assert!(ok["targetStreamStartedAt"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
        assert_eq!(ok["streamDurationSec"], 3600);
    }

    #[tokio::test]
    async fn broadcaster_filter_case_insensitiv_und_limit() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_raidhist_filter").await;
        seed(&pool).await;

        // 'alice' (case-insensitiv → matcht 'alice' + 'ALICE') = 2 Zeilen.
        let rows = load_raid_history(&pool, Some("Alice"), None).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r["fromBroadcasterLogin"].as_str().unwrap().to_lowercase() == "alice"));

        // limit=1 begrenzt auf eine Zeile.
        let limited = load_raid_history(&pool, Some("alice"), Some(1)).await.unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn limit_normalisierung() {
        assert_eq!(normalize_limit(None), 50);
        assert_eq!(normalize_limit(Some(0)), 1);
        assert_eq!(normalize_limit(Some(9999)), 500);
        assert_eq!(normalize_limit(Some(25)), 25);
    }
}
