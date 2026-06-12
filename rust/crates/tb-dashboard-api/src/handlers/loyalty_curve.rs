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

const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix", "deutschedeadlockcommunity", "fossabot", "moobot", "nightbot",
    "pretzelrocks", "soundalerts", "streamlabs", "streamelements", "wizebot",
];

#[derive(Deserialize)]
pub struct LoyaltyQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

pub async fn loyalty_curve_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<LoyaltyQuery>,
) -> impl IntoResponse {
    if matches!(auth, DashboardAuthLevel::None) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":"unauthorized"}))).into_response();
    }

    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response(),
    };

    // Bot-Exclusion: $2...$N+1
    let bot_placeholders: Vec<String> = (2..=KNOWN_CHAT_BOTS.len() + 1)
        .map(|i| format!("${i}"))
        .collect();
    let sql = format!(
        r#"SELECT total_sessions, COUNT(DISTINCT chatter_login) AS chatter_count
           FROM twitch_chatter_rollup
           WHERE LOWER(streamer_login) = $1
             AND chatter_login NOT IN ({})
           GROUP BY total_sessions
           ORDER BY total_sessions"#,
        bot_placeholders.join(", ")
    );

    let mut q = sqlx::query(&sql).bind(&streamer);
    for bot in KNOWN_CHAT_BOTS {
        q = q.bind(*bot);
    }

    match q.fetch_all(&pool).await {
        Err(e) => {
            tracing::error!("loyalty-curve DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) if rows.is_empty() => {
            Json(json!({"curve": [], "one_time_rate": null, "total_chatters": 0, "window": "all_time"})).into_response()
        }
        Ok(rows) => {
            let total: i64 = rows.iter()
                .map(|r| r.try_get::<i64, _>("chatter_count").unwrap_or(0))
                .sum();

            let first_sessions: i64 = rows.first()
                .and_then(|r| r.try_get::<i64, _>("total_sessions").ok())
                .unwrap_or(0);
            let one_time: i64 = if first_sessions == 1 {
                rows.first().and_then(|r| r.try_get::<i64, _>("chatter_count").ok()).unwrap_or(0)
            } else {
                0
            };

            let curve: Vec<serde_json::Value> = rows.iter().map(|r| {
                let chatters = r.try_get::<i64, _>("chatter_count").unwrap_or(0);
                json!({
                    "sessions": r.try_get::<i64, _>("total_sessions").unwrap_or(0),
                    "chatters": chatters,
                    "percentage": if total > 0 { (chatters as f64 / total as f64 * 1000.0).round() / 10.0 } else { 0.0 },
                })
            }).collect();

            Json(json!({
                "curve": curve,
                "total_chatters": total,
                "one_time_rate": if total > 0 { Some((one_time as f64 / total as f64 * 1000.0).round() / 10.0) } else { None::<f64> },
                "window": "all_time",
            })).into_response()
        }
    }
}
