//! Handler für `GET /twitch/api/v2/category-timings`.
//!
//! Port von `_load_category_timings_payload_sync` (api_performance.py:1311).
//! Methode: Median der Streamer-Mediane ("median of medians") + P25/P75.
//! Postgres PERCENTILE_CONT(0.5) ersetzt die Python-seitige Zeilen-Gruppierung.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};
use std::collections::HashMap;

use crate::auth::level::DashboardAuthLevel;
use crate::query_int::parse_bounded_query_int;

/// Median des sortierten Slice (exakt wie Python `statistics.median`).
fn median_sorted(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0)
    }
}

/// Quantile mit Python's `statistics.quantiles`-"exclusive"-Methode:
/// virtueller Index `x = (len+1)*q - 1` (0-basiert), dann lineare Interpolation.
fn quantile_exclusive(sorted: &[f64], q: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return sorted[0];
    }
    let x = (n as f64 + 1.0) * q - 1.0;
    if x <= 0.0 {
        return sorted[0];
    }
    let max_idx = (n - 1) as f64;
    if x >= max_idx {
        return sorted[n - 1];
    }
    let j = x.floor() as usize;
    let frac = x - j as f64;
    sorted[j] + frac * (sorted[j + 1] - sorted[j])
}

struct SlotStats {
    median: Option<f64>,
    p25: Option<f64>,
    p75: Option<f64>,
    streamer_count: usize,
    sample_count: usize,
}

/// Repliziert `_robust_stats` aus api_performance.py:1365.
fn robust_stats(mut per_streamer: Vec<f64>, sample_count: usize) -> SlotStats {
    per_streamer.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let count = per_streamer.len();
    if count == 0 {
        return SlotStats {
            median: None,
            p25: None,
            p75: None,
            streamer_count: 0,
            sample_count: 0,
        };
    }
    let med = median_sorted(&per_streamer);
    let (p25, p75) = if count >= 4 {
        (
            Some(quantile_exclusive(&per_streamer, 0.25)),
            Some(quantile_exclusive(&per_streamer, 0.75)),
        )
    } else if count >= 2 {
        (Some(per_streamer[0]), Some(per_streamer[count - 1]))
    } else {
        (Some(per_streamer[0]), Some(per_streamer[0]))
    };
    SlotStats {
        median: med,
        p25,
        p75,
        streamer_count: count,
        sample_count,
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[derive(Deserialize)]
pub struct TimingsQuery {
    // Rohwert: nicht-numerisches `days` → Python-konformes 400-JSON, siehe query_int.
    pub days: Option<String>,
    pub source: Option<String>,
}

/// `GET /twitch/api/v2/category-timings?days=30&source=category`
pub async fn category_timings_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<TimingsQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 90) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    let use_tracked = params.source.as_deref() == Some("tracked");
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    let table = if use_tracked {
        "twitch_stats_tracked"
    } else {
        "twitch_stats_category"
    };

    // Q1: Per-(streamer, hour) — Median + Sample-Count via PERCENTILE_CONT
    let hour_sql = format!(
        "SELECT streamer,
                EXTRACT(HOUR FROM ts_utc)::int AS hour,
                PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY viewer_count) AS med_vc,
                COUNT(*) AS cnt
         FROM {table}
         WHERE ts_utc >= $1 AND viewer_count IS NOT NULL AND viewer_count > 0
         GROUP BY streamer, EXTRACT(HOUR FROM ts_utc)::int"
    );
    let hour_rows = sqlx::query(&hour_sql)
        .bind(since)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

    // Q2: Per-(streamer, weekday)
    let dow_sql = format!(
        "SELECT streamer,
                EXTRACT(DOW FROM ts_utc)::int AS dow,
                PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY viewer_count) AS med_vc,
                COUNT(*) AS cnt
         FROM {table}
         WHERE ts_utc >= $1 AND viewer_count IS NOT NULL AND viewer_count > 0
         GROUP BY streamer, EXTRACT(DOW FROM ts_utc)::int"
    );
    let dow_rows = sqlx::query(&dow_sql)
        .bind(since)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

    // hour_data[hour][streamer] = median_vc
    // hour_samples[hour] = total sample count
    let mut hour_data: HashMap<i32, HashMap<String, f64>> = HashMap::new();
    let mut hour_samples: HashMap<i32, usize> = HashMap::new();
    let mut all_streamers: std::collections::HashSet<String> = std::collections::HashSet::new();

    for row in &hour_rows {
        let streamer: String = row.try_get("streamer").unwrap_or_default();
        let hour: i32 = row.try_get("hour").unwrap_or(-1);
        let med: f64 = row
            .try_get::<Option<f64>, _>("med_vc")
            .unwrap_or(None)
            .unwrap_or(0.0);
        let cnt: i64 = row.try_get("cnt").unwrap_or(0);
        if !(0..24).contains(&hour) {
            continue;
        }
        hour_data
            .entry(hour)
            .or_default()
            .insert(streamer.clone(), med);
        *hour_samples.entry(hour).or_default() += cnt as usize;
        all_streamers.insert(streamer);
    }

    let mut dow_data: HashMap<i32, HashMap<String, f64>> = HashMap::new();
    let mut dow_samples: HashMap<i32, usize> = HashMap::new();

    for row in &dow_rows {
        let streamer: String = row.try_get("streamer").unwrap_or_default();
        let dow: i32 = row.try_get("dow").unwrap_or(-1);
        let med: f64 = row
            .try_get::<Option<f64>, _>("med_vc")
            .unwrap_or(None)
            .unwrap_or(0.0);
        let cnt: i64 = row.try_get("cnt").unwrap_or(0);
        if !(0..7).contains(&dow) {
            continue;
        }
        dow_data
            .entry(dow)
            .or_default()
            .insert(streamer.clone(), med);
        *dow_samples.entry(dow).or_default() += cnt as usize;
        all_streamers.insert(streamer);
    }

    let total_streamers = all_streamers.len();

    // ── Stündliche Ausgabe ────────────────────────────────────────────────────
    let hourly: Vec<serde_json::Value> = (0..24)
        .map(|hour| {
            let slot_map = hour_data.get(&hour).cloned().unwrap_or_default();
            let sample_count = *hour_samples.get(&hour).unwrap_or(&0);
            let per_streamer: Vec<f64> = slot_map.values().copied().collect();
            let s = robust_stats(per_streamer, sample_count);
            json!({
                "hour": hour,
                "median": s.median.map(round1),
                "p25": s.p25.map(round1),
                "p75": s.p75.map(round1),
                "streamer_count": s.streamer_count,
                "sample_count": s.sample_count,
            })
        })
        .collect();

    // ── Wöchentliche Ausgabe (Mo–Sa–So Reihenfolge wie in Python) ────────────
    let weekday_names = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];
    let weekday_order: [i32; 7] = [1, 2, 3, 4, 5, 6, 0];
    let weekly: Vec<serde_json::Value> = weekday_order
        .iter()
        .map(|&wd| {
            let slot_map = dow_data.get(&wd).cloned().unwrap_or_default();
            let sample_count = *dow_samples.get(&wd).unwrap_or(&0);
            let per_streamer: Vec<f64> = slot_map.values().copied().collect();
            let s = robust_stats(per_streamer, sample_count);
            json!({
                "weekday": wd,
                "label": weekday_names[wd as usize],
                "median": s.median.map(round1),
                "p25": s.p25.map(round1),
                "p75": s.p75.map(round1),
                "streamer_count": s.streamer_count,
                "sample_count": s.sample_count,
            })
        })
        .collect();

    Json(json!({
        "hourly": hourly,
        "weekly": weekly,
        "total_streamers": total_streamers,
        "window_days": days,
        "method": "median_of_medians",
    }))
    .into_response()
}
