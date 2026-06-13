//! Handler für `GET /twitch/api/v2/session/{id}` und `session/{id}/events`.
//!
//! Port von `bot/analytics/api_v2.py:_api_v2_session_detail` + `_load_session_detail`
//! sowie `_api_v2_session_events` + `_load_session_events`.
//!
//! Partner-Isolierung: `DashboardAuthLevel::Partner` → Abfrage nur gegen eigene Sessions.
//! Admin/Localhost → beliebige Session abrufbar.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

// Statische Bot-Exclusion-Liste (tb-chat/chatter_tracking.rs Z.42, chat_bots.py Z.8–19).
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

fn owner_login(auth: &DashboardAuthLevel) -> Option<&str> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => Some(twitch_login.as_str()),
        _ => None,
    }
}

/// `GET /twitch/api/v2/session/{id}`
pub async fn session_detail_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(session_id_str): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let session_id: i64 = match session_id_str.parse() {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid session ID"}))).into_response(),
    };

    let owner = owner_login(&auth);

    // ── Haupt-Session-Row ────────────────────────────────────────────────────
    let row = match owner {
        Some(login) => sqlx::query(
            r#"SELECT id, streamer_login, started_at, ended_at, duration_seconds,
                      start_viewers, peak_viewers, end_viewers, avg_viewers,
                      retention_5m, retention_10m, retention_20m,
                      dropoff_pct, unique_chatters, first_time_chatters,
                      returning_chatters, stream_title
               FROM twitch_stream_sessions
               WHERE id = $1 AND LOWER(streamer_login) = $2"#,
        ).bind(session_id).bind(login).fetch_optional(&pool).await,

        None => sqlx::query(
            r#"SELECT id, streamer_login, started_at, ended_at, duration_seconds,
                      start_viewers, peak_viewers, end_viewers, avg_viewers,
                      retention_5m, retention_10m, retention_20m,
                      dropoff_pct, unique_chatters, first_time_chatters,
                      returning_chatters, stream_title
               FROM twitch_stream_sessions
               WHERE id = $1"#,
        ).bind(session_id).fetch_optional(&pool).await,
    };

    let row = match row {
        Err(e) => {
            tracing::error!("session_detail DB-Fehler (Haupt-Row): {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Session not found"}))).into_response(),
        Ok(Some(r)) => r,
    };

    // ── Prüfen ob twitch_session_chatters Daten hat ─────────────────────────
    let chatter_presence = sqlx::query(
        "SELECT 1 FROM twitch_session_chatters WHERE session_id = $1 LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(&pool)
    .await
    .unwrap_or(None);

    // ── Chatter-Stats (bot-bereinigt) ────────────────────────────────────────
    // Parameterindizes: $1 = session_id, $3..$N+2 = Bot-Logins.
    // $2 bleibt ungenutzt (Reservierung für zukünftige Extension; Python nutzte ein Tuple-Spread).
    // Tatsächlich: $1 = session_id, $2..$N+1 = Bot-Logins.
    let bot_in_clause_chatter_stats = {
        let placeholders: Vec<String> = (2..=(KNOWN_CHAT_BOTS.len() + 1))
            .map(|i| format!("${i}"))
            .collect();
        format!("sc.chatter_login NOT IN ({})", placeholders.join(", "))
    };

    let chatter_stats_sql = format!(
        r#"SELECT
               COUNT(DISTINCT CASE
                   WHEN sc.messages > 0 THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                   ELSE NULL END) AS unique_chatters,
               COUNT(DISTINCT CASE
                   WHEN sc.messages > 0
                        AND LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0'))
                            IN ('1', 't', 'true')
                   THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                   ELSE NULL END) AS first_time_chatters,
               COUNT(DISTINCT CASE
                   WHEN sc.messages > 0
                        AND LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0'))
                            NOT IN ('1', 't', 'true')
                   THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                   ELSE NULL END) AS returning_chatters
           FROM twitch_session_chatters sc
           WHERE sc.session_id = $1
             AND {bot_in_clause_chatter_stats}"#
    );

    let mut cs_query = sqlx::query(&chatter_stats_sql).bind(session_id);
    for bot in KNOWN_CHAT_BOTS {
        cs_query = cs_query.bind(*bot);
    }
    let chatter_stats = cs_query.fetch_optional(&pool).await.unwrap_or(None);

    // Fallback: Session-Row-Werte wenn keine Chatter-Tracking-Daten
    let (unique_chatters, first_time_chatters, returning_chatters) = if chatter_presence.is_some() {
        if let Some(cs) = &chatter_stats {
            (
                cs.try_get::<i64, _>("unique_chatters").unwrap_or(0),
                cs.try_get::<i64, _>("first_time_chatters").unwrap_or(0),
                cs.try_get::<i64, _>("returning_chatters").unwrap_or(0),
            )
        } else {
            (0i64, 0i64, 0i64)
        }
    } else {
        (
            row.try_get::<i32, _>("unique_chatters").unwrap_or(0) as i64,
            row.try_get::<i32, _>("first_time_chatters").unwrap_or(0) as i64,
            row.try_get::<i32, _>("returning_chatters").unwrap_or(0) as i64,
        )
    };

    // ── Viewer-Timeline ──────────────────────────────────────────────────────
    let timeline = sqlx::query(
        "SELECT minutes_from_start, viewer_count FROM twitch_session_viewers WHERE session_id = $1 ORDER BY minutes_from_start",
    )
    .bind(session_id)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    // ── Top-Chatters (bot-bereinigt, Top 20) ─────────────────────────────────
    let top_chatters_in_clause = {
        let placeholders: Vec<String> = (2..=(KNOWN_CHAT_BOTS.len() + 1))
            .map(|i| format!("${i}"))
            .collect();
        format!("sc.chatter_login NOT IN ({})", placeholders.join(", "))
    };
    let top_sql = format!(
        r#"SELECT chatter_login, messages FROM twitch_session_chatters sc
           WHERE sc.session_id = $1 AND {top_chatters_in_clause}
           ORDER BY messages DESC LIMIT 20"#
    );
    let mut top_query = sqlx::query(&top_sql).bind(session_id);
    for bot in KNOWN_CHAT_BOTS {
        top_query = top_query.bind(*bot);
    }
    let chatters = top_query.fetch_all(&pool).await.unwrap_or_default();

    // ── Response aufbauen ────────────────────────────────────────────────────
    let started_at: String = row.try_get::<chrono::DateTime<chrono::Utc>, _>("started_at")
        .map(|t| t.to_rfc3339())
        .unwrap_or_default();
    let ended_at: Option<String> = row.try_get::<chrono::DateTime<chrono::Utc>, _>("ended_at")
        .ok()
        .map(|t| t.to_rfc3339());

    Json(json!({
        "id": row.try_get::<i64, _>("id").unwrap_or(0),
        "streamerLogin": row.try_get::<String, _>("streamer_login").unwrap_or_default(),
        "startedAt": started_at,
        "endedAt": ended_at,
        "duration": row.try_get::<i32, _>("duration_seconds").unwrap_or(0),
        "startViewers": row.try_get::<i32, _>("start_viewers").unwrap_or(0),
        "peakViewers": row.try_get::<i32, _>("peak_viewers").unwrap_or(0),
        "endViewers": row.try_get::<i32, _>("end_viewers").unwrap_or(0),
        "avgViewers": row.try_get::<f64, _>("avg_viewers").unwrap_or(0.0),
        "retention5m": row.try_get::<f64, _>("retention_5m").map(|v| v * 100.0).unwrap_or(0.0),
        "retention10m": row.try_get::<f64, _>("retention_10m").map(|v| v * 100.0).unwrap_or(0.0),
        "retention20m": row.try_get::<f64, _>("retention_20m").map(|v| v * 100.0).unwrap_or(0.0),
        "dropoffPct": row.try_get::<f64, _>("dropoff_pct").map(|v| v * 100.0).unwrap_or(0.0),
        "uniqueChatters": unique_chatters,
        "firstTimeChatters": first_time_chatters,
        "returningChatters": returning_chatters,
        "title": row.try_get::<String, _>("stream_title").unwrap_or_default(),
        "timeline": timeline.iter().map(|t| json!({
            "minute": t.try_get::<i32, _>("minutes_from_start").unwrap_or(0),
            "viewers": t.try_get::<i32, _>("viewer_count").unwrap_or(0),
        })).collect::<Vec<_>>(),
        "chatters": chatters.iter().map(|c| json!({
            "login": c.try_get::<String, _>("chatter_login").unwrap_or_default(),
            "messages": c.try_get::<i32, _>("messages").unwrap_or(0),
        })).collect::<Vec<_>>(),
    })).into_response()
}

/// `GET /twitch/api/v2/session/{id}/events`
pub async fn session_events_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(session_id_str): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&auth) {
        return e.into_response();
    }

    let session_id: i64 = match session_id_str.parse() {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Invalid session ID"}))).into_response(),
    };

    let owner = owner_login(&auth);

    // Session-Metadaten holen (Owner-Isolierung + started/ended_at für Event-Fenster)
    let sess = match owner {
        Some(login) => sqlx::query(
            "SELECT streamer_login, started_at, ended_at FROM twitch_stream_sessions WHERE id = $1 AND LOWER(streamer_login) = $2",
        ).bind(session_id).bind(login).fetch_optional(&pool).await,
        None => sqlx::query(
            "SELECT streamer_login, started_at, ended_at FROM twitch_stream_sessions WHERE id = $1",
        ).bind(session_id).fetch_optional(&pool).await,
    };

    let sess = match sess {
        Err(e) => {
            tracing::error!("session_events DB-Fehler (Sess-Row): {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Session not found"}))).into_response(),
        Ok(Some(r)) => r,
    };

    let streamer_login: String = sess.try_get("streamer_login").unwrap_or_default();
    let started_at: chrono::DateTime<chrono::Utc> = sess.try_get("started_at").unwrap_or_default();
    let ended_at: Option<chrono::DateTime<chrono::Utc>> = sess.try_get("ended_at").ok();

    // Twitch-User-ID für Channel-Updates
    let uid_row = sqlx::query(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = $1 LIMIT 1",
    )
    .bind(streamer_login.to_lowercase())
    .fetch_optional(&pool)
    .await
    .unwrap_or(None);
    let twitch_user_id: Option<String> = uid_row.as_ref().and_then(|r| r.try_get("twitch_user_id").ok());

    // Channel-Updates im Session-Fenster
    let channel_updates: Vec<serde_json::Value> = if let Some(uid) = &twitch_user_id {
        let end_bound: chrono::DateTime<chrono::Utc> = ended_at.unwrap_or_else(chrono::Utc::now);
        sqlx::query(
            r#"SELECT recorded_at, title, game_name, language
               FROM twitch_channel_updates
               WHERE twitch_user_id = $1
                 AND recorded_at::timestamptz BETWEEN $2 AND $3
               ORDER BY recorded_at"#,
        )
        .bind(uid)
        .bind(started_at)
        .bind(end_bound)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| {
            let at: String = r.try_get::<chrono::DateTime<chrono::Utc>, _>("recorded_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default();
            json!({
                "recordedAt": at,
                "title": r.try_get::<String, _>("title").unwrap_or_default(),
                "gameName": r.try_get::<String, _>("game_name").unwrap_or_default(),
                "language": r.try_get::<String, _>("language").unwrap_or_default(),
            })
        })
        .collect()
    } else {
        vec![]
    };

    Json(json!({
        "sessionId": session_id,
        "streamerLogin": streamer_login,
        "channelUpdates": channel_updates,
        "raids": [],
        "follows": [],
    }))
    .into_response()
}
