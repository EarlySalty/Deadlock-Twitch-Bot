//! Chat-Analytics (`/twitch/api/v2/chat-analytics`).
//!
//! Port von `bot/analytics/api_insights.py:_api_v2_chat_analytics` (größte
//! Analytics-Einheit). **Teil 2: der Nachrichten-Klassifikator
//! `_classify_message`** (pure). Snapshot-Loader + Handler-Aggregation folgen.
//!
//! Die Keyword-Listen sind exakt aus der Python-Quelle generiert
//! ([`crate::chat_analytics_lexicon`]).

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::chat_analytics_lexicon::*;
use crate::engagement_metrics::{calculate_engagement, percentile_of, quantile, EngagementInputs};
use crate::raw_chat_status::{build_raw_chat_status, Scope};

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

/// Klassifiziert eine Chat-Nachricht (Python `_classify_message`).
/// Reihenfolge der Prüfungen ist relevant (erste Übereinstimmung gewinnt).
pub fn classify_message(content: &str) -> &'static str {
    if content.is_empty() {
        return "Other";
    }
    let cl = content.to_lowercase();
    if content.starts_with('!') {
        return "Command";
    }
    if HYPE.iter().any(|w| cl.contains(w)) {
        return "Hype";
    }
    if GREETING.iter().any(|w| cl.contains(w)) {
        return "Greeting";
    }
    // "?" wird im Original-Content geprüft (Python `"?" in content`).
    if content.contains('?') || QUESTION.iter().any(|w| cl.contains(w)) {
        return "Question";
    }
    if FEEDBACK.iter().any(|w| cl.contains(w)) {
        return "Feedback";
    }
    if TECHNICAL.iter().any(|w| cl.contains(w)) {
        return "Technical";
    }
    if SOCIAL.iter().any(|w| cl.contains(w)) {
        return "Social";
    }
    if REACTION.iter().any(|w| cl.contains(w)) {
        return "Reaction";
    }
    if GAME.iter().any(|w| cl.contains(w)) {
        return "Game-Related";
    }
    "Other"
}

/// Eine Roh-Chat-Nachricht aus dem Fenster (Python `all_messages`-Zeile).
#[derive(sqlx::FromRow)]
pub struct MessageRow {
    pub message_ts: DateTime<Utc>,
    pub content: Option<String>,
    pub is_command: Option<bool>,
    pub chatter_login: Option<String>,
    pub chatter_id: Option<String>,
}

/// Pro-Chatter-Aggregat inkl. Rollup-Verknüpfung (Python `chatter_rows`-Zeile).
#[derive(sqlx::FromRow)]
pub struct ChatterRow {
    pub chatter_key: Option<String>,
    pub chatter_login: Option<String>,
    pub session_count: i64,
    pub total_messages: Option<i64>,
    pub active_flag: i32,
    pub lurker_flag: i32,
    pub first_time_flag: i32,
    pub has_first_flag: i32,
    pub seen_flag: i32,
    pub seen_before: i32,
}

/// Top-Chatter-Zeile (Python `top_chatters`).
#[derive(sqlx::FromRow)]
pub struct TopChatter {
    pub chatter_key: Option<String>,
    pub messages: i64,
    pub sessions: i64,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

/// Roh-Snapshot aller Queries (Python `_load_chat_analytics_snapshot_sync`).
/// Verarbeitung (Klassifikation, Raten) erfolgt im Handler.
pub struct ChatAnalyticsSnapshot {
    pub session_count: i64,
    pub total_duration_seconds: f64,
    pub avg_viewers: Option<f64>,
    pub viewer_minutes_fallback: f64,
    pub viewer_sample_count: i64,
    pub viewer_minutes_samples: f64,
    /// (session_id, message_count, viewer_minutes)
    pub session_benchmark_rows: Vec<(i64, i64, f64)>,
    pub all_messages: Vec<MessageRow>,
    pub chatter_rows: Vec<ChatterRow>,
    pub sessions_with_chat: i64,
    pub top_chatters: Vec<TopChatter>,
    pub raw_chat_status: Value,
}

fn bots_vec() -> Vec<String> {
    KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect()
}

/// Lädt alle Roh-Daten für chat-analytics (Python `_load_chat_analytics_snapshot_sync`).
/// `$1=since`, `$2=streamer`, `$3=bots` werden je Query mehrfach referenziert.
pub async fn load_chat_analytics_snapshot(
    pool: &PgPool,
    streamer: &str,
    since: DateTime<Utc>,
) -> Result<ChatAnalyticsSnapshot, sqlx::Error> {
    let bots = bots_vec();

    let session_stats = sqlx::query!(
        r#"
        SELECT COUNT(*)::bigint AS "session_count!",
               COALESCE(SUM(s.duration_seconds), 0)::float8 AS "total_duration_seconds!",
               AVG(s.avg_viewers)::float8 AS avg_viewers,
               COALESCE(SUM(COALESCE(s.avg_viewers, 0) * GREATEST(COALESCE(s.duration_seconds, 0), 0) / 60.0), 0)::float8 AS "viewer_minutes_fallback!"
        FROM twitch_stream_sessions s
        WHERE s.started_at >= $1
          AND LOWER(s.streamer_login) = $2
          AND s.ended_at IS NOT NULL
        "#,
        since,
        streamer
    )
    .fetch_one(pool)
    .await?;
    let session_count = session_stats.session_count;
    let total_duration_seconds = session_stats.total_duration_seconds;
    let avg_viewers = session_stats.avg_viewers;
    let viewer_minutes_fallback = session_stats.viewer_minutes_fallback;

    let viewer_samples = sqlx::query!(
        r#"
        SELECT COUNT(*)::bigint AS "viewer_sample_count!",
               COALESCE(SUM(GREATEST(sv.viewer_count, 0)), 0)::float8 AS "viewer_minutes_samples!"
        FROM twitch_session_viewers sv
        JOIN twitch_stream_sessions s ON s.id = sv.session_id
        WHERE s.started_at >= $1
          AND LOWER(s.streamer_login) = $2
          AND s.ended_at IS NOT NULL
        "#,
        since,
        streamer
    )
    .fetch_one(pool)
    .await?;
    let viewer_sample_count = viewer_samples.viewer_sample_count;
    let viewer_minutes_samples = viewer_samples.viewer_minutes_samples;

    let session_benchmark_rows: Vec<(i64, i64, f64)> = sqlx::query!(
        r#"
        WITH session_messages AS (
            SELECT cm.session_id, COUNT(*) AS message_count
            FROM twitch_chat_messages cm
            JOIN twitch_stream_sessions s ON s.id = cm.session_id
            WHERE s.started_at >= $1
              AND LOWER(s.streamer_login) = $2
              AND s.ended_at IS NOT NULL
              AND (cm.chatter_login IS NULL OR cm.chatter_login = '' OR LOWER(cm.chatter_login) <> ALL($3))
            GROUP BY cm.session_id
        ), session_viewer_samples AS (
            SELECT sv.session_id,
                   COUNT(*) AS sample_count,
                   COALESCE(SUM(GREATEST(sv.viewer_count, 0)), 0) AS viewer_minutes
            FROM twitch_session_viewers sv
            JOIN twitch_stream_sessions s ON s.id = sv.session_id
            WHERE s.started_at >= $1
              AND LOWER(s.streamer_login) = $2
              AND s.ended_at IS NOT NULL
            GROUP BY sv.session_id
        )
        SELECT s.id::bigint AS "session_id!",
               COALESCE(sm.message_count, 0)::bigint AS "message_count!",
               (CASE WHEN COALESCE(svs.sample_count, 0) > 0 THEN COALESCE(svs.viewer_minutes, 0)
                     ELSE COALESCE(s.avg_viewers, 0) * GREATEST(COALESCE(s.duration_seconds, 0), 0) / 60.0 END)::float8 AS "viewer_minutes!"
        FROM twitch_stream_sessions s
        LEFT JOIN session_messages sm ON sm.session_id = s.id
        LEFT JOIN session_viewer_samples svs ON svs.session_id = s.id
        WHERE s.started_at >= $1
          AND LOWER(s.streamer_login) = $2
          AND s.ended_at IS NOT NULL
        "#,
        since,
        streamer,
        &bots
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.session_id, row.message_count, row.viewer_minutes))
    .collect();

    let all_messages: Vec<MessageRow> = sqlx::query_as!(
        MessageRow,
        r#"
        SELECT message_ts AS "message_ts!",
               content,
               is_command,
               chatter_login,
               chatter_id
        FROM twitch_chat_messages
        WHERE message_ts >= $1
          AND LOWER(streamer_login) = $2
          AND (chatter_login IS NULL OR chatter_login = '' OR LOWER(chatter_login) <> ALL($3))
        "#,
        since,
        streamer,
        &bots
    )
    .fetch_all(pool)
    .await?;

    let chatter_rows: Vec<ChatterRow> = sqlx::query_as!(
        ChatterRow,
        r#"
        WITH per_user AS (
            SELECT * FROM (
                SELECT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS chatter_key,
                       NULLIF(sc.chatter_login, '') AS chatter_login,
                       COUNT(DISTINCT sc.session_id) AS session_count,
                       SUM(sc.messages) AS total_messages,
                       MAX(CASE WHEN sc.messages > 0 THEN 1 ELSE 0 END) AS active_flag,
                       MAX(CASE WHEN sc.messages = 0 AND sc.seen_via_chatters_api IS TRUE THEN 1 ELSE 0 END) AS lurker_flag,
                       MAX(CASE WHEN sc.is_first_time_streamer IS TRUE THEN 1 ELSE 0 END) AS first_time_flag,
                       MAX(CASE WHEN sc.is_first_time_streamer IS NOT NULL THEN 1 ELSE 0 END) AS has_first_flag,
                       MAX(CASE WHEN sc.seen_via_chatters_api IS TRUE THEN 1 ELSE 0 END) AS seen_flag
                FROM twitch_session_chatters sc
                JOIN twitch_stream_sessions s ON s.id = sc.session_id
                WHERE s.started_at >= $1
                  AND LOWER(s.streamer_login) = $2
                  AND s.ended_at IS NOT NULL
                  AND (sc.chatter_login IS NULL OR sc.chatter_login = '' OR LOWER(sc.chatter_login) <> ALL($3))
                GROUP BY 1, 2
            ) grouped_chatters
            WHERE chatter_key IS NOT NULL
        ), rollup AS (
            SELECT LOWER(streamer_login) AS streamer_login,
                   LOWER(chatter_login) AS chatter_login,
                   first_seen_at
            FROM twitch_chatter_rollup
            WHERE LOWER(streamer_login) = $2
              AND (chatter_login IS NULL OR chatter_login = '' OR LOWER(chatter_login) <> ALL($3))
        )
        SELECT pu.chatter_key,
               pu.chatter_login,
               pu.session_count::bigint AS "session_count!",
               pu.total_messages::bigint AS total_messages,
               pu.active_flag::int AS "active_flag!",
               pu.lurker_flag::int AS "lurker_flag!",
               pu.first_time_flag::int AS "first_time_flag!",
               pu.has_first_flag::int AS "has_first_flag!",
               pu.seen_flag::int AS "seen_flag!",
               (CASE WHEN r.chatter_login IS NOT NULL AND r.first_seen_at < $1 THEN 1 ELSE 0 END)::int AS "seen_before!"
        FROM per_user pu
        LEFT JOIN rollup r ON r.chatter_login = LOWER(pu.chatter_login)
        "#,
        since,
        streamer,
        &bots
    )
    .fetch_all(pool)
    .await?;

    let sessions_with_chat: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(DISTINCT sc.session_id)::bigint AS "count!"
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE s.started_at >= $1
          AND LOWER(s.streamer_login) = $2
          AND s.ended_at IS NOT NULL
          AND (sc.chatter_login IS NULL OR sc.chatter_login = '' OR LOWER(sc.chatter_login) <> ALL($3))
        "#,
        since,
        streamer,
        &bots
    )
    .fetch_one(pool)
    .await?;

    let top_chatters: Vec<TopChatter> = sqlx::query_as!(
        TopChatter,
        r#"
        SELECT COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id) AS chatter_key,
               COUNT(*)::bigint AS "messages!",
               COUNT(DISTINCT cm.session_id)::bigint AS "sessions!",
               MIN(cm.message_ts) AS "first_seen!",
               MAX(cm.message_ts) AS "last_seen!"
        FROM twitch_chat_messages cm
        WHERE cm.message_ts >= $1
          AND LOWER(cm.streamer_login) = $2
          AND COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id) IS NOT NULL
          AND (cm.chatter_login IS NULL OR cm.chatter_login = '' OR LOWER(cm.chatter_login) <> ALL($3))
        GROUP BY COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id)
        ORDER BY COUNT(*) DESC
        LIMIT 20
        "#,
        since,
        streamer,
        &bots
    )
    .fetch_all(pool)
    .await?;

    let raw_chat_status = build_raw_chat_status(pool, streamer, Scope::Since(since)).await?;

    Ok(ChatAnalyticsSnapshot {
        session_count,
        total_duration_seconds,
        avg_viewers,
        viewer_minutes_fallback,
        viewer_sample_count,
        viewer_minutes_samples,
        session_benchmark_rows,
        all_messages,
        chatter_rows,
        sessions_with_chat,
        top_chatters,
        raw_chat_status,
    })
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn emit_iso(dt: DateTime<Utc>) -> String {
    use chrono::SecondsFormat;
    if dt.timestamp_subsec_nanos() == 0 {
        dt.to_rfc3339_opts(SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(SecondsFormat::Micros, false)
    }
}

/// Zielzeitzone auflösen (Python `_resolve_target_timezone`): IANA-Name → (Tz, Name),
/// leer/„UTC"/unbekannt → (UTC, "UTC").
fn resolve_target_timezone(requested: Option<&str>) -> (Tz, String) {
    let tz_name = requested.unwrap_or("UTC").trim();
    if tz_name.is_empty() || tz_name.eq_ignore_ascii_case("UTC") {
        return (Tz::UTC, "UTC".to_string());
    }
    match tz_name.parse::<Tz>() {
        Ok(tz) => (tz, tz_name.to_string()),
        Err(_) => (Tz::UTC, "UTC".to_string()),
    }
}

/// Lädt + aggregiert die chat-analytics (Python `_api_v2_chat_analytics`-Body).
pub async fn load_chat_analytics_payload(
    pool: &PgPool,
    streamer: &str,
    days: i64,
    timezone: Option<&str>,
) -> Result<Value, sqlx::Error> {
    let since = Utc::now() - Duration::days(days);
    let (target_tz, timezone_name) = resolve_target_timezone(timezone);
    let snap = load_chat_analytics_snapshot(pool, streamer, since).await?;

    let session_count = snap.session_count;
    let total_duration_seconds = snap.total_duration_seconds;
    let avg_viewers = snap.avg_viewers.unwrap_or(0.0);
    let viewer_minutes_fallback = snap.viewer_minutes_fallback;

    let viewer_sample_count = snap.viewer_sample_count;
    let viewer_minutes = if viewer_sample_count > 0 {
        snap.viewer_minutes_samples
    } else {
        viewer_minutes_fallback
    };
    let viewer_minutes_has_real_samples = viewer_sample_count > 0;

    // Pro-Session Message-Density (Nachrichten je 100 Viewer-Minuten).
    let mut density: Vec<f64> = Vec::new();
    for (_id, msg_count, vmin) in &snap.session_benchmark_rows {
        if *vmin <= 0.0 {
            continue;
        }
        density.push((*msg_count as f64 / *vmin) * 100.0);
    }

    // Nachrichten-Durchlauf: Klassifikation, Stunden-Histogramm, distinct Chatter.
    let total_messages = snap.all_messages.len() as i64;
    let mut command_messages = 0i64;
    let mut distinct: HashSet<String> = HashSet::new();
    let mut type_counts: HashMap<&'static str, i64> = HashMap::new();
    let mut type_order: Vec<&'static str> = Vec::new();
    let mut hour_counts: HashMap<u32, i64> = HashMap::new();
    for m in &snap.all_messages {
        let content = m.content.as_deref().unwrap_or("");
        if m.is_command.unwrap_or(false) {
            command_messages += 1;
        }
        let key = m
            .chatter_login
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| m.chatter_id.as_deref().filter(|s| !s.is_empty()));
        if let Some(k) = key {
            distinct.insert(k.to_string());
        }
        let mt = classify_message(content);
        if !type_counts.contains_key(mt) {
            type_order.push(mt);
        }
        *type_counts.entry(mt).or_insert(0) += 1;
        let hour = m.message_ts.with_timezone(&target_tz).hour();
        *hour_counts.entry(hour).or_insert(0) += 1;
    }
    let distinct_from_messages = distinct.len() as i64;

    // Chatter-Einträge.
    let mut has_first_flag_data = false;
    for c in &snap.chatter_rows {
        if c.has_first_flag != 0 {
            has_first_flag_data = true;
        }
    }
    let mut tracked_unique_viewers = snap.chatter_rows.len() as i64;
    let sessions_with_chat = snap.sessions_with_chat;

    let mut active_chatters_count = snap
        .chatter_rows
        .iter()
        .filter(|c| c.active_flag != 0)
        .count() as i64;
    let mut lurker_count = snap
        .chatter_rows
        .iter()
        .filter(|c| c.active_flag == 0 && c.lurker_flag != 0)
        .count() as i64;
    let mut chatters_api_seen = snap
        .chatter_rows
        .iter()
        .filter(|c| c.seen_flag != 0)
        .count() as i64;
    let total_messages_per_user: i64 = snap
        .chatter_rows
        .iter()
        .map(|c| c.total_messages.unwrap_or(0))
        .sum();
    let mut avg_messages_per_chatter = if active_chatters_count > 0 {
        round1(total_messages_per_user as f64 / active_chatters_count as f64)
    } else {
        0.0
    };

    let seen_before_count = snap
        .chatter_rows
        .iter()
        .filter(|c| c.seen_before != 0)
        .count() as i64;
    let n_chatters = snap.chatter_rows.len();
    let cold_rollup = n_chatters > 0 && (seen_before_count as f64 / n_chatters as f64) < 0.1;
    let loyal_session_threshold: i64 = if days <= 7 {
        2
    } else if days <= 30 {
        3
    } else if days <= 90 {
        8
    } else {
        12
    };

    let mut first_time_chatters = 0i64;
    let mut returning_viewers = 0i64;
    let mut core_loyal_viewers = 0i64;
    let mut silent_core_loyal_viewers = 0i64;
    for c in &snap.chatter_rows {
        let active = c.active_flag != 0;
        let lurker = c.lurker_flag != 0;
        let seen = c.seen_flag != 0;
        let seen_before = c.seen_before != 0;
        let has_login = c
            .chatter_login
            .as_deref()
            .filter(|s| !s.is_empty())
            .is_some();
        let is_first = if cold_rollup {
            c.session_count < 2
        } else if has_first_flag_data && c.has_first_flag != 0 {
            let mut f = c.first_time_flag != 0;
            if !f && lurker && !seen_before {
                f = true;
            }
            f
        } else if has_login {
            !seen_before
        } else {
            true
        };
        let is_returning = !is_first;
        if active && is_first {
            first_time_chatters += 1;
        }
        if is_returning {
            returning_viewers += 1;
            if c.session_count >= loyal_session_threshold && (active || lurker || seen) {
                core_loyal_viewers += 1;
                if !active {
                    silent_core_loyal_viewers += 1;
                }
            }
        }
    }

    // Override: keine aktiven Chatter, aber Message-Daten vorhanden.
    if active_chatters_count == 0 && distinct_from_messages > 0 {
        active_chatters_count = distinct_from_messages;
        first_time_chatters = distinct_from_messages;
        returning_viewers = 0;
        core_loyal_viewers = 0;
        silent_core_loyal_viewers = 0;
        lurker_count = 0;
        chatters_api_seen = 0;
        tracked_unique_viewers = distinct_from_messages;
        avg_messages_per_chatter = 0.0;
    }

    let unique_chatters = active_chatters_count;
    first_time_chatters = first_time_chatters.min(unique_chatters);
    let returning_chatters = (unique_chatters - first_time_chatters).max(0);
    let total_unique_viewers = if tracked_unique_viewers > 0 {
        tracked_unique_viewers
    } else {
        unique_chatters
    };
    let lurker_ratio = if total_unique_viewers > 0 {
        round3(lurker_count as f64 / total_unique_viewers as f64)
    } else {
        0.0
    };
    let total_minutes = if total_duration_seconds > 0.0 {
        total_duration_seconds / 60.0
    } else {
        0.0
    };
    let messages_per_minute = if total_minutes > 0.0 {
        total_messages as f64 / total_minutes
    } else {
        0.0
    };
    let chatter_return_rate = if unique_chatters > 0 {
        (returning_chatters as f64 / unique_chatters as f64) * 100.0
    } else {
        0.0
    };
    let core_loyal_viewer_rate = if total_unique_viewers > 0 {
        (core_loyal_viewers as f64 / total_unique_viewers as f64) * 100.0
    } else {
        0.0
    };
    let mut density_sorted = density.clone();
    density_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let benchmark_sessions = density_sorted.len() as i64;

    let engagement = calculate_engagement(&EngagementInputs {
        total_messages,
        active_chatters: active_chatters_count as usize,
        tracked_chat_accounts: total_unique_viewers as usize,
        chatters_api_seen: chatters_api_seen as usize,
        viewer_minutes,
        viewer_minutes_has_real_samples,
        avg_viewers,
        session_count,
        sessions_with_chat,
    });

    let (mp100_percentile, mp100_median, mp100_p25, mp100_p75): (Value, Value, Value, Value) =
        match engagement.messages_per_100_viewer_minutes {
            Some(mp100) if benchmark_sessions > 0 => (
                json!(round1(percentile_of(&density_sorted, mp100) * 100.0)),
                json!(round2(quantile(&density_sorted, 0.5))),
                json!(round2(quantile(&density_sorted, 0.25))),
                json!(round2(quantile(&density_sorted, 0.75))),
            ),
            _ => (Value::Null, Value::Null, Value::Null, Value::Null),
        };

    let chat_session_coverage_ratio = engagement.chat_session_coverage;
    let chat_session_coverage_pct = round1(chat_session_coverage_ratio * 100.0);

    let confidence = if engagement.method == "no_data" {
        "very_low"
    } else if chat_session_coverage_ratio >= 0.7 && total_messages >= 500 && session_count >= 10 {
        "high"
    } else if chat_session_coverage_ratio >= 0.4 && total_messages >= 150 && session_count >= 5 {
        "medium"
    } else {
        "low"
    };

    // messageTypes: nach count absteigend, Gleichstand → Erst-Auftreten (Counter.most_common).
    let mut message_types: Vec<(&'static str, i64)> =
        type_order.iter().map(|t| (*t, type_counts[t])).collect();
    message_types.sort_by(|a, b| b.1.cmp(&a.1));
    let message_types_json: Vec<Value> = message_types
        .iter()
        .map(|(t, v)| {
            let pct = if total_messages > 0 {
                json!(round1(*v as f64 / total_messages as f64 * 100.0))
            } else {
                json!(0)
            };
            json!({ "type": t, "count": v, "percentage": pct })
        })
        .collect();

    let top_chatters_json: Vec<Value> = snap
        .top_chatters
        .iter()
        .map(|t| {
            let loyalty =
                round1((t.sessions as f64 / session_count.max(1) as f64 * 100.0).min(100.0));
            json!({
                "login": t.chatter_key,
                "totalMessages": t.messages,
                "totalSessions": t.sessions,
                "firstSeen": emit_iso(t.first_seen),
                "lastSeen": emit_iso(t.last_seen),
                "loyaltyScore": loyalty,
            })
        })
        .collect();

    let hourly_activity: Vec<Value> = (0..24u32)
        .map(|h| json!({ "hour": h, "count": hour_counts.get(&h).copied().unwrap_or(0) }))
        .collect();

    Ok(json!({
        "totalMessages": total_messages,
        "totalChatterSessions": unique_chatters,
        "uniqueChatters": unique_chatters,
        "totalTrackedViewers": total_unique_viewers,
        "firstTimeChatters": first_time_chatters,
        "returningChatters": returning_chatters,
        "returningTrackedViewers": returning_viewers,
        "coreLoyalViewers": core_loyal_viewers,
        "silentCoreLoyalViewers": silent_core_loyal_viewers,
        "coreLoyalViewerRate": round1(core_loyal_viewer_rate),
        "loyaltySessionThreshold": loyal_session_threshold,
        "messagesPerMinute": round2(messages_per_minute),
        "chatterReturnRate": round1(chatter_return_rate),
        "chatPenetrationPct": engagement.chat_penetration_pct,
        "chatPenetrationReliable": engagement.chat_penetration_reliable,
        "messagesPer100ViewerMinutes": engagement.messages_per_100_viewer_minutes,
        "messagesPer100ViewerMinutesPercentile": mp100_percentile,
        "messagesPer100ViewerMinutesMedian": mp100_median,
        "messagesPer100ViewerMinutesP25": mp100_p25,
        "messagesPer100ViewerMinutesP75": mp100_p75,
        "messagesPer100ViewerMinutesBenchmarkSessions": benchmark_sessions,
        "viewerMinutes": engagement.viewer_minutes,
        "legacyInteractionActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
        "interactionRateActivePerViewer": engagement.chat_penetration_pct,
        "interactionRateActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
        "interactionRateReliable": engagement.chat_penetration_reliable,
        "commandMessages": command_messages,
        "nonCommandMessages": (total_messages - command_messages).max(0),
        "lurkerRatio": lurker_ratio,
        "lurkerCount": lurker_count,
        "activeChatters": active_chatters_count,
        "activeRatio": engagement.active_ratio,
        "avgMessagesPerChatter": avg_messages_per_chatter,
        "timezone": timezone_name,
        "topChatters": top_chatters_json,
        "messageTypes": message_types_json,
        "hourlyActivity": hourly_activity,
        "dataQuality": {
            "method": engagement.method,
            "coverage": round3(chat_session_coverage_ratio),
            "sampleCount": total_messages,
            "confidence": confidence,
            "sessions": session_count,
            "sessionsWithChat": sessions_with_chat,
            "chatSessionCoverage": chat_session_coverage_pct,
            "chattersCoverage": engagement.chatters_coverage,
            "chattersApiCoverage": engagement.chatters_coverage,
            "passiveViewerSamples": engagement.passive_viewer_samples,
            "viewerSampleCount": viewer_sample_count,
            "viewerMinutesSource": if viewer_minutes_has_real_samples { "real_samples" } else { "low_coverage" },
            "botFilterApplied": true,
        },
        "rawChatStatus": snap.raw_chat_status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        assert_eq!(classify_message(""), "Other");
        assert_eq!(classify_message("!uptime"), "Command");
        assert_eq!(classify_message("POG das war insane"), "Hype");
        assert_eq!(classify_message("moin"), "Greeting");
        assert_eq!(classify_message("warum lagt das?"), "Question"); // ? → Question
        assert_eq!(classify_message("wie geht es dir"), "Question"); // "wie" ohne ?
        assert_eq!(classify_message("nice play"), "Feedback");
        assert_eq!(classify_message("lag und fps drops"), "Technical");
        assert_eq!(classify_message("danke fuers following"), "Social");
        assert_eq!(classify_message("lol haha"), "Reaction");
        assert_eq!(classify_message("haze build ist gut"), "Game-Related");
        assert_eq!(classify_message("zzz"), "Other");
        // Reihenfolge: Command schlaegt alles.
        assert_eq!(classify_message("!pog"), "Command");
    }

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
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, avg_viewers REAL)",
            "CREATE TABLE twitch_session_viewers (session_id BIGINT, minutes_from_start INTEGER, viewer_count INTEGER)",
            "CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, content TEXT, is_command BOOLEAN, message_ts TIMESTAMPTZ)",
            "CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, messages INTEGER DEFAULT 0, seen_via_chatters_api BOOLEAN DEFAULT FALSE, is_first_time_streamer BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE twitch_chatter_rollup (streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, first_seen_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn snapshot_laedt() {
        let Some(pool) = make_pool("t_ca_snap").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at, duration_seconds, avg_viewers) VALUES (1,'nani',NOW()-INTERVAL '1 day',NOW()-INTERVAL '1 day'+INTERVAL '2 hours',7200,50)")
            .execute(&pool).await.unwrap();
        // aktiver Chatter mit messages>0; rollup first_seen_at < since → seen_before.
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login, messages, seen_via_chatters_api, is_first_time_streamer) VALUES (1,'nani','viewer',5,TRUE,FALSE)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chatter_rollup (streamer_login, chatter_login, first_seen_at, last_seen_at) VALUES ('nani','viewer',NOW()-INTERVAL '60 days',NOW())")
            .execute(&pool).await.unwrap();
        for (c, cmd) in [("hallo", false), ("!uptime", true), ("haze build", false)] {
            sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, is_command, message_ts) VALUES (1,'nani','viewer',$1,$2,NOW()-INTERVAL '12 hours')")
                .bind(c).bind(cmd).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO twitch_session_viewers (session_id, minutes_from_start, viewer_count) VALUES (1,0,40),(1,1,60)").execute(&pool).await.unwrap();

        let since = Utc::now() - chrono::Duration::days(30);
        let snap = load_chat_analytics_snapshot(&pool, "nani", since)
            .await
            .unwrap();
        assert_eq!(snap.session_count, 1);
        assert_eq!(snap.total_duration_seconds, 7200.0);
        assert_eq!(snap.all_messages.len(), 3);
        assert_eq!(snap.sessions_with_chat, 1);
        // Chatter: active_flag=1, seen_before=1 (rollup 60d alt < since), session_count=1.
        assert_eq!(snap.chatter_rows.len(), 1);
        let c = &snap.chatter_rows[0];
        assert_eq!(c.active_flag, 1);
        assert_eq!(c.seen_before, 1);
        assert_eq!(c.total_messages, Some(5));
        assert_eq!(c.chatter_login.as_deref(), Some("viewer"));
        // Top-Chatter: viewer mit 3 Nachrichten.
        assert_eq!(snap.top_chatters.len(), 1);
        assert_eq!(snap.top_chatters[0].messages, 3);
        assert_eq!(snap.raw_chat_status["available"], true);
        // viewer_minutes_samples = 40+60 = 100.
        assert_eq!(snap.viewer_minutes_samples, 100.0);
        assert_eq!(snap.session_benchmark_rows.len(), 1);
    }

    #[tokio::test]
    async fn payload_aggregiert() {
        let Some(pool) = make_pool("t_ca_payload").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at, duration_seconds, avg_viewers) VALUES (1,'nani',NOW()-INTERVAL '1 day',NOW()-INTERVAL '1 day'+INTERVAL '2 hours',7200,50)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_session_chatters (session_id, streamer_login, chatter_login, messages, seen_via_chatters_api) VALUES (1,'nani','viewer',5,TRUE)")
            .execute(&pool).await.unwrap();
        for (c, cmd) in [
            ("hallo zusammen", false),
            ("!uptime", true),
            ("haze ist op", false),
            ("lol haha", false),
        ] {
            sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, is_command, message_ts) VALUES (1,'nani','viewer',$1,$2,'2026-06-14 18:30:00+00')")
                .bind(c).bind(cmd).execute(&pool).await.unwrap();
        }
        let v = load_chat_analytics_payload(&pool, "nani", 30, Some("UTC"))
            .await
            .unwrap();
        assert_eq!(v["totalMessages"], 4);
        assert_eq!(v["commandMessages"], 1);
        assert_eq!(v["nonCommandMessages"], 3);
        assert_eq!(v["timezone"], "UTC");
        assert_eq!(v["loyaltySessionThreshold"], 3); // days=30
                                                     // hourlyActivity: 24 Buckets, Stunde 18 = 4 Nachrichten (alle 18:30 UTC).
        assert_eq!(v["hourlyActivity"].as_array().unwrap().len(), 24);
        assert_eq!(v["hourlyActivity"][18]["count"], 4);
        // messageTypes vorhanden + dataQuality-Struktur.
        assert!(v["messageTypes"].as_array().unwrap().len() >= 1);
        assert_eq!(v["dataQuality"]["botFilterApplied"], true);
        assert_eq!(v["dataQuality"]["sampleCount"], 4);
        assert_eq!(v["topChatters"][0]["totalMessages"], 4);
        assert_eq!(v["rawChatStatus"]["available"], true);
    }

    #[tokio::test]
    async fn tz_histogramm_verschiebt_stunde() {
        let Some(pool) = make_pool("t_ca_tz").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at, duration_seconds, avg_viewers) VALUES (1,'nani',NOW()-INTERVAL '1 day',NOW(),3600,10)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, is_command, message_ts) VALUES (1,'nani','v','hi',FALSE,'2026-06-14 23:30:00+00')")
            .execute(&pool).await.unwrap();
        // 23:30 UTC → Europe/Berlin (Sommerzeit +2) = 01:30 → Stunde 1.
        let v = load_chat_analytics_payload(&pool, "nani", 30, Some("Europe/Berlin"))
            .await
            .unwrap();
        assert_eq!(v["timezone"], "Europe/Berlin");
        assert_eq!(v["hourlyActivity"][1]["count"], 1);
        assert_eq!(v["hourlyActivity"][23]["count"], 0);
    }
}
