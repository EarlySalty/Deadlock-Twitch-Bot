//! Chat-Velocity + Viewer-Overlay je Session (`/twitch/api/v2/chat-hype-timeline`).
//!
//! Port von `bot/analytics/api_chat_deep.py:_load_chat_hype_timeline_payload`.
//! Pro Minute: Nachrichten/Chatter (Bot-gefiltert) + Viewer-Overlay, Spike-
//! Erkennung, Pearson-Korrelation Chat↔Viewer inkl. Lag-Detection, letzte
//! Sessions + geteilter [`crate::raw_chat_status`]-Block.
//!
//! **Timescale-Äquivalent:** Pythons `time_bucket('1 minute', ts)` wird durch das
//! identische `date_trunc('minute', ts)` ersetzt (gleiche 1-Min-Buckets, aber ohne
//! TimescaleDB-Abhängigkeit → in plain Postgres testbar).

use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::raw_chat_status::{build_raw_chat_status, Scope};

use crate::bekannte_bots::KNOWN_CHAT_BOTS;

/// Loader-Ergebnis inkl. HTTP-Status (Python gibt `(status, payload, _)` zurück).
pub enum HypeTimeline {
    Ok(Value),
    BadRequest(Value),
    NotFound(Value),
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn emit_iso(dt: DateTime<Utc>) -> String {
    if dt.timestamp_subsec_nanos() == 0 {
        dt.to_rfc3339_opts(SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(SecondsFormat::Micros, false)
    }
}

/// Pearson-Korrelationskoeffizient (Python `_pearson_r`).
fn pearson_r(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len();
    if n < 3 || ys.len() != n {
        return 0.0;
    }
    let nf = n as f64;
    let mx = xs.iter().sum::<f64>() / nf;
    let my = ys.iter().sum::<f64>() / nf;
    let num: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let dx = xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>().sqrt();
    let dy = ys.iter().map(|y| (y - my).powi(2)).sum::<f64>().sqrt();
    if dx == 0.0 || dy == 0.0 {
        return 0.0;
    }
    num / (dx * dy)
}

/// Klartext-Interpretation von `r` (Python `_interpret_r`).
fn interpret_r(r: f64) -> &'static str {
    let ar = r.abs();
    if ar >= 0.7 {
        if r > 0.0 {
            "strong_positive"
        } else {
            "strong_negative"
        }
    } else if ar >= 0.4 {
        if r > 0.0 {
            "moderate_positive"
        } else {
            "moderate_negative"
        }
    } else if ar >= 0.2 {
        if r > 0.0 {
            "weak_positive"
        } else {
            "weak_negative"
        }
    } else {
        "none"
    }
}

struct Point {
    minute: i64,
    messages: i64,
    chatters: i64,
    viewers: i64,
    is_spike: bool,
}

/// Lädt die Hype-Timeline einer Session (Python `_load_chat_hype_timeline_payload`).
pub async fn load_chat_hype_timeline(
    pool: &PgPool,
    streamer: &str,
    session_id_raw: &str,
) -> Result<HypeTimeline, sqlx::Error> {
    let bots: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();

    // Session bestimmen: expliziter Parameter ODER letzte Session.
    let session_id: i64 = if !session_id_raw.is_empty() {
        match session_id_raw.parse::<i64>() {
            Ok(id) => id,
            Err(_) => {
                return Ok(HypeTimeline::BadRequest(
                    json!({ "error": "Invalid session_id" }),
                ))
            }
        }
    } else {
        match sqlx::query_scalar!(
            r#"
            SELECT id AS "id!"
            FROM twitch_stream_sessions
            WHERE LOWER(streamer_login) = $1
            ORDER BY started_at DESC
            LIMIT 1
            "#,
            streamer
        )
        .fetch_optional(pool)
        .await?
        {
            Some(id) => id,
            None => {
                return Ok(HypeTimeline::NotFound(
                    json!({ "error": "No sessions found" }),
                ))
            }
        }
    };

    let sess = sqlx::query!(
        r#"
        SELECT started_at AS "started_at!",
               duration_seconds,
               stream_title
        FROM twitch_stream_sessions
        WHERE id = $1
        "#,
        session_id
    )
    .fetch_optional(pool)
    .await?;
    let (session_start, duration, title) = match sess {
        Some(row) => (
            row.started_at,
            i64::from(row.duration_seconds.unwrap_or(0)),
            row.stream_title.unwrap_or_default(),
        ),
        None => {
            return Ok(HypeTimeline::NotFound(
                json!({ "error": "Session not found" }),
            ))
        }
    };

    // Nachrichten/Chatter je Minute (date_trunc = time_bucket('1 minute')).
    let mpm_rows: Vec<(DateTime<Utc>, i64, i64)> = sqlx::query!(
        r#"
        SELECT date_trunc('minute', m.message_ts)::timestamptz AS "bucket!",
               COUNT(*)::bigint AS "messages!",
               COUNT(DISTINCT m.chatter_login)::bigint AS "chatters!"
        FROM twitch_chat_messages m
        WHERE m.session_id = $1
          AND m.chatter_login IS NOT NULL
          AND (m.chatter_login IS NULL OR m.chatter_login = '' OR (LOWER(m.chatter_login) <> ALL($2) AND LOWER(m.chatter_login) !~ '^justinfan[0-9]+$'))
        GROUP BY 1
        ORDER BY 1
        "#,
        session_id,
        &bots
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.bucket, row.messages, row.chatters))
    .collect();

    // Viewer-Overlay.
    let viewer_rows: Vec<(Option<i32>, i32)> = sqlx::query!(
        r#"
        SELECT minutes_from_start, viewer_count AS "viewer_count!"
        FROM twitch_session_viewers
        WHERE session_id = $1
        ORDER BY minutes_from_start
        "#,
        session_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.minutes_from_start, row.viewer_count))
    .collect();
    let mut viewer_map: HashMap<i64, i64> = HashMap::new();
    for (minute_raw, viewers_raw) in viewer_rows {
        let Some(minute) = minute_raw else { continue };
        let minute = i64::from(minute);
        if minute < 0 {
            continue;
        }
        viewer_map.insert(minute, i64::from(viewers_raw.max(0)));
    }

    let mut timeline: Vec<Point> = Vec::new();
    let mut msg_counts: Vec<i64> = Vec::new();
    for (bucket, msgs, chatters) in &mpm_rows {
        let minute = ((*bucket - session_start).num_milliseconds() as f64 / 60_000.0) as i64;
        let mut viewers = viewer_map.get(&minute).copied().unwrap_or(0);
        if viewers == 0 {
            for offset in -2..=2 {
                if let Some(&nearby) = viewer_map.get(&(minute + offset)) {
                    if nearby > 0 {
                        viewers = nearby;
                        break;
                    }
                }
            }
        }
        timeline.push(Point {
            minute,
            messages: *msgs,
            chatters: *chatters,
            viewers,
            is_spike: false,
        });
        msg_counts.push(*msgs);
    }

    let avg_mpm = if msg_counts.is_empty() {
        0.0
    } else {
        msg_counts.iter().sum::<i64>() as f64 / msg_counts.len() as f64
    };
    let peak_mpm = msg_counts.iter().copied().max().unwrap_or(0);

    // Spikes: messages >= max(avg*2, 3).
    let threshold = (avg_mpm * 2.0).max(3.0);
    let mut spikes: Vec<Value> = Vec::new();
    for point in &mut timeline {
        if point.messages as f64 >= threshold {
            point.is_spike = true;
            let multiplier = if avg_mpm > 0.0 {
                round1(point.messages as f64 / avg_mpm)
            } else {
                0.0
            };
            spikes.push(json!({ "minute": point.minute, "messages": point.messages, "multiplier": multiplier }));
        }
    }
    spikes.sort_by(|a, b| {
        b["messages"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["messages"].as_i64().unwrap_or(0))
    });
    spikes.truncate(20);

    // Pearson-Korrelation (nur Minuten mit Viewern).
    let chat_vals: Vec<f64> = timeline
        .iter()
        .filter(|p| p.viewers > 0)
        .map(|p| p.messages as f64)
        .collect();
    let viewer_vals: Vec<f64> = timeline
        .iter()
        .filter(|p| p.viewers > 0)
        .map(|p| p.viewers as f64)
        .collect();
    let r_val = pearson_r(&chat_vals, &viewer_vals);

    // Lag-Detection: führt der Chat den Viewern voraus?
    let mut chat_leads = false;
    let mut lag_minutes: i64 = 0;
    if timeline.len() >= 10 {
        let mut best_lag_r = r_val.abs();
        let n = chat_vals.len();
        for lag in 1..=10usize {
            let (lagged_chat, lagged_view): (&[f64], &[f64]) = if lag < n {
                (&chat_vals[..n - lag], &viewer_vals[lag..])
            } else {
                (&[], &[])
            };
            if lagged_chat.len() >= 5 {
                let lagged_r = pearson_r(lagged_chat, lagged_view).abs();
                if lagged_r > best_lag_r + 0.05 {
                    best_lag_r = lagged_r;
                    lag_minutes = lag as i64;
                    chat_leads = true;
                }
            }
        }
    }

    // Letzte 10 anderen Sessions + deren Ø-MPM.
    let recent_rows: Vec<(i64, chrono::NaiveDate, Option<String>)> = sqlx::query!(
        r#"
        SELECT s.id AS "id!",
               s.started_at::date AS "started_date!",
               s.stream_title
        FROM twitch_stream_sessions s
        WHERE LOWER(s.streamer_login) = $1
          AND s.id != $2
        ORDER BY s.started_at DESC
        LIMIT 10
        "#,
        streamer,
        session_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.id, row.started_date, row.stream_title))
    .collect();
    let mut recent_sessions: Vec<Value> = Vec::new();
    for (rid, rdate, rtitle) in recent_rows {
        let avg: Option<f64> = sqlx::query_scalar!(
            r#"
            SELECT (COUNT(*) * 1.0 / GREATEST(1, EXTRACT(EPOCH FROM MAX(m.message_ts) - MIN(m.message_ts)) / 60))::float8 AS avg_mpm
            FROM twitch_chat_messages m
            WHERE m.session_id = $1
              AND (m.chatter_login IS NULL OR m.chatter_login = '' OR (LOWER(m.chatter_login) <> ALL($2) AND LOWER(m.chatter_login) !~ '^justinfan[0-9]+$'))
            "#,
            rid,
            &bots
        )
        .fetch_one(pool)
        .await?;
        recent_sessions.push(json!({
            "id": rid,
            "date": rdate.to_string(),
            "title": rtitle.unwrap_or_default(),
            "avgMPM": round1(avg.unwrap_or(0.0)),
            "peakMPM": 0,
        }));
    }

    let raw_chat_status =
        build_raw_chat_status(pool, streamer, Scope::Sessions(&[session_id])).await?;

    let timeline_json: Vec<Value> = timeline
        .iter()
        .map(|p| json!({ "minute": p.minute, "messages": p.messages, "chatters": p.chatters, "viewers": p.viewers, "isSpike": p.is_spike }))
        .collect();

    Ok(HypeTimeline::Ok(json!({
        "sessionId": session_id,
        "sessionTitle": title,
        "startedAt": emit_iso(session_start),
        "duration": duration,
        "avgMPM": round1(avg_mpm),
        "peakMPM": peak_mpm,
        "timeline": timeline_json,
        "spikes": spikes,
        "correlation": {
            "chatViewerR": round2(r_val),
            "interpretation": interpret_r(r_val),
            "chatLeadsViewers": chat_leads,
            "lagMinutes": lag_minutes,
        },
        "recentSessions": recent_sessions,
        "rawChatStatus": raw_chat_status,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn pearson_und_interpret() {
        // Perfekt positiv korreliert → r = 1.0 → strong_positive.
        let r = pearson_r(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]);
        assert!((r - 1.0).abs() < 1e-9);
        assert_eq!(interpret_r(r), "strong_positive");
        assert_eq!(pearson_r(&[1.0, 2.0], &[1.0, 2.0]), 0.0); // n<3
        assert_eq!(interpret_r(0.0), "none");
        assert_eq!(interpret_r(-0.8), "strong_negative");
    }

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
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, stream_title TEXT)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, messages INTEGER DEFAULT 0)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, content TEXT, message_ts TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_viewers (session_id BIGINT, minutes_from_start INTEGER, viewer_count INTEGER)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT)").execute(&pool).await.unwrap();
        Some(pool)
    }

    fn unwrap_ok(r: HypeTimeline) -> Value {
        match r {
            HypeTimeline::Ok(v) => v,
            _ => panic!("erwartete Ok"),
        }
    }

    #[tokio::test]
    async fn keine_session_404() {
        let Some(pool) = make_pool("t_hype_404").await else {
            return;
        };
        let r = load_chat_hype_timeline(&pool, "nani", "").await.unwrap();
        assert!(matches!(r, HypeTimeline::NotFound(_)));
    }

    #[tokio::test]
    async fn invalid_session_id_400() {
        let Some(pool) = make_pool("t_hype_400").await else {
            return;
        };
        let r = load_chat_hype_timeline(&pool, "nani", "abc").await.unwrap();
        assert!(matches!(r, HypeTimeline::BadRequest(_)));
    }

    #[tokio::test]
    async fn timeline_spikes_korrelation() {
        let Some(pool) = make_pool("t_hype_ok").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, duration_seconds, stream_title) VALUES (1,'nani','2026-06-14 12:00:00+00',3600,'Test')").execute(&pool).await.unwrap();
        // Minute 0: 2 msgs, Minute 1: 2 msgs, Minute 2: 10 msgs (Spike).
        for ts in ["12:00:10", "12:00:20"] {
            sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (1,'nani','a','hi',$1::timestamptz)")
                .bind(format!("2026-06-14 {ts}+00")).execute(&pool).await.unwrap();
        }
        for ts in ["12:01:10", "12:01:20"] {
            sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) VALUES (1,'nani','b','hi',$1::timestamptz)")
                .bind(format!("2026-06-14 {ts}+00")).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, content, message_ts) SELECT 1,'nani','c'||(g%4),'hi', TIMESTAMPTZ '2026-06-14 12:02:00+00' + (g||' seconds')::interval FROM generate_series(0,9) g")
            .execute(&pool).await.unwrap();
        // Viewer steigen mit den Nachrichten → positive Korrelation.
        sqlx::query("INSERT INTO twitch_session_viewers (session_id, minutes_from_start, viewer_count) VALUES (1,0,50),(1,1,55),(1,2,100)").execute(&pool).await.unwrap();

        let v = unwrap_ok(load_chat_hype_timeline(&pool, "nani", "").await.unwrap());
        assert_eq!(v["sessionId"], 1);
        assert_eq!(v["sessionTitle"], "Test");
        assert_eq!(v["timeline"].as_array().unwrap().len(), 3);
        assert_eq!(v["peakMPM"], 10);
        assert_eq!(v["avgMPM"], 4.7); // (2+2+10)/3 = 4.6667 → 4.7
                                      // Spike bei Minute 2 (10 >= max(9.33,3)).
        assert_eq!(v["spikes"].as_array().unwrap().len(), 1);
        assert_eq!(v["spikes"][0]["minute"], 2);
        assert_eq!(v["spikes"][0]["messages"], 10);
        assert_eq!(v["spikes"][0]["multiplier"], 2.1); // 10/4.6667
        assert_eq!(v["timeline"][2]["isSpike"], true);
        assert_eq!(v["timeline"][0]["viewers"], 50);
        // Korrelation positiv.
        assert_eq!(v["correlation"]["interpretation"], "strong_positive");
        assert_eq!(v["rawChatStatus"]["available"], true);
    }
}
