//! Handler für Audience-Endpoints.
//!
//! - `GET /twitch/api/v2/tag-analysis` — Stub (Python gibt [] zurück).
//! - `GET /twitch/api/v2/viewer-overlap` — Jaccard-Overlap via twitch_chatter_rollup.
//!   Port von `bot/analytics/api_audience.py:_api_v2_viewer_overlap` (Z.749–848).
//!   Python macht N+1-Queries für die Totals — wir nutzen eine CTE.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
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
