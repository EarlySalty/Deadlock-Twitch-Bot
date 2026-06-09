//! Queries für `GET /twitch/api/v2/public/recent-raids`.
//!
//! Echte Spaltennamen in `twitch_raid_history`:
//!   `from_broadcaster_login`, `to_broadcaster_login`, `viewer_count` (INTEGER),
//!   `executed_at` (TIMESTAMPTZ).
//!
//! Die JSON-Feldnamen (`from_channel`, `to_channel`, `viewers`) entsprechen dem
//! Python-API-Output und werden via SQL-Aliase bereitgestellt.

use sqlx::PgPool;

/// Eine Zeile aus `twitch_raid_history`.
///
/// Feldnamen sind die JSON-seitigen Namen (Python-kompatibel).
/// `viewer_count` in der DB ist `INTEGER` → `Option<i32>`.
/// `executed_at` analog zu `received_at` in BanRow via `::text`.
#[derive(Debug, sqlx::FromRow)]
pub struct RaidRow {
    pub from_channel: String,
    pub to_channel: String,
    pub viewers: Option<i32>,
    pub executed_at: Option<String>,
}

/// Lädt die letzten 20 Raids.
pub async fn recent_raids(pool: &PgPool) -> Result<Vec<RaidRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            from_broadcaster_login  AS from_channel,
            to_broadcaster_login    AS to_channel,
            viewer_count            AS viewers,
            executed_at::text       AS executed_at
        FROM twitch_raid_history
        ORDER BY executed_at DESC
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Pool mit max 1 Connection + eigenem Schema für Isolation bei paralleler Ausführung.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen fehlgeschlagen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_history (
                id                     BIGSERIAL PRIMARY KEY,
                from_broadcaster_login TEXT NOT NULL,
                to_broadcaster_login   TEXT NOT NULL,
                viewer_count           INTEGER DEFAULT 0,
                executed_at            TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        pool
    }

    #[tokio::test]
    async fn recent_raids_leere_tabelle() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_raids_leer").await;
        sqlx::query("TRUNCATE twitch_raid_history")
            .execute(&pool)
            .await
            .unwrap();

        let rows = recent_raids(&pool).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn recent_raids_reihenfolge_und_felder() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_raids_reihenfolge").await;
        sqlx::query("TRUNCATE twitch_raid_history")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            r#"
            INSERT INTO twitch_raid_history
                (from_broadcaster_login, to_broadcaster_login, viewer_count, executed_at)
            VALUES
                ('kanal_a', 'kanal_b', 150, NOW() - INTERVAL '2 hours'),
                ('kanal_b', 'kanal_c', 80,  NOW() - INTERVAL '1 hour')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let rows = recent_raids(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        // Neuester zuerst (kanal_b→kanal_c, 1h alt)
        assert_eq!(rows[0].from_channel, "kanal_b");
        assert_eq!(rows[0].to_channel, "kanal_c");
        assert_eq!(rows[0].viewers, Some(80));
        assert!(rows[0]
            .executed_at
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false));
    }
}
