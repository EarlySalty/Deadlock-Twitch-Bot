//! Coaching-Engine (`/twitch/api/v2/coaching`).
//!
//! Port von `bot/analytics/coaching_engine.py` (1632 Z., regelbasiert/kein AI).
//! `get_coaching_data` ruft ~12 self-contained Analyse-Funktionen + einen
//! Recommendations-Builder. Wird in verifizierten Teil-Slices portiert — je
//! Analyse eine `pub fn`. **Teil 1: `_efficiency`** (Viewer-Stunden/Stream-Stunde
//! + Wachstum/10h, je mit Kategorie-Schnitt [Top-15 % gefiltert] + Perzentil).

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// p85-Schwelle (Python `sorted[max(0, int(len*0.85)-1)]`).
fn p85_threshold(sorted: &[f64]) -> f64 {
    let idx = ((sorted.len() as f64 * 0.85) as usize).saturating_sub(1);
    sorted[idx]
}

fn empty_efficiency() -> Value {
    json!({
        "viewerHoursPerStreamHour": 0,
        "categoryAvg": 0,
        "topPerformers": [],
        "percentile": 0,
        "totalStreamHours": 0,
        "totalViewerHours": 0,
        "growthPer10Hours": 0,
        "growthCategoryAvg": 0,
        "growthTopPerformers": [],
        "growthPercentile": 0,
    })
}

/// Effizienz-Analyse (Python `_efficiency`).
pub async fn efficiency(pool: &PgPool, streamer: &str, since: DateTime<Utc>) -> Result<Value, sqlx::Error> {
    // 1) Viewer-Stunden / Stream-Stunden je Streamer.
    let rows: Vec<(String, Option<f64>, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT s.streamer_login, \
                SUM(s.avg_viewers * s.duration_seconds / 3600.0)::float8, \
                SUM(s.duration_seconds / 3600.0)::float8, \
                (SUM(s.avg_viewers * s.duration_seconds / 3600.0) / NULLIF(SUM(s.duration_seconds / 3600.0), 0))::float8 \
           FROM twitch_stream_sessions s \
          WHERE s.started_at >= $1 AND s.duration_seconds > 300 \
          GROUP BY s.streamer_login \
         HAVING SUM(s.duration_seconds) / 3600.0 > 1 \
          ORDER BY 4 DESC",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let mut ratios: Vec<(String, f64)> = Vec::new();
    let mut your_ratio = 0.0;
    let mut your_vh = 0.0;
    let mut your_sh = 0.0;
    for (login, vh, sh, ratio) in &rows {
        let ratio = ratio.unwrap_or(0.0);
        ratios.push((login.clone(), ratio));
        if login == streamer {
            your_ratio = ratio;
            your_vh = vh.unwrap_or(0.0);
            your_sh = sh.unwrap_or(0.0);
        }
    }

    if ratios.is_empty() {
        return Ok(empty_efficiency());
    }

    let all_ratios: Vec<f64> = ratios.iter().map(|(_, v)| *v).collect();
    let mut sorted_ratios = all_ratios.clone();
    sorted_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let threshold = p85_threshold(&sorted_ratios);
    let filtered: Vec<&(String, f64)> = ratios.iter().filter(|(_, v)| *v <= threshold).collect();
    let cat_avg = if !filtered.is_empty() {
        filtered.iter().map(|(_, v)| *v).sum::<f64>() / filtered.len() as f64
    } else {
        all_ratios.iter().sum::<f64>() / all_ratios.len() as f64
    };
    let below = all_ratios.iter().filter(|r| **r < your_ratio).count();
    let percentile = (below as f64 / all_ratios.len() as f64 * 100.0) as i64;
    let top_performers: Vec<Value> = filtered
        .iter()
        .take(5)
        .map(|(login, v)| json!({ "streamer": login, "ratio": round1(*v) }))
        .collect();

    // 2) Wachstum: gewonnene Follower je 10 Stream-Stunden.
    let growth_rows: Vec<(String, Option<f64>)> = sqlx::query_as(
        "SELECT s.streamer_login, \
                (SUM(CASE WHEN s.follower_delta > 0 THEN s.follower_delta ELSE 0 END) / NULLIF(SUM(s.duration_seconds / 3600.0), 0) * 10.0)::float8 \
           FROM twitch_stream_sessions s \
          WHERE s.started_at >= $1 AND s.duration_seconds > 300 \
          GROUP BY s.streamer_login \
         HAVING SUM(s.duration_seconds) / 3600.0 > 1 \
          ORDER BY 2 DESC",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let mut your_growth = 0.0;
    let mut growth_ratios: Vec<(String, f64)> = Vec::new();
    for (login, g) in &growth_rows {
        let g = g.unwrap_or(0.0);
        growth_ratios.push((login.clone(), g));
        if login == streamer {
            your_growth = g;
        }
    }
    let all_growth: Vec<f64> = growth_ratios.iter().map(|(_, g)| *g).collect();
    let (growth_cat_avg, growth_top, growth_percentile): (f64, Vec<Value>, i64) = if !all_growth.is_empty() {
        let mut sorted_g = all_growth.clone();
        sorted_g.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let gt = p85_threshold(&sorted_g);
        let fg: Vec<&(String, f64)> = growth_ratios.iter().filter(|(_, g)| *g <= gt).collect();
        let avg = if !fg.is_empty() {
            fg.iter().map(|(_, g)| *g).sum::<f64>() / fg.len() as f64
        } else {
            0.0
        };
        let top: Vec<Value> = fg.iter().take(5).map(|(login, g)| json!({ "streamer": login, "value": round1(*g) })).collect();
        let below_g = all_growth.iter().filter(|g| **g < your_growth).count();
        (avg, top, (below_g as f64 / all_growth.len() as f64 * 100.0) as i64)
    } else {
        (0.0, Vec::new(), 0)
    };

    Ok(json!({
        "viewerHoursPerStreamHour": round1(your_ratio),
        "categoryAvg": round1(cat_avg),
        "topPerformers": top_performers,
        "percentile": percentile,
        "totalStreamHours": round1(your_sh),
        "totalViewerHours": round1(your_vh),
        "growthPer10Hours": round1(your_growth),
        "growthCategoryAvg": round1(growth_cat_avg),
        "growthTopPerformers": growth_top,
        "growthPercentile": growth_percentile,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, duration_seconds INTEGER, avg_viewers REAL, follower_delta INTEGER)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn efficiency_leer() {
        let Some(pool) = make_pool("t_coach_eff_empty").await else { return };
        let v = efficiency(&pool, "nani", Utc::now() - chrono::Duration::days(30)).await.unwrap();
        assert_eq!(v["viewerHoursPerStreamHour"], 0);
        assert_eq!(v["topPerformers"], json!([]));
    }

    #[tokio::test]
    async fn efficiency_berechnet() {
        let Some(pool) = make_pool("t_coach_eff").await else { return };
        // nani: 2h Stream, avg 50 → viewer_hours 100, ratio 50. other: 2h, avg 10 → ratio 10.
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds, avg_viewers, follower_delta) VALUES \
            ('nani', NOW()-INTERVAL '1 day', 7200, 50, 20), \
            ('other', NOW()-INTERVAL '1 day', 7200, 10, 5)")
            .execute(&pool).await.unwrap();
        let v = efficiency(&pool, "nani", Utc::now() - chrono::Duration::days(30)).await.unwrap();
        assert_eq!(v["viewerHoursPerStreamHour"], 50.0);
        assert_eq!(v["totalStreamHours"], 2.0);
        assert_eq!(v["totalViewerHours"], 100.0);
        // nani ratio 50 > other 10 → percentile 50 (1 von 2 darunter).
        assert_eq!(v["percentile"], 50);
        // Wachstum: nani 20 Follower / 2h *10 = 100/h... 20/2*10=100.
        assert_eq!(v["growthPer10Hours"], 100.0);
    }
}
