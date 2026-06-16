//! Erweiterte Tag-Performance-Analyse (`/twitch/api/v2/tag-analysis-extended`).
//!
//! Port von `bot/analytics/api_performance.py:_load_tag_analysis_extended_payload_sync`.
//! Aggregiert pro Stream-Tag (aus `twitch_stream_sessions.tags`, JSON-Array ODER
//! Komma-Liste, max. 5 je Session, dedupliziert) Median-Statistiken über
//! Viewer/Retention/Follower/Dauer + häufigste Streaming-Stunde.
//!
//! `peerBenchmark` (nur bei gesetztem `streamer`) liefert den Tier-basierten
//! Peer-Median für Viewer + Retention (Port von `_get_peer_group_stats`,
//! reduziert auf die beiden vom Endpoint ausgegebenen Felder).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

#[derive(Default)]
struct TagBucket {
    viewers: Vec<f64>,
    retention: Vec<f64>,
    followers: Vec<f64>,
    durations: Vec<f64>,
    hours: Vec<i32>,
    samples: i64,
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    let mid = n / 2;
    if n % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2.0
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Tags aus dem Spaltenwert parsen (JSON-Array oder Komma-Liste).
fn parse_tags(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.starts_with('[') {
        match serde_json::from_str::<Value>(raw) {
            Ok(Value::Array(arr)) => arr
                .iter()
                .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
                .collect(),
            _ => vec![raw.to_string()], // JSONDecodeError → [tags_str] (Python)
        }
    } else {
        raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
    }
}

/// Häufigste Stunde → "HH:00", sonst "18:00-22:00" (Python `best_slot`).
fn best_time_slot(hours: &[i32]) -> String {
    if hours.is_empty() {
        return "18:00-22:00".to_string();
    }
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for &h in hours {
        *counts.entry(h).or_insert(0) += 1;
    }
    // most_common(1): höchste Anzahl; bei Gleichstand zuerst eingefügte (Python Counter).
    let mut best_hour = hours[0];
    let mut best_count = 0usize;
    for &h in hours {
        let c = counts[&h];
        if c > best_count {
            best_count = c;
            best_hour = h;
        }
    }
    format!("{best_hour:02}:00")
}

/// Peer-Group-Tier anhand des Durchschnitts-Viewer-Werts (Python `_get_tier`).
/// 0=starter(<15) 1=rising(<50) 2=established(<150) 3=featured(<500) 4=top.
fn tier_of(avg: f64) -> u8 {
    if avg < 15.0 {
        0
    } else if avg < 50.0 {
        1
    } else if avg < 150.0 {
        2
    } else if avg < 500.0 {
        3
    } else {
        4
    }
}

/// Peer-Benchmark für einen Streamer (Python `_get_peer_group_stats`, reduziert
/// auf die beiden Felder, die `tag-analysis-extended` ausgibt:
/// `peerAvg.avgViewers` + `peerAvg.retention10m`). `None`, wenn keine
/// Kategorie-/Session-Daten vorliegen oder der Streamer keiner Peer-Group zuzuordnen ist.
async fn load_peer_benchmark(
    pool: &PgPool,
    streamer_login: &str,
    since: DateTime<Utc>,
) -> Result<Option<Value>, sqlx::Error> {
    let login = streamer_login.to_lowercase();

    // 1. Durchschnitts-Viewer aller Streamer der Kategorie.
    let avgs: Vec<(String, Option<f64>)> = sqlx::query_as(
        "SELECT streamer, AVG(viewer_count)::float8 FROM twitch_stats_category \
         WHERE ts_utc >= $1 GROUP BY streamer",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    if avgs.is_empty() {
        return Ok(None);
    }
    let mut streamer_avgs: HashMap<String, f64> = HashMap::new();
    for (s, avg) in avgs {
        if let Some(a) = avg {
            streamer_avgs.insert(s.to_lowercase(), a);
        }
    }

    // 2. Eigener Schnitt — Fallback aus den Sessions, wenn nicht in Kategorie-Daten.
    let my_avg = match streamer_avgs.get(&login).copied() {
        Some(a) => a,
        None => {
            let row: Option<(Option<f64>,)> = sqlx::query_as(
                "SELECT AVG(avg_viewers)::float8 FROM twitch_stream_sessions \
                 WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL",
            )
            .bind(&login)
            .bind(since)
            .fetch_optional(pool)
            .await?;
            match row.and_then(|(a,)| a) {
                Some(a) => a,
                None => return Ok(None),
            }
        }
    };

    // 3. Peers im selben Tier.
    let my_tier = tier_of(my_avg);
    let peer_logins: Vec<String> = streamer_avgs
        .iter()
        .filter(|(_, &a)| tier_of(a) == my_tier)
        .map(|(s, _)| s.clone())
        .collect();
    if peer_logins.is_empty() {
        return Ok(None);
    }

    // 4. Session-Metriken je Peer (nur avg_viewers + retention_10m nötig).
    let metrics: Vec<(Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT AVG(s.avg_viewers)::float8, AVG(s.retention_10m)::float8 \
         FROM twitch_stream_sessions s \
         WHERE LOWER(s.streamer_login) = ANY($1) AND s.started_at >= $2 AND s.ended_at IS NOT NULL \
         GROUP BY LOWER(s.streamer_login)",
    )
    .bind(&peer_logins)
    .bind(since)
    .fetch_all(pool)
    .await?;

    let avg_viewers_list: Vec<f64> = metrics.iter().filter_map(|(v, _)| *v).collect();
    let retention_list: Vec<f64> = metrics.iter().filter_map(|(_, r)| *r).collect();

    Ok(Some(json!({
        "avgViewers": round1(median(&avg_viewers_list)),
        "retention10m": round1(median(&retention_list) * 100.0),
    })))
}

/// Lädt die erweiterte Tag-Analyse (Python `_load_tag_analysis_extended_payload_sync`).
pub async fn load_tag_analysis_extended(
    pool: &PgPool,
    streamer: Option<&str>,
    days: i64,
    limit: i64,
) -> Result<Value, sqlx::Error> {
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    let streamer_login = streamer.map(|s| s.to_lowercase());

    type Row = (i64, Option<String>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<i32>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT s.id::bigint, s.tags, s.avg_viewers::float8, s.retention_10m::float8, \
                CASE WHEN s.follower_delta IS NOT NULL AND NOT (s.followers_end = 0 AND s.followers_start > 0) \
                     THEN s.follower_delta ELSE NULL END::float8 AS follower_delta, \
                s.duration_seconds::float8, EXTRACT(HOUR FROM s.started_at)::int AS start_hour \
         FROM twitch_stream_sessions s \
         WHERE s.started_at >= $1 AND s.ended_at IS NOT NULL AND s.tags IS NOT NULL \
           AND (COALESCE($2, '') = '' OR LOWER(s.streamer_login) = $2)",
    )
    .bind(since)
    .bind(streamer_login.as_deref())
    .fetch_all(pool)
    .await?;

    let mut tag_stats: HashMap<String, TagBucket> = HashMap::new();
    for (_id, tags, avg_viewers, retention, follower_delta, duration, start_hour) in rows {
        let parsed = parse_tags(tags.as_deref().unwrap_or(""));
        let mut seen: Vec<String> = Vec::new();
        for tag in parsed.into_iter().take(5) {
            if seen.contains(&tag) {
                continue;
            }
            seen.push(tag.clone());
            let bucket = tag_stats.entry(tag).or_default();
            bucket.viewers.push(avg_viewers.unwrap_or(0.0));
            if let Some(r) = retention {
                bucket.retention.push(r * 100.0);
            }
            if let Some(f) = follower_delta {
                bucket.followers.push(f);
            }
            bucket.durations.push(duration.unwrap_or(0.0));
            if let Some(h) = start_hour {
                bucket.hours.push(h);
            }
            bucket.samples += 1;
        }
    }

    // samples >= 3, sortiert nach (median(viewers), samples) absteigend.
    let mut filtered: Vec<(String, TagBucket)> =
        tag_stats.into_iter().filter(|(_, d)| d.samples >= 3).collect();
    filtered.sort_by(|(ka, a), (kb, b)| {
        let ma = median(&a.viewers);
        let mb = median(&b.viewers);
        mb.partial_cmp(&ma)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.samples.cmp(&a.samples))
            // Deterministischer Tiebreaker: bei identischem (Median-Viewers, Samples)
            // entschied sonst die HashMap-Iterationsreihenfolge → categoryRank flackerte
            // (z.B. zwei Tags mit gleicher Performance). Tag-Name aufsteigend macht den
            // Rang stabil — in Prod (reproduzierbare Ränge) wie im Test.
            .then(ka.cmp(kb))
    });

    let tags: Vec<Value> = filtered
        .into_iter()
        .take(limit.max(0) as usize)
        .enumerate()
        .map(|(idx, (tag, data))| {
            json!({
                "tagName": tag,
                "usageCount": data.samples,
                "avgViewers": round1(median(&data.viewers)),
                "avgRetention10m": round1(median(&data.retention)),
                "avgFollowerGain": round1(median(&data.followers)),
                "trend": "stable",
                "trendValue": 0,
                "bestTimeSlot": best_time_slot(&data.hours),
                "avgStreamDuration": median(&data.durations).round(),
                "categoryRank": idx + 1,
            })
        })
        .collect();

    // peerBenchmark nur bei gesetztem streamer (Python `if streamer_login`).
    let peer_benchmark = match streamer {
        Some(login) => load_peer_benchmark(pool, login, since).await?.unwrap_or(Value::Null),
        None => Value::Null,
    };

    Ok(json!({ "tags": tags, "peerBenchmark": peer_benchmark }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn parse_tags_json_und_csv() {
        assert_eq!(parse_tags(r#"["Deadlock","Deutsch"]"#), vec!["Deadlock", "Deutsch"]);
        assert_eq!(parse_tags("Deadlock, Deutsch , "), vec!["Deadlock", "Deutsch"]);
        assert_eq!(parse_tags("[kaputt"), vec!["[kaputt"]); // JSON-Fehler → [raw]
    }

    #[test]
    fn median_und_best_slot() {
        assert_eq!(median(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
        assert_eq!(best_time_slot(&[20, 20, 14]), "20:00");
        assert_eq!(best_time_slot(&[]), "18:00-22:00");
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema), ("timezone", "UTC")]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, tags TEXT, \
             avg_viewers REAL, retention_10m REAL, follower_delta INTEGER, followers_start INTEGER, followers_end INTEGER, \
             duration_seconds REAL, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_stats_category (streamer TEXT, viewer_count INTEGER, ts_utc TIMESTAMPTZ)")
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn tag_analysis_aggregiert() {
        let Some(pool) = make_pool("t_tagx").await else { return };
        // 3 Sessions mit Tag "Deadlock" (samples>=3 → erscheint), Viewer 100/200/300.
        for v in [100, 200, 300] {
            sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, tags, avg_viewers, retention_10m, follower_delta, followers_start, followers_end, duration_seconds, started_at, ended_at) VALUES ('nani', '[\"Deadlock\",\"Deutsch\"]', $1, 0.5, 10, 100, 110, 7200, CURRENT_DATE - INTERVAL '1 day' + TIME '20:00', NOW())")
                .bind(v as f32).execute(&pool).await.unwrap();
        }
        // 1 Session mit Tag "Solo" → samples=1 < 3 → gefiltert.
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, tags, avg_viewers, retention_10m, duration_seconds, started_at, ended_at) VALUES ('nani', 'Solo', 5, 0.1, 100, NOW() - INTERVAL '1 day', NOW())")
            .execute(&pool).await.unwrap();

        let v = load_tag_analysis_extended(&pool, Some("nani"), 3650, 20).await.unwrap();
        assert!(v["peerBenchmark"].is_null());
        let tags = v["tags"].as_array().unwrap();
        // "Deadlock" + "Deutsch" haben je 3 samples; "Solo" gefiltert.
        assert_eq!(tags.len(), 2);
        let dl = tags.iter().find(|t| t["tagName"] == "Deadlock").unwrap();
        assert_eq!(dl["usageCount"], 3);
        assert_eq!(dl["avgViewers"], 200.0); // median(100,200,300)
        assert_eq!(dl["avgRetention10m"], 50.0); // 0.5*100
        assert_eq!(dl["avgFollowerGain"], 10.0);
        assert_eq!(dl["bestTimeSlot"], "20:00");
        assert_eq!(dl["categoryRank"], 1);
        assert_eq!(dl["trend"], "stable");
    }

    #[tokio::test]
    async fn peer_benchmark_tier_median() {
        let Some(pool) = make_pool("t_tagx_peer").await else { return };
        // Kategorie-Schnitte: nani=75, peer1=80 (beide "established" 50..150),
        // peer2=10 (starter, anderes Tier → ausgeschlossen).
        for (s, vc) in [("nani", 75), ("peer1", 80), ("peer2", 10)] {
            sqlx::query("INSERT INTO twitch_stats_category (streamer, viewer_count, ts_utc) VALUES ($1, $2, NOW() - INTERVAL '1 hour')")
                .bind(s).bind(vc).execute(&pool).await.unwrap();
        }
        // Session-Metriken je Peer: nani avg=100/ret=0.5, peer1 avg=200/ret=0.6.
        for (s, avg, ret) in [("nani", 100.0f32, 0.5f32), ("peer1", 200.0, 0.6)] {
            sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, tags, avg_viewers, retention_10m, duration_seconds, started_at, ended_at) VALUES ($1, 'X', $2, $3, 3600, NOW() - INTERVAL '2 hours', NOW())")
                .bind(s).bind(avg).bind(ret).execute(&pool).await.unwrap();
        }
        let v = load_tag_analysis_extended(&pool, Some("nani"), 30, 20).await.unwrap();
        let pb = &v["peerBenchmark"];
        assert!(!pb.is_null());
        assert_eq!(pb["avgViewers"], 150.0); // median(100,200)
        assert_eq!(pb["retention10m"], 55.0); // median(0.5,0.6)*100
    }

    #[tokio::test]
    async fn tag_analysis_leer() {
        let Some(pool) = make_pool("t_tagx_empty").await else { return };
        let v = load_tag_analysis_extended(&pool, None, 30, 20).await.unwrap();
        assert_eq!(v["tags"], json!([]));
        assert!(v["peerBenchmark"].is_null());
    }
}
