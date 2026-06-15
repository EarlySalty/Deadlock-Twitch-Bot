//! Chat-Analytics (`/twitch/api/v2/chat-analytics`).
//!
//! Port von `bot/analytics/api_insights.py:_api_v2_chat_analytics` (größte
//! Analytics-Einheit). **Teil 2: der Nachrichten-Klassifikator
//! `_classify_message`** (pure). Snapshot-Loader + Handler-Aggregation folgen.
//!
//! Die Keyword-Listen sind exakt aus der Python-Quelle generiert
//! ([`crate::chat_analytics_lexicon`]).

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

use crate::chat_analytics_lexicon::*;
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

    let (session_count, total_duration_seconds, avg_viewers, viewer_minutes_fallback): (i64, f64, Option<f64>, f64) =
        sqlx::query_as(
            "SELECT COUNT(*)::bigint, \
                    COALESCE(SUM(s.duration_seconds), 0)::float8, \
                    AVG(s.avg_viewers)::float8, \
                    COALESCE(SUM(COALESCE(s.avg_viewers, 0) * GREATEST(COALESCE(s.duration_seconds, 0), 0) / 60.0), 0)::float8 \
               FROM twitch_stream_sessions s \
              WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL",
        )
        .bind(since)
        .bind(streamer)
        .fetch_one(pool)
        .await?;

    let (viewer_sample_count, viewer_minutes_samples): (i64, f64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COALESCE(SUM(GREATEST(sv.viewer_count, 0)), 0)::float8 \
           FROM twitch_session_viewers sv \
           JOIN twitch_stream_sessions s ON s.id = sv.session_id \
          WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL",
    )
    .bind(since)
    .bind(streamer)
    .fetch_one(pool)
    .await?;

    let session_benchmark_rows: Vec<(i64, i64, f64)> = sqlx::query_as(
        "WITH session_messages AS ( \
             SELECT cm.session_id, COUNT(*) AS message_count \
               FROM twitch_chat_messages cm \
               JOIN twitch_stream_sessions s ON s.id = cm.session_id \
              WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL \
                AND (cm.chatter_login IS NULL OR cm.chatter_login = '' OR LOWER(cm.chatter_login) <> ALL($3)) \
              GROUP BY cm.session_id \
         ), session_viewer_samples AS ( \
             SELECT sv.session_id, COUNT(*) AS sample_count, COALESCE(SUM(GREATEST(sv.viewer_count, 0)), 0) AS viewer_minutes \
               FROM twitch_session_viewers sv \
               JOIN twitch_stream_sessions s ON s.id = sv.session_id \
              WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL \
              GROUP BY sv.session_id \
         ) \
         SELECT s.id::bigint, COALESCE(sm.message_count, 0)::bigint, \
                (CASE WHEN COALESCE(svs.sample_count, 0) > 0 THEN COALESCE(svs.viewer_minutes, 0) \
                      ELSE COALESCE(s.avg_viewers, 0) * GREATEST(COALESCE(s.duration_seconds, 0), 0) / 60.0 END)::float8 \
           FROM twitch_stream_sessions s \
           LEFT JOIN session_messages sm ON sm.session_id = s.id \
           LEFT JOIN session_viewer_samples svs ON svs.session_id = s.id \
          WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL",
    )
    .bind(since)
    .bind(streamer)
    .bind(&bots)
    .fetch_all(pool)
    .await?;

    let all_messages: Vec<MessageRow> = sqlx::query_as(
        "SELECT message_ts, content, is_command, chatter_login, chatter_id \
           FROM twitch_chat_messages \
          WHERE message_ts >= $1 AND LOWER(streamer_login) = $2 \
            AND (chatter_login IS NULL OR chatter_login = '' OR LOWER(chatter_login) <> ALL($3))",
    )
    .bind(since)
    .bind(streamer)
    .bind(&bots)
    .fetch_all(pool)
    .await?;

    let chatter_rows: Vec<ChatterRow> = sqlx::query_as(
        "WITH per_user AS ( \
             SELECT * FROM ( \
                 SELECT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS chatter_key, \
                        NULLIF(sc.chatter_login, '') AS chatter_login, \
                        COUNT(DISTINCT sc.session_id) AS session_count, \
                        SUM(sc.messages) AS total_messages, \
                        MAX(CASE WHEN sc.messages > 0 THEN 1 ELSE 0 END) AS active_flag, \
                        MAX(CASE WHEN sc.messages = 0 AND LOWER(COALESCE(CAST(sc.seen_via_chatters_api AS TEXT), '0')) IN ('1','t','true') THEN 1 ELSE 0 END) AS lurker_flag, \
                        MAX(CASE WHEN LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0')) IN ('1','t','true') THEN 1 ELSE 0 END) AS first_time_flag, \
                        MAX(CASE WHEN sc.is_first_time_streamer IS NOT NULL THEN 1 ELSE 0 END) AS has_first_flag, \
                        MAX(CASE WHEN LOWER(COALESCE(CAST(sc.seen_via_chatters_api AS TEXT), '0')) IN ('1','t','true') THEN 1 ELSE 0 END) AS seen_flag \
                   FROM twitch_session_chatters sc \
                   JOIN twitch_stream_sessions s ON s.id = sc.session_id \
                  WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL \
                    AND (sc.chatter_login IS NULL OR sc.chatter_login = '' OR LOWER(sc.chatter_login) <> ALL($3)) \
                  GROUP BY 1, 2 \
             ) grouped_chatters WHERE chatter_key IS NOT NULL \
         ), rollup AS ( \
             SELECT LOWER(streamer_login) AS streamer_login, LOWER(chatter_login) AS chatter_login, first_seen_at \
               FROM twitch_chatter_rollup \
              WHERE LOWER(streamer_login) = $2 \
                AND (chatter_login IS NULL OR chatter_login = '' OR LOWER(chatter_login) <> ALL($3)) \
         ) \
         SELECT pu.chatter_key, pu.chatter_login, pu.session_count::bigint, pu.total_messages::bigint, \
                pu.active_flag::int, pu.lurker_flag::int, pu.first_time_flag::int, pu.has_first_flag::int, pu.seen_flag::int, \
                (CASE WHEN r.chatter_login IS NOT NULL AND r.first_seen_at < $1 THEN 1 ELSE 0 END)::int AS seen_before \
           FROM per_user pu \
           LEFT JOIN rollup r ON r.chatter_login = LOWER(pu.chatter_login)",
    )
    .bind(since)
    .bind(streamer)
    .bind(&bots)
    .fetch_all(pool)
    .await?;

    let sessions_with_chat: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT sc.session_id)::bigint \
           FROM twitch_session_chatters sc \
           JOIN twitch_stream_sessions s ON s.id = sc.session_id \
          WHERE s.started_at >= $1 AND LOWER(s.streamer_login) = $2 AND s.ended_at IS NOT NULL \
            AND (sc.chatter_login IS NULL OR sc.chatter_login = '' OR LOWER(sc.chatter_login) <> ALL($3))",
    )
    .bind(since)
    .bind(streamer)
    .bind(&bots)
    .fetch_one(pool)
    .await?;

    let top_chatters: Vec<TopChatter> = sqlx::query_as(
        "SELECT COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id) AS chatter_key, \
                COUNT(*)::bigint AS messages, COUNT(DISTINCT cm.session_id)::bigint AS sessions, \
                MIN(cm.message_ts) AS first_seen, MAX(cm.message_ts) AS last_seen \
           FROM twitch_chat_messages cm \
          WHERE cm.message_ts >= $1 AND LOWER(cm.streamer_login) = $2 \
            AND COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id) IS NOT NULL \
            AND (cm.chatter_login IS NULL OR cm.chatter_login = '' OR LOWER(cm.chatter_login) <> ALL($3)) \
          GROUP BY COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id) \
          ORDER BY messages DESC LIMIT 20",
    )
    .bind(since)
    .bind(streamer)
    .bind(&bots)
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
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
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
        let Some(pool) = make_pool("t_ca_snap").await else { return };
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
        let snap = load_chat_analytics_snapshot(&pool, "nani", since).await.unwrap();
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
}
