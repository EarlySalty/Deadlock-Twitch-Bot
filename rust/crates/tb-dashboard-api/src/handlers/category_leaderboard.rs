//! Handler für `GET /twitch/api/v2/category-leaderboard`.
//!
//! Port von `_load_category_leaderboard_payload_sync` (api_performance.py:1176).
//! Ein SQL-Query auf twitch_stats_category, Tier-Klassifikation und Rank-Berechnung in Rust.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

const EXTERNAL_REACH_AVG_THRESHOLD: f64 = 100.0;
const PARTNER_AGGREGATE_SQL: &str = "BOOL_OR(COALESCE(c.is_partner, FALSE)) AS is_partner";

fn tier_range(tier: &str) -> Option<(f64, f64)> {
    match tier {
        "starter" => Some((0.0, 15.0)),
        "rising" => Some((15.0, 50.0)),
        "established" => Some((50.0, 150.0)),
        "featured" => Some((150.0, 500.0)),
        "top" => Some((500.0, f64::INFINITY)),
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
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer_lower = params
        .streamer
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let days = params.days.unwrap_or(30).clamp(1, 365) as i64;
    let limit = params.limit.unwrap_or(25).clamp(5, 100) as usize;
    let sort_peak = params.sort.as_deref() == Some("peak");
    let tier_filter = params
        .tier
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());
    let exclude_external = params.exclude_external.as_deref() == Some("1");
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    // Conditional HAVING clause for external-reach threshold.
    // is_partner is BOOLEAN in Postgres; comparing it to 0 breaks the endpoint.
    let sql = if exclude_external {
        let order = if sort_peak {
            "peak_vc DESC"
        } else {
            "avg_vc DESC"
        };
        format!(
            r#"
            SELECT c.streamer,
                   AVG(c.viewer_count)::float8  AS avg_vc,
                   MAX(c.viewer_count)::float8  AS peak_vc,
                   {PARTNER_AGGREGATE_SQL}
            FROM twitch_stats_category c
            WHERE c.ts_utc >= $1
            GROUP BY c.streamer
            HAVING AVG(c.viewer_count) <= $2
            ORDER BY {order}
        "#
        )
    } else {
        let order = if sort_peak {
            "peak_vc DESC"
        } else {
            "avg_vc DESC"
        };
        format!(
            r#"
            SELECT c.streamer,
                   AVG(c.viewer_count)::float8  AS avg_vc,
                   MAX(c.viewer_count)::float8  AS peak_vc,
                   {PARTNER_AGGREGATE_SQL}
            FROM twitch_stats_category c
            WHERE c.ts_utc >= $1
            GROUP BY c.streamer
            ORDER BY {order}
        "#
        )
    };

    let rows_res = if exclude_external {
        sqlx::query(&sql)
            .bind(since)
            .bind(EXTERNAL_REACH_AVG_THRESHOLD)
            .fetch_all(&pool)
            .await
    } else {
        sqlx::query(&sql).bind(since).fetch_all(&pool).await
    };

    let rows = match rows_res {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("category-leaderboard DB-Fehler: {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };

    // Apply tier filter (Python-side range filter on avg_vc)
    let tier_bounds = tier_filter.as_deref().and_then(tier_range);
    let filtered: Vec<_> = rows
        .iter()
        .filter(|r| {
            if let Some((lo, hi)) = tier_bounds {
                let avg: f64 = r.try_get("avg_vc").unwrap_or(0.0);
                avg >= lo && avg < hi
            } else {
                true
            }
        })
        .collect();

    let total_streamers = filtered.len();
    let mut your_rank: Option<usize> = None;
    let mut your_entry: Option<serde_json::Value> = None;
    let mut leaderboard: Vec<serde_json::Value> = Vec::with_capacity(limit + 1);

    for (idx, row) in filtered.iter().enumerate() {
        let rank = idx + 1;
        let name: String = row.try_get("streamer").unwrap_or_default();
        let avg_vc: f64 = row
            .try_get::<Option<f64>, _>("avg_vc")
            .unwrap_or(None)
            .unwrap_or(0.0);
        let peak_vc: i64 = row
            .try_get::<Option<f64>, _>("peak_vc")
            .unwrap_or(None)
            .unwrap_or(0.0) as i64;
        let is_partner: bool = row.try_get("is_partner").unwrap_or(false);
        let is_you = !streamer_lower.is_empty() && name.to_lowercase() == streamer_lower;

        if is_you {
            your_rank = Some(rank);
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

    // yourTier aus der Peer-Gruppe (Python _get_peer_group_stats["tier"]):
    // ungefilterter all-category-AVG, `null` wenn keine same-tier-Peers
    // existieren — NICHT der tier/exclude-gefilterte Leaderboard-Schnitt.
    let your_tier: Option<String> = if !streamer_lower.is_empty() {
        match tb_analytics::peer_group::peer_group_stats(&pool, &streamer_lower, since).await {
            Ok(Some(pg)) => Some(pg.tier),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("category-leaderboard yourTier DB-Fehler: {e}");
                None
            }
        }
    } else {
        None
    };

    Json(json!({
        "leaderboard": leaderboard,
        "totalStreamers": total_streamers,
        "yourRank": your_rank,
        "yourTier": your_tier,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partner_aggregate_uses_boolean_schema() {
        assert_eq!(
            PARTNER_AGGREGATE_SQL,
            "BOOL_OR(COALESCE(c.is_partner, FALSE)) AS is_partner"
        );
        assert!(!PARTNER_AGGREGATE_SQL.contains("<> 0"));
    }
}
