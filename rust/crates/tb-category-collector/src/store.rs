//! PostgreSQL persistence. Raw inserts and the durable rollup queue commit
//! together. Retention removes only finalized whole-hour buckets (at most
//! 90 days old); it never deletes snapshots or hourly aggregates.
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tb_transport_twitch::streams::HelixStream;

#[derive(Debug, Clone, Serialize)]
pub struct ChatZeile {
    pub room_user_id: String,
    pub message_id: String,
    pub source_message_id: Option<String>,
    pub source_room_id: Option<String>,
    pub sent_at: DateTime<Utc>,
    pub chatter_user_id: String,
    pub chatter_login: String,
    pub message_text: String,
    pub text_len: i32,
    pub detected_lang: String,
    pub lang_confidence: Option<f32>,
    pub lang_method: String,
    pub emote_count: i32,
    pub irc_tags: Value,
}

pub async fn start_poll(pool: &PgPool, at: DateTime<Utc>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO category_polls(started_at) VALUES($1) RETURNING poll_id")
        .bind(at)
        .fetch_one(pool)
        .await
}
pub async fn fail_poll(
    pool: &PgPool,
    id: i64,
    pages: usize,
    incomplete: bool,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE category_polls SET status=$2, completed_at=now(), page_count=$3, error_detail=$4 WHERE poll_id=$1")
        .bind(id).bind(if incomplete { "incomplete" } else { "failed" })
        .bind(pages as i32).bind(detail).execute(pool).await?;
    error(pool, detail).await
}
pub async fn error(pool: &PgPool, detail: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE category_collector_status SET last_error=$1,last_error_at=now(),updated_at=now() WHERE id=1")
        .bind(detail).execute(pool).await?;
    Ok(())
}

/// All snapshots, channel names and the successful poll marker are atomic.
/// A failed commit can never advertise a complete measurement to the UI.
pub async fn complete_poll(
    pool: &PgPool,
    id: i64,
    at: DateTime<Utc>,
    streams: &[HelixStream],
    pages: usize,
) -> Result<(), sqlx::Error> {
    let rows: Vec<Value> = streams
        .iter()
        .map(|s| {
            json!({
                "stream_id":s.id, "user_id":s.user_id, "user_login":s.user_login,
                "display_name":s.user_name, "viewer_count":s.viewer_count, "title":s.title,
                "language":s.language, "started_at":s.started_at, "tags":s.tags,
                "thumbnail_url":s.thumbnail_url, "is_mature":s.is_mature
            })
        })
        .collect();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO category_stream_snapshots
          (poll_id,snapshot_at,stream_id,user_id,user_login,viewer_count,title,language,started_at,tags,thumbnail_url,is_mature)
        SELECT $1,$2,r.stream_id,r.user_id,r.user_login,r.viewer_count,r.title,r.language,r.started_at,r.tags,r.thumbnail_url,r.is_mature
        FROM jsonb_to_recordset($3) AS r(stream_id text,user_id text,user_login text,viewer_count integer,title text,
            language text,started_at timestamptz,tags jsonb,thumbnail_url text,is_mature boolean)
        ON CONFLICT(poll_id,stream_id) DO NOTHING")
        .bind(id).bind(at).bind(json!(rows)).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO category_channels(user_id,login,display_name,last_seen_live_at)
        SELECT DISTINCT ON(user_id) user_id,user_login,display_name,$2
        FROM jsonb_to_recordset($1) AS r(user_id text,user_login text,display_name text)
        ON CONFLICT(user_id) DO UPDATE SET login=EXCLUDED.login,
           display_name=COALESCE(NULLIF(EXCLUDED.display_name,''),category_channels.display_name),
           last_seen_live_at=EXCLUDED.last_seen_live_at",
    )
    .bind(json!(rows))
    .bind(at)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE category_polls SET status='complete',completed_at=now(),stream_count=$2,viewer_total=$3,page_count=$4,error_detail=NULL WHERE poll_id=$1")
        .bind(id).bind(streams.len() as i32).bind(streams.iter().map(|s| s.viewer_count).sum::<i64>())
        .bind(pages as i32).execute(&mut *tx).await?;
    sqlx::query("UPDATE category_collector_status SET last_poll_started_at=$1,last_complete_poll_at=now(),updated_at=now() WHERE id=1")
        .bind(at).execute(&mut *tx).await?;
    tx.commit().await
}

/// Returns inserted, duplicate and expired/rejected counts separately.
/// Whole-hour expiry prevents a late event from overwriting an already
/// finalized aggregate after part of that hour's raw data was deleted.
pub async fn insert_chat(
    pool: &PgPool,
    rows: &[ChatZeile],
) -> Result<(i64, i64, i64), sqlx::Error> {
    if rows.is_empty() {
        return Ok((0, 0, 0));
    }
    let value = serde_json::to_value(rows)
        .map_err(|_| sqlx::Error::Protocol("chat batch serialization failed".into()))?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(827445013)")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query_as("WITH incoming AS (
        SELECT * FROM jsonb_to_recordset($1) AS r(room_user_id text,message_id text,source_message_id text,
            source_room_id text,sent_at timestamptz,chatter_user_id text,chatter_login text,message_text text,
            text_len integer,detected_lang text,lang_confidence real,lang_method text,emote_count integer,irc_tags jsonb)
      ), eligible AS (
        SELECT r.* FROM incoming r CROSS JOIN category_collector_config cfg
        WHERE cfg.id=1 AND r.sent_at >= date_trunc('hour',now()-make_interval(days=>cfg.retention_days),'UTC') + interval '1 hour'
          AND r.sent_at <= now()+interval '5 minutes'
      ), inserted AS (
        INSERT INTO category_chat_messages(room_user_id,message_id,source_message_id,source_room_id,sent_at,
            chatter_user_id,chatter_login,message_text,text_len,detected_lang,lang_confidence,lang_method,emote_count,irc_tags)
        SELECT * FROM eligible ON CONFLICT(room_user_id,message_id) DO NOTHING RETURNING sent_at
      ), dirty AS (
        INSERT INTO category_chat_dirty_hours(hour_bucket)
        SELECT DISTINCT date_trunc('hour',sent_at,'UTC') FROM inserted
        ON CONFLICT(hour_bucket) DO UPDATE SET hour_bucket=EXCLUDED.hour_bucket RETURNING hour_bucket
      )
      SELECT (SELECT count(*) FROM inserted),
             (SELECT count(*) FROM eligible)-(SELECT count(*) FROM inserted),
             (SELECT count(*) FROM incoming)-(SELECT count(*) FROM eligible)")
        .bind(value).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(result)
}

/// Ordered moderation event: remove text only, retaining already observed
/// counts and language/length aggregates. Never erase future messages after
/// a CLEARCHAT, nor issue any Twitch moderation command.
pub async fn redact(
    pool: &PgPool,
    room: &str,
    message: Option<&str>,
    chatter: Option<&str>,
    before: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let count = sqlx::query(
        "UPDATE category_chat_messages SET message_text='',irc_tags='{}',redacted_at=now()
        WHERE room_user_id=$1 AND ($2::text IS NULL OR message_id=$2)
          AND ($3::text IS NULL OR chatter_user_id=$3) AND sent_at <= $4 AND redacted_at IS NULL",
    )
    .bind(room)
    .bind(message)
    .bind(chatter)
    .bind(before)
    .execute(pool)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE category_collector_status SET chat_redactions=chat_redactions+$1 WHERE id=1",
    )
    .bind(count as i64)
    .execute(pool)
    .await?;
    Ok(count)
}

/// Locks one durable dirty-hour row. Writers touch the same row in their
/// insert transaction, so concurrent arrivals cannot lose the catch-up mark.
pub async fn rollup_one(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let hour: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT hour_bucket FROM category_chat_dirty_hours
        WHERE hour_bucket < date_trunc('hour',now(),'UTC') ORDER BY hour_bucket
        LIMIT 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(hour) = hour else {
        tx.commit().await?;
        return Ok(false);
    };
    sqlx::query("DELETE FROM category_chat_rollup WHERE hour_bucket=$1")
        .bind(hour)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO category_chat_rollup(hour_bucket,room_user_id,lang,messages,distinct_chatters,total_len,avg_len)
        SELECT $1,room_user_id,detected_lang,count(*),count(DISTINCT chatter_user_id),sum(text_len::bigint),avg(text_len)::double precision
        FROM category_chat_messages WHERE sent_at >= $1 AND sent_at < $1+interval '1 hour'
          AND (source_room_id IS NULL OR source_room_id=room_user_id)
        GROUP BY room_user_id,detected_lang")
        .bind(hour).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM category_chat_dirty_hours WHERE hour_bucket=$1")
        .bind(hour)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE category_collector_status SET last_rollup_hour=GREATEST(last_rollup_hour,$1) WHERE id=1")
        .bind(hour).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

/// Retention is independent of enabled/chat_enabled and cannot be disabled.
/// An unaggregated hour is preserved for catch-up, not silently discarded.
pub async fn retention_batch(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(827445013)")
        .execute(&mut *tx)
        .await?;
    let deleted = sqlx::query("DELETE FROM category_chat_messages WHERE ctid IN (
        SELECT m.ctid FROM category_chat_messages m CROSS JOIN category_collector_config cfg
        WHERE cfg.id=1 AND m.sent_at < date_trunc('hour',now()-make_interval(days=>cfg.retention_days),'UTC')+interval '1 hour'
          AND NOT EXISTS (SELECT 1 FROM category_chat_dirty_hours d WHERE d.hour_bucket=date_trunc('hour',m.sent_at,'UTC'))
        LIMIT 5000)")
        .execute(&mut *tx).await?.rows_affected();
    sqlx::query("UPDATE category_collector_status SET retention_deleted_total=retention_deleted_total+$1,last_retention_run_at=now() WHERE id=1")
        .bind(deleted as i64).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(deleted)
}

/// Persist baseline plus monotonic process counters. Repeating an update after
/// an ambiguous connection failure cannot double-count drops or dispatches.
#[derive(Clone, Default, sqlx::FromRow)]
pub struct CounterBaseline {
    chat_privmsgs_dispatched: i64,
    chat_privmsgs_dropped: i64,
    chat_control_events_dropped: i64,
    chat_commands_dropped: i64,
    chat_invalid_logins_rejected: i64,
    chat_reconnects: i64,
    chat_queue_dropped: i64,
    chat_duplicates_skipped: i64,
}
pub async fn counter_baseline(pool: &PgPool) -> Result<CounterBaseline, sqlx::Error> {
    sqlx::query_as("SELECT chat_privmsgs_dispatched,chat_privmsgs_dropped,chat_control_events_dropped,
        chat_commands_dropped,chat_invalid_logins_rejected,chat_reconnects,chat_queue_dropped,chat_duplicates_skipped
        FROM category_collector_status WHERE id=1").fetch_one(pool).await
}
pub async fn status(
    pool: &PgPool,
    stats: &tb_monitoring::anon_chat::AnonChatStatsSnapshot,
    baseline: &CounterBaseline,
    rejected: u64,
    duplicates: u64,
) -> Result<(), sqlx::Error> {
    fn total(base: i64, delta: u64) -> i64 {
        base.saturating_add(i64::try_from(delta).unwrap_or(i64::MAX))
    }
    sqlx::query(
        "UPDATE category_collector_status SET roster_size=$1,connected_shards=$2,
        chat_privmsgs_dispatched=$3,chat_privmsgs_dropped=$4,chat_control_events_dropped=$5,
        chat_commands_dropped=$6,chat_invalid_logins_rejected=$7,chat_reconnects=$8,
        chat_queue_dropped=$9,chat_duplicates_skipped=$10,updated_at=now() WHERE id=1",
    )
    .bind(stats.channels_monitored as i32)
    .bind(stats.connected_shards as i32)
    .bind(total(
        baseline.chat_privmsgs_dispatched,
        stats.privmsgs_dispatched,
    ))
    .bind(total(
        baseline.chat_privmsgs_dropped,
        stats.privmsgs_dropped,
    ))
    .bind(total(
        baseline.chat_control_events_dropped,
        stats.control_events_dropped,
    ))
    .bind(total(
        baseline.chat_commands_dropped,
        stats.commands_dropped,
    ))
    .bind(total(
        baseline.chat_invalid_logins_rejected,
        stats.invalid_logins_rejected,
    ))
    .bind(total(baseline.chat_reconnects, stats.reconnects))
    .bind(total(baseline.chat_queue_dropped, rejected))
    .bind(total(baseline.chat_duplicates_skipped, duplicates))
    .execute(pool)
    .await?;
    Ok(())
}
