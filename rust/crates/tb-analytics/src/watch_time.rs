//! Watch-Time-Verteilung (`/twitch/api/v2/watch-time-distribution`).
//!
//! Port von `bot/analytics/api_audience.py` (`_api_v2_watch_time_distribution`,
//! `_calc_watch_distribution`, `_backfill_last_seen_from_messages`) + den
//! Window-Helfern aus `api_v2.py`.
//!
//! Ablauf in EINER Schreib-Transaktion: Fenster auflösen → Session-IDs der
//! aktuellen + vorherigen Periode laden → `last_seen_at` aus Chat-Nachrichten
//! backfillen (UPDATE) → je Periode die echte Pro-Viewer-Watch-Time aus
//! `twitch_session_chatters` (`last_seen_at − first_message_at`) bucketen →
//! Deltas + Confidence.
//!
//! **Output-1:1:** Die Session-Query liefert in Python remappte retention-Spalten,
//! die `_calc_watch_distribution` nie nutzt (nur `len(sessions)` + die IDs) — hier
//! daher nur `COUNT`/IDs. Der Backfill ist in Python ein SELECT + zwei
//! `executemany`-UPDATEs; hier ein einzelnes mengenbasiertes `UPDATE … FROM`
//! (identischer Effekt). **`window` = "full" oder "last_stream"** (die plan-
//! abhängige Auflösung sitzt im Handler).

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};

const WATCH_TIME_MIN_SAMPLES: i64 = 25;
const WATCH_TIME_MIN_COVERAGE: f64 = 0.15;

use crate::bekannte_bots::KNOWN_CHAT_BOTS;

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn bots_vec() -> Vec<String> {
    KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect()
}

/// (since, prev_since) für das Lesefenster.
/// `last_stream`: since = Start des letzten Streams, prev == since (Vorperiode leer).
/// `full`: rollierend now−days / now−2·days.
async fn window_since_dates(
    conn: &mut PgConnection,
    streamer: &str,
    days: i64,
    window: &str,
) -> Result<(DateTime<Utc>, DateTime<Utc>), sqlx::Error> {
    if window == "last_stream" {
        let latest = sqlx::query_scalar!(
            "SELECT MAX(s.started_at) AS started_at FROM twitch_stream_sessions s \
             WHERE s.ended_at IS NOT NULL AND LOWER(s.streamer_login) = $1",
            streamer
        )
        .fetch_one(conn)
        .await?;
        let since = latest.unwrap_or_else(|| Utc::now() - Duration::days(days));
        Ok((since, since))
    } else {
        let since = Utc::now() - Duration::days(days);
        let prev = Utc::now() - Duration::days(days * 2);
        Ok((since, prev))
    }
}

/// Session-IDs einer Periode (ended, Streamer, started_at im Halb-offenen Intervall).
async fn session_ids(
    conn: &mut PgConnection,
    streamer: &str,
    since: DateTime<Utc>,
    until: Option<DateTime<Utc>>,
) -> Result<Vec<i64>, sqlx::Error> {
    match until {
        Some(until) => {
            sqlx::query_scalar!(
                "SELECT s.id::bigint AS \"id!\" FROM twitch_stream_sessions s \
             WHERE s.started_at >= $1 AND s.started_at < $2 \
               AND LOWER(s.streamer_login) = $3 AND s.ended_at IS NOT NULL",
                since,
                until,
                streamer
            )
            .fetch_all(conn)
            .await
        }
        None => {
            sqlx::query_scalar!(
                "SELECT s.id::bigint AS \"id!\" FROM twitch_stream_sessions s \
             WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL",
                since,
                streamer
            )
            .fetch_all(conn)
            .await
        }
    }
}

/// Backfill von `last_seen_at` aus dem letzten Chat-Zeitstempel je Chatter+Session.
/// Mengenbasiert (ein UPDATE … FROM); deckt Login- UND anonyme (chatter_id)-Zeilen ab.
async fn backfill_last_seen(conn: &mut PgConnection, ids: &[i64]) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return Ok(());
    }
    let bots = bots_vec();
    sqlx::query!(
        "UPDATE twitch_session_chatters sc \
            SET last_seen_at = agg.max_ts \
           FROM ( \
             SELECT cm.session_id, \
                    LOWER(NULLIF(cm.chatter_login, '')) AS chatter_login, \
                    cm.chatter_id, \
                    MAX(cm.message_ts) AS max_ts \
               FROM twitch_chat_messages cm \
              WHERE cm.session_id = ANY($1::bigint[]) \
                AND (cm.chatter_login IS NULL OR cm.chatter_login = '' OR (LOWER(cm.chatter_login) <> ALL($2::text[]) AND LOWER(cm.chatter_login) !~ '^justinfan[0-9]+$')) \
              GROUP BY cm.session_id, LOWER(NULLIF(cm.chatter_login, '')), cm.chatter_id \
           ) agg \
          WHERE sc.session_id = agg.session_id \
            AND agg.max_ts IS NOT NULL \
            AND ( \
              (agg.chatter_login IS NOT NULL AND LOWER(sc.chatter_login) = agg.chatter_login) \
              OR (agg.chatter_login IS NULL AND agg.chatter_id IS NOT NULL \
                  AND sc.chatter_id = agg.chatter_id AND (sc.chatter_login IS NULL OR sc.chatter_login = '')) \
            ) \
            AND (sc.last_seen_at IS NULL OR sc.last_seen_at < agg.max_ts)",
        ids,
        &bots
    )
    .execute(conn)
    .await?;
    Ok(())
}

fn no_data_quality() -> Value {
    json!({
        "method": "no_data",
        "coverage": 0.0,
        "sample_count": 0,
        "viewer_base_count": 0,
        "required_min_samples": WATCH_TIME_MIN_SAMPLES,
        "required_min_coverage": WATCH_TIME_MIN_COVERAGE,
    })
}

fn base_with(session_count: i64, data_quality: Value) -> Value {
    json!({
        "under5min": 0, "min5to15": 0, "min15to30": 0, "min30to60": 0, "over60min": 0,
        "avgWatchTime": 0, "medianWatchTime": 0,
        "sessionCount": session_count,
        "dataQuality": data_quality,
    })
}

/// Watch-Time-Verteilung einer Periode (Python `_calc_watch_distribution`).
async fn calc_watch_distribution(
    conn: &mut PgConnection,
    ids: &[i64],
) -> Result<Value, sqlx::Error> {
    if ids.is_empty() {
        return Ok(base_with(0, no_data_quality()));
    }
    let total_sessions = ids.len() as i64;
    let bots = bots_vec();

    let viewer_base_count = sqlx::query_scalar!(
        "SELECT COUNT(DISTINCT COALESCE(NULLIF(chatter_login, ''), chatter_id))::bigint AS \"count!\" \
           FROM twitch_session_chatters \
          WHERE session_id = ANY($1::bigint[]) \
            AND COALESCE(NULLIF(chatter_login, ''), chatter_id) IS NOT NULL \
            AND (chatter_login IS NULL OR chatter_login = '' OR (LOWER(chatter_login) <> ALL($2::text[]) AND LOWER(chatter_login) !~ '^justinfan[0-9]+$'))",
        ids,
        &bots
    )
    .fetch_one(&mut *conn)
    .await?;

    let raw = sqlx::query_scalar!(
        "SELECT ROUND(GREATEST( \
                    EXTRACT(EPOCH FROM COALESCE(last_seen_at, first_message_at)) \
                    - EXTRACT(EPOCH FROM COALESCE(first_message_at, last_seen_at)), 0) / 60.0)::float8 AS minutes \
           FROM twitch_session_chatters \
          WHERE session_id = ANY($1::bigint[]) AND first_message_at IS NOT NULL AND last_seen_at IS NOT NULL \
            AND (chatter_login IS NULL OR chatter_login = '' OR (LOWER(chatter_login) <> ALL($2::text[]) AND LOWER(chatter_login) !~ '^justinfan[0-9]+$'))",
        ids,
        &bots
    )
    .fetch_all(&mut *conn)
    .await?;

    let real_minutes: Vec<f64> = raw.into_iter().flatten().filter(|m| *m >= 0.0).collect();
    let sample_count = real_minutes.len() as i64;
    let coverage_real = if viewer_base_count > 0 {
        sample_count as f64 / viewer_base_count.max(1) as f64
    } else {
        sample_count as f64 / total_sessions.max(1) as f64
    };

    let method = if sample_count <= 0 {
        "no_data"
    } else if sample_count < WATCH_TIME_MIN_SAMPLES || coverage_real < WATCH_TIME_MIN_COVERAGE {
        "low_coverage"
    } else {
        "real_samples"
    };

    let data_quality = json!({
        "method": method,
        "coverage": round3(coverage_real.clamp(0.0, 1.0)),
        "sample_count": sample_count,
        "viewer_base_count": viewer_base_count,
        "required_min_samples": WATCH_TIME_MIN_SAMPLES,
        "required_min_coverage": WATCH_TIME_MIN_COVERAGE,
    });

    if method != "real_samples" {
        return Ok(base_with(total_sessions, data_quality));
    }

    let total = real_minutes.len() as f64;
    let pct = |f: &dyn Fn(f64) -> bool| -> f64 {
        real_minutes.iter().filter(|m| f(**m)).count() as f64 / total * 100.0
    };
    let under5 = pct(&|m| m < 5.0);
    let m5_15 = pct(&|m| (5.0..15.0).contains(&m));
    let m15_30 = pct(&|m| (15.0..30.0).contains(&m));
    let m30_60 = pct(&|m| (30.0..60.0).contains(&m));
    let over60 = pct(&|m| m >= 60.0);
    let avg = real_minutes.iter().sum::<f64>() / total;
    let mut sorted = real_minutes.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    let median = if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    };

    Ok(json!({
        "under5min": round1(under5.max(0.0)),
        "min5to15": round1(m5_15.max(0.0)),
        "min15to30": round1(m15_30.max(0.0)),
        "min30to60": round1(m30_60.max(0.0)),
        "over60min": round1(over60.max(0.0)),
        "avgWatchTime": round1(avg),
        "medianWatchTime": round1(median),
        "sessionCount": total_sessions,
        "dataQuality": data_quality,
    }))
}

/// Lädt die Watch-Time-Verteilung inkl. Vorperioden-Vergleich (Python-Handler-Logik).
pub async fn load_watch_time_distribution(
    pool: &PgPool,
    streamer: &str,
    days: i64,
    window: &str,
) -> Result<Value, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let (since, prev_since) = window_since_dates(&mut tx, streamer, days, window).await?;
    let current_ids = session_ids(&mut tx, streamer, since, None).await?;
    let prev_ids = session_ids(&mut tx, streamer, prev_since, Some(since)).await?;

    let mut all_ids = current_ids.clone();
    all_ids.extend_from_slice(&prev_ids);
    backfill_last_seen(&mut tx, &all_ids).await?;

    let current = calc_watch_distribution(&mut tx, &current_ids).await?;
    let previous = calc_watch_distribution(&mut tx, &prev_ids).await?;
    tx.commit().await?;

    // Deltas der 6 Kennzahlen.
    let keys = [
        "under5min",
        "min5to15",
        "min15to30",
        "min30to60",
        "over60min",
        "avgWatchTime",
    ];
    let mut deltas = serde_json::Map::new();
    for k in keys {
        let curr = current[k].as_f64().unwrap_or(0.0);
        let prev = previous[k].as_f64().unwrap_or(0.0);
        if prev > 0.0 {
            deltas.insert(k.to_string(), json!(round1((curr - prev) / prev * 100.0)));
        } else {
            deltas.insert(k.to_string(), Value::Null);
        }
    }

    let dq = &current["dataQuality"];
    let method = dq["method"].as_str().unwrap_or("no_data").to_string();
    let sample_count = dq["sample_count"].as_i64().unwrap_or(0);
    let coverage = dq["coverage"].as_f64().unwrap_or(0.0);

    let confidence = if method == "real_samples" {
        if sample_count >= 200 && coverage >= 0.35 {
            "high"
        } else if sample_count >= 80 && coverage >= 0.20 {
            "medium"
        } else {
            "low"
        }
    } else if method == "low_coverage" {
        "low"
    } else {
        "very_low"
    };
    // Bei nicht-real_samples werden die Deltas genullt.
    if method != "real_samples" {
        for k in keys {
            deltas.insert(k.to_string(), Value::Null);
        }
    }

    let session_count = current["sessionCount"].as_i64().unwrap_or(0);
    let mut resp = current.as_object().cloned().unwrap_or_default();
    resp.insert("previous".to_string(), previous);
    resp.insert("deltas".to_string(), Value::Object(deltas));
    resp.insert(
        "dataQuality".to_string(),
        json!({
            "confidence": confidence,
            "sessions": session_count,
            "method": method,
            "coverage": round3(coverage),
            "sample_count": sample_count,
            "viewer_base_count": dq["viewer_base_count"].as_i64().unwrap_or(0),
            "required_min_samples": dq["required_min_samples"].as_i64().unwrap_or(WATCH_TIME_MIN_SAMPLES),
            "required_min_coverage": dq["required_min_coverage"].as_f64().unwrap_or(WATCH_TIME_MIN_COVERAGE),
            "botFilterApplied": true,
        }),
    );
    Ok(Value::Object(resp))
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
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_chatters (session_id BIGINT, chatter_login TEXT, chatter_id TEXT, first_message_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, chatter_login TEXT, chatter_id TEXT, message_ts TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn no_data_payload() {
        let Some(pool) = make_pool("t_wt_nodata").await else {
            return;
        };
        let v = load_watch_time_distribution(&pool, "nani", 30, "full")
            .await
            .unwrap();
        assert_eq!(v["under5min"], 0);
        assert_eq!(v["sessionCount"], 0);
        assert_eq!(v["dataQuality"]["method"], "no_data");
        assert_eq!(v["dataQuality"]["confidence"], "very_low");
        assert_eq!(v["dataQuality"]["botFilterApplied"], true);
        assert!(v["deltas"]["under5min"].is_null());
    }

    #[tokio::test]
    async fn real_samples_buckets() {
        let Some(pool) = make_pool("t_wt_real").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) VALUES (1,'nani',NOW()-INTERVAL '2 days',NOW()-INTERVAL '2 days'+INTERVAL '4 hours')")
            .execute(&pool).await.unwrap();
        // 30 Chatter, je 10 Min Watch-Time (first→last = 10 min) → min5to15-Bucket.
        for i in 0..30 {
            sqlx::query("INSERT INTO twitch_session_chatters (session_id, chatter_login, first_message_at, last_seen_at) VALUES (1, $1, NOW()-INTERVAL '2 days', NOW()-INTERVAL '2 days'+INTERVAL '10 minutes')")
                .bind(format!("chatter{i}")).execute(&pool).await.unwrap();
        }
        let v = load_watch_time_distribution(&pool, "nani", 30, "full")
            .await
            .unwrap();
        assert_eq!(v["dataQuality"]["method"], "real_samples");
        assert_eq!(v["dataQuality"]["sample_count"], 30);
        assert_eq!(v["dataQuality"]["coverage"], 1.0); // 30/30
        assert_eq!(v["dataQuality"]["confidence"], "low"); // real_samples, aber <80 samples
        assert_eq!(v["min5to15"], 100.0);
        assert_eq!(v["under5min"], 0.0);
        assert_eq!(v["avgWatchTime"], 10.0);
        assert_eq!(v["medianWatchTime"], 10.0);
        assert_eq!(v["sessionCount"], 1);
    }

    #[tokio::test]
    async fn backfill_fuellt_last_seen() {
        let Some(pool) = make_pool("t_wt_backfill").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) VALUES (1,'nani',NOW()-INTERVAL '2 days',NOW()-INTERVAL '2 days'+INTERVAL '1 hour')")
            .execute(&pool).await.unwrap();
        // Chatter ohne last_seen_at, aber mit Chat-Nachrichten → Backfill setzt last_seen.
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, chatter_login, first_message_at, last_seen_at) VALUES (1,'lurker',NOW()-INTERVAL '2 days',NULL)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, chatter_login, message_ts) VALUES (1,'lurker',NOW()-INTERVAL '2 days'+INTERVAL '7 minutes')")
            .execute(&pool).await.unwrap();
        let v = load_watch_time_distribution(&pool, "nani", 30, "full")
            .await
            .unwrap();
        // Ohne Backfill wäre sample_count 0 (last_seen NULL gefiltert); jetzt 1.
        assert_eq!(v["dataQuality"]["sample_count"], 1);
        // 1 Sample < 25 → low_coverage (Buckets bleiben 0).
        assert_eq!(v["dataQuality"]["method"], "low_coverage");
    }

    #[tokio::test]
    async fn last_stream_window_klemmt_auf_letzten() {
        let Some(pool) = make_pool("t_wt_laststream").await else {
            return;
        };
        // Alte + neue Session; last_stream → nur die neue ist im Fenster, Vorperiode leer.
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) VALUES (1,'nani',NOW()-INTERVAL '10 days',NOW()-INTERVAL '10 days'+INTERVAL '2 hours'),(2,'nani',NOW()-INTERVAL '1 day',NOW()-INTERVAL '1 day'+INTERVAL '2 hours')")
            .execute(&pool).await.unwrap();
        // Beide Sessions haben je 1 Chatter (10 min).
        for sid in [1, 2] {
            sqlx::query("INSERT INTO twitch_session_chatters (session_id, chatter_login, first_message_at, last_seen_at) VALUES ($1,'a',NOW()-INTERVAL '2 days',NOW()-INTERVAL '2 days'+INTERVAL '10 minutes')")
                .bind(sid as i64).execute(&pool).await.unwrap();
        }
        let v = load_watch_time_distribution(&pool, "nani", 30, "last_stream")
            .await
            .unwrap();
        // Nur Session 2 → sessionCount 1; Vorperiode leer → previous no_data.
        assert_eq!(v["sessionCount"], 1);
        assert_eq!(v["previous"]["dataQuality"]["method"], "no_data");
    }
}
