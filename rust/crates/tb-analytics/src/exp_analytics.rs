//! Experimentelle Analytics (`/twitch/api/v2/exp/*`) aus `exp_sessions`.
//!
//! Port von `bot/analytics/api_experimental.py`. `exp_sessions`-Spalten:
//! streamer TEXT, started_at TEXT (ISO), ended_at TEXT, game_name TEXT,
//! peak_viewers INT, avg_viewers REAL, duration_min REAL. `started_at` ist TEXT
//! → der `since`-Vergleich ist ein ISO-String-Vergleich (1:1 Python).

use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Cutoff-ISO-String (Python `(datetime.now(UTC) - timedelta(days=days)).isoformat()`).
fn since_iso(days: i64) -> String {
    (Utc::now() - Duration::days(days)).to_rfc3339_opts(SecondsFormat::Micros, false)
}

/// KPI-Overview je Streamer (Python `_load_exp_overview_payload`).
pub async fn load_exp_overview(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let since = since_iso(days);
    let streamer = streamer.to_lowercase();

    // max_avg_viewers wird in Python SELECTed, aber nicht ausgegeben → weggelassen.
    #[derive(sqlx::FromRow)]
    struct OverviewRow {
        total_sessions: i64,
        games_played: i64,
        avg_viewers: f64,
    }
    let overview: OverviewRow = sqlx::query_as(
        "SELECT COUNT(*)::bigint AS total_sessions, \
                COUNT(DISTINCT COALESCE(NULLIF(game_name, ''), '(unbekannt)'))::bigint AS games_played, \
                COALESCE(AVG(avg_viewers), 0)::float8 AS avg_viewers \
         FROM exp_sessions WHERE LOWER(streamer) = $1 AND started_at >= $2 AND ended_at IS NOT NULL",
    )
    .bind(&streamer)
    .bind(&since)
    .fetch_one(pool)
    .await?;

    let best = sqlx::query!(
        "SELECT game_name, AVG(avg_viewers)::float8 AS gam_avg \
         FROM exp_sessions \
         WHERE LOWER(streamer) = $1 AND started_at >= $2 AND ended_at IS NOT NULL \
           AND game_name IS NOT NULL AND game_name <> '' \
         GROUP BY game_name ORDER BY gam_avg DESC LIMIT 1",
        &streamer,
        &since
    )
    .fetch_optional(pool)
    .await?;

    let (best_game, best_game_avg) = match best {
        Some(row) => (
            row.game_name.unwrap_or_default(),
            row.gam_avg.unwrap_or(0.0),
        ),
        None => (String::new(), 0.0),
    };

    Ok(json!({
        "totalSessions": overview.total_sessions,
        "gamesPlayed": overview.games_played,
        "avgViewers": round1(overview.avg_viewers),
        "bestGame": best_game,
        "bestGameAvgViewers": round1(best_game_avg),
    }))
}

/// Per-Game-Aggregat je Streamer (Python `_load_exp_game_breakdown_payload`).
/// Gibt ein JSON-Array zurück (sortiert nach Ø-Viewern absteigend).
pub async fn load_exp_game_breakdown(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let since = since_iso(days);
    let streamer = streamer.to_lowercase();
    let rows = sqlx::query!(
        "SELECT COALESCE(game_name, '') AS \"game_name!\", COUNT(*)::bigint AS \"sessions!\", \
                COALESCE(AVG(avg_viewers), 0)::float8 AS \"avg_v!\", \
                COALESCE(MAX(peak_viewers), 0)::bigint AS \"peak_v!\", \
                COALESCE(AVG(duration_min), 0)::float8 AS \"avg_dur!\", \
                COALESCE(AVG(follower_delta), 0)::float8 AS \"avg_fd!\" \
         FROM exp_sessions \
         WHERE LOWER(streamer) = $1 AND started_at >= $2 AND ended_at IS NOT NULL \
         GROUP BY game_name ORDER BY COALESCE(AVG(avg_viewers), 0)::float8 DESC",
        &streamer,
        &since
    )
    .fetch_all(pool)
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let game = if row.game_name.is_empty() {
                "(unbekannt)".to_string()
            } else {
                row.game_name
            };
            json!({
                "game": game,
                "sessions": row.sessions,
                "avgViewers": round1(row.avg_v),
                "peakViewers": row.peak_v,
                "avgDurationMin": round1(row.avg_dur),
                "avgFollowerDelta": round1(row.avg_fd),
            })
        })
        .collect();
    Ok(Value::Array(items))
}

/// Game-Switch-Events je Streamer (Python `_load_exp_game_transitions_payload`).
/// JSON-Array, Top 50 nach Häufigkeit. `avgViewersAfter`/`viewerDelta` sind in
/// Python hartcodiert `0.0` (1:1 übernommen).
pub async fn load_exp_game_transitions(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let since = since_iso(days);
    let streamer = streamer.to_lowercase();
    let rows = sqlx::query!(
        "SELECT COALESCE(from_game, '(unbekannt)') AS \"from_game!\", \
                COALESCE(to_game, '(unbekannt)') AS \"to_game!\", \
                COUNT(*)::bigint AS \"cnt!\", COALESCE(AVG(viewer_count), 0)::float8 AS \"avg_v!\" \
         FROM exp_game_transitions WHERE LOWER(streamer) = $1 AND ts_utc >= $2 \
         GROUP BY from_game, to_game ORDER BY COUNT(*) DESC LIMIT 50",
        &streamer,
        &since
    )
    .fetch_all(pool)
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "fromGame": row.from_game,
                "toGame": row.to_game,
                "count": row.cnt,
                "avgViewersBefore": round1(row.avg_v),
                "avgViewersAfter": 0.0,
                "viewerDelta": 0.0,
            })
        })
        .collect();
    Ok(Value::Array(items))
}

/// Wachstumskurven je Spiel (Python `_load_exp_growth_curves_payload`):
/// Ø-Viewer pro 5-Minuten-Bucket aus `exp_snapshots` JOIN `exp_sessions`.
/// JSON-Array, sortiert nach game_name + Minuten-Bucket.
pub async fn load_exp_growth_curves(
    pool: &PgPool,
    streamer: &str,
    days: i64,
) -> Result<Value, sqlx::Error> {
    let since = since_iso(days);
    let streamer = streamer.to_lowercase();
    let rows = sqlx::query!(
        "SELECT COALESCE(es.game_name, '(unbekannt)') AS \"game_name!\", \
                (FLOOR(sn.minutes_from_start / 5) * 5)::bigint AS \"minute_bucket!\", \
                COALESCE(AVG(sn.viewer_count), 0)::float8 AS \"avg_v!\", COUNT(*)::bigint AS \"samples!\" \
         FROM exp_snapshots sn JOIN exp_sessions es ON es.id = sn.exp_session_id \
         WHERE LOWER(es.streamer) = $1 AND es.started_at >= $2 \
           AND sn.minutes_from_start IS NOT NULL AND sn.minutes_from_start >= 0 AND sn.minutes_from_start <= 360 \
         GROUP BY 1, 2 ORDER BY 1, 2",
        &streamer,
        &since
    )
    .fetch_all(pool)
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "game": row.game_name,
                "minuteFromStart": row.minute_bucket,
                "avgViewers": round1(row.avg_v),
                "sampleCount": row.samples,
            })
        })
        .collect();
    Ok(Value::Array(items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE exp_sessions (id BIGSERIAL PRIMARY KEY, streamer TEXT, started_at TEXT, ended_at TEXT, game_name TEXT, \
             peak_viewers INTEGER, avg_viewers REAL, duration_min REAL, follower_delta INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn game_breakdown_array_sortiert() {
        let Some(pool) = make_pool("t_exp_gb").await else {
            return;
        };
        sqlx::query("INSERT INTO exp_sessions (streamer, started_at, ended_at, game_name, peak_viewers, avg_viewers, duration_min, follower_delta) VALUES \
            ('nani','2026-06-10T00:00:00+00:00','2026-06-10T02:00:00+00:00','CS2',60,50,120,2), \
            ('nani','2026-06-11T00:00:00+00:00','2026-06-11T02:00:00+00:00','Deadlock',300,200,90,10), \
            ('nani','2026-06-12T00:00:00+00:00','2026-06-12T02:00:00+00:00','Deadlock',100,100,90,4), \
            ('nani','2026-06-13T00:00:00+00:00',NULL,'Deadlock',999,999,1,1)")
            .execute(&pool).await.unwrap();

        let v = load_exp_game_breakdown(&pool, "nani", 3650).await.unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2); // 2 Spiele (laufende ignoriert)
                                  // Sortiert nach avgViewers DESC → Deadlock (Ø 150) vor CS2 (50).
        assert_eq!(arr[0]["game"], "Deadlock");
        assert_eq!(arr[0]["sessions"], 2);
        assert_eq!(arr[0]["avgViewers"], 150.0);
        assert_eq!(arr[0]["peakViewers"], 300); // MAX
        assert_eq!(arr[0]["avgFollowerDelta"], 7.0); // (10+4)/2
        assert_eq!(arr[1]["game"], "CS2");
    }

    #[tokio::test]
    async fn game_breakdown_leeres_game_unbekannt() {
        let Some(pool) = make_pool("t_exp_gb_unknown").await else {
            return;
        };
        sqlx::query("INSERT INTO exp_sessions (streamer, started_at, ended_at, game_name, avg_viewers) VALUES ('nani','2026-06-10T00:00:00+00:00','2026-06-10T02:00:00+00:00',NULL,30)").execute(&pool).await.unwrap();
        let v = load_exp_game_breakdown(&pool, "nani", 3650).await.unwrap();
        assert_eq!(v[0]["game"], "(unbekannt)");
    }

    #[tokio::test]
    async fn growth_curves_buckets() {
        let Some(pool) = make_pool("t_exp_gc").await else {
            return;
        };
        sqlx::query("CREATE TABLE exp_snapshots (exp_session_id BIGINT, ts_utc TEXT, viewer_count INTEGER, minutes_from_start REAL)").execute(&pool).await.unwrap();
        let session_id: i64 = sqlx::query_scalar("INSERT INTO exp_sessions (streamer, started_at, game_name) VALUES ('nani','2026-06-10T00:00:00+00:00','Deadlock') RETURNING id").fetch_one(&pool).await.unwrap();
        // Snapshots Minute 2.0 + 3.0 → Bucket 0; Minute 12.0 → Bucket 10.
        sqlx::query("INSERT INTO exp_snapshots (exp_session_id, ts_utc, viewer_count, minutes_from_start) VALUES ($1,'t',100,3.0), ($1,'t',150,12.0), ($1,'t',200,2.0)")
            .bind(session_id).execute(&pool).await.unwrap();

        let v = load_exp_growth_curves(&pool, "nani", 3650).await.unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["game"], "Deadlock");
        assert_eq!(arr[0]["minuteFromStart"], 0); // Minute 2 + 3 → Bucket 0
        assert_eq!(arr[0]["avgViewers"], 150.0); // (100+200)/2
        assert_eq!(arr[0]["sampleCount"], 2);
        assert_eq!(arr[1]["minuteFromStart"], 10); // Minute 12 → Bucket 10
    }

    #[tokio::test]
    async fn game_transitions_array() {
        let Some(pool) = make_pool("t_exp_tr").await else {
            return;
        };
        sqlx::query("CREATE TABLE exp_game_transitions (streamer TEXT, ts_utc TEXT, from_game TEXT, to_game TEXT, viewer_count INTEGER)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO exp_game_transitions (streamer, ts_utc, from_game, to_game, viewer_count) VALUES \
            ('nani','2026-06-10T00:00:00+00:00','CS2','Deadlock',100), \
            ('nani','2026-06-11T00:00:00+00:00','CS2','Deadlock',200), \
            ('nani','2026-06-12T00:00:00+00:00',NULL,'Deadlock',40)")
            .execute(&pool).await.unwrap();

        let v = load_exp_game_transitions(&pool, "nani", 3650)
            .await
            .unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2); // (CS2→Deadlock) + (NULL→Deadlock)
                                  // Häufigster: CS2→Deadlock (2x), Ø-before (100+200)/2=150.
        assert_eq!(arr[0]["fromGame"], "CS2");
        assert_eq!(arr[0]["toGame"], "Deadlock");
        assert_eq!(arr[0]["count"], 2);
        assert_eq!(arr[0]["avgViewersBefore"], 150.0);
        assert_eq!(arr[0]["avgViewersAfter"], 0.0); // hartcodiert
        assert_eq!(arr[0]["viewerDelta"], 0.0);
        // NULL from_game → "(unbekannt)".
        assert_eq!(arr[1]["fromGame"], "(unbekannt)");
    }

    #[tokio::test]
    async fn overview_aggregiert_und_best_game() {
        let Some(pool) = make_pool("t_exp_ov").await else {
            return;
        };
        // 4 Sessions, 3 Buckets inkl. "(unbekannt)"; Deadlock Ø 150 → bestGame.
        sqlx::query("INSERT INTO exp_sessions (streamer, started_at, ended_at, game_name, avg_viewers) VALUES \
            ('Nani','2026-06-10T00:00:00+00:00','2026-06-10T02:00:00+00:00','Deadlock',100), \
            ('nani','2026-06-11T00:00:00+00:00','2026-06-11T02:00:00+00:00','Deadlock',200), \
            ('nani','2026-06-12T00:00:00+00:00','2026-06-12T02:00:00+00:00','CS2',50), \
            ('nani','2026-06-12T03:00:00+00:00','2026-06-12T04:00:00+00:00',NULL,30)")
            .execute(&pool).await.unwrap();
        // laufende Session (ended_at NULL) → ignoriert.
        sqlx::query("INSERT INTO exp_sessions (streamer, started_at, ended_at, game_name, avg_viewers) VALUES ('nani','2026-06-13T00:00:00+00:00',NULL,'Deadlock',999)").execute(&pool).await.unwrap();

        let v = load_exp_overview(&pool, "nani", 3650).await.unwrap();
        assert_eq!(v["totalSessions"], 4); // laufende ignoriert
        assert_eq!(v["gamesPlayed"], 3);
        assert_eq!(v["avgViewers"], 95.0);
        assert_eq!(v["bestGame"], "Deadlock");
        assert_eq!(v["bestGameAvgViewers"], 150.0);
    }

    #[tokio::test]
    async fn overview_leer_nullwerte() {
        let Some(pool) = make_pool("t_exp_ov_empty").await else {
            return;
        };
        let v = load_exp_overview(&pool, "ghost", 30).await.unwrap();
        assert_eq!(v["totalSessions"], 0);
        assert_eq!(v["gamesPlayed"], 0);
        assert_eq!(v["avgViewers"], 0.0);
        assert_eq!(v["bestGame"], "");
        assert_eq!(v["bestGameAvgViewers"], 0.0);
    }
}
