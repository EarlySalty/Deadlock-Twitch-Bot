//! Query für `GET /twitch/api/admin/system/eventsub`.
//!
//! Liest die neueste Zeile aus `twitch_eventsub_capacity_snapshot`
//! mit `listener_count > 0 AND listeners_json IS NOT NULL`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Neuester EventSub-Snapshot aus der DB.
///
/// Die Capacity-Felder (`used_slots`/`total_slots`/`headroom_slots`) werden vom
/// Monitoring-Prozess beim Schreiben des Snapshots aus dem Live-WebSocket-State
/// abgeleitet (`bot/monitoring/eventsub_mixin.py`). Für den Dashboard-Prozess
/// ist dieser Snapshot die einzige verfügbare Quelle der Live-Kapazität — der
/// WebSocket-Listener läuft im Bot-Prozess, nicht hier.
#[derive(Debug)]
pub struct EventsubSnapshot {
    pub ts_utc: DateTime<Utc>,
    pub listener_count: i64,
    pub used_slots: i64,
    pub total_slots: i64,
    pub headroom_slots: i64,
    /// Roh-JSON-String — wird im Handler geparst (max 200 Einträge).
    pub listeners_json: String,
}

/// Lädt den neuesten EventSub-Snapshot mit Daten.
/// Gibt `None` zurück wenn keine passende Zeile vorhanden.
///
/// Slot-/Listener-Counts sind in Prod int4 → Cast auf bigint, damit sqlx i64
/// dekodieren kann.
pub async fn eventsub_snapshot(pool: &PgPool) -> Result<Option<EventsubSnapshot>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT
            ts_utc AS "ts_utc!",
            listener_count::bigint AS "listener_count!",
            COALESCE(used_slots, 0)::bigint AS "used_slots!",
            COALESCE(total_slots, 0)::bigint AS "total_slots!",
            COALESCE(headroom_slots, 0)::bigint AS "headroom_slots!",
            listeners_json AS "listeners_json!"
        FROM twitch_eventsub_capacity_snapshot
        WHERE listener_count > 0
          AND listeners_json IS NOT NULL
        ORDER BY ts_utc DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| EventsubSnapshot {
        ts_utc: row.ts_utc,
        listener_count: row.listener_count,
        used_slots: row.used_slots,
        total_slots: row.total_slots,
        headroom_slots: row.headroom_slots,
        listeners_json: row.listeners_json,
    }))
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
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");
        // Prod-Typ: listener_count ist INTEGER (int4), nicht BIGINT
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_eventsub_capacity_snapshot (
                id             BIGSERIAL PRIMARY KEY,
                ts_utc         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                listener_count INTEGER NOT NULL DEFAULT 0,
                used_slots     INTEGER NOT NULL DEFAULT 0,
                total_slots    INTEGER NOT NULL DEFAULT 0,
                headroom_slots INTEGER NOT NULL DEFAULT 0,
                listeners_json TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL eventsub");
        sqlx::query("TRUNCATE twitch_eventsub_capacity_snapshot")
            .execute(&pool)
            .await
            .expect("TRUNCATE");
        pool
    }

    #[tokio::test]
    async fn leere_tabelle_gibt_none() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_eventsub_leer").await;
        let snap = eventsub_snapshot(&pool).await.unwrap();
        assert!(snap.is_none());
    }

    #[tokio::test]
    async fn snapshot_mit_daten_wird_gelesen() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_eventsub_daten").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_eventsub_capacity_snapshot
                (ts_utc, listener_count, used_slots, total_slots, headroom_slots, listeners_json)
            VALUES
                (NOW() - INTERVAL '5 minutes', 3, 7, 30, 23, '[{"id":"abc","type":"channel.update"}]')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let snap = eventsub_snapshot(&pool).await.unwrap();
        assert!(snap.is_some());
        let s = snap.unwrap();
        assert_eq!(s.listener_count, 3);
        assert_eq!(s.used_slots, 7);
        assert_eq!(s.total_slots, 30);
        assert_eq!(s.headroom_slots, 23);
        let parsed: serde_json::Value = serde_json::from_str(&s.listeners_json).unwrap();
        assert!(parsed.is_array());
    }

    #[tokio::test]
    async fn zeile_mit_null_json_wird_ignoriert() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_eventsub_nulljson").await;
        sqlx::query(
            "INSERT INTO twitch_eventsub_capacity_snapshot \
             (ts_utc, listener_count, listeners_json) VALUES (NOW(), 5, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // listener_count > 0 aber listeners_json IS NULL → excluded
        let snap = eventsub_snapshot(&pool).await.unwrap();
        assert!(snap.is_none());
    }
}
