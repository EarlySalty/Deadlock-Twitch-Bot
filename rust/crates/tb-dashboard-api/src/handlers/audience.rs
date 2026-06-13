//! Handler für Audience-Endpoints.
//!
//! - `GET /twitch/api/v2/tag-analysis` — Stub (Python gibt [] zurück).
//! - `GET /twitch/api/v2/viewer-overlap` — Jaccard-Overlap via twitch_chatter_rollup.
//! - `GET /twitch/api/v2/viewer-profiles` — Exklusivitäts-Verteilung aus twitch_chatter_rollup.
//! - `GET /twitch/api/v2/audience-sharing` — Cross-Streamer Overlap + Timeline.
//! - `GET /twitch/api/v2/audience-insights` — Return-Rate + Watch-Time-Trend.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

// Gleiche Bot-Exclusion-Liste wie in den anderen Handlers.
const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

fn require_auth(auth: &DashboardAuthLevel) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"unauthorized","message":"not authenticated"}))))
    } else {
        Ok(())
    }
}

/// `GET /twitch/api/v2/tag-analysis` — Stub wie Python.
pub async fn tag_analysis_handler(auth: DashboardAuthLevel) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }
    Json(json!([])).into_response()
}

#[derive(Deserialize)]
pub struct OverlapQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub limit: Option<i32>,
}

/// `GET /twitch/api/v2/viewer-overlap?streamer=&limit=20`
pub async fn viewer_overlap_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<OverlapQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response(),
    };
    let limit = params.limit.unwrap_or(20).max(5).min(50) as i64;

    // Bot-Exclusion: $1 = base_streamer, $2 = base_streamer (für !=), $3..$N+2 = Bots (c1),
    // $N+3..$2N+2 = Bots (c2), $2N+3 = limit.
    // Totals: In einer CTE alle Unique-Chatters pro Streamer berechnen → kein N+1.
    let n = KNOWN_CHAT_BOTS.len();
    let c1_bots: Vec<String> = (3..=(n + 2)).map(|i| format!("${i}")).collect();
    let c2_bots: Vec<String> = ((n + 3)..=(2 * n + 2)).map(|i| format!("${i}")).collect();
    let rollup_bots_a: Vec<String> = (3..=(n + 2)).map(|i| format!("${i}")).collect();

    let c1_clause = format!("c1.chatter_login NOT IN ({})", c1_bots.join(", "));
    let c2_clause = format!("c2.chatter_login NOT IN ({})", c2_bots.join(", "));
    // In totals_b CTE: gleiche Positionen wie c2_bots, aber Alias `cr`.
    let cr_clause = format!("cr.chatter_login NOT IN ({})", c2_bots.join(", "));
    let rollup_clause_a = format!("chatter_login NOT IN ({})", rollup_bots_a.join(", "));

    let limit_pos = 2 * n + 3;
    let sql = format!(
        r#"WITH shared AS (
               SELECT
                   c2.streamer_login AS other_streamer,
                   COUNT(DISTINCT c1.chatter_login) AS shared_chatters
               FROM twitch_chatter_rollup c1
               JOIN twitch_chatter_rollup c2 ON c1.chatter_login = c2.chatter_login
               WHERE LOWER(c1.streamer_login) = $1
                 AND LOWER(c2.streamer_login) != $2
                 AND {c1_clause}
                 AND {c2_clause}
               GROUP BY c2.streamer_login
               ORDER BY shared_chatters DESC
               LIMIT ${limit_pos}
           ),
           totals_b AS (
               SELECT LOWER(cr.streamer_login) AS streamer_login,
                      COUNT(DISTINCT cr.chatter_login) AS total_chatters
               FROM twitch_chatter_rollup cr
               WHERE LOWER(cr.streamer_login) IN (SELECT LOWER(other_streamer) FROM shared)
                 AND {cr_clause}
               GROUP BY LOWER(cr.streamer_login)
           )
           SELECT s.other_streamer,
                  s.shared_chatters,
                  COALESCE(tb.total_chatters, 1) AS total_b
           FROM shared s
           LEFT JOIN totals_b tb ON LOWER(s.other_streamer) = tb.streamer_login
           ORDER BY s.shared_chatters DESC"#
    );

    // Gesamt-Chatters von A (eigene Streamer-Basis)
    let total_a_sql = format!(
        "SELECT COUNT(DISTINCT chatter_login) AS total FROM twitch_chatter_rollup WHERE LOWER(streamer_login) = $1 AND {rollup_clause_a}"
    );

    // Bindings aufbauen
    let mut total_a_q = sqlx::query(&total_a_sql).bind(&streamer);
    for bot in KNOWN_CHAT_BOTS {
        total_a_q = total_a_q.bind(*bot);
    }
    let total_a: i64 = total_a_q
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<i64, _>("total").ok())
        .unwrap_or(1)
        .max(1);

    // Overlap-Query
    let mut overlap_q = sqlx::query(&sql).bind(&streamer).bind(&streamer);
    for bot in KNOWN_CHAT_BOTS { overlap_q = overlap_q.bind(*bot); } // c1 bots
    for bot in KNOWN_CHAT_BOTS { overlap_q = overlap_q.bind(*bot); } // c2 bots (auch in totals_b)
    overlap_q = overlap_q.bind(limit);

    let rows = overlap_q.fetch_all(&pool).await;

    match rows {
        Err(e) => {
            tracing::error!("viewer-overlap DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let data: Vec<serde_json::Value> = rows.iter().map(|r| {
                let other: String = r.try_get("other_streamer").unwrap_or_default();
                let shared: i64 = r.try_get("shared_chatters").unwrap_or(0);
                let total_b: i64 = r.try_get::<i64, _>("total_b").unwrap_or(1).max(1);
                let jaccard = shared as f64 / (total_a + total_b - shared).max(1) as f64 * 100.0;
                let jaccard = (jaccard * 10.0).round() / 10.0;
                json!({
                    "streamerA": streamer,
                    "streamerB": other,
                    "sharedChatters": shared,
                    "totalChattersA": total_a,
                    "totalChattersB": total_b,
                    "overlapAtoB": ((shared as f64 / total_a as f64 * 1000.0).round() / 10.0),
                    "overlapBtoA": ((shared as f64 / total_b as f64 * 1000.0).round() / 10.0),
                    "jaccard": jaccard,
                    "overlapPercentage": jaccard,
                })
            }).collect();
            Json(json!(data)).into_response()
        }
    }
}

// ─── viewer-profiles ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ProfilesQuery {
    pub streamer: Option<String>,
}

/// `GET /twitch/api/v2/viewer-profiles?streamer=`
///
/// Port von `_load_viewer_profiles` (api_overview.py:2138).
/// Für jeden Chatter dieser Streamer-Basis wird gezählt, auf wie vielen
/// Streamern er global auftaucht → Exklusivitäts-Verteilung.
pub async fn viewer_profiles_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ProfilesQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":"streamer required"}))).into_response();
        }
    };
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    let dist_sql = r#"
        WITH per_viewer AS (
            SELECT cr.chatter_login,
                   COUNT(DISTINCT cr.streamer_login) AS streamer_count
            FROM twitch_chatter_rollup cr
            WHERE cr.chatter_login IN (
                SELECT DISTINCT chatter_login
                FROM twitch_chatter_rollup
                WHERE LOWER(streamer_login) = $1
                  AND NOT (chatter_login = ANY($2::text[]))
            )
              AND NOT (cr.chatter_login = ANY($2::text[]))
            GROUP BY cr.chatter_login
        )
        SELECT streamer_count, COUNT(*) AS viewer_count
        FROM per_viewer
        GROUP BY streamer_count
        ORDER BY streamer_count
    "#;

    let passive_sql = r#"
        SELECT COUNT(*) AS passive
        FROM twitch_chatter_rollup
        WHERE LOWER(streamer_login) = $1
          AND total_sessions >= 3
          AND total_messages = 0
          AND NOT (chatter_login = ANY($2::text[]))
    "#;

    let dist_rows = match sqlx::query(dist_sql)
        .bind(&streamer)
        .bind(&bots[..])
        .fetch_all(&pool)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("viewer-profiles dist-Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
    };

    if dist_rows.is_empty() {
        return Json(json!({
            "dataAvailable": false,
            "message": "Keine Daten vorhanden",
            "profiles": {"exclusive":0,"loyalMulti":0,"casual":0,"explorer":0,"passive":0,"total":0},
            "exclusivityDistribution": []
        })).into_response();
    }

    let passive: i64 = match sqlx::query(passive_sql)
        .bind(&streamer)
        .bind(&bots[..])
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(r)) => r.try_get("passive").unwrap_or(0),
        _ => 0,
    };

    let dist: Vec<(i64, i64)> = dist_rows
        .iter()
        .map(|r| {
            let sc: i64 = r.try_get("streamer_count").unwrap_or(0);
            let vc: i64 = r.try_get("viewer_count").unwrap_or(0);
            (sc, vc)
        })
        .collect();

    let total: i64 = dist.iter().map(|(_, v)| v).sum();
    let exclusive: i64 = dist.iter().find(|(k, _)| *k == 1).map(|(_, v)| *v).unwrap_or(0);
    let loyal_multi: i64 = dist.iter().filter(|(k, _)| *k == 2 || *k == 3).map(|(_, v)| *v).sum();
    let explorer: i64 = dist.iter().filter(|(k, _)| *k >= 8).map(|(_, v)| *v).sum();
    let casual = (total - exclusive - loyal_multi - explorer - passive).max(0);

    let exclusivity_dist: Vec<serde_json::Value> = dist
        .iter()
        .map(|(sc, vc)| json!({"streamerCount": sc, "viewerCount": vc}))
        .collect();

    Json(json!({
        "dataAvailable": true,
        "profiles": {
            "exclusive": exclusive,
            "loyalMulti": loyal_multi,
            "casual": casual,
            "explorer": explorer,
            "passive": passive,
            "total": total,
        },
        "exclusivityDistribution": exclusivity_dist,
    })).into_response()
}

// ─── audience-sharing ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SharingQuery {
    pub streamer: Option<String>,
    pub days: Option<i32>,
}

/// `GET /twitch/api/v2/audience-sharing?streamer=&days=30`
///
/// Port von `_load_audience_sharing` (api_overview.py:2239).
/// Shared Viewer-Overlap mit Inflow/Outflow-Delta und Jaccard-Ähnlichkeit.
pub async fn audience_sharing_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<SharingQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":"streamer required"}))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;
    let since: DateTime<Utc> = Utc::now() - chrono::Duration::days(days);
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    let my_total_sql = r#"
        SELECT COUNT(DISTINCT chatter_login) AS total
        FROM twitch_chatter_rollup
        WHERE LOWER(streamer_login) = $1
          AND NOT (chatter_login = ANY($2::text[]))
    "#;
    let my_total: i64 = sqlx::query(my_total_sql)
        .bind(&streamer)
        .bind(&bots[..])
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get("total").ok())
        .unwrap_or(0);

    // $1=since (inflow/outflow cutoff), $2=streamer, $3=bots
    let shared_sql = r#"
        SELECT
            cr2.streamer_login AS other_streamer,
            COUNT(DISTINCT cr1.chatter_login) AS shared_viewers,
            COUNT(DISTINCT CASE WHEN cr2.first_seen_at >= $1 THEN cr1.chatter_login END) AS inflow,
            COUNT(DISTINCT CASE WHEN cr2.last_seen_at < $1 THEN cr1.chatter_login END) AS outflow,
            COUNT(DISTINCT cr2.chatter_login) AS other_total
        FROM twitch_chatter_rollup cr1
        JOIN twitch_chatter_rollup cr2
            ON cr1.chatter_login = cr2.chatter_login
           AND LOWER(cr2.streamer_login) != LOWER(cr1.streamer_login)
        WHERE LOWER(cr1.streamer_login) = $2
          AND NOT (cr1.chatter_login = ANY($3::text[]))
          AND NOT (cr2.chatter_login = ANY($3::text[]))
        GROUP BY cr2.streamer_login
        HAVING COUNT(DISTINCT cr1.chatter_login) >= 3
        ORDER BY shared_viewers DESC
        LIMIT 20
    "#;
    let shared_rows = match sqlx::query(shared_sql)
        .bind(since)
        .bind(&streamer)
        .bind(&bots[..])
        .fetch_all(&pool)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("audience-sharing shared-Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
    };

    if shared_rows.is_empty() {
        return Json(json!({
            "dataAvailable": false,
            "message": "Keine Daten vorhanden",
            "current": [],
            "timeline": [],
            "totalUniqueViewers": my_total,
            "dataQuality": {"months": 0, "minSharedFilter": 3},
        })).into_response();
    }

    let top_streamers: Vec<String> = shared_rows
        .iter()
        .take(5)
        .filter_map(|r| r.try_get::<String, _>("other_streamer").ok())
        .map(|s| s.to_lowercase())
        .collect();

    // $1=top_streamers array, $2=streamer, $3=bots
    let timeline_sql = r#"
        SELECT
            TO_CHAR(
                date_trunc('month',
                    CASE WHEN cr1.first_seen_at > cr2.first_seen_at
                         THEN cr1.first_seen_at
                         ELSE cr2.first_seen_at END
                ),
                'YYYY-MM'
            ) AS month,
            cr2.streamer_login AS other_streamer,
            COUNT(DISTINCT cr1.chatter_login) AS shared_viewers_that_month
        FROM twitch_chatter_rollup cr1
        JOIN twitch_chatter_rollup cr2
            ON cr1.chatter_login = cr2.chatter_login
           AND LOWER(cr2.streamer_login) = ANY($1::text[])
           AND LOWER(cr2.streamer_login) != LOWER(cr1.streamer_login)
        WHERE LOWER(cr1.streamer_login) = $2
          AND NOT (cr1.chatter_login = ANY($3::text[]))
          AND NOT (cr2.chatter_login = ANY($3::text[]))
        GROUP BY month, cr2.streamer_login
        ORDER BY month
    "#;
    let timeline_rows = if !top_streamers.is_empty() {
        sqlx::query(timeline_sql)
            .bind(&top_streamers[..])
            .bind(&streamer)
            .bind(&bots[..])
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    let mut months_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let current: Vec<serde_json::Value> = shared_rows
        .iter()
        .map(|r| {
            let other: String = r.try_get("other_streamer").unwrap_or_default();
            let shared: i64 = r.try_get("shared_viewers").unwrap_or(0);
            let inflow: i64 = r.try_get("inflow").unwrap_or(0);
            let outflow: i64 = r.try_get("outflow").unwrap_or(0);
            let other_total: i64 = r.try_get("other_total").unwrap_or(0);
            let union_total = my_total + other_total - shared;
            let jaccard = if union_total > 0 {
                (shared as f64 / union_total as f64 * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            json!({"streamer": other, "sharedViewers": shared, "inflow": inflow, "outflow": outflow, "jaccardSimilarity": jaccard})
        })
        .collect();

    let timeline: Vec<serde_json::Value> = timeline_rows
        .iter()
        .map(|r| {
            let month: String = r.try_get("month").unwrap_or_default();
            if !month.is_empty() {
                months_set.insert(month.clone());
            }
            json!({
                "month": month,
                "streamer": r.try_get::<String, _>("other_streamer").unwrap_or_default(),
                "sharedViewers": r.try_get::<i64, _>("shared_viewers_that_month").unwrap_or(0),
            })
        })
        .collect();

    Json(json!({
        "dataAvailable": true,
        "current": current,
        "timeline": timeline,
        "totalUniqueViewers": my_total,
        "dataQuality": {"months": months_set.len(), "minSharedFilter": 3},
    })).into_response()
}

// ─── audience-insights ──────────────────────────────────────────────────────

const WATCH_TIME_MIN_SAMPLES: usize = 25;
const WATCH_TIME_MIN_COVERAGE: f64 = 0.15;

struct WatchDist {
    avg: f64,
    method: &'static str,
}

async fn calc_watch_distribution(pool: &PgPool, session_ids: &[i64], bots: &[String]) -> WatchDist {
    let empty = WatchDist { avg: 0.0, method: "no_data" };
    if session_ids.is_empty() {
        return empty;
    }

    let base_sql = r#"
        SELECT COUNT(DISTINCT COALESCE(NULLIF(chatter_login, ''), chatter_id)) AS viewer_base_count
        FROM twitch_session_chatters
        WHERE session_id = ANY($1::bigint[])
          AND COALESCE(NULLIF(chatter_login, ''), chatter_id) IS NOT NULL
          AND NOT (chatter_login = ANY($2::text[]))
    "#;
    let viewer_base_count: i64 = sqlx::query(base_sql)
        .bind(session_ids)
        .bind(bots)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get("viewer_base_count").ok())
        .unwrap_or(0);

    let watch_sql = r#"
        SELECT CAST(ROUND(GREATEST(
            EXTRACT(EPOCH FROM COALESCE(last_seen_at, first_message_at))
            - EXTRACT(EPOCH FROM COALESCE(first_message_at, last_seen_at)),
            0
        ) / 60.0) AS DOUBLE PRECISION) AS watch_minutes
        FROM twitch_session_chatters
        WHERE session_id = ANY($1::bigint[])
          AND first_message_at IS NOT NULL
          AND last_seen_at IS NOT NULL
          AND NOT (chatter_login = ANY($2::text[]))
    "#;
    let watch_rows = match sqlx::query(watch_sql)
        .bind(session_ids)
        .bind(bots)
        .fetch_all(pool)
        .await
    {
        Ok(r) => r,
        Err(_) => return empty,
    };

    let real_minutes: Vec<f64> = watch_rows
        .iter()
        .filter_map(|r| r.try_get::<f64, _>("watch_minutes").ok())
        .filter(|&m| m >= 0.0)
        .collect();

    let sample_count = real_minutes.len();
    let coverage = if viewer_base_count > 0 {
        sample_count as f64 / viewer_base_count as f64
    } else {
        sample_count as f64 / session_ids.len().max(1) as f64
    };

    let method: &'static str = if sample_count == 0 {
        "no_data"
    } else if sample_count < WATCH_TIME_MIN_SAMPLES || coverage < WATCH_TIME_MIN_COVERAGE {
        "low_coverage"
    } else {
        "real_samples"
    };

    if method != "real_samples" {
        return WatchDist { avg: 0.0, method };
    }

    let total = real_minutes.len() as f64;
    let avg = ((real_minutes.iter().sum::<f64>() / total) * 10.0).round() / 10.0;

    WatchDist { avg, method: "real_samples" }
}

/// Anteil der Viewer dieser Periode, die bereits vorher bekannt waren (aus twitch_chatter_rollup).
/// `period_end = None` → bis jetzt; `Some(t)` → geschlossenes Fenster.
async fn true_return_rate(
    pool: &PgPool,
    period_start: DateTime<Utc>,
    period_end: Option<DateTime<Utc>>,
    streamer: &str,
    bots: &[String],
) -> (f64, i64) {
    // $1=period_start (used twice: WHERE started_at >= $1 AND cr.first_seen_at < $1)
    let sql_no_end = r#"
        WITH period_viewers AS (
            SELECT DISTINCT
                COALESCE(NULLIF(LOWER(sc.chatter_login), ''), sc.chatter_id) AS viewer_key,
                NULLIF(LOWER(sc.chatter_login), '') AS chatter_login
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE s.started_at >= $1
              AND LOWER(s.streamer_login) = $2
              AND s.ended_at IS NOT NULL
              AND COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) IS NOT NULL
              AND NOT (sc.chatter_login = ANY($3::text[]))
        )
        SELECT
            COUNT(DISTINCT pv.viewer_key) AS total_viewers,
            COUNT(DISTINCT CASE WHEN cr.chatter_login IS NOT NULL THEN pv.viewer_key END) AS returning_viewers
        FROM period_viewers pv
        LEFT JOIN twitch_chatter_rollup cr
            ON cr.chatter_login = pv.chatter_login
           AND LOWER(cr.streamer_login) = $2
           AND NOT (cr.chatter_login = ANY($3::text[]))
           AND cr.first_seen_at < $1
    "#;
    let sql_with_end = r#"
        WITH period_viewers AS (
            SELECT DISTINCT
                COALESCE(NULLIF(LOWER(sc.chatter_login), ''), sc.chatter_id) AS viewer_key,
                NULLIF(LOWER(sc.chatter_login), '') AS chatter_login
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE s.started_at >= $1
              AND s.started_at < $2
              AND LOWER(s.streamer_login) = $3
              AND s.ended_at IS NOT NULL
              AND COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) IS NOT NULL
              AND NOT (sc.chatter_login = ANY($4::text[]))
        )
        SELECT
            COUNT(DISTINCT pv.viewer_key) AS total_viewers,
            COUNT(DISTINCT CASE WHEN cr.chatter_login IS NOT NULL THEN pv.viewer_key END) AS returning_viewers
        FROM period_viewers pv
        LEFT JOIN twitch_chatter_rollup cr
            ON cr.chatter_login = pv.chatter_login
           AND LOWER(cr.streamer_login) = $3
           AND NOT (cr.chatter_login = ANY($4::text[]))
           AND cr.first_seen_at < $1
    "#;

    let result = if let Some(end) = period_end {
        sqlx::query(sql_with_end)
            .bind(period_start)
            .bind(end)
            .bind(streamer)
            .bind(bots)
            .fetch_optional(pool)
            .await
    } else {
        sqlx::query(sql_no_end)
            .bind(period_start)
            .bind(streamer)
            .bind(bots)
            .fetch_optional(pool)
            .await
    };

    match result {
        Ok(Some(r)) => {
            let total: i64 = r.try_get("total_viewers").unwrap_or(0);
            let returning: i64 = r.try_get("returning_viewers").unwrap_or(0);
            let rate = if total > 0 {
                (returning as f64 / total as f64 * 1000.0).round() / 10.0
            } else {
                0.0
            };
            (rate, total)
        }
        _ => (0.0, 0),
    }
}

#[derive(Deserialize)]
pub struct InsightsQuery {
    pub streamer: Option<String>,
    pub days: Option<i32>,
}

/// `GET /twitch/api/v2/audience-insights?streamer=&days=30`
///
/// Port von `_api_v2_audience_insights` (api_audience.py:856).
/// Watch-Time-Trend (Vergleich aktuelles vs. vorheriges Fenster) +
/// Return-Rate (Anteil bekannter Viewer via twitch_chatter_rollup.first_seen_at).
pub async fn audience_insights_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<InsightsQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;
    let now = Utc::now();
    let since = now - chrono::Duration::days(days);
    let prev_since = now - chrono::Duration::days(days * 2);
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    let sessions_sql = r#"
        SELECT id FROM twitch_stream_sessions
        WHERE started_at >= $1 AND started_at < $2
          AND LOWER(streamer_login) = $3
          AND ended_at IS NOT NULL
    "#;
    let current_ids: Vec<i64> = sqlx::query(sessions_sql)
        .bind(since).bind(now).bind(&streamer)
        .fetch_all(&pool).await.unwrap_or_default()
        .iter().filter_map(|r| r.try_get("id").ok()).collect();
    let prev_ids: Vec<i64> = sqlx::query(sessions_sql)
        .bind(prev_since).bind(since).bind(&streamer)
        .fetch_all(&pool).await.unwrap_or_default()
        .iter().filter_map(|r| r.try_get("id").ok()).collect();

    let current_watch = calc_watch_distribution(&pool, &current_ids, &bots).await;
    let previous_watch = calc_watch_distribution(&pool, &prev_ids, &bots).await;

    let (curr_rate, curr_total) = true_return_rate(&pool, since, None, &streamer, &bots).await;
    let (prev_rate, _) = true_return_rate(&pool, prev_since, Some(since), &streamer, &bots).await;

    let calc_trend = |curr: f64, prev: f64| -> f64 {
        if prev == 0.0 { 0.0 } else { ((curr - prev) / prev * 1000.0).round() / 10.0 }
    };

    let watch_time_trend_available = current_watch.method == "real_samples"
        && previous_watch.method == "real_samples"
        && previous_watch.avg > 0.0;
    let watch_time_change: Option<f64> = if watch_time_trend_available {
        Some(calc_trend(current_watch.avg, previous_watch.avg))
    } else {
        None
    };

    let viewer_return_trend_available = prev_rate > 0.0;
    let viewer_return_change: Option<f64> = if viewer_return_trend_available {
        Some(calc_trend(curr_rate, prev_rate))
    } else {
        None
    };

    Json(json!({
        "trends": {
            "watchTimeChange": watch_time_change,
            "conversionChange": null,
            "viewerReturnRate": curr_rate,
            "viewerReturnChange": viewer_return_change,
        },
        "distinctViewers": curr_total,
        "returnRateMethod": "distinct_rollup",
        "dataQuality": {
            "botFilterApplied": true,
            "watchTimeMethod": current_watch.method,
            "watchTimeTrendAvailable": watch_time_trend_available,
            "viewerReturnTrendAvailable": viewer_return_trend_available,
            "conversionTrendAvailable": false,
        },
    })).into_response()
}
