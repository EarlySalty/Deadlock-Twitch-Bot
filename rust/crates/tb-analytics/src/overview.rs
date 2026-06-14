//! Query für `GET /twitch/api/v2/overview` (Admin).

use sqlx::PgPool;

/// Aggregierte Metriken für einen Zeitraum.
///
/// Felder spiegeln Pythons `_calculate_overview_metrics` (session-abgeleitete
/// Teilmenge — die chatter-basierten Felder uniqueChatters/engagement folgen
/// separat, da sie Joins auf twitch_session_chatters + Bot-Filter brauchen).
/// Retention-Werte sind hier als Rohbruch (LEAST(1.0,..)) aggregiert; das *100
/// macht der Aufrufer (wie Python).
#[derive(Debug, sqlx::FromRow)]
pub struct OverviewMetricsRow {
    pub avg_avg_viewers: Option<f64>,
    pub max_peak_viewers: Option<i64>,
    pub total_hours_watched: Option<f64>,
    pub total_airtime_hours: Option<f64>,
    pub total_followers: Option<i64>,
    pub gained_followers: Option<i64>,
    pub avg_retention_10m: Option<f64>,
    pub retention_sample_count: Option<i64>,
    pub follower_valid_count: Option<i64>,
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
    until: Option<&str>,
) -> Result<Option<OverviewMetricsRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            AVG(s.avg_viewers)::FLOAT8                                AS avg_avg_viewers,
            MAX(s.peak_viewers)::BIGINT                               AS max_peak_viewers,
            SUM(s.avg_viewers * s.duration_seconds / 3600.0)::FLOAT8  AS total_hours_watched,
            SUM(s.duration_seconds / 3600.0)::FLOAT8                  AS total_airtime_hours,
            SUM(CASE
                    WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                    THEN s.follower_delta
                    ELSE 0
                END)::BIGINT                                          AS total_followers,
            COALESCE(SUM(CASE
                    WHEN s.follower_delta > 0
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                    THEN s.follower_delta
                    ELSE 0
                END), 0)::BIGINT                                      AS gained_followers,
            AVG(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0
                    THEN LEAST(1.0, s.retention_10m)
                    ELSE NULL
                END)::FLOAT8                                          AS avg_retention_10m,
            COUNT(CASE
                    WHEN s.avg_viewers >= 3 AND s.peak_viewers > 0 AND s.retention_10m IS NOT NULL
                    THEN 1
                END)::BIGINT                                          AS retention_sample_count,
            COUNT(CASE
                    WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                    THEN 1
                END)::BIGINT                                          AS follower_valid_count,
            COUNT(*)                                                  AS session_count
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
          AND ($3::TEXT IS NULL OR s.started_at < $3::TIMESTAMPTZ)
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .bind(until)
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

/// Bekannte Chat-Bot-Logins (Python `KNOWN_CHAT_BOTS`, core/chat_bots.py) —
/// werden aus Chatter-Zählungen gefiltert. Kleingeschrieben.
pub const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamelements",
    "streamlabs",
    "wizebot",
];

/// Chatter-abgeleitete Overview-Metriken (Bot-gefiltert).
#[derive(Debug, Default, Clone, Copy)]
pub struct OverviewChatterMetrics {
    /// Fenster-distinkte Chatter mit ≥1 Nachricht + Legacy-Aggregat für
    /// Sessions ohne Per-Chatter-Zeilen (Python total_unique_chatters).
    pub unique_chatters: i64,
    /// Distinkte aktive Chatter (≥1 Nachricht), Tracked-Teil (Python active_chatters).
    pub active_chatters: i64,
    /// Distinkte Zuschauer (Nachricht ODER via Chatters-API gesehen).
    pub unique_viewers: i64,
    /// active_chatters / unique_viewers * 100, 2 Nachkommastellen (Python engagement_rate).
    pub engagement_rate: f64,
}

/// Berechnet die chatter-basierten Overview-Metriken über
/// `twitch_session_chatters` (Bot-gefiltert, Python `_calculate_overview_metrics`-
/// Teilmenge: distinct_tracked + legacy_unique + active_chatters + distinct_viewers).
pub async fn overview_chatter_metrics(
    pool: &PgPool,
    since: &str,
    streamer_login: Option<&str>,
) -> Result<OverviewChatterMetrics, sqlx::Error> {
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|b| b.to_string()).collect();

    // distinct_tracked == active_chatters: distinkte Chatter mit ≥1 Nachricht.
    let (active,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id))::BIGINT
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE s.started_at >= $1::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
          AND sc.messages > 0
          AND (sc.chatter_login IS NULL OR sc.chatter_login = ''
               OR LOWER(sc.chatter_login) <> ALL($3))
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .bind(&bots)
    .fetch_one(pool)
    .await?;

    // legacy_unique: Alt-Sessions ohne Per-Chatter-Zeilen.
    let (legacy,): (i64,) = sqlx::query_as(
        r#"
        SELECT COALESCE(SUM(s.unique_chatters), 0)::BIGINT
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
          AND NOT EXISTS (
              SELECT 1 FROM twitch_session_chatters sc WHERE sc.session_id = s.id
          )
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .fetch_one(pool)
    .await?;

    // distinct_viewers: Nachricht ODER via Chatters-API gesehen.
    let (viewers,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id))::BIGINT
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE s.started_at >= $1::TIMESTAMPTZ
          AND s.ended_at IS NOT NULL
          AND ($2::TEXT IS NULL OR LOWER(s.streamer_login) = LOWER($2))
          AND (sc.messages > 0 OR COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE)
          AND (sc.chatter_login IS NULL OR sc.chatter_login = ''
               OR LOWER(sc.chatter_login) <> ALL($3))
        "#,
    )
    .bind(since)
    .bind(streamer_login)
    .bind(&bots)
    .fetch_one(pool)
    .await?;

    let engagement_rate = if viewers > 0 {
        ((active as f64 / viewers as f64) * 100.0 * 100.0).round() / 100.0
    } else {
        0.0
    };

    Ok(OverviewChatterMetrics {
        unique_chatters: active + legacy,
        active_chatters: active,
        unique_viewers: viewers,
        engagement_rate,
    })
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
        // Tabelle frisch anlegen, damit Schema-Änderungen (neue Spalten) auch im
        // persistenten Test-Container greifen (IF NOT EXISTS würde sie überspringen).
        sqlx::query("DROP TABLE IF EXISTS twitch_stream_sessions")
            .execute(&pool)
            .await
            .expect("DROP fehlgeschlagen");
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
                followers_end    BIGINT,
                retention_10m    REAL,
                unique_chatters  BIGINT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        sqlx::query("DROP TABLE IF EXISTS twitch_session_chatters")
            .execute(&pool)
            .await
            .expect("DROP chatters fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE twitch_session_chatters (
                session_id            BIGINT NOT NULL,
                chatter_login         TEXT,
                chatter_id            TEXT,
                messages              INTEGER DEFAULT 0,
                seen_via_chatters_api BOOLEAN DEFAULT FALSE
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL chatters fehlgeschlagen");
        // Tabellen leeren damit Wiederholungsläufe nicht alte Daten sehen
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
                 duration_seconds, follower_delta, followers_start, followers_end, retention_10m)
            VALUES
                ('streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours',
                 100.0, 200, 3600, 5, 1000, 1005, 0.5)
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

        let metrics = overview_metrics(&pool, since, Some("streamer_x"), None)
            .await
            .unwrap()
            .expect("Sollte Metriken liefern");
        assert_eq!(metrics.session_count, Some(1));
        assert!((metrics.avg_avg_viewers.unwrap() - 100.0).abs() < 0.001);
        // Neue session-abgeleitete Felder.
        assert_eq!(metrics.gained_followers, Some(5));
        assert_eq!(metrics.follower_valid_count, Some(1));
        assert_eq!(metrics.retention_sample_count, Some(1));
        assert!((metrics.avg_retention_10m.unwrap() - 0.5).abs() < 0.001);
    }

    #[tokio::test]
    async fn chatter_metrics_bot_gefiltert_und_engagement() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_overview_chatter").await;
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers, duration_seconds)
            VALUES (1, 'streamer_x', NOW() - INTERVAL '1 day', NOW() - INTERVAL '23 hours', 100.0, 200, 3600)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, messages, seen_via_chatters_api)
            VALUES
                (1, 'alice', 'a1', 3, FALSE),       -- aktiver Chatter + Viewer
                (1, 'streamlabs', 'sl', 9, FALSE),  -- Bot → gefiltert
                (1, 'bob', 'b2', 0, TRUE)           -- nur via API gesehen → Viewer, nicht aktiv
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let since = "2000-01-01T00:00:00+00:00";
        let m = overview_chatter_metrics(&pool, since, Some("streamer_x"))
            .await
            .unwrap();
        assert_eq!(m.active_chatters, 1, "nur alice aktiv (streamlabs=Bot)");
        assert_eq!(m.unique_viewers, 2, "alice + bob (bob via API), streamlabs=Bot raus");
        assert_eq!(m.unique_chatters, 1, "1 tracked + 0 legacy (Session hat Chatter-Zeilen)");
        assert!((m.engagement_rate - 50.0).abs() < 0.001, "1/2*100");
    }
}
