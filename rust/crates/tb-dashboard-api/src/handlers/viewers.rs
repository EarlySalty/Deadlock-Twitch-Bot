//! viewer-directory, viewer-detail, viewer-segments.
//!
//! Port von bot/analytics/api_viewers.py + raw_chat_status.py.
//! Batch-IN-Queries der Python-Version werden durch Postgres-Array (= ANY($n)) ersetzt.

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

const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix", "deutschedeadlockcommunity", "fossabot", "moobot", "nightbot",
    "pretzelrocks", "soundalerts", "streamlabs", "streamelements", "wizebot",
];

/// Viewer-Exklusionsliste: statische Known-Bots **plus** der Streamer selbst.
///
/// Port von `api_viewers.py::_collect_viewer_exclusion_logins`, das in den
/// Exklusions-Set immer den eigenen Streamer-Login legt (Z.33). Ohne diesen
/// Self-Ausschluss zählt ein Streamer, der im eigenen Chat schreibt, als
/// eigener Viewer in Directory/Segments. Die dynamischen Bot-Accounts
/// (chat-/raid-bot-Login) deckt `KNOWN_CHAT_BOTS` bereits ab; sie sind in
/// diesem Crate nicht aus der Bot-Config greifbar.
///
/// `streamer` wird klein geschrieben erwartet (Aufrufer normalisieren bereits).
fn viewer_exclusion_logins(streamer: &str) -> Vec<String> {
    let mut logins: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();
    let own = streamer.to_lowercase();
    if !own.is_empty() && !logins.contains(&own) {
        logins.push(own);
    }
    logins
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared: classify_viewer (identisch zu viewer_timeline.rs)
// ─────────────────────────────────────────────────────────────────────────────

fn classify_viewer(
    total_sessions: i64,
    total_messages: i64,
    first_seen_at: Option<DateTime<Utc>>,
    _last_seen_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> &'static str {
    let days_since_first = first_seen_at.map(|fs| (now - fs).num_days()).unwrap_or(9999);
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

// ─────────────────────────────────────────────────────────────────────────────
// Shared: build_raw_chat_status
// (raw_chat_status.py:166 — inlined, vereinfacht, gleiche Felder)
// ─────────────────────────────────────────────────────────────────────────────

async fn build_raw_chat_status(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> serde_json::Value {
    // Presence-Stats
    let pres = sqlx::query(
        r#"SELECT COUNT(*) AS presence_rows,
                  COUNT(DISTINCT sc.session_id) AS sessions_with_presence
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(s.streamer_login) = $1 AND s.started_at >= $2"#,
    )
    .bind(streamer)
    .bind(since)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let presence_rows: i64 = pres.as_ref().and_then(|r| r.try_get("presence_rows").ok()).unwrap_or(0);
    let sessions_with_presence: i64 = pres.as_ref().and_then(|r| r.try_get("sessions_with_presence").ok()).unwrap_or(0);

    // Gap-Start: früheste Session mit Presence aber ohne Raw-Nachrichten
    let gap_row = sqlx::query(
        r#"SELECT MIN(s.started_at) AS gap_start
           FROM twitch_stream_sessions s
           WHERE LOWER(s.streamer_login) = $1 AND s.started_at >= $2
             AND EXISTS (SELECT 1 FROM twitch_session_chatters sc WHERE sc.session_id = s.id)
             AND NOT EXISTS (SELECT 1 FROM twitch_chat_messages m WHERE m.session_id = s.id)"#,
    )
    .bind(streamer)
    .bind(since)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let gap_start: Option<String> = gap_row.as_ref()
        .and_then(|r| r.try_get::<Option<DateTime<Utc>>, _>("gap_start").ok())
        .flatten()
        .map(|t| t.to_rfc3339());

    // Raw-Stats
    let raw_row = sqlx::query(
        r#"SELECT COUNT(*) AS raw_rows,
                  COUNT(DISTINCT m.session_id) AS sessions_with_raw,
                  MAX(m.message_ts) AS last_message_at
           FROM twitch_chat_messages m
           WHERE LOWER(m.streamer_login) = $1 AND m.message_ts >= $2"#,
    )
    .bind(streamer)
    .bind(since)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let raw_rows: i64 = raw_row.as_ref().and_then(|r| r.try_get("raw_rows").ok()).unwrap_or(0);
    let sessions_with_raw: i64 = raw_row.as_ref().and_then(|r| r.try_get("sessions_with_raw").ok()).unwrap_or(0);
    let last_message_at: Option<String> = raw_row.as_ref()
        .and_then(|r| r.try_get::<Option<DateTime<Utc>>, _>("last_message_at").ok())
        .flatten()
        .map(|t| t.to_rfc3339());

    // Ingest-Health (best-effort, Fehler ignorieren)
    let health = sqlx::query(
        r#"SELECT last_raw_chat_insert_ok_at, last_raw_chat_insert_error_at, last_raw_chat_error
           FROM twitch_raw_chat_ingest_health WHERE LOWER(streamer_login) = $1 LIMIT 1"#,
    )
    .bind(streamer)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let last_insert_ok: Option<String> = health.as_ref()
        .and_then(|r| r.try_get::<Option<DateTime<Utc>>, _>("last_raw_chat_insert_ok_at").ok())
        .flatten()
        .map(|t| t.to_rfc3339());
    let last_insert_err: Option<String> = health.as_ref()
        .and_then(|r| r.try_get::<Option<DateTime<Utc>>, _>("last_raw_chat_insert_error_at").ok())
        .flatten()
        .map(|t| t.to_rfc3339());
    let last_error: Option<String> = health.as_ref()
        .and_then(|r| r.try_get::<Option<String>, _>("last_raw_chat_error").ok())
        .flatten()
        .filter(|s| !s.trim().is_empty());

    let suspected_issue = (presence_rows > 0 && raw_rows == 0)
        || (sessions_with_presence > sessions_with_raw && sessions_with_raw > 0);

    // Backfill-Status
    let backfill = sqlx::query(
        r#"SELECT status FROM twitch_raw_chat_backfill_runs
           WHERE LOWER(streamer_login) = $1
           ORDER BY COALESCE(finished_at, started_at) DESC LIMIT 1"#,
    )
    .bind(streamer)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let backfill_state = backfill
        .and_then(|r| r.try_get::<Option<String>, _>("status").ok())
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| if suspected_issue { "not_started".into() } else { "not_needed".into() });

    let note: Option<String> = if suspected_issue && raw_rows == 0 {
        Some("Presence-/Rollup-Daten vorhanden, aber keine Roh-Chat-Nachrichten im gewählten Zeitraum.".into())
    } else if suspected_issue {
        Some("Roh-Chat-Nachrichten sind im gewählten Zeitraum nur teilweise vorhanden; message-basierte KPIs sind unvollständig.".into())
    } else if raw_rows == 0 {
        Some("Keine Roh-Chat-Nachrichten im gewählten Zeitraum.".into())
    } else if last_error.is_some() && last_insert_err.is_some() {
        last_error.as_ref().map(|e| format!("Letzter Roh-Chat-Insert-Fehler: {e}"))
    } else {
        None
    };

    json!({
        "available": raw_rows > 0,
        "lastMessageAt": last_message_at,
        "gapStart": gap_start,
        "suspectedIngestionIssue": suspected_issue,
        "backfillState": backfill_state,
        "note": note,
        "lastInsertOkAt": last_insert_ok,
        "lastInsertErrorAt": last_insert_err,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared: viewer_window_metadata für eine Liste von Logins
// (raw_chat_status.py:291)
// ─────────────────────────────────────────────────────────────────────────────

struct WindowMeta {
    presence_sessions: i64,
    presence_messages: i64,
    raw_messages: i64,
}

async fn viewer_window_metadata(
    pool: &PgPool,
    streamer: &str,
    logins: &[String],
    since: DateTime<Utc>,
) -> std::collections::HashMap<String, WindowMeta> {
    let mut result: std::collections::HashMap<String, WindowMeta> = logins.iter()
        .map(|l| (l.clone(), WindowMeta { presence_sessions: 0, presence_messages: 0, raw_messages: 0 }))
        .collect();

    let pres_rows = sqlx::query(
        r#"SELECT LOWER(sc.chatter_login) AS login,
                  COUNT(DISTINCT sc.session_id) AS window_sessions,
                  COALESCE(SUM(sc.messages), 0) AS window_messages
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(s.streamer_login) = $1 AND s.started_at >= $2
             AND LOWER(sc.chatter_login) = ANY($3)
           GROUP BY LOWER(sc.chatter_login)"#,
    )
    .bind(streamer)
    .bind(since)
    .bind(logins)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for r in &pres_rows {
        let login: String = r.try_get("login").unwrap_or_default();
        if let Some(m) = result.get_mut(&login) {
            m.presence_sessions = r.try_get("window_sessions").unwrap_or(0);
            m.presence_messages = r.try_get("window_messages").unwrap_or(0);
        }
    }

    let raw_rows = sqlx::query(
        r#"SELECT LOWER(m.chatter_login) AS login, COUNT(*) AS raw_messages
           FROM twitch_chat_messages m
           WHERE LOWER(m.streamer_login) = $1 AND m.message_ts >= $2
             AND LOWER(m.chatter_login) = ANY($3)
           GROUP BY LOWER(m.chatter_login)"#,
    )
    .bind(streamer)
    .bind(since)
    .bind(logins)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for r in &raw_rows {
        let login: String = r.try_get("login").unwrap_or_default();
        if let Some(m) = result.get_mut(&login) {
            m.raw_messages = r.try_get("raw_messages").unwrap_or(0);
        }
    }
    result
}

fn window_meta_to_json(meta: Option<&WindowMeta>) -> serde_json::Value {
    let (ps, pm, rm) = match meta {
        Some(m) => (m.presence_sessions, m.presence_messages, m.raw_messages),
        None => (0, 0, 0),
    };
    let presence_only = ps > 0 && rm == 0;
    json!({
        "windowPresenceSessions": ps,
        "windowPresenceMessages": pm,
        "windowRawMessages": rm,
        "hasRawMessages": rm > 0,
        "presenceOnlyInWindow": presence_only,
        "messageGapNote": if presence_only {
            Some("Nur Presence-/Rollup-Daten im gewählten Zeitraum; keine Roh-Chat-Nachrichten vorhanden.")
        } else { None },
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared: _fetch_window_viewer_rows (api_viewers.py:131)
// Gibt alle Viewer eines Streamers im Zeitfenster zurück
// ─────────────────────────────────────────────────────────────────────────────

struct ViewerRow {
    login: String,
    total_sessions: i64,
    total_messages: i64,
    first_seen_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
}

async fn fetch_window_viewer_rows(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<Vec<ViewerRow>, sqlx::Error> {
    // Streamer-Self- + Bot-Exklusion (Python: _collect_viewer_exclusion_logins).
    let excluded = viewer_exclusion_logins(streamer);
    let rows = sqlx::query(
        r#"SELECT LOWER(sc.chatter_login) AS chatter_login,
                  COUNT(DISTINCT sc.session_id) AS total_sessions,
                  COALESCE(SUM(sc.messages), 0) AS total_messages,
                  MIN(s.started_at) AS first_seen_at,
                  MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(sc.streamer_login) = $1
             AND s.started_at >= $2
             AND LOWER(sc.chatter_login) != ALL($3)
           GROUP BY LOWER(sc.chatter_login)"#,
    )
    .bind(streamer)
    .bind(since)
    .bind(&excluded)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(|r| ViewerRow {
        login: r.try_get("chatter_login").unwrap_or_default(),
        total_sessions: r.try_get("total_sessions").unwrap_or(0),
        total_messages: r.try_get("total_messages").unwrap_or(0),
        first_seen_at: r.try_get("first_seen_at").ok(),
        last_seen_at: r.try_get("last_seen_at").ok(),
    }).filter(|v| !v.login.is_empty()).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /twitch/api/v2/viewer-directory
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DirectoryQuery {
    streamer: Option<String>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    order: Option<String>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    per_page: Option<i64>,
    #[serde(default)]
    days: Option<i32>,
}

pub async fn viewer_directory_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DirectoryQuery>,
) -> impl IntoResponse {
    // Python: _require_v2_auth + _require_extended_plan (Paywall-Feature).
    // extended_gate deckt beides ab: None→401, Free-Partner→403, Admin/Localhost→pass.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response(),
    };
    let days = params.days.unwrap_or(30).clamp(1, 365);
    let since: DateTime<Utc> = Utc::now() - chrono::Duration::days(days as i64);

    let sort = match params.sort.as_deref().unwrap_or("sessions") {
        "messages" => "totalMessages",
        "last_seen" => "daysSinceLastSeen",
        "other_channels" => "otherChannels",
        "first_seen" => "firstSeen",
        _ => "totalSessions",
    };
    let order_desc = params.order.as_deref().unwrap_or("desc") == "desc";
    let filter_type = params.filter.as_deref().unwrap_or("all");
    let search = params.search.as_deref().unwrap_or("").trim().to_lowercase();
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).clamp(10, 100);

    // 1. Alle Viewer im Fenster laden
    let viewer_rows = match fetch_window_viewer_rows(&pool, &streamer, since).await {
        Err(e) => {
            tracing::error!("viewer-directory rows Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
        Ok(r) => r,
    };

    let now = Utc::now();

    if viewer_rows.is_empty() {
        let raw_status = build_raw_chat_status(&pool, &streamer, since).await;
        return Json(json!({
            "viewers": [],
            "total": 0,
            "page": page,
            "perPage": per_page,
            "days": days,
            "summary": {
                "totalViewers": 0, "activeViewers": 0, "lurkers": 0,
                "exclusiveViewers": 0, "sharedViewers": 0,
                "avgSessionsPerViewer": 0, "avgOtherChannels": 0,
            },
            "rawChatStatus": raw_status,
        })).into_response();
    }

    // 2. Window-Metadata für alle Logins
    let all_logins: Vec<String> = viewer_rows.iter().map(|r| r.login.clone()).collect();
    let window_meta = viewer_window_metadata(&pool, &streamer, &all_logins, since).await;

    // 3. Cross-Channel-Zählung (andere Kanäle pro Viewer)
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();
    let cc_rows = sqlx::query(
        r#"SELECT LOWER(sc.chatter_login) AS login,
                  COUNT(DISTINCT LOWER(sc.streamer_login)) - 1 AS other_count
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(sc.chatter_login) = ANY($1)
             AND s.started_at >= $2
             AND LOWER(sc.chatter_login) != ALL($3)
           GROUP BY LOWER(sc.chatter_login)"#,
    )
    .bind(&all_logins)
    .bind(since)
    .bind(&bots)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();
    let cross_channel: std::collections::HashMap<String, i64> = cc_rows.iter()
        .filter_map(|r| {
            let login: String = r.try_get("login").ok()?;
            let cnt: i64 = r.try_get("other_count").unwrap_or(0).max(0);
            Some((login, cnt))
        })
        .collect();

    // 4. Top-3 andere Kanäle pro Viewer
    let tc_rows = sqlx::query(
        r#"SELECT LOWER(sc.chatter_login) AS login,
                  LOWER(sc.streamer_login) AS other_streamer,
                  COUNT(DISTINCT sc.session_id) AS sessions
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(sc.chatter_login) = ANY($1)
             AND s.started_at >= $2
             AND LOWER(sc.streamer_login) != $3
             AND LOWER(sc.chatter_login) != ALL($4)
           GROUP BY LOWER(sc.chatter_login), LOWER(sc.streamer_login)
           ORDER BY login, sessions DESC"#,
    )
    .bind(&all_logins)
    .bind(since)
    .bind(&streamer)
    .bind(&bots)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let mut top_channels: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for r in &tc_rows {
        let login: String = r.try_get("login").unwrap_or_default();
        let other: String = r.try_get("other_streamer").unwrap_or_default();
        let entry = top_channels.entry(login).or_default();
        if entry.len() < 3 {
            entry.push(other);
        }
    }

    // 5. Raw-Chat-Status
    let raw_status = build_raw_chat_status(&pool, &streamer, since).await;

    // 6. Viewer aufbauen + summieren
    let mut viewers: Vec<serde_json::Value> = Vec::new();
    let mut total_lurkers = 0i64;
    let mut total_exclusive = 0i64;
    let mut total_shared = 0i64;
    let mut total_active = 0i64;
    let mut sum_sessions = 0i64;
    let mut sum_other = 0i64;

    for v in &viewer_rows {
        let days_since = v.last_seen_at.map(|ls| (now - ls).num_days()).unwrap_or(9999);
        let other_ch = *cross_channel.get(&v.login).unwrap_or(&0);
        let category = classify_viewer(v.total_sessions, v.total_messages, v.first_seen_at, v.last_seen_at, now);
        let is_lurker = v.total_messages == 0;
        let avg_msg = if v.total_sessions > 0 {
            (v.total_messages as f64 / v.total_sessions as f64 * 10.0).round() / 10.0
        } else { 0.0 };

        sum_sessions += v.total_sessions;
        sum_other += other_ch;
        if is_lurker { total_lurkers += 1; }
        if other_ch == 0 { total_exclusive += 1; } else { total_shared += 1; }
        if days_since <= 14 { total_active += 1; }

        let meta = window_meta.get(&v.login);
        let wm = window_meta_to_json(meta);
        viewers.push(json!({
            "login": v.login,
            "totalSessions": v.total_sessions,
            "totalMessages": v.total_messages,
            "firstSeen": v.first_seen_at.map(|t| t.to_rfc3339()),
            "lastSeen": v.last_seen_at.map(|t| t.to_rfc3339()),
            "daysSinceLastSeen": days_since,
            "otherChannels": other_ch,
            "topOtherChannels": top_channels.get(&v.login).cloned().unwrap_or_default(),
            "category": category,
            "avgMessagesPerSession": avg_msg,
            "isLurker": is_lurker,
            "windowPresenceSessions": wm["windowPresenceSessions"],
            "windowPresenceMessages": wm["windowPresenceMessages"],
            "windowRawMessages": wm["windowRawMessages"],
            "hasRawMessages": wm["hasRawMessages"],
            "presenceOnlyInWindow": wm["presenceOnlyInWindow"],
            "messageGapNote": wm["messageGapNote"],
        }));
    }

    let total_viewers = viewer_rows.len() as i64;
    let avg_sessions = if total_viewers > 0 { (sum_sessions as f64 / total_viewers as f64 * 10.0).round() / 10.0 } else { 0.0 };
    let avg_other = if total_viewers > 0 { (sum_other as f64 / total_viewers as f64 * 10.0).round() / 10.0 } else { 0.0 };

    // 7. Filter
    viewers.retain(|v| {
        let days_since = v["daysSinceLastSeen"].as_i64().unwrap_or(9999);
        let is_lurker = v["isLurker"].as_bool().unwrap_or(false);
        let other_ch = v["otherChannels"].as_i64().unwrap_or(0);
        let category = v["category"].as_str().unwrap_or("");
        let login = v["login"].as_str().unwrap_or("");

        let filter_ok = match filter_type {
            "active" => days_since <= 14,
            "lurker" => is_lurker,
            "exclusive" => other_ch == 0,
            "shared" => other_ch > 0,
            "new" => category == "new",
            "churned" => days_since > 30,
            _ => true,
        };
        let search_ok = search.is_empty() || login.contains(search.as_str());
        filter_ok && search_ok
    });

    // 8. Sortieren
    // last_seen: asc=sort by days_since desc (oldest first)
    viewers.sort_by(|a, b| {
        let key_of = |v: &serde_json::Value| -> i64 {
            match sort {
                "totalMessages" => v["totalMessages"].as_i64().unwrap_or(0),
                "daysSinceLastSeen" => v["daysSinceLastSeen"].as_i64().unwrap_or(9999),
                "otherChannels" => v["otherChannels"].as_i64().unwrap_or(0),
                _ => v["totalSessions"].as_i64().unwrap_or(0),
            }
        };
        let ka = key_of(a);
        let kb = key_of(b);
        // last_seen sort: "asc" means oldest first = highest days_since first = desc numeric
        let effective_desc = if sort == "daysSinceLastSeen" { !order_desc } else { order_desc };
        if effective_desc { kb.cmp(&ka) } else { ka.cmp(&kb) }
    });

    let filtered_total = viewers.len() as i64;
    let start = ((page - 1) * per_page) as usize;
    let page_viewers: Vec<_> = viewers.into_iter().skip(start).take(per_page as usize).collect();

    Json(json!({
        "viewers": page_viewers,
        "total": filtered_total,
        "page": page,
        "perPage": per_page,
        "days": days,
        "summary": {
            "totalViewers": total_viewers,
            "activeViewers": total_active,
            "lurkers": total_lurkers,
            "exclusiveViewers": total_exclusive,
            "sharedViewers": total_shared,
            "avgSessionsPerViewer": avg_sessions,
            "avgOtherChannels": avg_other,
        },
        "rawChatStatus": raw_status,
    })).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /twitch/api/v2/viewer-detail
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DetailQuery {
    streamer: Option<String>,
    login: Option<String>,
    #[serde(default)]
    days: Option<i32>,
}

pub async fn viewer_detail_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<DetailQuery>,
) -> impl IntoResponse {
    // Python: _require_v2_auth + _require_extended_plan (Paywall-Feature).
    // extended_gate deckt beides ab: None→401, Free-Partner→403, Admin/Localhost→pass.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"streamer and login required"}))).into_response(),
    };
    let login = match params.login.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(l) => l.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"streamer and login required"}))).into_response(),
    };
    if KNOWN_CHAT_BOTS.contains(&login.as_str()) || login == streamer {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"Viewer not found"}))).into_response();
    }
    let days = params.days.unwrap_or(30).clamp(1, 365);
    let since: DateTime<Utc> = Utc::now() - chrono::Duration::days(days as i64);
    let now = Utc::now();

    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    // Single-viewer aggregat im Fenster
    let viewer_row = sqlx::query(
        r#"SELECT COUNT(DISTINCT sc.session_id) AS total_sessions,
                  COALESCE(SUM(sc.messages), 0) AS total_messages,
                  MIN(s.started_at) AS first_seen_at,
                  MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(sc.streamer_login) = $1 AND LOWER(sc.chatter_login) = $2
             AND s.started_at >= $3
             AND LOWER(sc.chatter_login) != ALL($4)"#,
    )
    .bind(&streamer).bind(&login).bind(since).bind(&bots)
    .fetch_optional(&pool).await;

    let viewer_row = match viewer_row {
        Err(e) => {
            tracing::error!("viewer-detail row Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error":"Viewer not found"}))).into_response(),
        Ok(Some(r)) => r,
    };

    let total_sessions: i64 = viewer_row.try_get("total_sessions").unwrap_or(0);
    if total_sessions == 0 {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"Viewer not found"}))).into_response();
    }
    let total_messages: i64 = viewer_row.try_get("total_messages").unwrap_or(0);
    let first_seen_at: Option<DateTime<Utc>> = viewer_row.try_get("first_seen_at").ok();
    let last_seen_at: Option<DateTime<Utc>> = viewer_row.try_get("last_seen_at").ok();
    let days_since = last_seen_at.map(|ls| (now - ls).num_days()).unwrap_or(9999);
    let category = classify_viewer(total_sessions, total_messages, first_seen_at, last_seen_at, now);

    // Window-Metadata
    let logins = vec![login.clone()];
    let wm_map = viewer_window_metadata(&pool, &streamer, &logins, since).await;
    let wm = window_meta_to_json(wm_map.get(&login));

    // Activity-Timeline (pro Tag)
    let tl_rows = sqlx::query(
        r#"SELECT DATE(s.started_at) AS session_date,
                  COUNT(*) AS sessions,
                  COALESCE(SUM(sc.messages), 0) AS messages
           FROM twitch_stream_sessions s
           JOIN twitch_session_chatters sc ON sc.session_id = s.id
           WHERE LOWER(s.streamer_login) = $1 AND LOWER(sc.chatter_login) = $2
             AND s.started_at >= $3
           GROUP BY DATE(s.started_at)
           ORDER BY session_date"#,
    )
    .bind(&streamer).bind(&login).bind(since)
    .fetch_all(&pool).await.unwrap_or_default();

    let activity_timeline: Vec<serde_json::Value> = tl_rows.iter().map(|r| {
        json!({
            "date": r.try_get::<chrono::NaiveDate, _>("session_date").map(|d| d.to_string()).unwrap_or_default(),
            "sessions": r.try_get::<i64, _>("sessions").unwrap_or(0),
            "messages": r.try_get::<i64, _>("messages").unwrap_or(0),
        })
    }).collect();

    // Cross-Channel: andere Kanäle wo dieser Viewer war
    let cc_rows = sqlx::query(
        r#"SELECT LOWER(s.streamer_login) AS streamer_login,
                  COUNT(DISTINCT sc.session_id) AS sessions,
                  COALESCE(SUM(sc.messages), 0) AS messages,
                  MIN(s.started_at) AS first_seen_at,
                  MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(sc.chatter_login) = $1 AND LOWER(s.streamer_login) != $2
             AND s.started_at >= $3
           GROUP BY LOWER(s.streamer_login)
           ORDER BY sessions DESC LIMIT 15"#,
    )
    .bind(&login).bind(&streamer).bind(since)
    .fetch_all(&pool).await.unwrap_or_default();

    let cross_channel: Vec<serde_json::Value> = cc_rows.iter().map(|r| {
        let cc_first: Option<DateTime<Utc>> = r.try_get("first_seen_at").ok();
        let cc_last: Option<DateTime<Utc>> = r.try_get("last_seen_at").ok();
        let overlap = match (first_seen_at, cc_first) {
            (Some(fs), Some(cf)) => if cf < fs { "before" } else { "after" },
            _ => "unknown",
        };
        json!({
            "streamer": r.try_get::<String, _>("streamer_login").unwrap_or_default(),
            "sessions": r.try_get::<i64, _>("sessions").unwrap_or(0),
            "messages": r.try_get::<i64, _>("messages").unwrap_or(0),
            "firstSeen": cc_first.map(|t| t.to_rfc3339()),
            "lastSeen": cc_last.map(|t| t.to_rfc3339()),
            "overlap": overlap,
        })
    }).collect();

    // Chat-Patterns aus twitch_chat_messages
    let chat_rows = sqlx::query(
        r#"SELECT EXTRACT(HOUR FROM message_ts)::int AS hour,
                  EXTRACT(DOW FROM message_ts)::int AS dow,
                  COUNT(*) AS cnt
           FROM twitch_chat_messages
           WHERE LOWER(chatter_login) = $1 AND LOWER(streamer_login) = $2 AND message_ts >= $3
           GROUP BY EXTRACT(HOUR FROM message_ts)::int, EXTRACT(DOW FROM message_ts)::int"#,
    )
    .bind(&login).bind(&streamer).bind(since)
    .fetch_all(&pool).await.unwrap_or_default();

    let mut hour_counts = [0i64; 24];
    let mut dow_counts = [0i64; 7];
    for r in &chat_rows {
        let h = r.try_get::<i32, _>("hour").unwrap_or(0).clamp(0, 23) as usize;
        let d = r.try_get::<i32, _>("dow").unwrap_or(0).clamp(0, 6) as usize;
        let c: i64 = r.try_get("cnt").unwrap_or(0);
        hour_counts[h] += c;
        dow_counts[d] += c;
    }
    // Python api_viewers.py:541-543: ohne Roh-Chat → peakHours=[] und mostActiveDay="N/A"
    // (nicht künstlich [0,1,2]+Sonntag aus lauter Nullen).
    let mut peak_hours: Vec<i64> = (0..24).filter(|&h| hour_counts[h as usize] > 0).collect();
    peak_hours.sort_by_key(|&h| std::cmp::Reverse(hour_counts[h as usize]));
    let peak_hours: Vec<i64> = peak_hours.into_iter().take(3).collect();

    let dow_names = ["Sonntag","Montag","Dienstag","Mittwoch","Donnerstag","Freitag","Samstag"];
    // Python max() liefert bei Gleichstand den ERSTEN Tag → position() statt max_by_key.
    let max_dow = *dow_counts.iter().max().unwrap_or(&0);
    let most_active_day = if max_dow == 0 {
        "N/A"
    } else {
        dow_names[dow_counts.iter().position(|&c| c == max_dow).unwrap_or(0)]
    };

    // Trend
    let trend = if activity_timeline.len() >= 4 {
        let mid = activity_timeline.len() / 2;
        let first_half: i64 = activity_timeline[..mid].iter()
            .map(|v| v["messages"].as_i64().unwrap_or(0)).sum();
        let second_half: i64 = activity_timeline[mid..].iter()
            .map(|v| v["messages"].as_i64().unwrap_or(0)).sum();
        if second_half > (first_half as f64 * 1.2) as i64 { "increasing" }
        else if first_half > (second_half as f64 * 1.2) as i64 { "decreasing" }
        else { "stable" }
    } else { "insufficient_data" };

    let raw_status = build_raw_chat_status(&pool, &streamer, since).await;
    let avg_msg = if total_sessions > 0 {
        (total_messages as f64 / total_sessions as f64 * 10.0).round() / 10.0
    } else { 0.0 };

    // Personality: bis zu 2000 Chat-Nachrichten des Viewers klassifizieren
    // (Python api_viewers.py:545-573). Ohne Roh-Chat → null.
    let personality: serde_json::Value = {
        let msg_rows = sqlx::query(
            r#"SELECT m.content
               FROM twitch_chat_messages m
               JOIN twitch_stream_sessions s ON s.id = m.session_id
               WHERE LOWER(s.streamer_login) = $1
                 AND LOWER(m.chatter_login) = $2
                 AND m.message_ts >= $3
               LIMIT 2000"#,
        )
        .bind(&streamer)
        .bind(&login)
        .bind(since)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        if msg_rows.is_empty() {
            json!(null)
        } else {
            let mut counts: std::collections::HashMap<&'static str, i64> =
                std::collections::HashMap::new();
            for r in &msg_rows {
                let content: String = r.try_get("content").unwrap_or_default();
                *counts.entry(classify_message(&content)).or_insert(0) += 1;
            }
            let primary = counts
                .iter()
                .max_by_key(|(_, &c)| c)
                .map(|(t, _)| *t)
                .unwrap_or("Other");
            let distribution: serde_json::Map<String, serde_json::Value> =
                counts.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
            json!({ "primary": primary, "distribution": distribution })
        }
    };

    Json(json!({
        "login": login,
        "days": days,
        "overview": {
            "totalSessions": total_sessions,
            "totalMessages": total_messages,
            "firstSeen": first_seen_at.map(|t| t.to_rfc3339()),
            "lastSeen": last_seen_at.map(|t| t.to_rfc3339()),
            "category": category,
            "isLurker": total_messages == 0,
            "daysSinceLastSeen": days_since,
            "windowPresenceSessions": wm["windowPresenceSessions"],
            "windowPresenceMessages": wm["windowPresenceMessages"],
            "windowRawMessages": wm["windowRawMessages"],
            "hasRawMessages": wm["hasRawMessages"],
            "presenceOnlyInWindow": wm["presenceOnlyInWindow"],
            "messageGapNote": wm["messageGapNote"],
        },
        "activityTimeline": activity_timeline,
        "crossChannelPresence": cross_channel,
        "chatPatterns": {
            "peakHours": peak_hours,
            "avgMessagesPerSession": avg_msg,
            "mostActiveDay": most_active_day,
            "messagesTrend": trend,
        },
        "rawChatStatus": raw_status,
        "personality": personality,
    })).into_response()
}

/// Klassifiziert eine Chat-Nachricht in einen Personality-Typ (Port von
/// `_classify_message`, api_v2.py:678). First-Match über Substring-Listen.
fn classify_message(content: &str) -> &'static str {
    if content.is_empty() {
        return "Other";
    }
    if content.starts_with('!') {
        return "Command";
    }
    let lower = content.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if any(&[
        "pog", "poggers", "pogchamp", "hype", "letsgo", "lets go", "lfg", "omg", "wow",
        "krass", "geil", "banger", "insane", "crazy", "gg", "wp", "ggs", "ez", "clutch",
    ]) {
        return "Hype";
    }
    if any(&[
        "hi", "hello", "hey", "moin", "nabend", "guten", "welcome", "hallo", "servus",
        "moinmoin", "ciao", "bye", "tschüss",
    ]) {
        return "Greeting";
    }
    if content.contains('?')
        || any(&[
            "was", "wo", "wer", "wie", "wann", "why", "how", "warum", "weshalb",
            "wie geht", "kann man", "darf man",
        ])
    {
        return "Question";
    }
    if any(&[
        "gut gemacht", "nice play", "stark", "schlecht", "fehler", "bug", "langweilig",
        "spannend", "lustig", "witzig", "gefällt", "liebe",
    ]) {
        return "Feedback";
    }
    if any(&[
        "lag", "fps", "sound", "audio", "mic", "ton", "bild", "standbild", "leise",
        "laut", "verzögerung", "delay",
    ]) {
        return "Technical";
    }
    if any(&[
        "follow", "sub", "prime", "raid", "host", "danke", "thanks", "thx", "discord",
        "social", "insta", "twitter", "yt", "youtube", "clip",
    ]) {
        return "Social";
    }
    if any(&[
        "lol", "lmao", "haha", "lul", "kek", "xd", ":)", ":d", "f", "o7", "rofl",
        "hehe", "huhu",
    ]) {
        return "Reaction";
    }
    if any(&[
        "deadlock", "hero", "build", "skill", "rank", "elo", "match", "play", "game",
        "win", "lose", "mmr", "lane", "ult", "item", "soul", "orb", "patron",
        "mid boss", "guardian", "walker", "urn", "shrine", "abrams", "bebop", "dynamo",
        "grey talon", "haze", "infernus", "ivy", "kelvin", "lady geist", "lash",
        "mcginnis", "mirage", "pocket", "seven", "shiv", "vindicta", "viscous",
        "warden", "wraith", "yamato",
    ]) {
        return "Game-Related";
    }
    "Other"
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /twitch/api/v2/viewer-segments
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SegmentsQuery {
    streamer: Option<String>,
    #[serde(default)]
    days: Option<i32>,
}

pub async fn viewer_segments_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<SegmentsQuery>,
) -> impl IntoResponse {
    // Python: _require_v2_auth + _require_extended_plan (Paywall-Feature).
    // extended_gate deckt beides ab: None→401, Free-Partner→403, Admin/Localhost→pass.
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error":"Streamer required"}))).into_response(),
    };
    let days = params.days.unwrap_or(30).clamp(1, 365);
    let since: DateTime<Utc> = Utc::now() - chrono::Duration::days(days as i64);
    let now = Utc::now();

    let viewer_rows = match fetch_window_viewer_rows(&pool, &streamer, since).await {
        Err(e) => {
            tracing::error!("viewer-segments rows Fehler: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"internal_error"}))).into_response();
        }
        Ok(r) => r,
    };

    if viewer_rows.is_empty() {
        return Json(json!({
            "days": days,
            "segments": {},
            "churnRisk": {"atRisk": 0, "recentlyChurned": 0, "atRiskViewers": []},
            "crossChannelStats": {"exclusiveViewersPct": 0, "avgOtherChannels": 0, "topSharedChannels": []},
        })).into_response();
    }

    // 1. Klassifizieren
    let mut seg_groups: std::collections::HashMap<&str, Vec<serde_json::Value>> = std::collections::HashMap::new();
    let mut at_risk: Vec<serde_json::Value> = vec![];
    let mut recently_churned: Vec<serde_json::Value> = vec![];

    for v in &viewer_rows {
        let days_since = v.last_seen_at.map(|ls| (now - ls).num_days()).unwrap_or(9999);
        let category = classify_viewer(v.total_sessions, v.total_messages, v.first_seen_at, v.last_seen_at, now);
        let entry = json!({"login": v.login, "sessions": v.total_sessions, "messages": v.total_messages});
        seg_groups.entry(category).or_default().push(entry);

        let is_valuable = v.total_sessions >= 3 && v.total_messages > 0;
        if is_valuable && days_since > 14 && days_since <= 45 {
            at_risk.push(json!({
                "login": v.login, "sessions": v.total_sessions,
                "messages": v.total_messages, "daysSinceLastSeen": days_since, "category": category,
            }));
        } else if is_valuable && days_since > 45 {
            recently_churned.push(json!({
                "login": v.login, "sessions": v.total_sessions,
                "messages": v.total_messages, "daysSinceLastSeen": days_since, "category": category,
            }));
        }
    }

    // Sortieren nach sessions*2 + messages
    let score = |v: &serde_json::Value| {
        v["sessions"].as_i64().unwrap_or(0) * 2 + v["messages"].as_i64().unwrap_or(0)
    };
    at_risk.sort_by_key(|v| std::cmp::Reverse(score(v)));
    recently_churned.sort_by_key(|v| std::cmp::Reverse(score(v)));

    // 2. Whereabouts für Top-20 At-Risk
    let at_risk_logins: Vec<String> = at_risk.iter().take(20)
        .filter_map(|v| v["login"].as_str().map(str::to_string))
        .collect();
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();
    let mut whereabouts: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    if !at_risk_logins.is_empty() {
        let thirty_days_ago = now - chrono::Duration::days(30);
        let wa_rows = sqlx::query(
            r#"SELECT LOWER(chatter_login) AS login, LOWER(streamer_login) AS streamer_login, last_seen_at
               FROM twitch_chatter_rollup
               WHERE LOWER(chatter_login) = ANY($1)
                 AND LOWER(streamer_login) != $2
                 AND last_seen_at >= $3
                 AND LOWER(streamer_login) != ALL($4)
               ORDER BY login, last_seen_at DESC"#,
        )
        .bind(&at_risk_logins).bind(&streamer).bind(thirty_days_ago).bind(&bots)
        .fetch_all(&pool).await.unwrap_or_default();

        for r in &wa_rows {
            let login: String = r.try_get("login").unwrap_or_default();
            let other: String = r.try_get("streamer_login").unwrap_or_default();
            let entry = whereabouts.entry(login).or_default();
            if entry.len() < 3 { entry.push(other); }
        }
    }
    let at_risk_with_wa: Vec<serde_json::Value> = at_risk.iter().take(20).map(|v| {
        let login = v["login"].as_str().unwrap_or_default();
        let mut entry = v.clone();
        entry["recentlySeenAt"] = json!(whereabouts.get(login).cloned().unwrap_or_default());
        entry
    }).collect();

    // 3. Segment-Stats
    let total = viewer_rows.len() as i64;
    let mut segment_stats = serde_json::Map::new();
    for seg_name in ["dedicated", "regular", "casual", "lurker", "new"] {
        let list = seg_groups.get(seg_name).cloned().unwrap_or_default();
        let count = list.len() as i64;
        let avg_msgs = if count > 0 {
            (list.iter().map(|v| v["messages"].as_i64().unwrap_or(0)).sum::<i64>() as f64 / count as f64 * 10.0).round() / 10.0
        } else { 0.0 };
        let avg_sess = if count > 0 {
            (list.iter().map(|v| v["sessions"].as_i64().unwrap_or(0)).sum::<i64>() as f64 / count as f64 * 10.0).round() / 10.0
        } else { 0.0 };
        segment_stats.insert(seg_name.into(), json!({
            "count": count,
            "pct": if total > 0 { (count as f64 / total as f64 * 1000.0).round() / 10.0 } else { 0.0 },
            "avgMessages": avg_msgs,
            "avgSessions": avg_sess,
        }));
    }

    // 4. Cross-Channel-Exklusivität
    // P1.36: Der eigene (Home-)Kanal des Streamers darf NICHT in
    // COUNT(DISTINCT streamer_login) zählen — sonst gilt ein Viewer, der nur
    // im Home-Kanal chattet, als nicht-exklusiv. Python schließt den
    // Home-Login in `_collect_viewer_exclusion_logins` aus; hier ergänzen wir
    // ihn nur für die streamer_login-Exklusion (Chatter-Filter bleibt = bots).
    let all_logins: Vec<String> = viewer_rows.iter().map(|r| r.login.clone()).collect();
    let mut streamer_exclusion = bots.clone();
    if !streamer.is_empty() && !streamer_exclusion.contains(&streamer) {
        streamer_exclusion.push(streamer.clone());
    }
    let cc_rows = sqlx::query(
        r#"SELECT LOWER(sc.chatter_login) AS login,
                  COUNT(DISTINCT LOWER(sc.streamer_login)) AS ch_count
           FROM twitch_session_chatters sc
           JOIN twitch_stream_sessions s ON s.id = sc.session_id
           WHERE LOWER(sc.chatter_login) = ANY($1)
             AND s.started_at >= $2
             AND LOWER(sc.chatter_login) != ALL($3)
             AND LOWER(sc.streamer_login) != ALL($4)
           GROUP BY LOWER(sc.chatter_login)"#,
    )
    .bind(&all_logins).bind(since).bind(&bots).bind(&streamer_exclusion)
    .fetch_all(&pool).await.unwrap_or_default();

    let mut exclusive_count = 0i64;
    let mut other_sum = 0i64;
    for r in &cc_rows {
        let ch: i64 = r.try_get("ch_count").unwrap_or(0);
        if ch <= 1 { exclusive_count += 1; }
        other_sum += 0i64.max(ch - 1);
    }
    let exclusive_pct = if total > 0 { (exclusive_count as f64 / total as f64 * 1000.0).round() / 10.0 } else { 0.0 };
    let avg_other = if total > 0 { (other_sum as f64 / total as f64 * 10.0).round() / 10.0 } else { 0.0 };

    // 5. Top Shared Channels
    let shared_rows = sqlx::query(
        r#"SELECT LOWER(sc2.streamer_login) AS streamer_login,
                  COUNT(DISTINCT LOWER(sc2.chatter_login)) AS shared_count
           FROM twitch_session_chatters sc1
           JOIN twitch_stream_sessions s1 ON s1.id = sc1.session_id
           JOIN twitch_session_chatters sc2 ON LOWER(sc1.chatter_login) = LOWER(sc2.chatter_login)
           JOIN twitch_stream_sessions s2 ON s2.id = sc2.session_id
           WHERE LOWER(sc1.streamer_login) = $1
             AND s1.started_at >= $2
             AND LOWER(sc2.streamer_login) != $3
             AND s2.started_at >= $4
             AND LOWER(sc1.chatter_login) != ALL($5)
             AND LOWER(sc2.streamer_login) != ALL($5)
           GROUP BY LOWER(sc2.streamer_login)
           ORDER BY shared_count DESC
           LIMIT 10"#,
    )
    .bind(&streamer).bind(since).bind(&streamer).bind(since).bind(&bots)
    .fetch_all(&pool).await.unwrap_or_default();

    // Direction-Votes für Top-Shared
    let other_streamers: Vec<String> = shared_rows.iter()
        .filter_map(|r| r.try_get::<String, _>("streamer_login").ok())
        .collect();
    let mut direction_map: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    if !other_streamers.is_empty() {
        let dir_rows = sqlx::query(
            r#"SELECT LOWER(other_rollup.streamer_login) AS streamer_login,
                      SUM(CASE WHEN target_rollup.first_seen_at < other_rollup.first_seen_at THEN 1 ELSE 0 END) AS outgoing_votes,
                      SUM(CASE WHEN other_rollup.first_seen_at < target_rollup.first_seen_at THEN 1 ELSE 0 END) AS incoming_votes
               FROM twitch_chatter_rollup target_rollup
               JOIN twitch_chatter_rollup other_rollup
                 ON LOWER(target_rollup.chatter_login) = LOWER(other_rollup.chatter_login)
               WHERE LOWER(target_rollup.streamer_login) = $1
                 AND LOWER(other_rollup.streamer_login) = ANY($2)
                 AND LOWER(target_rollup.chatter_login) != ALL($3)
                 AND LOWER(other_rollup.chatter_login) != ALL($3)
               GROUP BY LOWER(other_rollup.streamer_login)"#,
        )
        .bind(&streamer).bind(&other_streamers).bind(&bots)
        .fetch_all(&pool).await.unwrap_or_default();

        for r in &dir_rows {
            let s: String = r.try_get("streamer_login").unwrap_or_default();
            let out: i64 = r.try_get("outgoing_votes").unwrap_or(0);
            let inc: i64 = r.try_get("incoming_votes").unwrap_or(0);
            let dir = if inc > 0 && out > 0 { "bidirectional" }
                else if inc > 0 { "incoming" }
                else if out > 0 { "outgoing" }
                else { "unknown" };
            direction_map.insert(s, dir);
        }
    }

    let top_shared: Vec<serde_json::Value> = shared_rows.iter().map(|r| {
        let s: String = r.try_get("streamer_login").unwrap_or_default();
        let dir = direction_map.get(&s).copied().unwrap_or("unknown");
        json!({
            "streamer": s,
            "sharedCount": r.try_get::<i64, _>("shared_count").unwrap_or(0),
            "direction": dir,
        })
    }).collect();

    Json(json!({
        "days": days,
        "segments": segment_stats,
        "churnRisk": {
            "atRisk": at_risk.len(),
            "recentlyChurned": recently_churned.len(),
            "atRiskViewers": at_risk_with_wa,
        },
        "crossChannelStats": {
            "exclusiveViewersPct": exclusive_pct,
            "avgOtherChannels": avg_other,
            "topSharedChannels": top_shared,
        },
    })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    // ── Reine Logik: Self- + Bot-Exklusionsliste ────────────────────────────
    #[test]
    fn exclusion_list_enthaelt_streamer_und_bots() {
        let logins = viewer_exclusion_logins("MyStreamer");
        assert!(logins.contains(&"mystreamer".to_string()),
            "Streamer-Self-Login muss in der Exklusionsliste stehen");
        assert!(logins.contains(&"nightbot".to_string()),
            "Known-Bots müssen erhalten bleiben");
    }

    #[test]
    fn exclusion_list_kein_doppelter_streamer() {
        // Streamer-Login der zufällig ein bekannter Bot ist → nicht doppelt.
        let logins = viewer_exclusion_logins("nightbot");
        let count = logins.iter().filter(|l| *l == "nightbot").count();
        assert_eq!(count, 1, "Login darf nicht doppelt erscheinen");
    }

    #[test]
    fn exclusion_list_leerer_streamer_nur_bots() {
        let logins = viewer_exclusion_logins("");
        assert!(!logins.iter().any(|l| l.is_empty()),
            "Leerer Streamer darf keinen Leer-Eintrag erzeugen");
        assert!(logins.contains(&"wizebot".to_string()));
    }

    // ── DB-Regression (env-gated): Bot + Streamer fallen nach Aggregation raus ─
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

    /// Prod-treues Schema: chatter-/streamer-Logins TEXT, messages INTEGER.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new().max_connections(1).connect(dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_stream_sessions (
                   id BIGSERIAL PRIMARY KEY,
                   streamer_login TEXT NOT NULL,
                   started_at TIMESTAMPTZ,
                   ended_at TIMESTAMPTZ
               )"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_session_chatters (
                   session_id BIGINT NOT NULL,
                   chatter_login TEXT NOT NULL,
                   streamer_login TEXT NOT NULL,
                   messages INTEGER DEFAULT 0
               )"#,
        ).execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn fetch_window_excludes_bot_and_self() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "viewers_self_excl").await;

        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) \
             VALUES (1, 'host', NOW() - INTERVAL '1 day', NOW())",
        ).execute(&pool).await.unwrap();
        // echter Viewer + Bot (nightbot) + der Streamer selbst im eigenen Chat
        sqlx::query(
            "INSERT INTO twitch_session_chatters (session_id, chatter_login, streamer_login, messages) \
             VALUES (1, 'realviewer', 'host', 5), \
                    (1, 'nightbot',   'host', 99), \
                    (1, 'host',       'host', 42)",
        ).execute(&pool).await.unwrap();

        let since = Utc::now() - chrono::Duration::days(30);
        let rows = fetch_window_viewer_rows(&pool, "host", since).await.unwrap();

        let logins: Vec<&str> = rows.iter().map(|r| r.login.as_str()).collect();
        assert!(logins.contains(&"realviewer"), "Echter Viewer muss bleiben");
        assert!(!logins.contains(&"nightbot"), "Bot-Login darf nicht auftauchen");
        assert!(!logins.contains(&"host"), "Streamer-Self-Login darf nicht auftauchen");
    }

    // ── Plan-Gate-Verdrahtung (env-gated) ───────────────────────────────────
    // Python ruft in jedem dieser Endpoints _require_v2_auth UND
    // _require_extended_plan. Vor dem Fix prüfte Rust nur None→401 und ließ
    // einen Free-Partner durch (Paywall umgangen). Ein Partner ohne Plan
    // (leere streamer_plans/twitch_billing_subscriptions) muss 403 erhalten.
    /// Schema mit nur den Plan-Tabellen — Partner ohne Eintrag löst raid_free
    /// (= nicht extended) aus, also 403. twitch_user_id leer lassen, damit der
    /// Trial-Grant-Pfad (braucht user_id+login) nicht anspringt.
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

    fn free_partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "freeloader".to_string(),
            twitch_user_id: String::new(),
            display_name: String::new(),
        }
    }

    #[tokio::test]
    async fn viewer_directory_gates_free_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_plan_pool(&dsn, "viewers_gate_dir").await;
        let resp = viewer_directory_handler(
            free_partner(),
            State(pool),
            Query(DirectoryQuery {
                streamer: Some("host".into()),
                sort: None,
                order: None,
                filter: None,
                search: None,
                page: None,
                per_page: None,
                days: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Free-Partner muss 403 erhalten");
    }

    #[tokio::test]
    async fn viewer_detail_gates_free_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_plan_pool(&dsn, "viewers_gate_det").await;
        let resp = viewer_detail_handler(
            free_partner(),
            State(pool),
            Query(DetailQuery {
                streamer: Some("host".into()),
                login: Some("someviewer".into()),
                days: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Free-Partner muss 403 erhalten");
    }

    #[tokio::test]
    async fn viewer_segments_gates_free_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_plan_pool(&dsn, "viewers_gate_seg").await;
        let resp = viewer_segments_handler(
            free_partner(),
            State(pool),
            Query(SegmentsQuery { streamer: Some("host".into()), days: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Free-Partner muss 403 erhalten");
    }

    /// P1.36: Cross-Channel-Exklusivität schließt den Home-Kanal aus. Ein Viewer,
    /// der im Home-Kanal UND in genau EINEM anderen Kanal chattet, muss als
    /// exklusiv (ch_count=1) zählen. Vor dem Fix zählte der Home-Kanal mit
    /// (ch_count=2 → nicht exklusiv, avgOtherChannels=1).
    #[tokio::test]
    async fn segments_cross_channel_exclusivity_excludes_home() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "viewers_xchannel").await;
        // twitch_chatter_rollup wird von der shared/whereabouts-Query gebraucht.
        sqlx::query(
            r#"CREATE TABLE twitch_chatter_rollup (
                   chatter_login TEXT, streamer_login TEXT, last_seen_at TIMESTAMPTZ
               )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Session im Home-Kanal 'host' + eine im Fremdkanal 'otherchan'. Viewer
        // 'loyal' chattet in BEIDEN: aus Sicht von 'host' ist er exklusiv, weil
        // außerhalb des Home-Kanals nur EIN weiterer Kanal zählt.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) \
             VALUES (1, 'host', NOW() - INTERVAL '1 day', NOW()), \
                    (2, 'otherchan', NOW() - INTERVAL '1 day', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_session_chatters (session_id, chatter_login, streamer_login, messages) \
             VALUES (1, 'loyal', 'host', 10), \
                    (2, 'loyal', 'otherchan', 5)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let resp = viewer_segments_handler(
            DashboardAuthLevel::Localhost,
            State(pool),
            Query(SegmentsQuery { streamer: Some("host".into()), days: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let cc = &body["crossChannelStats"];
        assert_eq!(
            cc["exclusiveViewersPct"], 100.0,
            "Home+1-other-Viewer muss 100% exklusiv sein (Home ausgeschlossen), body: {body}"
        );
        assert_eq!(
            cc["avgOtherChannels"], 0.0,
            "avgOtherChannels muss 0 sein (Home-Kanal zählt nicht als 'anderer')"
        );
    }
}
