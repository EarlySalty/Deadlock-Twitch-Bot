//! Hintergrund-Jobs für den Engagement-Layer (Port von
//! `bot/engagement/background.py`).
//!
//! Sieben periodische Loops (jittered): Thread-Extractor (15min), Match-Poller
//! (30s), Auto-Closer (1h), Conversation-Trim (24h, behält 500/Channel),
//! Global-Sentiment (20min), Soul-Anchor (3h), Channel-Profile (4h). Jeder Loop
//! ist best-effort. Der Stream-Transkript-Loop (Audio-Capture + Whisper-STT)
//! folgt separat, sobald das STT-Subsystem in Rust existiert.

use std::time::{Duration, Instant};

use chrono::Utc;
use sqlx::PgPool;

use crate::audio_capture::AudioCapturer;
use crate::channel_background::ChannelBackground;
use crate::global_sentiment::GlobalSentiment;
use crate::match_context::MatchContext;
use crate::minimax_chat::EngagementMinimaxClient;
use crate::soul_store::SoulStore;
use crate::stream_transcripts::{
    transcript_capture_seconds, transcript_poll_interval_seconds, transcript_quality,
    StreamTranscriptSegment, StreamTranscripts,
};
use crate::threads::Threads;
use crate::transcribe::OpenAiTranscriber;

const THREAD_EXTRACTOR_INTERVAL: f64 = 15.0 * 60.0;
const MATCH_POLLER_INTERVAL: f64 = 30.0;
const AUTO_CLOSER_INTERVAL: f64 = 60.0 * 60.0;
const CONVERSATION_TRIM_INTERVAL: f64 = 24.0 * 60.0 * 60.0;
const CONVERSATION_KEEP_PER_CHANNEL: i64 = 500;
const GLOBAL_SENTIMENT_INTERVAL: f64 = 20.0 * 60.0;
const SOUL_ANCHOR_INTERVAL: f64 = 3.0 * 60.0 * 60.0;
const CHANNEL_PROFILE_INTERVAL: f64 = 4.0 * 60.0 * 60.0;
const TRANSCRIPT_TRIM_INTERVAL: Duration = Duration::from_secs(15 * 60);
const AI_TIMEOUT: Duration = Duration::from_secs(180);

/// `ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED` (Default an). Aus → kein Capture.
fn stream_transcripts_enabled() -> bool {
    match std::env::var("ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED") {
        Ok(v) => !matches!(v.trim().to_lowercase().as_str(), "" | "0" | "false" | "no" | "off"),
        Err(_) => true,
    }
}

/// Aktive Engagement-Channels samt steam_id.
async fn load_enabled_channels(pool: &PgPool) -> Vec<(String, Option<String>)> {
    sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT channel_login, steam_id FROM twitch_engagement_settings WHERE enabled = TRUE",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Trimmt den Conversation-Buffer auf die jüngsten `keep` Turns je Channel.
async fn trim_conversation(pool: &PgPool, keep: i64) -> u64 {
    sqlx::query(
        "DELETE FROM twitch_engagement_conversation WHERE id IN (\
           SELECT id FROM (\
             SELECT id, ROW_NUMBER() OVER (\
               PARTITION BY channel_login ORDER BY ts DESC) AS rn \
             FROM twitch_engagement_conversation) ranked \
           WHERE rn > $1)",
    )
    .bind(keep)
    .execute(pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0)
}

/// Schlaf mit ±10% Jitter (gegen Thundering Herd), mindestens 1s.
async fn jittered_sleep(base_sec: f64) {
    let jitter = base_sec * 0.1 * (rand::random::<f64>() * 2.0 - 1.0);
    let secs = (base_sec + jitter).max(1.0);
    tokio::time::sleep(Duration::from_secs_f64(secs)).await;
}

// ---- run-once-Funktionen (eine Loop-Iteration, testbar) ---------------------

async fn run_thread_extractor_once(pool: &PgPool, minimax: &EngagementMinimaxClient) {
    let threads = Threads::new(pool.clone());
    for (channel, _steam) in load_enabled_channels(pool).await {
        threads.extract_threads(&channel, minimax, 6, 80).await;
    }
}

async fn run_match_poller_once(pool: &PgPool) {
    let mc = MatchContext::new(pool.clone());
    for (channel, steam) in load_enabled_channels(pool).await {
        if let Some(steam) = steam.filter(|s| !s.is_empty()) {
            mc.poll_match_state(&channel, &steam).await;
        }
    }
}

async fn run_auto_closer_once(pool: &PgPool) {
    Threads::new(pool.clone()).auto_close_stale().await;
}

async fn run_global_sentiment_once(pool: &PgPool, minimax: &EngagementMinimaxClient) {
    GlobalSentiment::new(pool.clone()).rebuild_global_sentiment(minimax).await;
}

async fn run_soul_anchor_once(pool: &PgPool, minimax: &EngagementMinimaxClient) {
    SoulStore::new(pool.clone()).reflect_and_store_anchor(minimax).await;
}

async fn run_channel_profile_once(pool: &PgPool, minimax: &EngagementMinimaxClient) {
    ChannelBackground::new(pool.clone()).rebuild_all_channel_profiles(minimax).await;
}

/// Captured + transkribiert einen Stream-Ausschnitt eines Channels und legt das
/// Segment ab (Port von `_transcribe_capture`). Best-effort; Workdir wird immer
/// aufgeräumt.
async fn run_transcribe_capture(
    pool: &PgPool,
    channel: &str,
    capturer: &AudioCapturer,
    transcriber: &OpenAiTranscriber,
) {
    let capture = match capturer
        .capture(channel, transcript_capture_seconds().max(0) as u64, &transcript_quality(), None)
        .await
    {
        Ok(c) => c,
        Err(error) => {
            tracing::debug!(channel, %error, "stream-transcript: Capture fehlgeschlagen");
            return;
        }
    };
    let transcription = transcriber.transcribe_clip(&capture.media_path).await;
    capture.cleanup().await; // entspricht Pythons finally
    let result = match transcription {
        Ok(r) => r,
        Err(error) => {
            tracing::debug!(channel, %error, "stream-transcript: Transkription fehlgeschlagen");
            return;
        }
    };

    let text = result.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return;
    }
    // Dauer: Modell → Capture-Ist → Capture-Soll (Python `or`-Kette).
    let duration = if result.duration_seconds > 0.0 {
        result.duration_seconds
    } else if capture.actual_duration_seconds > 0.0 {
        capture.actual_duration_seconds
    } else {
        capture.requested_duration_seconds as f64
    };
    let ended_at = Utc::now();
    let started_at = ended_at - chrono::Duration::seconds(duration.max(1.0) as i64);
    let segment = StreamTranscriptSegment {
        channel_login: channel.to_string(),
        started_at,
        ended_at,
        text,
        engine: result.engine,
        model: Some(result.model).filter(|m| !m.is_empty()),
    };
    let _ = StreamTranscripts::new(pool.clone()).append_segment(&segment).await;
}

fn ai_client() -> EngagementMinimaxClient {
    EngagementMinimaxClient::new(None, None, None, Some(AI_TIMEOUT))
}

// ---- Endlos-Loops -----------------------------------------------------------

/// Thread-Extractor (alle 15min, pro enabled Channel).
pub async fn schedule_thread_extractor(pool: PgPool) {
    let minimax = EngagementMinimaxClient::new(None, None, None, None);
    loop {
        run_thread_extractor_once(&pool, &minimax).await;
        jittered_sleep(THREAD_EXTRACTOR_INTERVAL).await;
    }
}

/// Match-Poller (alle 30s, pro enabled Channel mit steam_id).
pub async fn schedule_match_poller(pool: PgPool) {
    loop {
        run_match_poller_once(&pool).await;
        jittered_sleep(MATCH_POLLER_INTERVAL).await;
    }
}

/// Thread-Auto-Closer (alle 1h).
pub async fn schedule_auto_closer(pool: PgPool) {
    loop {
        run_auto_closer_once(&pool).await;
        jittered_sleep(AUTO_CLOSER_INTERVAL).await;
    }
}

/// Conversation-Trim (alle 24h, behält 500/Channel).
pub async fn schedule_conversation_trim(pool: PgPool) {
    loop {
        let _ = trim_conversation(&pool, CONVERSATION_KEEP_PER_CHANNEL).await;
        jittered_sleep(CONVERSATION_TRIM_INTERVAL).await;
    }
}

/// Global-Sentiment-Rebuild (alle 20min).
pub async fn schedule_global_sentiment(pool: PgPool) {
    let minimax = ai_client();
    loop {
        run_global_sentiment_once(&pool, &minimax).await;
        jittered_sleep(GLOBAL_SENTIMENT_INTERVAL).await;
    }
}

/// Soul-Anchor-Reflexion (alle 3h).
pub async fn schedule_soul_anchor(pool: PgPool) {
    let minimax = ai_client();
    loop {
        run_soul_anchor_once(&pool, &minimax).await;
        jittered_sleep(SOUL_ANCHOR_INTERVAL).await;
    }
}

/// Channel-Profile-Rebuild (alle 4h).
pub async fn schedule_channel_profile(pool: PgPool) {
    let minimax = ai_client();
    loop {
        run_channel_profile_once(&pool, &minimax).await;
        jittered_sleep(CHANNEL_PROFILE_INTERVAL).await;
    }
}

/// Stream-Transkript-Loop (Port von `_run_stream_transcript_loop`): pro
/// enabled Channel ein streamlink-Capture + OpenAI-Whisper-Transkription, dazu
/// periodisches Trimmen. Aus (Env-Flag) oder ohne `OPENAI_API_KEY` → still im
/// Poll-Takt warten und retryen.
pub async fn schedule_stream_transcripts(pool: PgPool) {
    let capturer = AudioCapturer::from_env();
    let mut transcriber: Option<OpenAiTranscriber> = None;
    let mut last_trim: Option<Instant> = None;
    loop {
        if stream_transcripts_enabled() {
            if transcriber.is_none() {
                transcriber = OpenAiTranscriber::from_env();
            }
            match &transcriber {
                Some(t) => {
                    for (channel, _steam) in load_enabled_channels(&pool).await {
                        run_transcribe_capture(&pool, &channel, &capturer, t).await;
                    }
                    if last_trim.map_or(true, |t| t.elapsed() >= TRANSCRIPT_TRIM_INTERVAL) {
                        last_trim = Some(Instant::now());
                        let _ = StreamTranscripts::new(pool.clone()).trim_segments(None, None).await;
                    }
                }
                None => tracing::debug!(
                    "stream-transcripts: kein OPENAI_API_KEY — Transcriber nicht verfügbar"
                ),
            }
        }
        jittered_sleep(transcript_poll_interval_seconds()).await;
    }
}

/// Spawnt alle acht Background-Loops als tokio-Tasks (Python `ensure_started`).
pub fn spawn_all(pool: PgPool) {
    tokio::spawn(schedule_thread_extractor(pool.clone()));
    tokio::spawn(schedule_match_poller(pool.clone()));
    tokio::spawn(schedule_auto_closer(pool.clone()));
    tokio::spawn(schedule_conversation_trim(pool.clone()));
    tokio::spawn(schedule_global_sentiment(pool.clone()));
    tokio::spawn(schedule_soul_anchor(pool.clone()));
    tokio::spawn(schedule_channel_profile(pool.clone()));
    tokio::spawn(schedule_stream_transcripts(pool));
    tracing::info!(
        "Engagement-Background-Jobs gestartet (thread-extractor=15min, match-poller=30s, \
         auto-closer=1h, conv-trim=24h, global-sentiment=20min, soul-anchor=3h, \
         channel-profile=4h, stream-transcripts=poll)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
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
        sqlx::query("CREATE TABLE twitch_engagement_settings (channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT FALSE, steam_id TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_engagement_conversation (id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, content TEXT, ts TIMESTAMPTZ NOT NULL DEFAULT NOW())")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_user_threads (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL, twitch_login TEXT NOT NULL, channel_login TEXT, thread_type TEXT NOT NULL, summary TEXT NOT NULL, due_at TIMESTAMPTZ, status TEXT NOT NULL DEFAULT 'open', last_referenced_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn enabled_channels_und_trim() {
        let Some(pool) = make_pool("t_eng_bg").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, steam_id) VALUES ('nani', TRUE, '123'), ('aus', FALSE, '9'), ('nosteam', TRUE, NULL)")
            .execute(&pool).await.unwrap();
        let chans = load_enabled_channels(&pool).await;
        // Nur enabled: nani + nosteam (aus ist disabled).
        assert_eq!(chans.len(), 2);
        assert!(chans.iter().any(|(c, s)| c == "nani" && s.as_deref() == Some("123")));
        assert!(chans.iter().any(|(c, s)| c == "nosteam" && s.is_none()));

        // 3 Turns für 'nani', trim auf 1 → 2 gelöscht.
        sqlx::query("INSERT INTO twitch_engagement_conversation (channel_login, role, content, ts) VALUES \
             ('nani','user','a', NOW() - INTERVAL '3 min'), ('nani','user','b', NOW() - INTERVAL '2 min'), ('nani','user','c', NOW() - INTERVAL '1 min')")
            .execute(&pool).await.unwrap();
        let deleted = trim_conversation(&pool, 1).await;
        assert_eq!(deleted, 2);
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_conversation").fetch_one(&pool).await.unwrap();
        assert_eq!(remaining, 1);
    }

    #[tokio::test]
    async fn auto_closer_once_laeuft() {
        let Some(pool) = make_pool("t_eng_bg_close").await else { return };
        sqlx::query("INSERT INTO twitch_user_threads (twitch_user_id, twitch_login, thread_type, summary, status, due_at) VALUES ('u','user','upcoming_event','x','open', NOW() - INTERVAL '1 hour')")
            .execute(&pool).await.unwrap();
        run_auto_closer_once(&pool).await;
        let status: String = sqlx::query_scalar("SELECT status FROM twitch_user_threads LIMIT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(status, "follow_up_due"); // open+fällig → follow_up_due
    }
}
