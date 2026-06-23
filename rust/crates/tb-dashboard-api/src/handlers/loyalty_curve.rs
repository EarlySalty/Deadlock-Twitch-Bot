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
    // Python _api_v2_loyalty_curve: _require_v2_auth + _require_extended_plan.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }

    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"Streamer required"})),
                )
                    .into_response()
            }
            Err(resp) => return resp,
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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"internal_error"})),
            )
                .into_response()
        }
        Ok(rows) if rows.is_empty() => Json(
            json!({"curve": [], "one_time_rate": null, "total_chatters": 0, "window": "all_time"}),
        )
        .into_response(),
        Ok(rows) => {
            let total: i64 = rows
                .iter()
                .map(|r| r.try_get::<i64, _>("chatter_count").unwrap_or(0))
                .sum();

            let first_sessions: i64 = rows
                .first()
                .and_then(|r| r.try_get::<i64, _>("total_sessions").ok())
                .unwrap_or(0);
            let one_time: i64 = if first_sessions == 1 {
                rows.first()
                    .and_then(|r| r.try_get::<i64, _>("chatter_count").ok())
                    .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
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

    /// Richtet ein berechtigtes Partner-Plan-Snapshot ein (Manual-Override mit
    /// Analytics-Plan), damit `extended_gate` für den Partner passiert.
    async fn grant_partner_analytics(pool: &PgPool, login: &str) {
        sqlx::query(
            "CREATE TABLE streamer_plans (twitch_user_id TEXT, twitch_login TEXT, manual_plan_id TEXT, manual_plan_expires_at TEXT, manual_plan_notes TEXT, manual_plan_updated_at TEXT)",
        ).execute(pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_billing_subscriptions (customer_reference TEXT, plan_id TEXT, status TEXT, current_period_end TEXT, updated_at TEXT)")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO streamer_plans (twitch_login, manual_plan_id) VALUES ($1, 'analysis_dashboard')")
            .bind(login).execute(pool).await.unwrap();
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: String::new(),
            display_name: login.to_string(),
        }
    }

    /// IDOR: ein berechtigter Partner darf NICHT die Loyalty-Kurve eines fremden
    /// Streamers lesen (`?streamer=<fremd>` → 403).
    #[tokio::test]
    async fn partner_fremder_streamer_ist_forbidden() {
        let Some(pool) = make_pool("t_loyalty_idor").await else {
            return;
        };
        grant_partner_analytics(&pool, "earlysalty").await;
        let resp = loyalty_curve_handler(
            partner("earlysalty"),
            State(pool),
            Query(LoyaltyQuery {
                streamer: Some("ismile_e".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
