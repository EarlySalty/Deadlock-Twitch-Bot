//! Handler für `GET /twitch/api/v2/category-comparison`.
//!
//! Port von `_load_category_comparison_payload_sync` (api_performance.py:861).
//! 8 SQL-Queries, Python-seitige Percentile-Berechnungen in Rust repliziert.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;
use crate::query_int::parse_bounded_query_int;

const EXTERNAL_REACH_AVG_THRESHOLD: f64 = 100.0;

fn get_tier(avg: f64) -> (&'static str, &'static str) {
    if avg < 15.0 {
        ("starter", "Starter (0–15 Ø)")
    } else if avg < 50.0 {
        ("rising", "Rising (15–50 Ø)")
    } else if avg < 150.0 {
        ("established", "Established (50–150 Ø)")
    } else if avg < 500.0 {
        ("featured", "Featured (150–500 Ø)")
    } else {
        ("top", "Top (500+ Ø)")
    }
}

/// Python's `_percentile_of`: (below + 0.5*equal) / total * 100, als i32.
fn percentile_of(sorted: &[f64], value: f64) -> i32 {
    if sorted.is_empty() {
        return 50;
    }
    let below = sorted.partition_point(|&v| v < value);
    let above = sorted.partition_point(|&v| v <= value);
    let equal = above - below;
    ((below as f64 + 0.5 * equal as f64) / sorted.len() as f64 * 100.0) as i32
}

/// Python's `_peer_percentile`: count_below / total * 100.
fn peer_percentile_of(sorted: &[f64], value: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let below = sorted.partition_point(|&v| v < value);
    Some((below as f64 / sorted.len() as f64 * 1000.0).round() / 10.0)
}

fn safe_median(sorted: &[f64]) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[mid])
    } else {
        Some((sorted[mid - 1] + sorted[mid]) / 2.0)
    }
}

#[derive(Deserialize)]
pub struct ComparisonQuery {
    pub streamer: Option<String>,
    // Rohwert: nicht-numerisches `days` → Python-konformes 400-JSON, siehe query_int.
    pub days: Option<String>,
    pub exclude_external: Option<String>,
}

/// `GET /twitch/api/v2/category-comparison?streamer=&days=30&exclude_external=0`
pub async fn category_comparison_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ComparisonQuery>,
) -> impl IntoResponse {
    // Python _api_v2_category_comparison ruft NUR _require_v2_auth (KEIN
    // _require_extended_plan) — also reiner Authentifizierungs-Check, kein
    // Plan-Gate. (Rust hatte hier fälschlich extended_gate → 403 für
    // authentifizierte Nicht-Extended-Partner.)
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }
    // days VOR streamer-Pflicht (Python-Reihenfolge in _api_v2_category_comparison).
    let days = match parse_bounded_query_int(params.days.as_deref(), "days", 30, 7, 365) {
        Ok(d) => d,
        Err(resp) => return resp.into_response(),
    };
    // IDOR-Guard: NUR der Subjekt-Streamer (yourStats/categoryRank) wird auf den
    // eigenen Login geklemmt — Partner sehen ihre eigenen Werte gegen die
    // Kategorie-Aggregate (categoryAvg/percentiles/peerGroup bleiben global über
    // alle Streamer berechnet). Admin/Localhost dürfen `streamer` frei wählen.
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"Streamer required"})),
                )
                    .into_response();
            }
            Err(resp) => return resp,
        };
    let exclude_external = params.exclude_external.as_deref() == Some("1");
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    // ── Q1: Your tracked stats ──────────────────────────────────────────────
    let tracked_row = sqlx::query(
        "SELECT AVG(viewer_count)::float8 AS avg_vc, MAX(viewer_count)::float8 AS peak_vc
         FROM twitch_stats_tracked
         WHERE ts_utc >= $1 AND LOWER(streamer) = $2",
    )
    .bind(since)
    .bind(&streamer)
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();

    let your_tracked_avg: f64 = tracked_row
        .as_ref()
        .and_then(|r| r.try_get::<Option<f64>, _>("avg_vc").ok().flatten())
        .unwrap_or(0.0);
    let your_tracked_peak: i64 = tracked_row
        .as_ref()
        .and_then(|r| r.try_get::<Option<f64>, _>("peak_vc").ok().flatten())
        .map(|v| v as i64)
        .unwrap_or(0);

    // ── Q2: Your session stats ──────────────────────────────────────────────
    let sess_row = sqlx::query(r#"
        SELECT AVG(avg_viewers) AS avg_v,
               MAX(peak_viewers)::float8 AS peak_v,
               AVG(retention_10m) AS ret10,
               AVG(CASE WHEN avg_viewers > 0 THEN unique_chatters * 100.0 / avg_viewers ELSE 0 END) AS chat_h
        FROM twitch_stream_sessions
        WHERE started_at >= $1 AND LOWER(streamer_login) = $2 AND ended_at IS NOT NULL
    "#).bind(since).bind(&streamer).fetch_optional(&pool).await.ok().flatten();

    let your_avg = if your_tracked_avg > 0.0 {
        your_tracked_avg
    } else {
        sess_row
            .as_ref()
            .and_then(|r| r.try_get::<Option<f64>, _>("avg_v").ok().flatten())
            .unwrap_or(0.0)
    };
    let your_peak = if your_tracked_peak > 0 {
        your_tracked_peak
    } else {
        sess_row
            .as_ref()
            .and_then(|r| r.try_get::<Option<f64>, _>("peak_v").ok().flatten())
            .map(|v| v as i64)
            .unwrap_or(0)
    };
    let your_ret = sess_row
        .as_ref()
        .and_then(|r| r.try_get::<Option<f64>, _>("ret10").ok().flatten())
        .map(|v| v * 100.0)
        .unwrap_or(0.0);
    let your_chat = sess_row
        .as_ref()
        .and_then(|r| r.try_get::<Option<f64>, _>("chat_h").ok().flatten())
        .unwrap_or(0.0);

    // ── Q3: All category avgs (unfiltered — needed for peer group + percentile base) ─
    let all_avgs_rows = sqlx::query(
        "SELECT streamer, AVG(viewer_count)::float8 AS avg_vc FROM twitch_stats_category
         WHERE ts_utc >= $1 GROUP BY streamer ORDER BY avg_vc",
    )
    .bind(since)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let all_avgs: Vec<(String, f64)> = all_avgs_rows
        .iter()
        .filter_map(|r| {
            let s: String = r.try_get("streamer").ok()?;
            let v: f64 = r.try_get::<Option<f64>, _>("avg_vc").ok().flatten()?;
            Some((s.to_lowercase(), v))
        })
        .collect();

    // Threshold-filtered avg list for percentile calculation
    let sorted_avgs: Vec<f64> = if exclude_external {
        all_avgs
            .iter()
            .filter(|(_, v)| *v <= EXTERNAL_REACH_AVG_THRESHOLD)
            .map(|(_, v)| *v)
            .collect()
    } else {
        all_avgs.iter().map(|(_, v)| *v).collect()
    };
    let category_total = sorted_avgs.len();

    let cat_avg_viewers = if sorted_avgs.is_empty() {
        0.0
    } else {
        sorted_avgs.iter().sum::<f64>() / sorted_avgs.len() as f64
    };

    // ── Q4: Category peak avg (with optional threshold) ─────────────────────
    let cat_avg_peak: f64 = if exclude_external {
        sqlx::query(
            "SELECT AVG(max_vc)::float8 AS r FROM (
                 SELECT MAX(viewer_count) AS max_vc FROM twitch_stats_category
                 WHERE ts_utc >= $1 GROUP BY streamer HAVING AVG(viewer_count) <= $2
             ) s",
        )
        .bind(since)
        .bind(EXTERNAL_REACH_AVG_THRESHOLD)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<Option<f64>, _>("r").ok().flatten())
        .unwrap_or(0.0)
    } else {
        sqlx::query(
            "SELECT AVG(max_vc)::float8 AS r FROM (
                 SELECT MAX(viewer_count) AS max_vc FROM twitch_stats_category
                 WHERE ts_utc >= $1 GROUP BY streamer
             ) s",
        )
        .bind(since)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<Option<f64>, _>("r").ok().flatten())
        .unwrap_or(0.0)
    };

    // ── Q5: Category session averages (ret + chat) ──────────────────────────
    let (cat_avg_ret, cat_avg_chat): (f64, f64) = if exclude_external {
        let r = sqlx::query(r#"
            SELECT AVG(retention_10m) AS avg_ret,
                   AVG(CASE WHEN avg_viewers > 0 THEN unique_chatters * 100.0 / avg_viewers ELSE 0 END) AS avg_chat
            FROM twitch_stream_sessions
            WHERE started_at >= $1 AND ended_at IS NOT NULL
              AND LOWER(streamer_login) NOT IN (
                  SELECT LOWER(streamer_login) FROM twitch_stream_sessions
                  WHERE started_at >= $1 AND ended_at IS NOT NULL
                  GROUP BY LOWER(streamer_login) HAVING AVG(avg_viewers) > $2
              )
        "#).bind(since).bind(EXTERNAL_REACH_AVG_THRESHOLD).fetch_optional(&pool).await.ok().flatten();
        let ret = r
            .as_ref()
            .and_then(|row| row.try_get::<Option<f64>, _>("avg_ret").ok().flatten())
            .unwrap_or(0.0)
            * 100.0;
        let chat = r
            .as_ref()
            .and_then(|row| row.try_get::<Option<f64>, _>("avg_chat").ok().flatten())
            .unwrap_or(0.0);
        (ret, chat)
    } else {
        let r = sqlx::query(r#"
            SELECT AVG(retention_10m) AS avg_ret,
                   AVG(CASE WHEN avg_viewers > 0 THEN unique_chatters * 100.0 / avg_viewers ELSE 0 END) AS avg_chat
            FROM twitch_stream_sessions
            WHERE started_at >= $1 AND ended_at IS NOT NULL
        "#).bind(since).fetch_optional(&pool).await.ok().flatten();
        let ret = r
            .as_ref()
            .and_then(|row| row.try_get::<Option<f64>, _>("avg_ret").ok().flatten())
            .unwrap_or(0.0)
            * 100.0;
        let chat = r
            .as_ref()
            .and_then(|row| row.try_get::<Option<f64>, _>("avg_chat").ok().flatten())
            .unwrap_or(0.0);
        (ret, chat)
    };

    // ── Q6+Q7: Per-streamer ret + chat sorted lists ─────────────────────────
    let mut ret_sorted: Vec<f64>;
    let mut chat_sorted: Vec<f64>;
    if exclude_external {
        let having = "HAVING AVG(avg_viewers) <= $2";
        let ret_rows = sqlx::query(&format!(
            "SELECT AVG(retention_10m) AS ret FROM twitch_stream_sessions
             WHERE started_at >= $1 AND ended_at IS NOT NULL
             GROUP BY LOWER(streamer_login) {having} ORDER BY ret"
        ))
        .bind(since)
        .bind(EXTERNAL_REACH_AVG_THRESHOLD)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        ret_sorted = ret_rows
            .iter()
            .filter_map(|r| r.try_get::<Option<f64>, _>("ret").ok().flatten())
            .map(|v| v * 100.0)
            .collect();
        ret_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let chat_rows = sqlx::query(&format!(
            "SELECT AVG(CASE WHEN avg_viewers > 0 THEN unique_chatters * 100.0 / avg_viewers ELSE 0 END) AS ch
             FROM twitch_stream_sessions
             WHERE started_at >= $1 AND ended_at IS NOT NULL
             GROUP BY LOWER(streamer_login) {having} ORDER BY ch"
        )).bind(since).bind(EXTERNAL_REACH_AVG_THRESHOLD).fetch_all(&pool).await.unwrap_or_default();
        chat_sorted = chat_rows
            .iter()
            .filter_map(|r| r.try_get::<Option<f64>, _>("ch").ok().flatten())
            .collect();
        chat_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    } else {
        let ret_rows = sqlx::query(
            "SELECT AVG(retention_10m) AS ret FROM twitch_stream_sessions
             WHERE started_at >= $1 AND ended_at IS NOT NULL
             GROUP BY LOWER(streamer_login) ORDER BY ret",
        )
        .bind(since)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        ret_sorted = ret_rows
            .iter()
            .filter_map(|r| r.try_get::<Option<f64>, _>("ret").ok().flatten())
            .map(|v| v * 100.0)
            .collect();
        ret_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let chat_rows = sqlx::query(
            "SELECT AVG(CASE WHEN avg_viewers > 0 THEN unique_chatters * 100.0 / avg_viewers ELSE 0 END) AS ch
             FROM twitch_stream_sessions
             WHERE started_at >= $1 AND ended_at IS NOT NULL
             GROUP BY LOWER(streamer_login) ORDER BY ch"
        ).bind(since).fetch_all(&pool).await.unwrap_or_default();
        chat_sorted = chat_rows
            .iter()
            .filter_map(|r| r.try_get::<Option<f64>, _>("ch").ok().flatten())
            .collect();
        chat_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    }

    // ── Q8: Peak sorted list ─────────────────────────────────────────────────
    let peak_sorted: Vec<f64> = if exclude_external {
        sqlx::query(
            "SELECT MAX(viewer_count)::float8 AS peak FROM twitch_stats_category
             WHERE ts_utc >= $1 GROUP BY streamer HAVING AVG(viewer_count) <= $2 ORDER BY peak",
        )
        .bind(since)
        .bind(EXTERNAL_REACH_AVG_THRESHOLD)
        .fetch_all(&pool)
        .await
    } else {
        sqlx::query(
            "SELECT MAX(viewer_count)::float8 AS peak FROM twitch_stats_category
             WHERE ts_utc >= $1 GROUP BY streamer ORDER BY peak",
        )
        .bind(since)
        .fetch_all(&pool)
        .await
    }
    .unwrap_or_default()
    .iter()
    .filter_map(|r| r.try_get::<Option<f64>, _>("peak").ok().flatten())
    .collect();

    // ── Percentiles ──────────────────────────────────────────────────────────
    let avg_percentile = percentile_of(&sorted_avgs, your_avg);
    let peak_percentile = percentile_of(&peak_sorted, your_peak as f64);
    let ret_percentile = percentile_of(&ret_sorted, your_ret);
    let chat_percentile = percentile_of(&chat_sorted, your_chat);
    // Rang exakt wie Python (api_performance.py:1016): category_total -
    // int(avg_percentile/100 * category_total) — inkl. Integer-Trunkierung, damit
    // der Rang auch bei Zwischenwerten (your_avg strikt zwischen zwei Peers) stimmt.
    let category_rank = if category_total > 0 {
        (category_total as i64) - ((avg_percentile as f64 / 100.0 * category_total as f64) as i64)
    } else {
        0
    };

    // ── Peer group (no threshold — same as Python _get_peer_group_stats) ─────
    let my_avg_for_tier = if your_avg > 0.0 {
        your_avg
    } else {
        all_avgs
            .iter()
            .find(|(s, _)| s == &streamer)
            .map(|(_, v)| *v)
            .unwrap_or(0.0)
    };

    let peer_group: serde_json::Value = if my_avg_for_tier > 0.0 {
        let (my_tier, my_tier_label) = get_tier(my_avg_for_tier);
        let peer_logins: Vec<String> = all_avgs
            .iter()
            .filter(|(_, avg)| get_tier(*avg).0 == my_tier)
            .map(|(s, _)| s.clone())
            .collect();
        let tier_size = peer_logins.len();

        if peer_logins.is_empty() {
            json!(null)
        } else {
            // Q9: Peer session metrics
            let peer_rows = sqlx::query(r#"
                SELECT LOWER(streamer_login) AS login,
                       AVG(avg_viewers) AS avg_v,
                       MAX(peak_viewers)::float8 AS peak_v,
                       AVG(retention_10m) AS ret10,
                       AVG(CASE WHEN avg_viewers > 0 THEN unique_chatters * 100.0 / avg_viewers ELSE 0 END) AS chat_h
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = ANY($1::text[])
                  AND started_at >= $2 AND ended_at IS NOT NULL
                GROUP BY LOWER(streamer_login)
            "#).bind(&peer_logins[..]).bind(since).fetch_all(&pool).await.unwrap_or_default();

            let mut avg_list: Vec<f64> = peer_rows
                .iter()
                .filter_map(|r| r.try_get::<Option<f64>, _>("avg_v").ok().flatten())
                .collect();
            avg_list.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut peak_list: Vec<f64> = peer_rows
                .iter()
                .filter_map(|r| r.try_get::<Option<f64>, _>("peak_v").ok().flatten())
                .collect();
            peak_list.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut ret_list: Vec<f64> = peer_rows
                .iter()
                .filter_map(|r| r.try_get::<Option<f64>, _>("ret10").ok().flatten())
                .map(|v| v * 100.0)
                .collect();
            ret_list.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut chat_list: Vec<f64> = peer_rows
                .iter()
                .filter_map(|r| r.try_get::<Option<f64>, _>("chat_h").ok().flatten())
                .collect();
            chat_list.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let my_row = peer_rows.iter().find(|r| {
                r.try_get::<String, _>("login").ok().as_deref() == Some(streamer.as_str())
            });
            let my_peer_avg = my_row
                .and_then(|r| r.try_get::<Option<f64>, _>("avg_v").ok().flatten())
                .unwrap_or(my_avg_for_tier);
            let my_peer_peak =
                my_row.and_then(|r| r.try_get::<Option<f64>, _>("peak_v").ok().flatten());
            let my_peer_ret = my_row
                .and_then(|r| r.try_get::<Option<f64>, _>("ret10").ok().flatten())
                .map(|v| v * 100.0);
            let my_peer_chat =
                my_row.and_then(|r| r.try_get::<Option<f64>, _>("chat_h").ok().flatten());

            let round1 = |v: f64| (v * 10.0).round() / 10.0;

            json!({
                "tier": my_tier,
                "tierLabel": my_tier_label,
                "tierSize": tier_size,
                "peerAvg": {
                    "avgViewers":   round1(safe_median(&avg_list).unwrap_or(0.0)),
                    "peakViewers":  (safe_median(&peak_list).unwrap_or(0.0)).round(),
                    "retention10m": round1(safe_median(&ret_list).unwrap_or(0.0)),
                    "chatHealth":   round1(safe_median(&chat_list).unwrap_or(0.0)),
                },
                "peerPercentiles": {
                    "avgViewers":   peer_percentile_of(&avg_list, my_peer_avg),
                    "peakViewers":  my_peer_peak.and_then(|v| peer_percentile_of(&peak_list, v)),
                    "retention10m": my_peer_ret.and_then(|v| peer_percentile_of(&ret_list, v)),
                    "chatHealth":   my_peer_chat.and_then(|v| peer_percentile_of(&chat_list, v)),
                },
            })
        }
    } else {
        json!(null)
    };

    let round1 = |v: f64| (v * 10.0).round() / 10.0;

    Json(json!({
        "yourStats": {
            "avgViewers":   round1(your_avg),
            "peakViewers":  your_peak,
            "retention10m": round1(your_ret),
            "chatHealth":   round1(your_chat),
        },
        "categoryAvg": {
            "avgViewers":   round1(cat_avg_viewers),
            "peakViewers":  cat_avg_peak.round(),
            "retention10m": round1(cat_avg_ret),
            "chatHealth":   round1(cat_avg_chat),
        },
        "percentiles": {
            "avgViewers":   avg_percentile,
            "peakViewers":  peak_percentile,
            "retention10m": ret_percentile,
            "chatHealth":   chat_percentile,
        },
        "categoryRank":  category_rank,
        "categoryTotal": category_total,
        "peerGroup":     peer_group,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    // Leeres Schema genügt: der IDOR-403 schlägt vor jedem DB-Zugriff zu.
    async fn empty_pool(schema: &str) -> Option<PgPool> {
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
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    // IDOR-Guard: Partner mit fremdem ?streamer= → 403 (vor jedem DB-Zugriff).
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = empty_pool("t_catcmp_idor").await else {
            return;
        };
        let resp = category_comparison_handler(
            partner("earlysalty"),
            State(pool),
            Query(ComparisonQuery {
                streamer: Some("ismile_e".into()),
                days: Some("30".into()),
                exclude_external: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
