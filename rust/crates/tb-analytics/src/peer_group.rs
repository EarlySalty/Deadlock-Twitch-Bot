//! Peer-Group-Statistik für einen Streamer.
//!
//! Port von `bot/analytics/api_performance.py:_get_peer_group_stats` (Z.210-276).
//! Liefert die Tier-Einstufung **und** den Peer-Benchmark (`avgViewers`,
//! `retention10m`) eines Streamers anhand seiner Peer-Gruppe (gleiches Tier).
//!
//! Wird sowohl vom Title-Performance-Endpoint (`peerBenchmark`) als auch vom
//! Category-Leaderboard (`yourTier`) konsumiert. Beide Felder folgen exakt der
//! Python-Logik:
//!   - `my_avg` aus dem **ungefilterten** Kategorie-Schnitt (Fallback Sessions),
//!   - `None`, wenn keine Kategorie-Daten existieren, der Streamer keinen Schnitt
//!     hat oder keine Peers im selben Tier gefunden werden.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Ergebnis der Peer-Group-Berechnung.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerGroup {
    /// Tier-Schlüssel (`starter`/`rising`/`established`/`featured`/`top`).
    pub tier: String,
    /// Median der Peer-Durchschnitts-Viewer, auf 1 Nachkommastelle gerundet.
    pub avg_viewers: f64,
    /// Median der Peer-10-Minuten-Retention in Prozent, auf 1 Nk. gerundet.
    pub retention_10m: f64,
}

/// Tier-Schwellen 1:1 zu Python `_get_tier`.
fn tier_of(avg: f64) -> &'static str {
    if avg < 15.0 {
        "starter"
    } else if avg < 50.0 {
        "rising"
    } else if avg < 150.0 {
        "established"
    } else if avg < 500.0 {
        "featured"
    } else {
        "top"
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Berechnet Tier + Peer-Benchmark für `streamer_login` seit `since`.
///
/// `None`, wenn keine Kategorie-Daten vorliegen, der Streamer keinen Schnitt hat
/// oder keine Peers im selben Tier existieren (Python-Parität).
pub async fn peer_group_stats(
    pool: &PgPool,
    streamer_login: &str,
    since: DateTime<Utc>,
) -> Result<Option<PeerGroup>, sqlx::Error> {
    let login = streamer_login.to_lowercase();

    // 1. Durchschnitts-Viewer aller Streamer der Kategorie (ungefiltert).
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

    // 2. Eigener Schnitt — Fallback aus Sessions, wenn nicht in Kategorie-Daten.
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

    // 4. Session-Metriken je Peer (avg_viewers + retention_10m).
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

    Ok(Some(PeerGroup {
        tier: my_tier.to_string(),
        avg_viewers: round1(median(&avg_viewers_list)),
        retention_10m: round1(median(&retention_list) * 100.0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
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
            "CREATE TABLE twitch_stats_category (\
                streamer TEXT, viewer_count INTEGER, ts_utc TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .expect("DDL stats_category");
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (\
                id BIGSERIAL PRIMARY KEY, streamer_login TEXT, \
                avg_viewers FLOAT8, retention_10m FLOAT8, \
                started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .expect("DDL stream_sessions");
        pool
    }

    #[test]
    fn tier_grenzen() {
        assert_eq!(tier_of(0.0), "starter");
        assert_eq!(tier_of(14.9), "starter");
        assert_eq!(tier_of(15.0), "rising");
        assert_eq!(tier_of(49.9), "rising");
        assert_eq!(tier_of(50.0), "established");
        assert_eq!(tier_of(150.0), "featured");
        assert_eq!(tier_of(500.0), "top");
    }

    #[test]
    fn median_gerade_und_ungerade() {
        assert_eq!(median(&[1.0, 3.0, 2.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[tokio::test]
    async fn keine_kategorie_daten_gibt_none() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_pg_empty").await;
        let since = Utc::now() - Duration::days(30);
        let res = peer_group_stats(&pool, "wer", since).await.unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn peer_group_mit_peers_liefert_tier_und_benchmark() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_pg_peers").await;
        let since = Utc::now() - Duration::days(30);

        // me + ein Peer im selben Tier (rising: 15-50), und ein Streamer im
        // anderen Tier (top), der NICHT als Peer zählen darf.
        sqlx::query(
            "INSERT INTO twitch_stats_category (streamer, viewer_count, ts_utc) VALUES \
             ('me',   30, NOW() - INTERVAL '1 day'), \
             ('peer', 40, NOW() - INTERVAL '1 day'), \
             ('whale', 800, NOW() - INTERVAL '1 day')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, avg_viewers, retention_10m, started_at, ended_at) VALUES \
             ('me',   30.0, 0.5, NOW() - INTERVAL '2 days', NOW() - INTERVAL '2 days' + INTERVAL '2 hours'), \
             ('peer', 40.0, 0.7, NOW() - INTERVAL '2 days', NOW() - INTERVAL '2 days' + INTERVAL '2 hours')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let res = peer_group_stats(&pool, "me", since).await.unwrap().unwrap();
        assert_eq!(res.tier, "rising");
        // Median über zwei Peer-Schnitte (30, 40) = 35.0
        assert_eq!(res.avg_viewers, 35.0);
        // Median über Retention (0.5, 0.7)=0.6 → *100 = 60.0
        assert_eq!(res.retention_10m, 60.0);
    }

    #[tokio::test]
    async fn streamer_ohne_schnitt_gibt_none() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_pg_noavg").await;
        let since = Utc::now() - Duration::days(30);
        // Kategorie-Daten existieren, aber nicht für 'ghost' und keine Sessions.
        sqlx::query(
            "INSERT INTO twitch_stats_category (streamer, viewer_count, ts_utc) VALUES ('other', 20, NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        let res = peer_group_stats(&pool, "ghost", since).await.unwrap();
        assert!(res.is_none());
    }
}
