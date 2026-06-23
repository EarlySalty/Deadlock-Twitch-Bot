//! Handler für `GET /twitch/api/v2/{streamer}/viewer-timeline` und
//! `GET /twitch/api/v2/{streamer}/viewer-timeline/profile`.
//!
//! Port von `bot/analytics/api_viewer_timeline.py`.
//! Komplexeste CTE: LAG-Window-Funktion detektiert Lücken > 2 Min in Presence-Ticks
//! und gruppiert sie zu Anwesenheits-Spans.
//! _classify_viewer: 1. new, 2. lurker, 3. dedicated/regular/casual.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix", "deutschedeadlockcommunity", "fossabot", "moobot", "nightbot",
    "pretzelrocks", "soundalerts", "streamlabs", "streamelements", "wizebot",
];

// ------------------------------------------------------------------
// Viewer-Klassifikation (Python-Parität: api_viewers.py Z.87–128)
// ------------------------------------------------------------------
fn classify_viewer(
    total_sessions: i64,
    total_messages: i64,
    first_seen_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> &'static str {
    let days_since_first = first_seen_at
        .map(|fs| (now - fs).num_days())
        .unwrap_or(9999);
    let days_since_last = last_seen_at
        .map(|ls| (now - ls).num_days())
        .unwrap_or(9999);
    let _ = days_since_last; // Parität: nicht direkt genutzt in Classify, nur für future use

    if days_since_first <= 14 && total_sessions <= 3 {
        return "new";
    }
    if total_messages == 0 {
        return "lurker";
    }
    let weeks_active = (days_since_first as f64 / 7.0).max(1.0);
    let sessions_per_week = total_sessions as f64 / weeks_active;
    let msgs_per_session = total_messages as f64 / total_sessions.max(1) as f64;

    if sessions_per_week >= 1.5 && msgs_per_session >= 3.0 && total_sessions >= 4 {
        return "dedicated";
    }
    if sessions_per_week >= 0.5 && total_sessions >= 3 {
        return "regular";
    }
    "casual"
}

// ------------------------------------------------------------------
// Query-Hilfsfunktionen für Bot-Exclusion
// ------------------------------------------------------------------
fn bot_not_in_sql(start_idx: usize, col: &str, extra: &[String]) -> (String, Vec<String>) {
    let all: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string())
        .chain(extra.iter().cloned())
        .collect();
    let placeholders: Vec<String> = (start_idx..start_idx + all.len())
        .map(|i| format!("${i}"))
        .collect();
    let clause = format!("{col} NOT IN ({})", placeholders.join(", "));
    (clause, all)
}

// ------------------------------------------------------------------
// Query Params
// ------------------------------------------------------------------
#[derive(Deserialize)]
pub struct ViewerTimelineQuery {
    pub session_id: Option<i64>,
    #[serde(default)]
    pub min_present_min: Option<i64>,
    #[serde(default)]
    pub segment: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct ViewerProfileQuery {
    pub login: Option<String>,
}

// ------------------------------------------------------------------
// GET /twitch/api/v2/{streamer}/viewer-timeline
// ------------------------------------------------------------------
pub async fn viewer_timeline_handler(
    auth: DashboardAuthLevel,
    Path(streamer_raw): Path<String>,
    State(pool): State<PgPool>,
    Query(params): Query<ViewerTimelineQuery>,
) -> impl IntoResponse {
    // Python: _require_v2_auth + _require_extended_plan (Paywall-Feature).
    // extended_gate deckt beides ab: None→401, Free-Partner→403, Admin/Localhost→pass.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }

    let streamer = streamer_raw.trim().to_lowercase();
    if streamer.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response();
    }

    let session_id = match params.session_id {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"session_id required"}))).into_response(),
    };
    let min_present_min = params.min_present_min.unwrap_or(0).max(0);
    let segment_filter: Option<String> = params.segment
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "all")
        .map(str::to_lowercase);
    let search_filter = params.search.as_deref().map(str::trim).unwrap_or("").to_lowercase();
    let limit = params.limit.unwrap_or(200).clamp(1, 1000);

    // 1. Session holen
    let session_row = sqlx::query(
        r#"SELECT id, started_at,
                  ROUND(EXTRACT(EPOCH FROM (
                      COALESCE(ended_at, started_at + COALESCE(duration_seconds, 0) * INTERVAL '1 second')
                      - started_at
                  )) / 60)::int AS duration_min
           FROM twitch_stream_sessions
           WHERE id = $1 AND LOWER(streamer_login) = $2
           LIMIT 1"#,
    )
    .bind(session_id)
    .bind(&streamer)
    .fetch_optional(&pool)
    .await;

    let session_row = match session_row {
        Err(e) => {
            tracing::error!("viewer-timeline session lookup Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Session not found"}))).into_response(),
        Ok(Some(r)) => r,
    };

    let session_start: DateTime<Utc> = match session_row.try_get("started_at") {
        Ok(ts) => ts,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"Session start missing"}))).into_response(),
    };
    let session_duration_min: i32 = session_row.try_get("duration_min").unwrap_or(0).max(0);

    // Exclusions: streamer selbst + bekannte Bots
    let extra_excluded = vec![streamer.clone()];
    let (span_bot_clause, span_bots) = bot_not_in_sql(3, "LOWER(viewer_login)", &extra_excluded);

    // 2. Presence-Spans per CTE aus twitch_viewer_presence_ticks
    let span_sql = format!(
        r#"WITH ticked AS (
               SELECT LOWER(viewer_login) AS viewer_login, tick_at,
                      EXTRACT(EPOCH FROM (tick_at - LAG(tick_at) OVER (
                          PARTITION BY LOWER(viewer_login) ORDER BY tick_at
                      ))) / 60 AS gap_min
               FROM twitch_viewer_presence_ticks
               WHERE session_id = $1
                 AND {span_bot_clause}
           ),
           grouped AS (
               SELECT viewer_login, tick_at,
                      SUM(CASE WHEN gap_min > 2 OR gap_min IS NULL THEN 1 ELSE 0 END)
                          OVER (PARTITION BY viewer_login ORDER BY tick_at) AS span_id
               FROM ticked
           )
           SELECT viewer_login,
                  GREATEST(0, ROUND(EXTRACT(EPOCH FROM (MIN(tick_at) - $2::timestamptz)) / 60)::int) AS start_min,
                  GREATEST(0, ROUND(EXTRACT(EPOCH FROM (MAX(tick_at) - $2::timestamptz)) / 60)::int) AS end_min
           FROM grouped
           GROUP BY viewer_login, span_id
           ORDER BY viewer_login, start_min"#
    );

    let mut span_q = sqlx::query(&span_sql)
        .bind(session_id)
        .bind(session_start);
    for bot in &span_bots {
        span_q = span_q.bind(bot);
    }

    let span_rows = match span_q.fetch_all(&pool).await {
        Err(e) => {
            tracing::error!("viewer-timeline spans Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
        Ok(r) => r,
    };

    // viewer_spans aufbauen
    let mut viewer_spans: std::collections::HashMap<String, Vec<(i32, i32)>> = std::collections::HashMap::new();
    for row in &span_rows {
        let login: String = row.try_get("viewer_login").unwrap_or_default();
        if login.is_empty() {
            continue;
        }
        let mut start_min: i32 = row.try_get("start_min").unwrap_or(0).max(0);
        let mut end_min: i32 = row.try_get("end_min").unwrap_or(0).max(start_min);
        if session_duration_min > 0 {
            start_min = start_min.min(session_duration_min);
            end_min = end_min.min(session_duration_min);
        }
        viewer_spans.entry(login).or_default().push((start_min, end_min));
    }

    if viewer_spans.is_empty() {
        return Json(json!({
            "session_id": session_id,
            "session_start": session_start.to_rfc3339(),
            "session_duration_min": session_duration_min,
            "viewers": [],
            "total_unique_tracked": 0,
        })).into_response();
    }

    // 3. Chat-Messages aus twitch_session_chatters
    let (msg_bot_clause, msg_bots) = bot_not_in_sql(3, "LOWER(chatter_login)", &extra_excluded);
    let msg_sql = format!(
        r#"SELECT LOWER(chatter_login) AS viewer_login, COALESCE(messages, 0)::bigint AS messages
           FROM twitch_session_chatters
           WHERE session_id = $1 AND LOWER(streamer_login) = $2
             AND {msg_bot_clause}"#
    );
    let mut msg_q = sqlx::query(&msg_sql).bind(session_id).bind(&streamer);
    for bot in &msg_bots {
        msg_q = msg_q.bind(bot);
    }
    let msg_rows = msg_q.fetch_all(&pool).await.unwrap_or_default();
    let chat_by_viewer: std::collections::HashMap<String, i64> = msg_rows.iter()
        .filter_map(|r| {
            let login: String = r.try_get("viewer_login").ok()?;
            let msgs: i64 = r.try_get("messages").unwrap_or(0);
            if login.is_empty() { None } else { Some((login, msgs)) }
        })
        .collect();

    // 4. Viewer-Profile (aggregiert über alle Sessions)
    let viewer_logins: Vec<String> = viewer_spans.keys().cloned().collect();
    let (prof_bot_clause, prof_bots) = bot_not_in_sql(3, "LOWER(sc.chatter_login)", &extra_excluded);
    let prof_sql = format!(
        r#"SELECT LOWER(sc.chatter_login) AS viewer_login,
                  COUNT(DISTINCT sc.session_id) AS total_sessions,
                  COALESCE(SUM(sc.messages), 0) AS total_messages,
                  MIN(s.started_at) AS first_seen_at,
                  MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(sc.streamer_login) = $1
             AND LOWER(sc.chatter_login) = ANY($2)
             AND {prof_bot_clause}
           GROUP BY LOWER(sc.chatter_login)"#
    );
    let mut prof_q = sqlx::query(&prof_sql).bind(&streamer).bind(&viewer_logins);
    for bot in &prof_bots {
        prof_q = prof_q.bind(bot);
    }
    let prof_rows = prof_q.fetch_all(&pool).await.unwrap_or_default();

    struct Profile {
        total_sessions: i64,
        total_messages: i64,
        first_seen_at: Option<DateTime<Utc>>,
        last_seen_at: Option<DateTime<Utc>>,
    }
    let profiles: std::collections::HashMap<String, Profile> = prof_rows.iter()
        .filter_map(|r| {
            let login: String = r.try_get("viewer_login").ok()?;
            if login.is_empty() { return None; }
            Some((login, Profile {
                total_sessions: r.try_get("total_sessions").unwrap_or(0),
                total_messages: r.try_get("total_messages").unwrap_or(0),
                first_seen_at: r.try_get("first_seen_at").ok(),
                last_seen_at: r.try_get("last_seen_at").ok(),
            }))
        })
        .collect();

    let now = Utc::now();

    // 5. Filtern, sortieren, limitieren
    let mut result_viewers: Vec<(i64, i64, String, serde_json::Value)> = viewer_spans
        .into_iter()
        .filter_map(|(login, spans)| {
            if !search_filter.is_empty() && !login.contains(&search_filter) {
                return None;
            }
            let total_present_min: i64 = spans.iter()
                .map(|(s, e)| (e - s).max(0) as i64)
                .sum();
            if total_present_min < min_present_min {
                return None;
            }
            let segment = profiles.get(&login).map(|p| {
                classify_viewer(p.total_sessions, p.total_messages, p.first_seen_at, p.last_seen_at, now)
            });
            if let Some(sf) = &segment_filter {
                if segment != Some(sf.as_str()) {
                    return None;
                }
            }
            let spans_json: Vec<serde_json::Value> = spans.iter()
                .map(|(s, e)| json!({"start_min": s, "end_min": e}))
                .collect();
            let chat_messages = *chat_by_viewer.get(&login).unwrap_or(&0);
            let viewer_json = json!({
                "login": login,
                "segment": segment,
                "spans": spans_json,
                "total_present_min": total_present_min,
                "chat_messages": chat_messages,
            });
            Some((-total_present_min, -chat_messages, login, viewer_json))
        })
        .collect();

    // sort: by (-present_min, -chat_messages, login)
    result_viewers.sort_by(|a, b| {
        a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))
    });
    let total_unique = result_viewers.len();
    let viewers: Vec<serde_json::Value> = result_viewers.into_iter()
        .take(limit as usize)
        .map(|(_, _, _, v)| v)
        .collect();

    Json(json!({
        "session_id": session_id,
        "session_start": session_start.to_rfc3339(),
        "session_duration_min": session_duration_min,
        "viewers": viewers,
        "total_unique_tracked": total_unique,
    })).into_response()
}

// ------------------------------------------------------------------
// GET /twitch/api/v2/{streamer}/viewer-timeline/profile
// ------------------------------------------------------------------
pub async fn viewer_timeline_profile_handler(
    auth: DashboardAuthLevel,
    Path(streamer_raw): Path<String>,
    State(pool): State<PgPool>,
    Query(params): Query<ViewerProfileQuery>,
) -> impl IntoResponse {
    // Python: _require_v2_auth + _require_extended_plan (Paywall-Feature).
    // extended_gate deckt beides ab: None→401, Free-Partner→403, Admin/Localhost→pass.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }

    let streamer = streamer_raw.trim().to_lowercase();
    let login = match params.login.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(l) => l.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"streamer and login required"}))).into_response(),
    };

    if streamer.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"streamer and login required"}))).into_response();
    }

    // Bekannte Bots → 404
    if KNOWN_CHAT_BOTS.contains(&login.as_str()) || login == streamer {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"Viewer not found"}))).into_response();
    }

    // CTE: alle Sessions in denen der Viewer präsent war (Ticks oder Chat)
    let rows = sqlx::query(
        r#"WITH session_ids AS (
               SELECT DISTINCT session_id FROM twitch_viewer_presence_ticks
               WHERE LOWER(streamer_login) = $1 AND LOWER(viewer_login) = $2
               UNION
               SELECT DISTINCT session_id FROM twitch_session_chatters
               WHERE LOWER(streamer_login) = $3 AND LOWER(chatter_login) = $4
           ),
           ticked AS (
               SELECT session_id, tick_at,
                      EXTRACT(EPOCH FROM (tick_at - LAG(tick_at) OVER (
                          PARTITION BY session_id ORDER BY tick_at
                      ))) / 60 AS gap_min
               FROM twitch_viewer_presence_ticks
               WHERE LOWER(streamer_login) = $5 AND LOWER(viewer_login) = $6
           ),
           grouped AS (
               SELECT session_id, tick_at,
                      SUM(CASE WHEN gap_min > 2 OR gap_min IS NULL THEN 1 ELSE 0 END)
                          OVER (PARTITION BY session_id ORDER BY tick_at) AS span_id
               FROM ticked
           ),
           span_groups AS (
               SELECT session_id,
                      GREATEST(0, ROUND(EXTRACT(EPOCH FROM (MAX(tick_at) - MIN(tick_at))) / 60)::int) AS span_present_min
               FROM grouped
               GROUP BY session_id, span_id
           ),
           presence_totals AS (
               SELECT session_id, COALESCE(SUM(span_present_min), 0) AS total_present_min
               FROM span_groups
               GROUP BY session_id
           )
           SELECT s.id AS session_id, s.started_at,
                  COALESCE(p.total_present_min, 0) AS total_present_min,
                  COALESCE(sc.messages, 0)::bigint AS chat_messages
           FROM session_ids sid
           JOIN twitch_stream_sessions s ON s.id = sid.session_id
           LEFT JOIN presence_totals p ON p.session_id = s.id
           LEFT JOIN twitch_session_chatters sc
               ON sc.session_id = s.id
              AND LOWER(sc.streamer_login) = $7
              AND LOWER(sc.chatter_login) = $8
           WHERE LOWER(s.streamer_login) = $9
           ORDER BY s.started_at DESC"#,
    )
    .bind(&streamer)
    .bind(&login)
    .bind(&streamer)
    .bind(&login)
    .bind(&streamer)
    .bind(&login)
    .bind(&streamer)
    .bind(&login)
    .bind(&streamer)
    .fetch_all(&pool)
    .await;

    match rows {
        Err(e) => {
            tracing::error!("viewer-timeline/profile DB-Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response()
        }
        Ok(rows) => {
            let sessions: Vec<serde_json::Value> = rows.iter().map(|r| {
                let started_at: Option<DateTime<Utc>> = r.try_get("started_at").ok();
                json!({
                    "session_id": r.try_get::<i64, _>("session_id").unwrap_or(0),
                    "started_at": started_at.map(|t| t.to_rfc3339()),
                    "total_present_min": r.try_get::<i64, _>("total_present_min").unwrap_or(0),
                    "chat_messages": r.try_get::<i64, _>("chat_messages").unwrap_or(0),
                })
            }).collect();
            let total_sessions = sessions.len();
            Json(json!({
                "streamer": streamer,
                "login": login,
                "sessions": sessions,
                "total_sessions": total_sessions,
            })).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    // ── Plan-Gate-Verdrahtung (env-gated) ───────────────────────────────────
    // Python ruft hier _require_v2_auth UND _require_extended_plan. Vor dem Fix
    // prüfte Rust nur None→401, ein Free-Partner umging die Paywall. Ein Partner
    // ohne Plan muss 403 erhalten. twitch_user_id leer → Trial-Grant-Pfad
    // (braucht user_id+login) springt nicht an, leere Plan-Tabellen → raid_free.
    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_plan_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new().max_connections(1).connect(dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE streamer_plans (
                   twitch_user_id TEXT,
                   twitch_login TEXT,
                   manual_plan_id TEXT,
                   manual_plan_expires_at TEXT,
                   manual_plan_updated_at TIMESTAMPTZ
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_billing_subscriptions (
                   customer_reference TEXT,
                   plan_id TEXT,
                   status TEXT,
                   current_period_end TEXT,
                   updated_at TIMESTAMPTZ
               )"#,
        ).execute(&pool).await.unwrap();
        pool
    }

    async fn make_timeline_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new().max_connections(1).connect(dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_stream_sessions (
                   id BIGSERIAL PRIMARY KEY,
                   streamer_login TEXT NOT NULL,
                   started_at TIMESTAMPTZ NOT NULL,
                   ended_at TIMESTAMPTZ,
                   duration_seconds BIGINT
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_viewer_presence_ticks (
                   session_id BIGINT NOT NULL,
                   streamer_login TEXT NOT NULL,
                   viewer_login TEXT NOT NULL,
                   tick_at TIMESTAMPTZ NOT NULL
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_session_chatters (
                   session_id BIGINT NOT NULL,
                   streamer_login TEXT NOT NULL,
                   chatter_login TEXT NOT NULL,
                   messages INTEGER DEFAULT 0
               )"#,
        ).execute(&pool).await.unwrap();
        pool
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn free_partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "freeloader".to_string(),
            twitch_user_id: String::new(),
            display_name: String::new(),
        }
    }

    #[tokio::test]
    async fn viewer_timeline_gates_free_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_plan_pool(&dsn, "timeline_gate_session").await;
        let resp = viewer_timeline_handler(
            free_partner(),
            Path("host".to_string()),
            State(pool),
            Query(ViewerTimelineQuery {
                session_id: Some(1),
                min_present_min: None,
                segment: None,
                search: None,
                limit: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Free-Partner muss 403 erhalten");
    }

    #[tokio::test]
    async fn viewer_timeline_profile_gates_free_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_plan_pool(&dsn, "timeline_gate_profile").await;
        let resp = viewer_timeline_profile_handler(
            free_partner(),
            Path("host".to_string()),
            State(pool),
            Query(ViewerProfileQuery { login: Some("someviewer".into()) }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Free-Partner muss 403 erhalten");
    }

    #[tokio::test]
    async fn viewer_timeline_decodes_session_chat_messages_as_bigint() {
        let dsn = db_dsn_or_skip!();
        let pool = make_timeline_pool(&dsn, "timeline_chat_messages_bigint").await;
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) \
             VALUES (100, 'host', '2026-06-23T10:00:00Z', '2026-06-23T11:00:00Z')",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_viewer_presence_ticks (session_id, streamer_login, viewer_login, tick_at) \
             VALUES \
             (100, 'host', 'viewer1', '2026-06-23T10:05:00Z'), \
             (100, 'host', 'viewer1', '2026-06-23T10:10:00Z')",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login, messages) \
             VALUES (100, 'host', 'viewer1', 7)",
        ).execute(&pool).await.unwrap();

        let timeline_resp = viewer_timeline_handler(
            DashboardAuthLevel::admin(),
            Path("host".to_string()),
            State(pool.clone()),
            Query(ViewerTimelineQuery {
                session_id: Some(100),
                min_present_min: None,
                segment: None,
                search: None,
                limit: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(timeline_resp.status(), StatusCode::OK);
        let body = json_body(timeline_resp).await;
        assert_eq!(body["viewers"][0]["login"], "viewer1");
        assert_eq!(body["viewers"][0]["chat_messages"], 7);

        let profile_resp = viewer_timeline_profile_handler(
            DashboardAuthLevel::admin(),
            Path("host".to_string()),
            State(pool),
            Query(ViewerProfileQuery { login: Some("viewer1".to_string()) }),
        )
        .await
        .into_response();
        assert_eq!(profile_resp.status(), StatusCode::OK);
        let body = json_body(profile_resp).await;
        assert_eq!(body["sessions"][0]["chat_messages"], 7);
    }
}
