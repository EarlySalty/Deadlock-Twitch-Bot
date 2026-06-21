//! Handler für `GET /twitch/api/v2/lurker-analysis`.
//!
//! Port von `_load_lurker_analysis` (api_overview.py:1762).
//! Zwei Queries (Aggregate + Top-Lurker-Liste) über eine gemeinsame Sessions-CTE.

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
pub struct LurkerQuery {
    pub streamer: Option<String>,
    pub days: Option<i32>,
}

/// `GET /twitch/api/v2/lurker-analysis?streamer=&days=30`
pub async fn lurker_analysis_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<LurkerQuery>,
) -> impl IntoResponse {
    // Python api_overview.py:1759 _require_extended_plan (Paywall-Feature).
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({"dataAvailable":false,"message":"Streamer required"}))).into_response();
        }
    };
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    // Query 1: Aggregate — total, lurker_count, conversion
    // $1=since, $2=streamer, $3=bots
    let agg_sql = r#"
        WITH sessions AS (
            SELECT id FROM twitch_stream_sessions
            WHERE started_at >= $1
              AND ended_at IS NOT NULL
              AND LOWER(streamer_login) = $2
        ),
        chatter AS (
            SELECT
                COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS viewer_id,
                COUNT(DISTINCT sc.session_id) AS session_count,
                SUM(COALESCE(sc.messages, 0)) AS msg_sum,
                SUM(CASE WHEN sc.messages = 0 AND COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                         THEN 1 ELSE 0 END) AS lurk_samples,
                SUM(CASE WHEN sc.messages > 0 THEN 1 ELSE 0 END) AS active_samples,
                MAX(CASE WHEN COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                         THEN 1 ELSE 0 END) AS seen_via_api,
                MIN(CASE WHEN sc.messages = 0 AND COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                         THEN COALESCE(sc.first_message_at, sc.last_seen_at) ELSE NULL END) AS first_lurk_seen,
                MIN(CASE WHEN sc.messages > 0
                         THEN COALESCE(sc.first_message_at, sc.last_seen_at) ELSE NULL END) AS first_active_seen
            FROM twitch_session_chatters sc
            JOIN sessions s ON s.id = sc.session_id
            WHERE (sc.chatter_login IS NULL OR sc.chatter_login = ''
                   OR LOWER(sc.chatter_login) <> ALL($3::text[]))
            GROUP BY 1
        )
        SELECT
            COUNT(*) AS total_viewers,
            COUNT(*) FILTER (WHERE seen_via_api = 1) AS seen_sample_viewers,
            COUNT(*) FILTER (WHERE seen_via_api = 1 AND msg_sum = 0) AS lurker_count,
            AVG(session_count) FILTER (WHERE seen_via_api = 1 AND msg_sum = 0) AS avg_sessions_lurkers,
            COUNT(*) FILTER (WHERE seen_via_api = 1 AND lurk_samples > 0) AS eligible_lurkers,
            COUNT(*) FILTER (
                WHERE seen_via_api = 1
                  AND lurk_samples > 0
                  AND active_samples > 0
                  AND first_active_seen IS NOT NULL
                  AND first_lurk_seen IS NOT NULL
                  AND first_active_seen > first_lurk_seen
            ) AS converted_lurkers
        FROM chatter
    "#;

    let agg_row = match sqlx::query(agg_sql)
        .bind(since)
        .bind(&streamer)
        .bind(&bots[..])
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Json(json!({"dataAvailable":false,"message":"Keine Daten für den Zeitraum"})).into_response();
        }
        Err(e) => {
            // Python api_overview.py:1779 fängt jede Exception und liefert bewusst
            // 200 + dataAvailable:false (das Frontend wertet dataAvailable aus und
            // bräche bei 500). Fehler wird geloggt, aber nicht als 500 propagiert.
            tracing::error!("lurker-analysis agg-Fehler: {e}");
            return Json(json!({"dataAvailable":false,"message":"Keine Daten verfügbar"})).into_response();
        }
    };

    let total_viewers: i64 = agg_row.try_get("total_viewers").unwrap_or(0);
    let seen_sample_viewers: i64 = agg_row.try_get("seen_sample_viewers").unwrap_or(0);
    let lurker_count: i64 = agg_row.try_get("lurker_count").unwrap_or(0);
    let avg_sessions_lurkers: f64 = agg_row.try_get::<Option<f64>, _>("avg_sessions_lurkers").unwrap_or(None).unwrap_or(0.0);
    let eligible_lurkers: i64 = agg_row.try_get("eligible_lurkers").unwrap_or(0);
    let converted_lurkers: i64 = agg_row.try_get("converted_lurkers").unwrap_or(0);

    if total_viewers == 0 {
        return Json(json!({"dataAvailable":false,"message":"Keine Daten für den Zeitraum"})).into_response();
    }
    if seen_sample_viewers == 0 {
        return Json(json!({"dataAvailable":false,"message":"Zu wenig Chatter-API/Lurker-Daten im Zeitraum"})).into_response();
    }

    // Query 2: Top-25 pure lurkers by session count
    let top_sql = r#"
        WITH sessions AS (
            SELECT id FROM twitch_stream_sessions
            WHERE started_at >= $1
              AND ended_at IS NOT NULL
              AND LOWER(streamer_login) = $2
        ),
        chatter AS (
            SELECT
                COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS viewer_id,
                COUNT(DISTINCT sc.session_id) AS session_count,
                SUM(COALESCE(sc.messages, 0)) AS msg_sum,
                MAX(CASE WHEN COALESCE(sc.seen_via_chatters_api, FALSE) IS TRUE
                         THEN 1 ELSE 0 END) AS seen_via_api,
                MIN(sc.first_message_at) AS first_seen,
                MAX(sc.last_seen_at) AS last_seen
            FROM twitch_session_chatters sc
            JOIN sessions s ON s.id = sc.session_id
            WHERE (sc.chatter_login IS NULL OR sc.chatter_login = ''
                   OR LOWER(sc.chatter_login) <> ALL($3::text[]))
            GROUP BY 1
        )
        SELECT viewer_id, session_count, first_seen, last_seen
        FROM chatter
        WHERE msg_sum = 0 AND seen_via_api = 1
        ORDER BY session_count DESC
        LIMIT 25
    "#;

    let top_rows = sqlx::query(top_sql)
        .bind(since)
        .bind(&streamer)
        .bind(&bots[..])
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

    let regular_lurkers: Vec<serde_json::Value> = top_rows
        .iter()
        .map(|r| {
            let login: String = r.try_get("viewer_id").unwrap_or_default();
            let sessions: i64 = r.try_get("session_count").unwrap_or(0);
            let first: Option<DateTime<Utc>> = r.try_get("first_seen").unwrap_or(None);
            let last: Option<DateTime<Utc>> = r.try_get("last_seen").unwrap_or(None);
            json!({
                "login": login,
                "lurkSessions": sessions,
                "firstSeen": first.map(|dt| dt.to_rfc3339()),
                "lastSeen": last.map(|dt| dt.to_rfc3339()),
            })
        })
        .collect();

    let lurker_ratio = if seen_sample_viewers > 0 {
        (lurker_count as f64 / seen_sample_viewers as f64 * 1000.0).round() / 1000.0
    } else {
        0.0
    };
    let conversion_rate = if eligible_lurkers > 0 {
        (converted_lurkers as f64 / eligible_lurkers as f64 * 1000.0).round() / 1000.0
    } else {
        0.0
    };

    Json(json!({
        "dataAvailable": true,
        "regularLurkers": regular_lurkers,
        "lurkerStats": {
            "ratio": lurker_ratio,
            "avgSessions": (avg_sessions_lurkers * 10.0).round() / 10.0,
            "totalLurkers": lurker_count,
            "totalViewers": seen_sample_viewers,
        },
        "conversionStats": {
            "rate": conversion_rate,
            "eligible": eligible_lurkers,
            "converted": converted_lurkers,
        },
    })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
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
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_session_chatters (
                session_id BIGINT, chatter_login TEXT, chatter_id TEXT,
                messages INTEGER DEFAULT 0, seen_via_chatters_api BOOLEAN DEFAULT FALSE,
                first_message_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ)"
        ).execute(&pool).await.unwrap();
        Some(pool)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn run(pool: PgPool) -> serde_json::Value {
        let resp = lurker_analysis_handler(
            DashboardAuthLevel::Localhost,
            State(pool),
            Query(LurkerQuery { streamer: Some("nani".into()), days: Some(30) }),
        ).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        body_json(resp).await
    }

    // P2.68: anonyme Chatter (NULL chatter_login, gültige chatter_id) müssen als
    // Lurker mitgezählt werden — nicht durch den Bot-Filter fallen.
    #[tokio::test]
    async fn anonymous_null_login_lurker_counted() {
        let Some(pool) = make_pool("t_lurker_anon").await else { return };
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at) VALUES ('nani', NOW()-INTERVAL '2 days', NOW()-INTERVAL '2 days'+INTERVAL '3 hours')")
            .execute(&pool).await.unwrap();
        // Anonymer Lurker: kein Login, nur chatter_id, via Chatter-API gesehen, 0 Messages
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, messages, seen_via_chatters_api, first_message_at, last_seen_at) VALUES (1, NULL, 'anon-123', 0, TRUE, NULL, NOW()-INTERVAL '2 days')")
            .execute(&pool).await.unwrap();

        let v = run(pool).await;
        assert_eq!(v["dataAvailable"], true, "anonymer Lurker muss Daten liefern");
        assert_eq!(v["lurkerStats"]["totalLurkers"], 1, "anonymer NULL-Login-Lurker muss gezählt werden");
    }

    // P2.68: gemischt-groß geschriebener Bot-Login muss case-insensitiv gefiltert werden.
    #[tokio::test]
    async fn mixed_case_bot_filtered() {
        let Some(pool) = make_pool("t_lurker_botcase").await else { return };
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at) VALUES ('nani', NOW()-INTERVAL '2 days', NOW()-INTERVAL '2 days'+INTERVAL '3 hours')")
            .execute(&pool).await.unwrap();
        // Echter Lurker
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, messages, seen_via_chatters_api, last_seen_at) VALUES (1, 'realviewer', 'id-1', 0, TRUE, NOW()-INTERVAL '2 days')")
            .execute(&pool).await.unwrap();
        // Bot mit gemischter Groß-/Kleinschreibung → muss rausgefiltert werden
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, messages, seen_via_chatters_api, last_seen_at) VALUES (1, 'Nightbot', 'id-2', 0, TRUE, NOW()-INTERVAL '2 days')")
            .execute(&pool).await.unwrap();

        let v = run(pool).await;
        assert_eq!(v["lurkerStats"]["totalLurkers"], 1, "Nightbot (mixed-case) darf nicht als Lurker zählen");
    }
}
