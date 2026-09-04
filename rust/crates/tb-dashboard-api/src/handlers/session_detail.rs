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

struct SessionDetailRow {
    id: i64,
    streamer_login: String,
    started_at: chrono::DateTime<chrono::Utc>,
    ended_at: Option<chrono::DateTime<chrono::Utc>>,
    duration_seconds: Option<i32>,
    start_viewers: Option<i32>,
    peak_viewers: Option<i32>,
    end_viewers: Option<i32>,
    avg_viewers: Option<f64>,
    retention_5m: Option<f64>,
    retention_10m: Option<f64>,
    retention_20m: Option<f64>,
    dropoff_pct: Option<f64>,
    unique_chatters: Option<i32>,
    first_time_chatters: Option<i32>,
    returning_chatters: Option<i32>,
    stream_title: Option<String>,
}

struct SessionEventSessionRow {
    streamer_login: String,
    started_at: chrono::DateTime<chrono::Utc>,
    ended_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn require_auth(auth: &DashboardAuthLevel) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(auth, DashboardAuthLevel::None) {
        Err(crate::auth::unauthorized_v2_json())
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
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"Invalid session ID"})),
            )
                .into_response()
        }
    };

    let owner = owner_login(&auth);

    // ── Last-Stream-Clamp (Paywall gegen Historie-Leak) ──────────────────────
    // Ohne das konsolidierte `analytics`-Flag darf ein Partner nur die zuletzt
    // BEENDETE eigene Session abrufen, exakt dieselbe Definition wie overview.rs.
    // Localhost/Admin überspringen den Clamp. Die IDOR-Grenze (Owner-Query unten)
    // bleibt unangetastet: das Flag weitet NUR das Zeitfenster, nie den Zugriff
    // auf fremde Sessions.
    if let Some(resp) = letzte_session_klemme(&pool, &auth, session_id).await {
        return resp;
    }

    // ── Haupt-Session-Row ────────────────────────────────────────────────────
    let row = match owner {
        Some(login) => {
            sqlx::query_as!(
                SessionDetailRow,
                r#"SELECT id, streamer_login, started_at, ended_at, duration_seconds,
                      start_viewers, peak_viewers, end_viewers, avg_viewers,
                      retention_5m, retention_10m, retention_20m,
                      dropoff_pct, unique_chatters, first_time_chatters,
                      returning_chatters, stream_title
               FROM twitch_stream_sessions
               WHERE id = $1 AND LOWER(streamer_login) = $2"#,
                session_id,
                login
            )
            .fetch_optional(&pool)
            .await
        }

        None => {
            sqlx::query_as!(
                SessionDetailRow,
                r#"SELECT id, streamer_login, started_at, ended_at, duration_seconds,
                      start_viewers, peak_viewers, end_viewers, avg_viewers,
                      retention_5m, retention_10m, retention_20m,
                      dropoff_pct, unique_chatters, first_time_chatters,
                      returning_chatters, stream_title
               FROM twitch_stream_sessions
               WHERE id = $1"#,
                session_id
            )
            .fetch_optional(&pool)
            .await
        }
    };

    let row = match row {
        Err(e) => {
            tracing::error!("session_detail DB-Fehler (Haupt-Row): {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"Session not found"})),
            )
                .into_response()
        }
        Ok(Some(r)) => r,
    };

    // ── Prüfen ob twitch_session_chatters Daten hat ─────────────────────────
    let chatter_presence = sqlx::query!(
        "SELECT 1 AS \"present!\" FROM twitch_session_chatters WHERE session_id = $1 LIMIT 1",
        session_id
    )
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
            i64::from(row.unique_chatters.unwrap_or(0)),
            i64::from(row.first_time_chatters.unwrap_or(0)),
            i64::from(row.returning_chatters.unwrap_or(0)),
        )
    };

    // ── Viewer-Timeline ──────────────────────────────────────────────────────
    let timeline = sqlx::query!(
        "SELECT minutes_from_start, viewer_count FROM twitch_session_viewers WHERE session_id = $1 ORDER BY minutes_from_start",
        session_id
    )
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
    let started_at: String = row.started_at.to_rfc3339();
    let ended_at: Option<String> = row.ended_at.map(|t| t.to_rfc3339());

    Json(json!({
        "id": row.id,
        "streamerLogin": row.streamer_login,
        "startedAt": started_at,
        "endedAt": ended_at,
        "duration": row.duration_seconds.unwrap_or(0),
        "startViewers": row.start_viewers.unwrap_or(0),
        "peakViewers": row.peak_viewers.unwrap_or(0),
        "endViewers": row.end_viewers.unwrap_or(0),
        // NULL (Session ohne Viewer-Samples, z. B. frisch bei stream.online
        // eroeffnet) bleibt JSON null statt 0.0 -> Frontend kann "noch keine
        // Daten" von einer echten 0 unterscheiden.
        "avgViewers": row.avg_viewers,
        "retention5m": row.retention_5m.map(|v| v * 100.0),
        "retention10m": row.retention_10m.map(|v| v * 100.0),
        "retention20m": row.retention_20m.map(|v| v * 100.0),
        "dropoffPct": row.dropoff_pct.map(|v| v * 100.0),
        "uniqueChatters": unique_chatters,
        "firstTimeChatters": first_time_chatters,
        "returningChatters": returning_chatters,
        "title": row.stream_title.unwrap_or_default(),
        "timeline": timeline.iter().map(|t| json!({
            "minute": t.minutes_from_start.unwrap_or(0),
            "viewers": t.viewer_count,
        })).collect::<Vec<_>>(),
        "chatters": chatters.iter().map(|c| json!({
            "login": c.try_get::<String, _>("chatter_login").unwrap_or_default(),
            "messages": c.try_get::<i32, _>("messages").unwrap_or(0),
        })).collect::<Vec<_>>(),
    }))
    .into_response()
}

/// Last-Stream-Klemme gegen den Historie-Leak.
///
/// Ohne Netzwerk Plus darf ein Partner nur die zuletzt BEENDETE eigene Session
/// abrufen, exakt dieselbe Definition wie in `overview.rs`. Admin und Localhost
/// ueberspringen die Klemme. Die IDOR-Grenze bleibt unangetastet: die Stufe
/// weitet nur das Zeitfenster, nie den Zugriff auf fremde Sessions.
///
/// Steht als eigene Funktion da, weil sie an ZWEI Endpunkten haengt: die
/// Session-Detailansicht und die Ereignisliste derselben Session. Die
/// Ereignisliste hatte sie frueher nicht und war damit der Nebeneingang zur
/// kompletten Historie.
async fn letzte_session_klemme(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    session_id: i64,
) -> Option<axum::response::Response> {
    let DashboardAuthLevel::Partner {
        twitch_login,
        twitch_user_id,
        ..
    } = auth
    else {
        return None;
    };
    let login = twitch_login.to_lowercase();
    if crate::auth::has_analytics_entitlement(pool, &login, twitch_user_id).await {
        return None;
    }
    let latest = crate::handlers::last_session::latest_ended_session(pool, &login)
        .await
        .map(|s| s.id);
    if latest == Some(session_id) {
        return None;
    }
    Some(crate::auth::plan_required_response())
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
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"Invalid session ID"})),
            )
                .into_response()
        }
    };

    // Dieselbe Klemme wie in der Detailansicht: ohne Plus nur der letzte Stream.
    if let Some(resp) = letzte_session_klemme(&pool, &auth, session_id).await {
        return resp;
    }

    let owner = owner_login(&auth);

    // Session-Metadaten holen (Owner-Isolierung + started/ended_at für Event-Fenster)
    let sess = match owner {
        Some(login) => sqlx::query_as!(
            SessionEventSessionRow,
            "SELECT streamer_login, started_at, ended_at FROM twitch_stream_sessions WHERE id = $1 AND LOWER(streamer_login) = $2",
            session_id,
            login
        ).fetch_optional(&pool).await,
        None => sqlx::query_as!(
            SessionEventSessionRow,
            "SELECT streamer_login, started_at, ended_at FROM twitch_stream_sessions WHERE id = $1",
            session_id
        ).fetch_optional(&pool).await,
    };

    let sess = match sess {
        Err(e) => {
            tracing::error!("session_events DB-Fehler (Sess-Row): {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"Session not found"})),
            )
                .into_response()
        }
        Ok(Some(r)) => r,
    };

    let streamer_login = sess.streamer_login;
    let started_at = sess.started_at;
    let ended_at = sess.ended_at;

    // Twitch-User-ID für Channel-Updates
    let twitch_user_id: Option<String> = sqlx::query_scalar!(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = $1 LIMIT 1",
        streamer_login.to_lowercase()
    )
    .fetch_optional(&pool)
    .await
    .unwrap_or(None)
    .flatten();

    // Channel-Updates im Session-Fenster
    let channel_updates: Vec<serde_json::Value> = if let Some(uid) = &twitch_user_id {
        let end_bound: chrono::DateTime<chrono::Utc> = ended_at.unwrap_or_else(chrono::Utc::now);
        sqlx::query!(
            r#"SELECT recorded_at, title, game_name, language
               FROM twitch_channel_updates
               WHERE twitch_user_id = $1
                 AND recorded_at::timestamptz BETWEEN $2 AND $3
               ORDER BY recorded_at"#,
            uid,
            started_at,
            end_bound
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| {
            let at = r.recorded_at.to_rfc3339();
            json!({
                "at": at,
                "title": r.title.clone().unwrap_or_default(),
                "game": r.game_name.clone().unwrap_or_default(),
                "language": r.language.clone().unwrap_or_default(),
            })
        })
        .collect()
    } else {
        vec![]
    };

    // Raids im Session-Fenster: eingehend (arrival_tracking) + ausgehend
    // (raid_history), UNION ALL nach Zeit sortiert (Port api_v2.py:2388-2427).
    // viewer_count ist INTEGER -> ::bigint, sonst stiller Decode-Fehler.
    let streamer_lower = streamer_login.to_lowercase();
    let raids: Vec<serde_json::Value> = sqlx::query!(
        r#"
        SELECT detected_at AS "at!", from_broadcaster_login AS "channel!",
               viewer_count::bigint AS "viewers!", 'incoming'::text AS "direction!"
          FROM twitch_raid_arrival_tracking
         WHERE LOWER(to_broadcaster_login) = $1
           AND detected_at BETWEEN $2 AND COALESCE($3, NOW())
        UNION ALL
        SELECT executed_at AS "at!", to_broadcaster_login AS "channel!",
               COALESCE(viewer_count, 0)::bigint AS "viewers!", 'outgoing'::text AS "direction!"
          FROM twitch_raid_history
         WHERE LOWER(from_broadcaster_login) = $1
           AND executed_at BETWEEN $2 AND COALESCE($3, NOW())
        ORDER BY 1
        "#,
        &streamer_lower,
        started_at,
        ended_at
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default()
    .iter()
    .map(|r| {
        json!({
            "at": r.at.to_rfc3339(),
            "channel": r.channel.clone(),
            "viewers": r.viewers,
            "direction": r.direction.clone(),
        })
    })
    .collect();

    // Follows pro Minute im Session-Fenster (Port api_v2.py:2429-2456).
    let follows: Vec<serde_json::Value> = sqlx::query!(
        r#"
        SELECT DATE_TRUNC('minute', followed_at::timestamptz) AS "minute!", COUNT(*) AS "cnt!"
          FROM twitch_follow_events
         WHERE LOWER(streamer_login) = $1
           AND followed_at::timestamptz BETWEEN $2 AND COALESCE($3, NOW())
         GROUP BY 1
         ORDER BY 1
        "#,
        &streamer_lower,
        started_at,
        ended_at
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default()
    .iter()
    .map(|r| {
        json!({
            "minute": r.minute.to_rfc3339(),
            "count": r.cnt,
        })
    })
    .collect();

    Json(json!({
        "sessionId": session_id,
        "streamerLogin": streamer_login,
        "channel_updates": channel_updates,
        "raids": raids,
        "follows_per_minute": follows,
    }))
    .into_response()
}
