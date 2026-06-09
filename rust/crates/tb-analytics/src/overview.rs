//! Query für `GET /twitch/api/v2/overview` (Admin).

use sqlx::PgPool;

/// Aggregierte Metriken für einen Zeitraum.
#[derive(Debug, sqlx::FromRow)]
pub struct OverviewMetricsRow {
    pub avg_avg_viewers: Option<f64>,
    pub max_peak_viewers: Option<i64>,
    pub total_hours_watched: Option<f64>,
    pub total_airtime_hours: Option<f64>,
    pub total_followers: Option<i64>,
    pub session_count: Option<i64>,
}

/// Holt aggregierte Metriken für einen Streamer im angegebenen Zeitraum.
///
/// `streamer_login`: `None` → alle Streamer aggregiert.
/// `since`: ISO-8601-String (>= since_date).
pub async fn overview_metrics(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<Option<OverviewMetricsRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            AVG(s.avg_viewers)::FLOAT8                                AS avg_avg_viewers,
            MAX(s.peak_viewers)                                       AS max_peak_viewers,
            SUM(s.avg_viewers * s.duration_seconds / 3600.0)::FLOAT8  AS total_hours_watched,
            SUM(s.duration_seconds / 3600.0)::FLOAT8                  AS total_airtime_hours,
            SUM(CASE
                    WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                    THEN s.follower_delta
                    ELSE 0
                END)::BIGINT                                          AS total_followers,
            COUNT(*)                                                  AS session_count
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .fetch_optional(pool)
    .await
}

/// Existenz-Check: gibt 0 zurück wenn keine Sessions vorhanden.
pub async fn overview_session_count(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
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
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id               BIGSERIAL PRIMARY KEY,
                streamer_login   TEXT NOT NULL,
                started_at       TIMESTAMPTZ NOT NULL,
                ended_at         TIMESTAMPTZ,
                avg_viewers      DOUBLE PRECISION,
                peak_viewers     BIGINT,
                duration_seconds BIGINT,
                follower_delta   BIGINT,
                followers_start  BIGINT,
                followers_end    BIGINT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        // Tabelle leeren damit Wiederholungsläufe nicht alte Daten sehen
        sqlx::query("TRUNCATE twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("TRUNCATE fehlgeschlagen");
        pool
    }

    #[tokio::test]
    async fn leere_tabelle_gibt_null_count() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_leer").await;
        let since = "2000-01-01T00:00:00+00:00";
        let count = overview_session_count(&pool, since, None).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn session_count_und_metrics_fuer_bekannten_streamer() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_mit_daten").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (streamer_login, started_at, ended_at, avg_viewers, peak_viewers,
                 duration_seconds, follower_delta, followers_start, followers_end)
            VALUES
                ('streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours',
                 100.0, 200, 3600, 5, 1000, 1005)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        let count = overview_session_count(&pool, since, Some("streamer_x"))
            .await
            .unwrap();
        assert_eq!(count, 1);

        let metrics = overview_metrics(&pool, since, Some("streamer_x"))
            .await
            .unwrap()
            .expect("Sollte Metriken liefern");
        assert_eq!(metrics.session_count, Some(1));
        assert!((metrics.avg_avg_viewers.unwrap() - 100.0).abs() < 0.001);
    }
}
