//! Handler für `GET /twitch/api/v2/category-leaderboard`.
//!
//! Port von `_load_category_leaderboard_payload_sync` (api_performance.py:1176).
//! Ein SQL-Query auf twitch_stats_category, Tier-Klassifikation und Rank-Berechnung in Rust.

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

const EXTERNAL_REACH_AVG_THRESHOLD: f64 = 100.0;

fn get_tier(avg: f64) -> &'static str {
    if avg < 15.0 { "starter" }
    else if avg < 50.0 { "rising" }
    else if avg < 150.0 { "established" }
    else if avg < 500.0 { "featured" }
    else { "top" }
}

fn tier_range(tier: &str) -> Option<(f64, f64)> {
    match tier {
        "starter"     => Some((0.0, 15.0)),
        "rising"      => Some((15.0, 50.0)),
        "established" => Some((50.0, 150.0)),
        "featured"    => Some((150.0, 500.0)),
        "top"         => Some((500.0, f64::INFINITY)),
        _ => None,
    }
}

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    pub streamer: Option<String>,
    pub days: Option<i32>,
    pub limit: Option<i32>,
    pub sort: Option<String>,
    pub tier: Option<String>,
    pub exclude_external: Option<String>,
}

/// `GET /twitch/api/v2/category-leaderboard?streamer=&days=30&limit=25&sort=avg&tier=&exclude_external=0`
pub async fn category_leaderboard_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<LeaderboardQuery>,
) -> impl IntoResponse {
    if matches!(auth, DashboardAuthLevel::None) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":"unauthorized"}))).into_response();
    }
    let streamer_lower = params.streamer.as_deref()
        .map(str::trim).filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let days = params.days.unwrap_or(30).clamp(1, 365) as i64;
    let limit = params.limit.unwrap_or(25).clamp(5, 100) as usize;
    let sort_peak = params.sort.as_deref() == Some("peak");
    let tier_filter = params.tier.as_deref()
        .map(str::trim).filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());
    let exclude_external = params.exclude_external.as_deref() == Some("1");
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    // Conditional HAVING clause for external-reach threshold
    let sql = if exclude_external {
        let order = if sort_peak { "peak_vc DESC" } else { "avg_vc DESC" };
        format!(r#"
            SELECT c.streamer,
                   AVG(c.viewer_count)  AS avg_vc,
                   MAX(c.viewer_count)  AS peak_vc,
                   BOOL_OR(c.is_partner) AS is_partner
            FROM twitch_stats_category c
            WHERE c.ts_utc >= $1
            GROUP BY c.streamer
            HAVING AVG(c.viewer_count) <= $2
            ORDER BY {order}
        "#)
    } else {
        let order = if sort_peak { "peak_vc DESC" } else { "avg_vc DESC" };
        format!(r#"
            SELECT c.streamer,
                   AVG(c.viewer_count)  AS avg_vc,
                   MAX(c.viewer_count)  AS peak_vc,
                   BOOL_OR(c.is_partner) AS is_partner
            FROM twitch_stats_category c
            WHERE c.ts_utc >= $1
            GROUP BY c.streamer
            ORDER BY {order}
        "#)
    };

    let rows_res = if exclude_external {
        sqlx::query(&sql)
            .bind(since)
            .bind(EXTERNAL_REACH_AVG_THRESHOLD)
            .fetch_all(&pool)
            .await
    } else {
        sqlx::query(&sql)
            .bind(since)
            .fetch_all(&pool)
            .await
    };

    let rows = match rows_res {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("category-leaderboard DB-Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
    };

    // Apply tier filter (Python-side range filter on avg_vc)
    let tier_bounds = tier_filter.as_deref().and_then(tier_range);
    let filtered: Vec<_> = rows.iter().filter(|r| {
        if let Some((lo, hi)) = tier_bounds {
            let avg: f64 = r.try_get("avg_vc").unwrap_or(0.0);
            avg >= lo && avg < hi
        } else {
            true
        }
    }).collect();

    let total_streamers = filtered.len();
    let mut your_rank: Option<usize> = None;
    let mut your_entry: Option<serde_json::Value> = None;
    let mut leaderboard: Vec<serde_json::Value> = Vec::with_capacity(limit + 1);

    // Also compute your_tier from avg_vc found in result or session fallback
    let mut your_avg_opt: Option<f64> = None;

    for (idx, row) in filtered.iter().enumerate() {
        let rank = idx + 1;
        let name: String = row.try_get("streamer").unwrap_or_default();
        let avg_vc: f64 = row.try_get::<Option<f64>, _>("avg_vc").unwrap_or(None).unwrap_or(0.0);
        let peak_vc: i64 = row.try_get::<Option<f64>, _>("peak_vc").unwrap_or(None).unwrap_or(0.0) as i64;
        let is_partner: bool = row.try_get("is_partner").unwrap_or(false);
        let is_you = !streamer_lower.is_empty() && name.to_lowercase() == streamer_lower;

        if is_you {
            your_rank = Some(rank);
            your_avg_opt = Some(avg_vc);
        }

        let entry = json!({
            "rank": rank,
            "streamer": name,
            "avgViewers": (avg_vc * 10.0).round() / 10.0,
            "peakViewers": peak_vc,
            "isPartner": is_partner,
            "isYou": is_you,
        });

        if is_you && rank > limit {
            your_entry = Some(entry);
        } else if rank <= limit {
            leaderboard.push(entry);
        }
    }

    // Append your entry at the end if you're outside the top-limit window
    if let Some(entry) = your_entry {
        leaderboard.push(entry);
    }

    // Determine your tier: from category data if found, else session fallback
    let your_tier: Option<String> = if !streamer_lower.is_empty() {
        let avg = match your_avg_opt {
            Some(a) => Some(a),
            None => {
                // Fallback: twitch_stream_sessions
                let fb_sql = r#"
                    SELECT AVG(avg_viewers) AS avg_vc
                    FROM twitch_stream_sessions
                    WHERE LOWER(streamer_login) = $1 AND started_at >= $2 AND ended_at IS NOT NULL
                "#;
                sqlx::query(fb_sql)
                    .bind(&streamer_lower)
                    .bind(since)
                    .fetch_optional(&pool)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|r| r.try_get::<Option<f64>, _>("avg_vc").ok().flatten())
            }
        };
        avg.map(|a| get_tier(a).to_string())
    } else {
        None
    };

    Json(json!({
        "leaderboard": leaderboard,
        "totalStreamers": total_streamers,
        "yourRank": your_rank,
        "yourTier": your_tier,
    })).into_response()
}
