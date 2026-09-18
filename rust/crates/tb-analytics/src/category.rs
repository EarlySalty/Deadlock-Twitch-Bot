//! Global category analytics. Public data only; no outbound Twitch actions.
//! Raw data is bounded and deduplicated; rollups are rebuilt from dirty buckets.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tb_transport_twitch::{
    irc_message::{parse_privmsg, parse_tags},
    streams::HelixStream,
};

pub const DETECTOR: &str = "whatlang-0.18.0/conservative-v1";

pub fn language_code(code: &str) -> String {
    let code = code.trim().to_ascii_lowercase();
    let mapped = match code.as_str() {
        "eng" => "en",
        "deu" => "de",
        "rus" => "ru",
        "spa" => "es",
        "por" => "pt",
        "fra" => "fr",
        "jpn" => "ja",
        "kor" => "ko",
        "cmn" | "zho" => "zh",
        "tur" => "tr",
        "pol" => "pl",
        "ukr" => "uk",
        "ita" => "it",
        "nld" => "nl",
        "swe" => "sv",
        "dan" => "da",
        "fin" => "fi",
        "nob" | "nor" => "no",
        "ces" => "cs",
        "slk" => "sk",
        "hun" => "hu",
        "ron" => "ro",
        "bul" => "bg",
        "ell" => "el",
        "ara" => "ar",
        "heb" => "he",
        "hin" => "hi",
        "ind" => "id",
        "vie" => "vi",
        "tha" => "th",
        "cat" => "ca",
        "hrv" => "hr",
        "srp" => "sr",
        "slv" => "sl",
        "lit" => "lt",
        "lav" => "lv",
        "est" => "et",
        "fas" => "fa",
        "ben" => "bn",
        "tam" => "ta",
        "tel" => "te",
        "mar" => "mr",
        "mal" => "ml",
        "kan" => "kn",
        "urd" => "ur",
        "guj" => "gu",
        "pan" => "pa",
        "nep" => "ne",
        "sin" => "si",
        "khm" => "km",
        "kat" => "ka",
        "hye" => "hy",
        "aze" => "az",
        "uzb" => "uz",
        "kaz" => "kk",
        "mkd" => "mk",
        "bel" => "be",
        "epo" => "eo",
        "afr" => "af",
        "tgl" => "tl",
        "msa" => "ms",
        "bos" => "bs",
        "sqi" => "sq",
        "isl" => "is",
        "gle" => "ga",
        "cym" => "cy",
        "eus" => "eu",
        "glg" => "gl",
        "lat" => "la",
        "swa" => "sw",
        "zul" => "zu",
        "yor" => "yo",
        "amh" => "am",
        "mya" => "my",
        "lao" => "lo",
        "" | "other" => "und",
        other => other,
    };
    if (2..=3).contains(&mapped.len()) && mapped.bytes().all(|b| b.is_ascii_lowercase()) {
        mapped.into()
    } else {
        "und".into()
    }
}

pub fn detect_language(text: &str, emotes: &str) -> (String, f64, i32) {
    let mut ranges = Vec::new();
    for item in emotes.split('/') {
        let Some((_, positions)) = item.split_once(':') else {
            continue;
        };
        for position in positions.split(',') {
            if let Some((a, b)) = position.split_once('-') {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    if a <= b && b < 10_000 {
                        ranges.push((a, b));
                    }
                }
            }
        }
    }
    let filtered: String = text
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if ranges.iter().any(|(a, b)| (*a..=*b).contains(&i)) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<_> = filtered
        .split_whitespace()
        .filter(|s| !s.starts_with("http") && !s.starts_with('@'))
        .collect();
    let sample = words.join(" ");
    let letters = sample.chars().filter(|c| c.is_alphabetic()).count();
    // Short messages, emotes and game jargon cannot be assigned reliably.
    if letters < 20 {
        return ("und".into(), 0.0, ranges.len() as i32);
    }
    match whatlang::detect(&sample) {
        Some(info) if info.is_reliable() && info.confidence() >= 0.85 => (
            language_code(info.lang().code()),
            info.confidence(),
            ranges.len() as i32,
        ),
        Some(info) => ("und".into(), info.confidence(), ranges.len() as i32),
        None => ("und".into(), 0.0, ranges.len() as i32),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMessage {
    pub sent_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub message_id: String,
    pub room_user_id: String,
    pub chatter_user_id: String,
    pub chatter_login: String,
    pub message_text: String,
    pub message_len: i32,
    pub detected_lang: String,
    pub language_confidence: f64,
    pub stream_language: String,
    pub emote_count: i32,
    pub shared_chat_copy: bool,
    pub tags: Value,
}

pub fn raw_message(
    line: &str,
    received_at: DateTime<Utc>,
    room_id: &str,
    stream_language: &str,
) -> Option<RawMessage> {
    let parsed = parse_privmsg(line)?;
    let room = parsed.tags.get("room-id")?;
    let user = parsed.tags.get("user-id")?;
    let message_id = parsed.tags.get("id")?;
    if room != room_id || user.is_empty() || message_id.is_empty() || parsed.text.len() > 16_384 {
        return None;
    }
    let sent_at = DateTime::from_timestamp_millis(parsed.tags.get("tmi-sent-ts")?.parse().ok()?)?;
    // IRC has no backfill. Reject invalid timestamps rather than corrupt retention.
    if sent_at > received_at + chrono::Duration::minutes(2)
        || sent_at < received_at - chrono::Duration::minutes(15)
    {
        return None;
    }
    let (lang, confidence, emotes) = detect_language(
        &parsed.text,
        parsed.tags.get("emotes").map(String::as_str).unwrap_or(""),
    );
    let shared_chat_copy = parsed
        .tags
        .get("source-room-id")
        .is_some_and(|source| !source.is_empty() && source != room);
    Some(RawMessage {
        sent_at,
        received_at,
        message_id: message_id.clone(),
        room_user_id: room.clone(),
        chatter_user_id: user.clone(),
        chatter_login: parsed.login.to_ascii_lowercase(),
        message_len: parsed.text.chars().count() as i32,
        message_text: parsed.text,
        detected_lang: lang,
        language_confidence: confidence,
        stream_language: language_code(stream_language),
        emote_count: emotes,
        shared_chat_copy,
        tags: json!(parsed.tags),
    })
}

pub async fn store_messages(pool: &PgPool, messages: &[RawMessage]) -> Result<i64, sqlx::Error> {
    if messages.is_empty() {
        return Ok(0);
    }
    let payload =
        serde_json::to_string(messages).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    // Mark only inserted rows as dirty. Replayed deliveries cannot inflate counts.
    sqlx::query_scalar(
        "WITH inserted AS (INSERT INTO category_chat_messages
         SELECT * FROM jsonb_to_recordset($1::jsonb) AS x(sent_at timestamptz,received_at timestamptz,
         message_id text,room_user_id text,chatter_user_id text,chatter_login text,message_text text,
         message_len integer,detected_lang text,language_confidence double precision,stream_language text,
         emote_count integer,shared_chat_copy boolean,tags jsonb)
         ON CONFLICT DO NOTHING RETURNING sent_at,room_user_id,detected_lang),
         dirty AS (INSERT INTO category_chat_dirty SELECT DISTINCT date_trunc('hour',sent_at),room_user_id,detected_lang
         FROM inserted ON CONFLICT DO NOTHING)
         SELECT count(*)::bigint FROM inserted")
        .bind(payload).fetch_one(pool).await
}

/// Honor Twitch deletions, without keeping a second raw-text audit copy.
pub async fn delete_chat(pool: &PgPool, line: &str, room_id: &str) -> Result<u64, sqlx::Error> {
    let Some(tags_part) = line.strip_prefix('@').and_then(|s| s.split_once(' ')) else {
        return Ok(0);
    };
    let tags = parse_tags(tags_part.0);
    if tags.get("room-id").is_some_and(|id| id != room_id) {
        return Ok(0);
    }
    let command = tags_part.1.split_whitespace().nth(1).unwrap_or("");
    let (message, user) = match command {
        "CLEARMSG" => (tags.get("target-msg-id").cloned(), None),
        "CLEARCHAT" => (None, tags.get("target-user-id").cloned()),
        _ => return Ok(0),
    };
    if command == "CLEARMSG" && message.is_none() {
        return Ok(0);
    }
    let result = sqlx::query(
        "WITH removed AS (DELETE FROM category_chat_messages WHERE room_user_id=$1
         AND ($2::text IS NULL OR message_id=$2) AND ($3::text IS NULL OR chatter_user_id=$3)
         RETURNING sent_at,room_user_id,detected_lang)
         INSERT INTO category_chat_dirty SELECT DISTINCT date_trunc('hour',sent_at),room_user_id,detected_lang
         FROM removed ON CONFLICT DO NOTHING")
        .bind(room_id).bind(message).bind(user).execute(pool).await?;
    Ok(result.rows_affected())
}

pub async fn flush_rollups(pool: &PgPool, limit: i64) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // Lock keys while computing: a concurrent writer cannot lose its dirty mark.
    let keys: Vec<(DateTime<Utc>, String, String)> = sqlx::query_as(
        "SELECT hour_at,room_user_id,language FROM category_chat_dirty ORDER BY hour_at
         LIMIT $1 FOR UPDATE SKIP LOCKED",
    )
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    if keys.is_empty() {
        return Ok(0);
    }
    for (hour, room, lang) in &keys {
        sqlx::query(
            "INSERT INTO category_chat_rollup(hour_at,room_user_id,language,messages,distinct_chatter,total_chars,avg_len)
             SELECT $1,$2,$3,count(*),count(DISTINCT chatter_user_id),coalesce(sum(message_len),0),coalesce(avg(message_len),0)::float8
             FROM category_chat_messages WHERE sent_at >= $1 AND sent_at < $1 + interval '1 hour'
             AND room_user_id=$2 AND detected_lang=$3 AND NOT shared_chat_copy
             ON CONFLICT(hour_at,room_user_id,language) DO UPDATE SET messages=excluded.messages,
             distinct_chatter=excluded.distinct_chatter,total_chars=excluded.total_chars,avg_len=excluded.avg_len,updated_at=now()")
            .bind(hour).bind(room).bind(lang).execute(&mut *tx).await?;
        sqlx::query(
            "DELETE FROM category_chat_dirty WHERE hour_at=$1 AND room_user_id=$2 AND language=$3",
        )
        .bind(hour)
        .bind(room)
        .bind(lang)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(keys.len())
}

pub async fn store_snapshot(
    pool: &PgPool,
    at: DateTime<Utc>,
    streams: &[HelixStream],
    poll_seconds: i32,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let previous: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT max(snapshot_at) FROM category_collection_runs")
            .fetch_one(&mut *tx)
            .await?;
    // Downtime is NOT counted as observed airtime. Estimates integrate at most one poll.
    let elapsed = previous
        .map(|p| (at - p).num_milliseconds().max(0) as f64 / 1000.0)
        .unwrap_or(0.0);
    let duration = if elapsed > f64::from(poll_seconds) * 2.0 {
        0.0
    } else {
        elapsed.min(f64::from(poll_seconds))
    };
    let viewers: i64 = streams.iter().map(|s| s.viewer_count.max(0)).sum();
    sqlx::query("INSERT INTO category_collection_runs VALUES($1,now(),$2,$3,$4)")
        .bind(at)
        .bind(streams.len() as i32)
        .bind(viewers)
        .bind(poll_seconds)
        .execute(&mut *tx)
        .await?;
    for stream in streams {
        let started = DateTime::parse_from_rfc3339(&stream.started_at)
            .map_err(|_| sqlx::Error::Protocol("invalid stream start timestamp".into()))?
            .with_timezone(&Utc);
        let language = language_code(&stream.language);
        sqlx::query("INSERT INTO category_channels(user_id,login,display_name,first_seen,last_seen,broadcaster_language)
            VALUES($1,$2,$3,$4,$4,$5) ON CONFLICT(user_id) DO UPDATE SET login=excluded.login,
            display_name=excluded.display_name,last_seen=excluded.last_seen,broadcaster_language=excluded.broadcaster_language")
            .bind(&stream.user_id).bind(stream.user_login.to_ascii_lowercase()).bind(&stream.user_name).bind(at).bind(&language).execute(&mut *tx).await?;
        let seconds = duration.min((at - started).num_milliseconds().max(0) as f64 / 1000.0);
        sqlx::query(
            "INSERT INTO category_stream_snapshots VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(at)
        .bind(&stream.id)
        .bind(&stream.user_id)
        .bind(&stream.user_login)
        .bind(stream.viewer_count.max(0))
        .bind(&stream.title)
        .bind(language)
        .bind(started)
        .bind(stream.tags.clone().unwrap_or_default())
        .bind(&stream.thumbnail_url)
        .bind(stream.is_mature)
        .bind(seconds)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Exact 90-day cutoff within the boundary partition, after finalizing buckets.
/// The daily drop function reclaims fully expired partitions physically.
pub async fn trim_expired_rows(pool: &PgPool, days: i32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("WITH expired AS (SELECT tableoid,ctid FROM category_chat_messages m
        WHERE sent_at < now() - make_interval(days => $1)
        AND NOT EXISTS(SELECT 1 FROM category_chat_dirty d WHERE d.hour_at=date_trunc('hour',m.sent_at)
            AND d.room_user_id=m.room_user_id AND d.language=m.detected_lang)
        LIMIT 10000)
        DELETE FROM category_chat_messages m USING expired e WHERE m.tableoid=e.tableoid AND m.ctid=e.ctid")
        .bind(days).execute(pool).await?;
    Ok(result.rows_affected())
}

async fn json_query(pool: &PgPool, sql: &str, days: i32) -> Result<Value, sqlx::Error> {
    let text: String = sqlx::query_scalar(sql).bind(days).fetch_one(pool).await?;
    serde_json::from_str(&text).map_err(|e| sqlx::Error::Protocol(e.to_string()))
}

pub async fn report(pool: &PgPool, days: i32) -> Result<Value, sqlx::Error> {
    let languages = json_query(pool, "WITH s AS (SELECT language,count(DISTINCT stream_id) AS streams,
        count(DISTINCT user_id) AS channels,sum(sample_seconds)/3600 AS airtime_hours,
        sum(viewer_count*sample_seconds)/NULLIF(sum(sample_seconds),0) AS avg_viewers,
        sum(viewer_count*sample_seconds)/3600 AS viewer_hours
        FROM category_stream_snapshots WHERE snapshot_at >= now()-make_interval(days=>$1) GROUP BY language),
        c AS (SELECT language,sum(messages) AS messages FROM category_chat_rollup
        WHERE hour_at >= date_trunc('hour',now()-make_interval(days=>$1)) GROUP BY language)
        SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY viewer_hours DESC NULLS LAST),'[]')::text
        FROM (SELECT coalesce(s.language,c.language) AS language,s.streams,s.channels,s.airtime_hours,s.avg_viewers,s.viewer_hours,
        coalesce(c.messages,0) AS messages FROM s FULL JOIN c USING(language)) x", days).await?;
    let trend = json_query(
        pool,
        "WITH x AS (SELECT date_trunc('hour',snapshot_at) AS at,
        avg(streams)::float8 AS streams,avg(viewers)::float8 AS viewers,count(*) AS polls,
        max(streams) AS peak_streams,max(viewers) AS peak_viewers
        FROM category_collection_runs WHERE snapshot_at >= now()-make_interval(days=>$1) GROUP BY 1)
        SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY at),'[]')::text FROM x",
        days,
    )
    .await?;
    let top = json_query(pool, "WITH totals AS (SELECT language,user_id,max(user_login) AS login,
        sum(sample_seconds)/3600 AS airtime_hours,sum(viewer_count*sample_seconds)/NULLIF(sum(sample_seconds),0) AS avg_viewers,
        sum(viewer_count*sample_seconds)/3600 AS viewer_hours FROM category_stream_snapshots
        WHERE snapshot_at >= now()-make_interval(days=>$1) GROUP BY language,user_id),
        ranked AS (SELECT *,row_number() OVER(PARTITION BY language ORDER BY viewer_hours DESC,user_id) AS rank FROM totals)
        SELECT coalesce(jsonb_agg(to_jsonb(ranked) ORDER BY language,rank),'[]')::text FROM ranked WHERE rank<=10",days).await?;
    let hourly = json_query(pool, "WITH x AS (SELECT language,extract(hour FROM hour_at AT TIME ZONE 'UTC')::int AS hour,
        sum(messages) AS messages FROM category_chat_rollup WHERE hour_at >= date_trunc('hour',now()-make_interval(days=>$1))
        GROUP BY 1,2) SELECT coalesce(jsonb_agg(to_jsonb(x) ORDER BY language,hour),'[]')::text FROM x",days).await?;
    let status: Option<(DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT heartbeat_at,details::text FROM category_collector_status WHERE singleton",
    )
    .fetch_optional(pool)
    .await?;
    let coverage = sqlx::query("SELECT min(snapshot_at) AS first,max(snapshot_at) AS last,count(*) AS polls FROM category_collection_runs").fetch_one(pool).await?;
    let details = status
        .as_ref()
        .map(|(_, s)| serde_json::from_str::<Value>(s))
        .transpose()
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    Ok(
        json!({"days":days,"generated_at":Utc::now(),"languages":languages,"trend":trend,"top_channels":top,"hourly":hourly,
        "status":details,"heartbeat_at":status.map(|s|s.0),
        "first_snapshot":coverage.try_get::<Option<DateTime<Utc>>,_>("first")?,
        "last_snapshot":coverage.try_get::<Option<DateTime<Utc>>,_>("last")?,"total_polls":coverage.try_get::<i64,_>("polls")?,
        "method":{"region":"language_not_geography","detector":DETECTOR,"unknown_language":"und","timezone":"UTC",
        "airtime":"sampled; gaps are not extrapolated","chat":"detected message language; shared-chat copies excluded",
        "distinct_chatters":"only unique within one channel-language-hour; never unique category viewers"}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_normalization_is_not_geolocation() {
        assert_eq!(language_code("deu"), "de");
        assert_eq!(language_code("en-US"), "und");
        assert_eq!(detect_language("gg ez Kappa", "25:6-10").0, "und");
        assert_eq!(
            detect_language(
                "Dies ist ein vollständiger deutscher Satz mit vielen unterschiedlichen Wörtern.",
                ""
            )
            .0,
            "de"
        );
    }
    #[test]
    fn ids_timestamps_and_shared_chat_are_explicit() {
        let now = Utc::now();
        let line=format!("@room-id=1;user-id=2;id=m1;tmi-sent-ts={};source-room-id=9 :a!a@a PRIVMSG #room :hello",now.timestamp_millis());
        assert!(raw_message(&line, now, "1", "en").unwrap().shared_chat_copy);
        assert!(raw_message(&line, now, "wrong", "en").is_none());
        assert!(raw_message(":a!a@a PRIVMSG #room :hello", now, "1", "en").is_none());
    }
}
