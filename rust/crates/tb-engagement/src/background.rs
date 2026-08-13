//! Hintergrund-Jobs für den Engagement-Layer (Port von
//! `bot/engagement/background.py`).
//!
//! Sieben periodische Loops (jittered): Thread-Extractor (15min), Match-Poller
//! (30s), Auto-Closer (1h), Conversation-Trim (24h, behält 500/Channel),
//! Global-Sentiment (20min), Soul-Anchor (3h), Channel-Profile (4h). Jeder Loop
//! ist best-effort. Der Stream-Transkript-Loop (Audio-Capture + Whisper-STT)
//! folgt separat, sobald das STT-Subsystem in Rust existiert.

use std::collections::HashSet;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use sqlx::PgPool;

use crate::audio_capture::AudioCapturer;
use crate::channel_background::ChannelBackground;
use crate::global_sentiment::GlobalSentiment;
use crate::match_context::MatchContext;
use crate::minimax_chat::EngagementMinimaxClient;
use crate::reaction_learning::{
    capture_seconds as learn_capture_seconds, learn_enabled, LearnTranscriptSegment,
    ReactionLearning,
};
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
/// Wie oft der Lern-Supervisor nach neuen heißen Kanälen schaut.
const LEARN_SUPERVISOR_INTERVAL: f64 = 15.0;
const LEARN_MAPPER_INTERVAL: f64 = 60.0;
const LEARN_TRIM_INTERVAL: Duration = Duration::from_secs(60 * 60);
const LEARN_PROFILE_INTERVAL: f64 = 6.0 * 60.0 * 60.0;
const AI_TIMEOUT: Duration = Duration::from_secs(180);

/// Stream-Transkription über ALLE aktiven Channels ist standardmäßig AUS
/// (Grillme Block 19). Der Grund ist inzwischen nicht mehr der Anbieter, denn
/// transkribiert wird lokal, sondern die Dauerlast: jeder aktive Channel würde
/// rund um die Uhr einen Whisper-Lauf pro Poll erzeugen.
///
/// Zeitlich begrenzte Aufnahmen hängen deshalb nicht an diesem Gate, sondern an
/// ihrem eigenen Anlass, siehe Lernmodus und Smalltalk-Testsitzung.
fn stream_transcripts_enabled() -> bool {
    match std::env::var("ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED") {
        Ok(v) => v.trim() == "1",
        Err(_) => false,
    }
}

/// Aktive Engagement-Channels samt steam_id.
async fn load_enabled_channels(pool: &PgPool) -> Vec<(String, Option<String>)> {
    sqlx::query!(
        r#"SELECT channel_login AS "channel_login!", steam_id
           FROM twitch_engagement_settings
           WHERE enabled = TRUE"#
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|r| (r.channel_login, r.steam_id))
    .collect()
}

/// Trimmt den Conversation-Buffer auf die jüngsten `keep` Turns je Channel.
async fn trim_conversation(pool: &PgPool, keep: i64) -> u64 {
    match sqlx::query!(
        "DELETE FROM twitch_engagement_conversation WHERE id IN (\
           SELECT id FROM (\
             SELECT id, ROW_NUMBER() OVER (\
               PARTITION BY channel_login ORDER BY ts DESC) AS rn \
             FROM twitch_engagement_conversation) ranked \
           WHERE rn > $1)",
        keep
    )
    .execute(pool)
    .await
    {
        Ok(result) => result.rows_affected(),
        Err(error) => {
            tracing::warn!(%error, keep, "Engagement-Conversation-Trim fehlgeschlagen");
            0
        }
    }
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
    let Some(segment) = capture_transcript_segment(channel, capturer, transcriber).await else {
        return;
    };
    if let Err(error) = StreamTranscripts::new(pool.clone()).append_segment(&segment).await {
        tracing::warn!(
            %error,
            channel,
            "stream-transcripts: Segment konnte nicht gespeichert werden"
        );
    }
}

/// Nimmt einen Block Stream-Audio auf und transkribiert ihn lokal. `None`, wenn
/// Capture, Transkription oder Text ausfallen; alles davon ist best-effort und
/// ein stiller Block ist ein normales Ergebnis, kein Fehler.
///
/// Getrennt vom Schreiben, weil derselbe Block an mehreren Stellen gebraucht
/// wird: der Prompt-Ringpuffer will ihn, die Smalltalk-Auswertung will ihn
/// dauerhaft, und keiner der beiden soll dafür eine zweite Aufnahme fahren.
pub async fn capture_transcript_segment(
    channel: &str,
    capturer: &AudioCapturer,
    transcriber: &OpenAiTranscriber,
) -> Option<StreamTranscriptSegment> {
    let capture = match capturer
        .capture(channel, transcript_capture_seconds().max(0) as u64, &transcript_quality(), None)
        .await
    {
        Ok(c) => c,
        Err(error) => {
            tracing::debug!(channel, %error, "stream-transcript: Capture fehlgeschlagen");
            return None;
        }
    };
    let transcription = transcriber.transcribe_clip(&capture.media_path).await;
    capture.cleanup().await; // entspricht Pythons finally
    let result = match transcription {
        Ok(r) => r,
        Err(error) => {
            tracing::debug!(channel, %error, "stream-transcript: Transkription fehlgeschlagen");
            return None;
        }
    };

    let text = result.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
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
    Some(StreamTranscriptSegment {
        channel_login: channel.to_string(),
        started_at,
        ended_at,
        text,
        engine: result.engine,
        model: Some(result.model).filter(|m| !m.is_empty()),
    })
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
        trim_conversation(&pool, CONVERSATION_KEEP_PER_CHANNEL).await;
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
/// enabled Channel ein streamlink-Capture + Whisper-Transkription (lokal, siehe
/// [`OpenAiTranscriber::from_env`]), dazu periodisches Trimmen. Aus (Env-Flag)
/// oder ohne baubaren HTTP-Client → still im Poll-Takt warten und retryen.
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
                    if last_trim.is_none_or(|t| t.elapsed() >= TRANSCRIPT_TRIM_INTERVAL) {
                        last_trim = Some(Instant::now());
                        StreamTranscripts::new(pool.clone()).trim_segments(None, None).await;
                    }
                }
                None => {
                    tracing::debug!("stream-transcripts: Transcriber nicht verfügbar")
                }
            }
        }
        jittered_sleep(transcript_poll_interval_seconds()).await;
    }
}

// ---- Reaktions-Lernmodus ----------------------------------------------------

/// Nimmt einen Block Audio auf und legt das Whisper-Segment im Lern-Archiv ab.
///
/// Anders als [`run_transcribe_capture`] wird die Segmentzeit aus dem
/// Capture-Start gerechnet, nicht aus dem Zeitpunkt nach der Transkription: das
/// Mapping stellt Nachricht und Stream-Moment sekundengenau gegenüber, und die
/// Whisper-Laufzeit würde jedes Segment sonst um ihre eigene Dauer nach hinten
/// verschieben.
async fn run_learn_capture(
    learn: &ReactionLearning,
    channel: &str,
    capturer: &AudioCapturer,
    transcriber: &OpenAiTranscriber,
) {
    let started_at = Utc::now();
    let capture = match capturer
        .capture(channel, learn_capture_seconds().max(0) as u64, &transcript_quality(), None)
        .await
    {
        Ok(c) => c,
        Err(error) => {
            tracing::debug!(channel, %error, "learn-capture: Capture fehlgeschlagen");
            return;
        }
    };
    let transcription = transcriber.transcribe_clip(&capture.media_path).await;
    capture.cleanup().await;
    let result = match transcription {
        Ok(r) => r,
        Err(error) => {
            tracing::debug!(channel, %error, "learn-capture: Transkription fehlgeschlagen");
            return;
        }
    };
    let text = result.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return; // stille oder reine Spielsound-Passage
    }
    let duration = if result.duration_seconds > 0.0 {
        result.duration_seconds
    } else if capture.actual_duration_seconds > 0.0 {
        capture.actual_duration_seconds
    } else {
        capture.requested_duration_seconds as f64
    };
    let segment = LearnTranscriptSegment {
        channel_login: channel.to_string(),
        started_at,
        ended_at: started_at + chrono::Duration::seconds(duration.max(1.0) as i64),
        text,
        engine: result.engine,
        model: Some(result.model).filter(|m| !m.is_empty()),
    };
    if let Err(error) = learn.append_transcript(&segment).await {
        tracing::warn!(%error, channel, "learn-capture: Segment nicht gespeichert");
    }
}

/// Nimmt einen Kanal am Stück auf, solange er aufgenommen werden soll.
async fn learn_channel_loop(
    learn: Arc<ReactionLearning>,
    channel: String,
    transcriber: Arc<OpenAiTranscriber>,
    running: Arc<Mutex<HashSet<String>>>,
) {
    let capturer = AudioCapturer::from_env();
    tracing::info!(channel = %channel, "learn-capture: Aufnahme gestartet");
    while learn.should_capture(&channel).await {
        run_learn_capture(&learn, &channel, &capturer, &transcriber).await;
    }
    running.lock().unwrap_or_else(|p| p.into_inner()).remove(&channel);
    tracing::info!(channel = %channel, "learn-capture: Aufnahme beendet (Stream aus oder ausgekühlt)");
}

/// Supervisor: startet je aufzunehmendem Kanal einen eigenen Aufnahme-Task.
///
/// Ein Task pro Kanal statt einer gemeinsamen Runde, weil die Blöcke sonst
/// reihum liefen und jeder Kanal nur einen Bruchteil der Zeit aufgenommen
/// würde. Genau die Lücken dazwischen wären die Momente, auf die reagiert wird.
///
/// Partner-Kanäle werden ab Stream-Beginn aufgenommen, nicht erst wenn der
/// Owner auftaucht: taucht er später auf, ist der Verlauf davor sonst weg.
pub async fn schedule_learn_capture(learn: Arc<ReactionLearning>) {
    learn.warm_cache().await;
    let running: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut transcriber: Option<Arc<OpenAiTranscriber>> = None;
    loop {
        if transcriber.is_none() {
            transcriber = OpenAiTranscriber::from_env().map(Arc::new);
        }
        match &transcriber {
            Some(t) => {
                let channels = learn.capture_channels().await;
                // Der Chat-Pfad schreibt in genau diesen Kanälen mit.
                learn.set_recording(&channels);
                for channel in channels {
                    {
                        let mut guard = running.lock().unwrap_or_else(|p| p.into_inner());
                        if !guard.insert(channel.clone()) {
                            continue; // läuft schon
                        }
                    }
                    tokio::spawn(learn_channel_loop(
                        Arc::clone(&learn),
                        channel,
                        Arc::clone(t),
                        Arc::clone(&running),
                    ));
                }
            }
            None => tracing::warn!("learn-capture: HTTP-Client nicht baubar, keine Transkription"),
        }
        jittered_sleep(LEARN_SUPERVISOR_INTERVAL).await;
    }
}

/// Mappt fällige eigene Nachrichten auf ihren Kontext und räumt Rohdaten auf.
pub async fn schedule_learn_mapper(learn: Arc<ReactionLearning>) {
    let mut last_trim: Option<Instant> = None;
    loop {
        let created = learn.map_pending().await;
        if created > 0 {
            let total = learn.sample_count().await;
            tracing::info!(created, total, "learn-mapper: neue Reaktions-Samples");
        }
        if last_trim.is_none_or(|t| t.elapsed() >= LEARN_TRIM_INTERVAL) {
            last_trim = Some(Instant::now());
            learn.trim().await;
        }
        jittered_sleep(LEARN_MAPPER_INTERVAL).await;
    }
}

/// Destilliert periodisch das Reaktionsprofil aus den gesammelten Samples.
pub async fn schedule_learn_profile(learn: Arc<ReactionLearning>) {
    let minimax = ai_client();
    loop {
        // Erst warten: direkt nach dem Start gibt es garantiert nichts Neues
        // zu destillieren, und ein Lauf kostet einen Modell-Call.
        jittered_sleep(LEARN_PROFILE_INTERVAL).await;
        learn.distill_profile(&minimax).await;
    }
}

/// Startet die Loops des Lernmodus. No-op, wenn er aus ist.
pub fn spawn_learn_jobs(learn: Arc<ReactionLearning>) {
    if !learn_enabled() {
        tracing::info!("Engagement-Reaktions-Lernmodus deaktiviert");
        return;
    }
    tracing::info!(
        owner = %learn.owner_login(),
        "Engagement-Reaktions-Lernmodus aktiv (Aufnahme, Mapping, Profil)"
    );
    spawn_logged("engagement_learn_capture", schedule_learn_capture(Arc::clone(&learn)));
    spawn_logged("engagement_learn_mapper", schedule_learn_mapper(Arc::clone(&learn)));
    spawn_logged("engagement_learn_profile", schedule_learn_profile(learn));
}

/// Spawnt alle acht Background-Loops als tokio-Tasks (Python `ensure_started`).
pub fn spawn_all(pool: PgPool) {
    spawn_logged("engagement_thread_extractor", schedule_thread_extractor(pool.clone()));
    spawn_logged("engagement_match_poller", schedule_match_poller(pool.clone()));
    spawn_logged("engagement_auto_closer", schedule_auto_closer(pool.clone()));
    spawn_logged("engagement_conversation_trim", schedule_conversation_trim(pool.clone()));
    spawn_logged("engagement_global_sentiment", schedule_global_sentiment(pool.clone()));
    spawn_logged("engagement_soul_anchor", schedule_soul_anchor(pool.clone()));
    spawn_logged("engagement_channel_profile", schedule_channel_profile(pool.clone()));
    if stream_transcripts_enabled() {
        spawn_logged("engagement_stream_transcripts", schedule_stream_transcripts(pool));
    } else {
        tracing::info!("Engagement-Stream-Transkription deaktiviert");
    }
    tracing::info!(
        "Engagement-Background-Jobs gestartet (thread-extractor=15min, match-poller=30s, \
         auto-closer=1h, conv-trim=24h, global-sentiment=20min, soul-anchor=3h, \
         channel-profile=4h, stream-transcripts=poll)"
    );
}

fn spawn_logged<F>(task: &'static str, future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(future);
    tokio::spawn(async move {
        if let Err(error) = handle.await {
            tracing::error!(task, %error, "Engagement-Background-Task unerwartet beendet");
        }
    });
}

#[cfg(test)]
mod gate_tests {
    use super::stream_transcripts_enabled;

    #[test]
    fn transkription_ist_ohne_explizites_opt_in_aus() {
        unsafe {
            std::env::remove_var("ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED");
        }
        assert!(!stream_transcripts_enabled());
    }
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
