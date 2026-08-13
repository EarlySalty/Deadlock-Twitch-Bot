//! Reaktions-Lernmodus: aufzeichnen, WORAUF der Owner im Chat reagiert und WIE.
//!
//! Die KI bekommt ihren Grundgeschmack bisher aus einem handgepflegten
//! Gold-Register in [`crate::style_examples`]. Dieses Modul ersetzt das Raten
//! durch Beobachtung: jede eigene Chat-Nachricht wird mit dem Stream-Audio der
//! Sekunden davor und den Chat-Zeilen davor zu einem Stimulus/Response-Paar
//! verknüpft.
//!
//! Ablauf:
//! 1. [`ReactionLearning::observe`] hängt an jeder eingehenden Chat-Nachricht,
//!    noch vor allen Partner-/Engagement-Gates. Schreibt der Owner in einem
//!    Kanal, wird der Kanal für [`hot_ttl_minutes`] „lern-heiß": ab da wandert
//!    dort jede Chat-Zeile in den Lern-Puffer und die Audio-Aufnahme startet.
//! 2. Der Capture-Loop in [`crate::background`] nimmt lern-heiße Kanäle
//!    lückenlos auf und legt Whisper-Segmente ab.
//! 3. [`ReactionLearning::map_pending`] koppelt beides: pro eigener Nachricht
//!    das Transkript-Fenster davor plus die letzten Chat-Zeilen.
//!
//! Bewusst getrennt von [`crate::stream_transcripts`]: die operative Tabelle
//! ist flüchtig (60 min) und läuft nur für engagement-aktive Partner-Kanäle,
//! der Lernmodus braucht beliebige Kanäle und längere Haltbarkeit.
//!
//! Der schnelle Pfad zählt: `observe` läuft auf JEDER Chat-Nachricht des Bots.
//! Ist der Kanal nicht lern-heiß und der Absender nicht der Owner, kostet der
//! Aufruf einen Lookup in einer Map und sonst nichts.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use crate::minimax_chat::EngagementMinimaxClient;

/// Default-Owner: der Chat-Account, dessen Reaktionen gelernt werden.
const DEFAULT_OWNER_LOGIN: &str = "earlysalty";
/// So lange nach der letzten eigenen Nachricht bleibt ein Kanal lern-heiß.
const DEFAULT_HOT_TTL_MINUTES: i64 = 45;
/// Transkript-Fenster vor der eigenen Nachricht.
const DEFAULT_WINDOW_PRE_SECONDS: i64 = 45;
/// Transkript-Fenster nach der eigenen Nachricht (der Streamer redet weiter).
const DEFAULT_WINDOW_POST_SECONDS: i64 = 10;
/// So viele Chat-Zeilen vor der eigenen Nachricht als Umgebung.
const DEFAULT_CHAT_CONTEXT_LINES: i64 = 8;
/// So weit zurück dürfen diese Chat-Zeilen liegen.
const DEFAULT_CHAT_CONTEXT_MINUTES: i64 = 4;
/// Aufbewahrung des Zeitstrahls (Samples bleiben unberührt). Eine Woche, weil
/// Text billig ist und ein gebündelter Verlauf erst über mehrere Sitzungen
/// hinweg etwas hergibt, das ein einzelnes Sample nicht zeigt.
const DEFAULT_RETENTION_HOURS: i64 = 168;
/// Capture-Länge je Aufnahmeblock.
const DEFAULT_CAPTURE_SECONDS: i64 = 30;
/// So viele Kanäle gleichzeitig aufnehmen.
///
/// Die Grenze ist der Whisper-Dienst, nicht die Bandbreite: er verarbeitet
/// Anfragen nacheinander und braucht rund 7 s je 30-Sekunden-Block. Drei
/// Kanäle erzeugen alle 10 s eine Anfrage und lasten ihn damit gut aus; bei
/// mehr stauen sich die Blöcke und der Zeitstrahl bekommt Löcher.
const DEFAULT_MAX_CAPTURE_CHANNELS: usize = 3;
/// Nachrichten erst mappen, wenn das Fenster danach transkribiert sein kann.
const MAP_LAG_EXTRA_SECONDS: i64 = 45;
/// Obergrenze je Mapper-Durchlauf.
const MAP_BATCH: i64 = 200;
/// Kontext-Texte im Sample kappen.
const MAX_CONTEXT_CHARS: usize = 2000;
/// So viele Samples sieht die Destillation an.
const DISTILL_SAMPLE_LIMIT: i64 = 60;
/// Darunter lohnt sich kein Profil.
const MIN_SAMPLES_FOR_PROFILE: usize = 15;
/// Längere Modell-Antworten sind kein Profil mehr, sondern ein Aufsatz.
const MAX_PROFILE_CHARS: usize = 1800;
/// Kontext-Zeilen je Sample im Destillat-Prompt.
const DISTILL_CONTEXT_CHARS: usize = 400;

const PROFILE_SYS: &str = "Du analysierst das Chat-Verhalten einer einzelnen Person und \
beschreibst es knapp und konkret. Keine Floskeln, keine Einleitung, keine Zusammenfassung \
am Ende.";

/// Rendert Samples als „Situation → Reaktion"-Blöcke für die Destillation.
fn render_samples(samples: &[ReactionSample]) -> String {
    samples
        .iter()
        .rev()
        .enumerate()
        .map(|(i, s)| {
            let stream = truncate_chars(&s.stream_context, DISTILL_CONTEXT_CHARS);
            let chat = truncate_chars(&s.chat_context, DISTILL_CONTEXT_CHARS);
            let mut block = format!("--- {} ---\n", i + 1);
            if !stream.is_empty() {
                block.push_str(&format!("Streamer sagt gerade:\n{stream}\n"));
            }
            if !chat.is_empty() {
                block.push_str(&format!("Chat davor:\n{chat}\n"));
            }
            block.push_str(&format!("ER SCHREIBT: {}", s.my_message));
            block
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn truncate_chars(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + " …"
}

fn profile_user_prompt(samples: &str) -> String {
    format!(
        "Unten stehen echte Situationen aus Twitch-Chats und die Zeile, die eine bestimmte \
         Person daraufhin geschrieben hat. Leite daraus ihr Reaktionsmuster ab.\n\n\
         Schreib maximal 12 knappe Stichpunkte, aufgeteilt in:\n\
         WORAUF ER REAGIERT: welche Art von Moment ihn zum Schreiben bringt.\n\
         WORAUF NICHT: was in den Situationen offensichtlich vorkam, ohne dass er \
         reagiert hat.\n\
         WIE: Länge, Tonfall, typische Satzform, was er nie tut.\n\n\
         Nur was du an den Beispielen wirklich siehst. Erfinde nichts dazu, und schreib \
         keine allgemeinen Chat-Weisheiten hin.\n\n{samples}"
    )
}

fn env_str(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_int(name: &str, default: i64, minimum: i64) -> i64 {
    match env_str(name).and_then(|v| v.parse::<i64>().ok()) {
        Some(v) => v.max(minimum),
        None => default,
    }
}

/// Lernmodus aktiv? Default aus — er nimmt Audio auf und muss bewusst an.
pub fn learn_enabled() -> bool {
    env_str("ENGAGEMENT_LEARN_ENABLED").as_deref() == Some("1")
}

/// Login, dessen Reaktionen gelernt werden (`ENGAGEMENT_LEARN_LOGIN`).
pub fn owner_login() -> String {
    env_str("ENGAGEMENT_LEARN_LOGIN")
        .unwrap_or_else(|| DEFAULT_OWNER_LOGIN.to_string())
        .to_lowercase()
}

/// Wie lange ein Kanal nach der letzten eigenen Nachricht lern-heiß bleibt.
pub fn hot_ttl_minutes() -> i64 {
    env_int("ENGAGEMENT_LEARN_HOT_MINUTES", DEFAULT_HOT_TTL_MINUTES, 1)
}

/// Capture-Länge je Aufnahmeblock (Sekunden).
pub fn capture_seconds() -> i64 {
    env_int("ENGAGEMENT_LEARN_CAPTURE_SECONDS", DEFAULT_CAPTURE_SECONDS, 10)
}

/// So viele lern-heiße Kanäle werden parallel aufgenommen.
pub fn max_capture_channels() -> usize {
    env_int("ENGAGEMENT_LEARN_MAX_CHANNELS", DEFAULT_MAX_CAPTURE_CHANNELS as i64, 1) as usize
}

/// Aufbewahrung der Rohdaten (Chat-Puffer, Transkripte) in Stunden.
pub fn retention_hours() -> i64 {
    env_int("ENGAGEMENT_LEARN_RETENTION_HOURS", DEFAULT_RETENTION_HOURS, 1)
}

/// Ein Whisper-Segment aus einem lern-heißen Kanal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnTranscriptSegment {
    pub channel_login: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub text: String,
    pub engine: String,
    pub model: Option<String>,
}

/// Eine Zeile des gebündelten Zeitstrahls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub ts: DateTime<Utc>,
    /// `stream`, `chat` oder `own`.
    pub kind: String,
    pub author: Option<String>,
    pub content: String,
}

/// Ein fertiges Stimulus/Response-Paar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionSample {
    pub id: i64,
    pub channel_login: String,
    pub message_ts: DateTime<Utc>,
    pub my_message: String,
    pub stream_context: String,
    pub chat_context: String,
    pub has_stream_context: bool,
    pub verdict: Option<String>,
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Kappt einen Kontext-Text auf [`MAX_CONTEXT_CHARS`], vom Anfang her (das
/// Jüngste steht hinten und ist das Relevantere).
fn cap_context(text: &str) -> String {
    let len = text.chars().count();
    if len <= MAX_CONTEXT_CHARS {
        return text.to_string();
    }
    let tail: String = text.chars().skip(len - MAX_CONTEXT_CHARS).collect();
    match tail.split_once('\n') {
        Some((_, rest)) => rest.to_string(),
        None => tail,
    }
}

/// Baut den Stream-Kontext: eine Zeile je Segment, chronologisch, mit dem
/// Sekunden-Versatz zur eigenen Nachricht (negativ = davor gesagt).
pub fn build_stream_context(
    segments: &[(DateTime<Utc>, String)],
    message_ts: DateTime<Utc>,
) -> String {
    let lines: Vec<String> = segments
        .iter()
        .filter_map(|(ended_at, text)| {
            let text = normalize_ws(text);
            if text.is_empty() {
                return None;
            }
            let offset = (*ended_at - message_ts).num_seconds();
            Some(format!("[{offset:+}s] {text}"))
        })
        .collect();
    cap_context(&lines.join("\n"))
}

/// Baut den Chat-Kontext: `login: text` je Zeile, chronologisch.
pub fn build_chat_context(lines: &[(String, String)]) -> String {
    let rendered: Vec<String> = lines
        .iter()
        .filter_map(|(login, content)| {
            let content = normalize_ws(content);
            if content.is_empty() {
                None
            } else {
                Some(format!("{login}: {content}"))
            }
        })
        .collect();
    cap_context(&rendered.join("\n"))
}

/// Erfassung und Verknüpfung der eigenen Chat-Reaktionen.
pub struct ReactionLearning {
    pool: PgPool,
    owner_login: String,
    /// Einmal beim Start gelesen: [`observe`](Self::observe) hängt im heißen
    /// Chat-Pfad und darf nicht pro Nachricht die Env befragen.
    enabled: bool,
    /// Kanal → letzte eigene Nachricht. Hält den Fast-Path DB-frei.
    hot: Mutex<HashMap<String, DateTime<Utc>>>,
    /// Kanäle, die gerade aufgenommen werden. Vom Capture-Supervisor gepflegt,
    /// damit [`observe`](Self::observe) den Chat dieser Kanäle mitschreibt,
    /// ohne pro Nachricht die Datenbank zu fragen. Ohne das gäbe es in
    /// Partner-Kanälen Stream-Ton ohne den Chat, der dazu lief.
    recording: Mutex<HashSet<String>>,
}

impl ReactionLearning {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            owner_login: owner_login(),
            enabled: learn_enabled(),
            hot: Mutex::new(HashMap::new()),
            recording: Mutex::new(HashSet::new()),
        }
    }

    /// Setzt Owner-Login und schaltet die Erfassung scharf (Tests).
    pub fn with_owner(mut self, login: &str) -> Self {
        self.owner_login = login.to_lowercase();
        self.enabled = true;
        self
    }

    /// Läuft der Lernmodus?
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn owner_login(&self) -> &str {
        &self.owner_login
    }

    fn ttl(&self) -> Duration {
        Duration::minutes(hot_ttl_minutes())
    }

    /// Kanäle, die gerade aufgenommen werden sollen: jüngste eigene Nachricht
    /// zuerst, auf [`max_capture_channels`] begrenzt.
    pub fn hot_channels(&self) -> Vec<String> {
        let cutoff = Utc::now() - self.ttl();
        let mut hot = self.hot.lock().unwrap_or_else(|p| p.into_inner());
        hot.retain(|_, seen| *seen >= cutoff);
        let mut entries: Vec<(String, DateTime<Utc>)> =
            hot.iter().map(|(c, t)| (c.clone(), *t)).collect();
        entries.sort_by_key(|(_, seen)| std::cmp::Reverse(*seen));
        entries.into_iter().take(max_capture_channels()).map(|(c, _)| c).collect()
    }

    /// Kanäle, die JETZT aufgenommen werden sollen.
    ///
    /// Zwei Quellen, und die erste ist die wichtigere:
    /// - **Partner-Kanäle, die gerade live Deadlock streamen** — dort wird vom
    ///   Stream-Anfang an aufgenommen, unabhängig davon, ob der Owner schon da
    ///   ist. Nur so ist der Verlauf vollständig, wenn er später dazukommt.
    /// - **Fremde Kanäle, in denen der Owner gesichtet wurde** — dort geht es
    ///   nicht früher, weil man erst durch seine Nachricht erfährt, dass er
    ///   zuschaut.
    ///
    /// Beide Sorten werden gegen `twitch_live_state` geprüft: ist der Stream
    /// vorbei, endet die Aufnahme sofort, statt bis zum Ablauf der Nachlaufzeit
    /// gegen einen toten Kanal zu laufen.
    pub async fn capture_channels(&self) -> Vec<String> {
        let seen = self.hot_channels();
        let mut live = self.live_partner_channels().await;
        for channel in seen {
            if !live.contains(&channel) {
                live.push(channel);
            }
        }
        let online = self.filter_live(&live).await;
        online.into_iter().take(max_capture_channels()).collect()
    }

    /// Partner-Kanäle mit eingeschaltetem Engagement, die live Deadlock streamen.
    async fn live_partner_channels(&self) -> Vec<String> {
        sqlx::query_scalar!(
            r#"SELECT LOWER(s.channel_login) AS "channel_login!"
               FROM twitch_engagement_settings s
               JOIN twitch_live_state l
                 ON LOWER(l.streamer_login) = LOWER(s.channel_login)
               WHERE s.enabled = TRUE
                 AND COALESCE(l.is_live, 0) <> 0
                 AND LOWER(TRIM(COALESCE(l.last_game, ''))) = 'deadlock'"#
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    /// Behält nur Kanäle, die laut `twitch_live_state` gerade live sind.
    ///
    /// Kanäle ohne Eintrag bleiben drin: der Scout kennt nur deutschsprachige
    /// Deadlock-Streams, und ein Kanal, in dem der Owner nachweislich gerade
    /// schreibt, soll nicht daran scheitern, dass er dort nicht gelistet ist.
    async fn filter_live(&self, channels: &[String]) -> Vec<String> {
        if channels.is_empty() {
            return Vec::new();
        }
        let offline: Vec<String> = sqlx::query_scalar!(
            r#"SELECT LOWER(streamer_login) AS "login!"
               FROM twitch_live_state
               WHERE LOWER(streamer_login) = ANY($1) AND COALESCE(is_live, 0) = 0"#,
            channels
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        channels.iter().filter(|c| !offline.contains(c)).cloned().collect()
    }

    /// Soll dieser Kanal gerade aufgenommen werden? Prüft dieselben Quellen
    /// wie [`capture_channels`], damit ein laufender Aufnahme-Task endet,
    /// sobald der Stream aus ist oder der Kanal auskühlt.
    pub async fn should_capture(&self, channel_login: &str) -> bool {
        let channel = channel_login.trim().to_lowercase();
        self.capture_channels().await.contains(&channel)
    }

    /// Nimmt der Lernmodus diesen Kanal gerade auf?
    pub fn is_channel_hot(&self, channel_login: &str) -> bool {
        self.is_hot(&channel_login.trim().to_lowercase())
    }

    /// Merkt sich, welche Kanäle gerade aufgenommen werden. Nur so weiß der
    /// Chat-Pfad, dass er dort mitschreiben soll, ohne die Datenbank zu fragen.
    pub fn set_recording(&self, channels: &[String]) {
        let mut recording = self.recording.lock().unwrap_or_else(|p| p.into_inner());
        *recording = channels.iter().cloned().collect();
    }

    fn is_recording(&self, channel_login: &str) -> bool {
        self.recording
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(channel_login)
    }

    fn is_hot(&self, channel_login: &str) -> bool {
        let cutoff = Utc::now() - self.ttl();
        self.hot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(channel_login)
            .is_some_and(|seen| *seen >= cutoff)
    }

    fn mark_hot(&self, channel_login: &str, at: DateTime<Utc>) {
        self.hot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(channel_login.to_string(), at);
    }

    /// Lädt noch heiße Kanäle nach einem Neustart zurück in den Cache.
    pub async fn warm_cache(&self) {
        let cutoff = Utc::now() - self.ttl();
        let rows = sqlx::query!(
            r#"SELECT channel_login AS "channel_login!", last_seen_at AS "last_seen_at!"
               FROM twitch_engagement_learn_channels
               WHERE last_seen_at >= $1"#,
            cutoff
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        for row in rows {
            self.mark_hot(&row.channel_login, row.last_seen_at);
        }
    }

    /// Hook für jede eingehende Chat-Nachricht, vor allen Engagement-Gates.
    ///
    /// Owner-Nachricht → Kanal wird lern-heiß, Nachricht wird als Response
    /// vorgemerkt. Fremde Nachricht in einem heißen Kanal → Umgebungs-Puffer.
    /// Alles andere → no-op ohne DB-Zugriff.
    pub async fn observe(
        &self,
        channel_login: &str,
        channel_user_id: Option<&str>,
        twitch_login: &str,
        content: &str,
        message_id: Option<&str>,
    ) {
        if !self.enabled {
            return;
        }
        let channel = channel_login.trim().to_lowercase();
        let login = twitch_login.trim().to_lowercase();
        let text = normalize_ws(content);
        if channel.is_empty() || login.is_empty() || text.is_empty() {
            return;
        }
        let is_owner = login == self.owner_login;
        // Fremde Zeilen zählen, wenn der Kanal aufgenommen wird — sonst gäbe es
        // in Partner-Kanälen Stream-Ton ohne den Chat, der dazu lief.
        if !is_owner && !self.is_hot(&channel) && !self.is_recording(&channel) {
            return;
        }

        if is_owner {
            self.mark_hot(&channel, Utc::now());
            if let Err(error) = self.touch_channel(&channel, channel_user_id).await {
                tracing::warn!(%error, channel = %channel, "learn: Kanal-Upsert fehlgeschlagen");
            }
        }
        // Eigene Zeilen stehen als 'own' im selben Zeitstrahl wie fremde. Sie
        // sind Response UND Umgebung des nächsten Turns — zweimal speichern
        // hiesse, sie beim Aufräumen zweimal treffen zu müssen.
        let kind = if is_owner { "own" } else { "chat" };
        if let Err(error) = self.append_chat_entry(&channel, kind, &login, &text, message_id).await {
            tracing::warn!(%error, channel = %channel, kind, "learn: Timeline-Insert fehlgeschlagen");
        }
    }

    async fn touch_channel(
        &self,
        channel_login: &str,
        channel_user_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO twitch_engagement_learn_channels \
             (channel_login, channel_user_id, message_count) \
             VALUES ($1, $2, 1) \
             ON CONFLICT (channel_login) DO UPDATE SET \
               last_seen_at = CURRENT_TIMESTAMP, \
               message_count = twitch_engagement_learn_channels.message_count + 1, \
               channel_user_id = COALESCE(EXCLUDED.channel_user_id, \
                                          twitch_engagement_learn_channels.channel_user_id)",
            channel_login,
            channel_user_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Hängt eine Chat-Zeile an den Zeitstrahl (`kind` = `chat` oder `own`).
    ///
    /// Dieselbe Nachricht kann zweimal ankommen: der EventSub-Hook und der
    /// Lern-IRC-Reader überschneiden sich in Partner-Kanälen, die live Deadlock
    /// streamen. Die Twitch-Message-ID ist in beiden Pfaden dieselbe, der
    /// zweite Weg fällt darum still weg.
    async fn append_chat_entry(
        &self,
        channel_login: &str,
        kind: &str,
        twitch_login: &str,
        content: &str,
        message_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, author, content, message_id) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (channel_login, message_id) WHERE message_id IS NOT NULL DO NOTHING",
            channel_login,
            kind,
            twitch_login,
            content,
            message_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Hängt ein Whisper-Segment an den Zeitstrahl (leerer Text → kein Insert).
    ///
    /// `ts` ist das Segment-ENDE, nicht der Anfang: dann war der Satz zu Ende
    /// gesprochen, und genau darauf reagiert jemand.
    pub async fn append_transcript(
        &self,
        segment: &LearnTranscriptSegment,
    ) -> Result<(), sqlx::Error> {
        let text = normalize_ws(&segment.text);
        if text.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, ts, started_at, content, engine, model) \
             VALUES ($1, 'stream', $2, $3, $4, $5, $6)",
            &segment.channel_login,
            segment.ended_at,
            segment.started_at,
            &text,
            &segment.engine,
            segment.model.as_deref()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Verknüpft fällige eigene Nachrichten mit ihrem Kontext. Liefert die
    /// Anzahl neu entstandener Samples.
    pub async fn map_pending(&self) -> u64 {
        let pre = Duration::seconds(env_int(
            "ENGAGEMENT_LEARN_WINDOW_PRE_SECONDS",
            DEFAULT_WINDOW_PRE_SECONDS,
            1,
        ));
        let post = Duration::seconds(env_int(
            "ENGAGEMENT_LEARN_WINDOW_POST_SECONDS",
            DEFAULT_WINDOW_POST_SECONDS,
            0,
        ));
        // Erst mappen, wenn das Fenster nach der Nachricht aufgenommen UND
        // transkribiert sein kann. Sonst entstünde ein Sample ohne die zweite
        // Hälfte seines Kontexts, und ein zweiter Versuch gibt es nicht.
        let lag = post + Duration::seconds(capture_seconds() + MAP_LAG_EXTRA_SECONDS);
        let ready_before = Utc::now() - lag;

        let pending = sqlx::query!(
            r#"SELECT id AS "id!", channel_login AS "channel_login!", content AS "content!",
                      ts AS "ts!"
               FROM twitch_engagement_learn_timeline
               WHERE kind = 'own' AND mapped_at IS NULL AND ts <= $1
               ORDER BY ts LIMIT $2"#,
            ready_before,
            MAP_BATCH
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let mut created = 0u64;
        for row in pending {
            let stream = self
                .load_stream_window(&row.channel_login, row.ts - pre, row.ts + post)
                .await;
            let stream_context = build_stream_context(&stream, row.ts);
            let chat = self.load_chat_window(&row.channel_login, row.ts).await;
            let chat_context = build_chat_context(&chat);

            let inserted = sqlx::query!(
                "INSERT INTO twitch_engagement_reaction_samples \
                 (channel_login, message_ts, my_message, stream_context, chat_context, \
                  has_stream_context) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT ON CONSTRAINT twitch_engagement_reaction_samples_unique \
                 DO NOTHING",
                &row.channel_login,
                row.ts,
                &row.content,
                &stream_context,
                &chat_context,
                !stream_context.is_empty()
            )
            .execute(&self.pool)
            .await;

            match inserted {
                Ok(result) => created += result.rows_affected(),
                Err(error) => {
                    tracing::warn!(%error, channel = %row.channel_login, "learn: Sample-Insert fehlgeschlagen");
                    continue; // mapped_at offen lassen, nächster Lauf versucht es erneut
                }
            }
            let _ = sqlx::query!(
                "UPDATE twitch_engagement_learn_timeline \
                 SET mapped_at = CURRENT_TIMESTAMP WHERE id = $1",
                row.id
            )
            .execute(&self.pool)
            .await;
        }
        created
    }

    async fn load_stream_window(
        &self,
        channel_login: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<(DateTime<Utc>, String)> {
        sqlx::query!(
            r#"SELECT ts AS "ts!", content AS "content!"
               FROM twitch_engagement_learn_timeline
               WHERE channel_login = $1 AND kind = 'stream' AND ts >= $2 AND ts <= $3
               ORDER BY ts"#,
            channel_login,
            from,
            to
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.ts, r.content))
        .collect()
    }

    async fn load_chat_window(
        &self,
        channel_login: &str,
        before: DateTime<Utc>,
    ) -> Vec<(String, String)> {
        let lines =
            env_int("ENGAGEMENT_LEARN_CHAT_LINES", DEFAULT_CHAT_CONTEXT_LINES, 1);
        let minutes = env_int(
            "ENGAGEMENT_LEARN_CHAT_MINUTES",
            DEFAULT_CHAT_CONTEXT_MINUTES,
            1,
        );
        // 'own' zählt mit: die eigene vorherige Zeile ist Teil des Verlaufs,
        // auf den die nächste antwortet.
        let rows = sqlx::query!(
            r#"SELECT COALESCE(author, '') AS "author!", content AS "content!"
               FROM twitch_engagement_learn_timeline
               WHERE channel_login = $1 AND kind IN ('chat', 'own')
                 AND ts < $2 AND ts >= $3
               ORDER BY ts DESC LIMIT $4"#,
            channel_login,
            before,
            before - Duration::minutes(minutes),
            lines
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.into_iter().rev().map(|r| (r.author, r.content)).collect()
    }

    /// Der gebündelte Zeitstrahl eines Kanals, chronologisch. Für die Sichtung.
    pub async fn timeline(
        &self,
        channel_login: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Vec<TimelineEntry> {
        let rows = sqlx::query!(
            r#"SELECT ts AS "ts!", kind AS "kind!", author, content AS "content!"
               FROM twitch_engagement_learn_timeline
               WHERE channel_login = $1 AND ts >= $2
               ORDER BY ts DESC LIMIT $3"#,
            channel_login.trim().to_lowercase(),
            since,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.into_iter()
            .rev()
            .map(|r| TimelineEntry {
                ts: r.ts,
                kind: r.kind,
                author: r.author.filter(|a| !a.is_empty()),
                content: r.content,
            })
            .collect()
    }

    /// Die jüngsten Samples (für Sichtung und Few-Shot-Aufbau).
    pub async fn recent_samples(&self, limit: i64, only_with_stream: bool) -> Vec<ReactionSample> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", channel_login AS "channel_login!", message_ts AS "message_ts!",
                      my_message AS "my_message!", stream_context AS "stream_context!",
                      chat_context AS "chat_context!",
                      has_stream_context AS "has_stream_context!", verdict
               FROM twitch_engagement_reaction_samples
               WHERE (NOT $2 OR has_stream_context)
                 AND (verdict IS NULL OR verdict <> 'bad')
               ORDER BY message_ts DESC LIMIT $1"#,
            limit,
            only_with_stream
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.into_iter()
            .map(|r| ReactionSample {
                id: r.id,
                channel_login: r.channel_login,
                message_ts: r.message_ts,
                my_message: r.my_message,
                stream_context: r.stream_context,
                chat_context: r.chat_context,
                has_stream_context: r.has_stream_context,
                verdict: r.verdict,
            })
            .collect()
    }

    /// Wie viele Samples es insgesamt gibt (Fortschritt der Lernphase).
    pub async fn sample_count(&self) -> i64 {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM twitch_engagement_reaction_samples"#
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0)
    }

    /// Destilliert aus den gesammelten Samples ein Reaktionsprofil und legt es
    /// als Soul-Eintrag `reaction_profile` ab. `None`, wenn zu wenig Material
    /// da ist oder das Modell nichts Brauchbares liefert.
    ///
    /// Das Profil beantwortet die Frage, die einzelne Stil-Zeilen nicht
    /// beantworten können: nicht WIE geschrieben wird, sondern WORAUF hin.
    pub async fn distill_profile(
        &self,
        minimax: &EngagementMinimaxClient,
    ) -> Option<String> {
        let samples = self.recent_samples(DISTILL_SAMPLE_LIMIT, true).await;
        if samples.len() < MIN_SAMPLES_FOR_PROFILE {
            tracing::debug!(
                have = samples.len(),
                need = MIN_SAMPLES_FOR_PROFILE,
                "learn-profile: noch zu wenig Samples"
            );
            return None;
        }
        let rendered = render_samples(&samples);
        let raw = minimax
            .raw_completion(PROFILE_SYS, &profile_user_prompt(&rendered), 2000, 0.4)
            .await
            .ok()?;
        let profile = crate::minimax_chat::strip_think(&raw).trim().to_string();
        if profile.is_empty() || profile.chars().count() > MAX_PROFILE_CHARS {
            tracing::warn!(len = profile.chars().count(), "learn-profile: Antwort unbrauchbar");
            return None;
        }
        if let Err(error) = sqlx::query!(
            "INSERT INTO twitch_engagement_soul (kind, content) VALUES ('reaction_profile', $1)",
            &profile
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, "learn-profile: Profil nicht gespeichert");
            return None;
        }
        tracing::info!(
            samples = samples.len(),
            len = profile.chars().count(),
            "learn-profile: neues Reaktionsprofil"
        );
        Some(profile)
    }

    /// Räumt den Zeitstrahl auf. Samples und Kanal-Liste bleiben.
    ///
    /// Noch ungemappte eigene Nachrichten überleben unabhängig vom Alter: sie
    /// zu löschen hiesse, eine Reaktion wegzuwerfen, bevor sie ausgewertet
    /// wurde (etwa nach einem längeren Ausfall des Mappers).
    pub async fn trim(&self) -> u64 {
        let cutoff = Utc::now() - Duration::hours(retention_hours());
        match sqlx::query!(
            "DELETE FROM twitch_engagement_learn_timeline \
             WHERE created_at < $1 AND NOT (kind = 'own' AND mapped_at IS NULL)",
            cutoff
        )
        .execute(&self.pool)
        .await
        {
            Ok(result) => result.rows_affected(),
            Err(error) => {
                tracing::warn!(%error, "learn: Trim fehlgeschlagen");
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn stream_context_zeigt_versatz() {
        let msg = ts(100);
        let segments = vec![
            (ts(70), "der geht da rein".to_string()),
            (ts(95), "  boah   was  ".to_string()),
            (ts(105), "".to_string()), // leer → raus
        ];
        let out = build_stream_context(&segments, msg);
        assert_eq!(out, "[-30s] der geht da rein\n[-5s] boah was");
    }

    #[test]
    fn stream_context_leer_bleibt_leer() {
        assert_eq!(build_stream_context(&[], ts(0)), "");
        assert_eq!(build_stream_context(&[(ts(0), "   ".to_string())], ts(0)), "");
    }

    #[test]
    fn chat_context_rendert_login_und_text() {
        let lines = vec![
            ("chatterA".to_string(), "lol".to_string()),
            ("chatterB".to_string(), "  was war das  ".to_string()),
            ("chatterC".to_string(), "  ".to_string()), // leer → raus
        ];
        assert_eq!(build_chat_context(&lines), "chatterA: lol\nchatterB: was war das");
    }

    #[test]
    fn kontext_wird_gekappt() {
        let long = "x".repeat(MAX_CONTEXT_CHARS + 500);
        let capped = cap_context(&format!("erste zeile\n{long}"));
        assert!(capped.chars().count() <= MAX_CONTEXT_CHARS);
        assert!(!capped.contains("erste zeile"));
    }

    fn sample(msg: &str, stream: &str, chat: &str) -> ReactionSample {
        ReactionSample {
            id: 1,
            channel_login: "nani".to_string(),
            message_ts: ts(0),
            my_message: msg.to_string(),
            stream_context: stream.to_string(),
            chat_context: chat.to_string(),
            has_stream_context: !stream.is_empty(),
            verdict: None,
        }
    }

    #[test]
    fn samples_werden_chronologisch_gerendert() {
        // recent_samples liefert neueste zuerst — im Prompt soll die Zeitachse
        // wieder vorwärts laufen, sonst liest das Modell die Entwicklung falsch.
        let samples = vec![
            sample("das neuere", "streamer neu", ""),
            sample("das aeltere", "streamer alt", "chatter: hi"),
        ];
        let out = render_samples(&samples);
        assert!(out.find("das aeltere").unwrap() < out.find("das neuere").unwrap());
        assert!(out.contains("Streamer sagt gerade:\nstreamer alt"));
        assert!(out.contains("Chat davor:\nchatter: hi"));
        assert!(out.contains("ER SCHREIBT: das neuere"));
        // Fehlender Chat-Kontext erzeugt keine leere Überschrift.
        assert_eq!(out.matches("Chat davor:").count(), 1);
    }

    #[test]
    fn lange_kontexte_werden_im_prompt_gekuerzt() {
        let long = "wort ".repeat(500);
        let out = render_samples(&[sample("kurz", &long, "")]);
        assert!(out.contains('…'));
        assert!(out.chars().count() < long.chars().count());
    }

    #[test]
    fn profil_prompt_fragt_nach_dem_nicht_reagieren() {
        let prompt = profile_user_prompt("--- 1 ---\nER SCHREIBT: hi");
        assert!(prompt.contains("WORAUF NICHT"));
        assert!(prompt.contains("ER SCHREIBT: hi"));
    }

    #[test]
    fn owner_default_und_env() {
        // Ohne gesetzte Env greift der Default.
        if std::env::var("ENGAGEMENT_LEARN_LOGIN").is_err() {
            assert_eq!(owner_login(), DEFAULT_OWNER_LOGIN);
        }
        if std::env::var("ENGAGEMENT_LEARN_ENABLED").is_err() {
            assert!(!learn_enabled(), "Lernmodus ist standardmäßig aus");
        }
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        for statement in [
            "CREATE TABLE twitch_engagement_learn_channels (\
               channel_login TEXT PRIMARY KEY, channel_user_id TEXT, \
               first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
               last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
               message_count BIGINT NOT NULL DEFAULT 0)",
            "CREATE TABLE twitch_engagement_learn_timeline (\
               id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, \
               kind TEXT NOT NULL CHECK (kind IN ('stream','chat','own')), \
               ts TIMESTAMPTZ NOT NULL DEFAULT NOW(), started_at TIMESTAMPTZ, \
               author TEXT, content TEXT NOT NULL, engine TEXT, model TEXT, \
               message_id TEXT, mapped_at TIMESTAMPTZ, \
               created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE UNIQUE INDEX uq_engagement_learn_timeline_message \
               ON twitch_engagement_learn_timeline (channel_login, message_id) \
               WHERE message_id IS NOT NULL",
            "CREATE TABLE twitch_engagement_settings (\
               channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT FALSE)",
            "CREATE TABLE twitch_live_state (\
               twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, \
               is_live INTEGER DEFAULT 0, last_game TEXT)",
            "CREATE TABLE twitch_engagement_reaction_samples (\
               id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, \
               message_ts TIMESTAMPTZ NOT NULL, my_message TEXT NOT NULL, \
               stream_context TEXT NOT NULL DEFAULT '', chat_context TEXT NOT NULL DEFAULT '', \
               has_stream_context BOOLEAN NOT NULL DEFAULT FALSE, verdict TEXT, \
               created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
               CONSTRAINT twitch_engagement_reaction_samples_unique \
                 UNIQUE (channel_login, message_ts, my_message))",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn fremde_nachricht_in_kaltem_kanal_wird_ignoriert() {
        let Some(pool) = make_pool("t_eng_learn_cold").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        learn.observe("nani", None, "fremder", "hallo", None).await;
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_learn_timeline")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "kalter Kanal schreibt nichts weg");
        assert!(learn.hot_channels().is_empty());
    }

    #[tokio::test]
    async fn owner_macht_kanal_heiss_und_fremde_landen_im_puffer() {
        let Some(pool) = make_pool("t_eng_learn_hot").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        learn.observe("Nani", Some("42"), "Owner", "wilder take", Some("m1")).await;
        learn.observe("nani", None, "fremder", "haha ja", None).await;

        assert_eq!(learn.hot_channels(), vec!["nani".to_string()]);
        let (login, count, user_id): (String, i64, Option<String>) = sqlx::query_as(
            "SELECT channel_login, message_count, channel_user_id \
             FROM twitch_engagement_learn_channels",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(login, "nani");
        assert_eq!(count, 1);
        assert_eq!(user_id.as_deref(), Some("42"));

        // Beide Zeilen stehen in EINEM Zeitstrahl, unterschieden nur per kind.
        let kinds: Vec<(String, String)> = sqlx::query_as(
            "SELECT kind, content FROM twitch_engagement_learn_timeline ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            kinds,
            vec![
                ("own".to_string(), "wilder take".to_string()),
                ("chat".to_string(), "haha ja".to_string()),
            ]
        );
    }

    /// EventSub-Hook und Lern-IRC-Reader sehen dieselbe Nachricht, sobald ein
    /// Partner live Deadlock streamt. Ohne Dedup entstuenden daraus zwei
    /// Zeitstrahl-Zeilen und am Ende zwei Samples fuer eine Reaktion.
    #[tokio::test]
    async fn dieselbe_nachricht_aus_zwei_quellen_zaehlt_einmal() {
        let Some(pool) = make_pool("t_eng_learn_dedup").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        learn.observe("nani", Some("42"), "owner", "wilder take", Some("msg-1")).await;
        // Zweiter Pfad, gleiche Twitch-Message-ID, minimal andere Formatierung.
        learn.observe("nani", None, "owner", "wilder take", Some("msg-1")).await;
        // Andere ID im selben Kanal bleibt eine eigene Zeile.
        learn.observe("nani", None, "fremder", "haha", Some("msg-2")).await;
        // Ohne ID (etwa aus einem Pfad ohne Tags) greift der Index nicht.
        learn.observe("nani", None, "fremder", "ohne id", None).await;
        learn.observe("nani", None, "fremder", "ohne id", None).await;

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_learn_timeline")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 4, "einmal msg-1, einmal msg-2, zweimal ohne ID");
    }

    /// Kern des Auftrags: bei Partnern laeuft die Aufnahme ab Stream-Beginn,
    /// nicht erst wenn der Owner auftaucht. Sonst fehlt genau der Verlauf, der
    /// erklaert, worauf er spaeter reagiert.
    #[tokio::test]
    async fn partner_wird_ohne_owner_aufgenommen_und_endet_mit_dem_stream() {
        let Some(pool) = make_pool("t_eng_learn_capture").await else { return };
        sqlx::query(
            "INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES \
             ('partner_live', TRUE), ('partner_offline', TRUE), ('partner_aus', FALSE), \
             ('partner_anderes_spiel', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES \
             ('1','partner_live',1,'Deadlock'), \
             ('2','partner_offline',0,'Deadlock'), \
             ('3','partner_aus',1,'Deadlock'), \
             ('4','partner_anderes_spiel',1,'Dota 2')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        let channels = learn.capture_channels().await;
        assert_eq!(channels, vec!["partner_live".to_string()]);
        assert!(learn.should_capture("Partner_Live").await);
        assert!(!learn.should_capture("partner_offline").await);
    }

    #[tokio::test]
    async fn fremder_kanal_wird_ab_sichtung_aufgenommen_bis_der_stream_endet() {
        let Some(pool) = make_pool("t_eng_learn_capture_fremd").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        // Kein Partner, kein Live-State-Eintrag: unbekannte Kanaele bleiben
        // drin, sobald der Owner dort nachweislich schreibt.
        learn.observe("fremd", None, "owner", "wilder take", Some("m1")).await;
        assert_eq!(learn.capture_channels().await, vec!["fremd".to_string()]);

        // Geht der Stream aus, endet die Aufnahme sofort statt nach Ablauf der
        // Nachlaufzeit gegen einen toten Kanal weiterzulaufen.
        sqlx::query(
            "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) \
             VALUES ('9','fremd',0,'Deadlock')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(learn.capture_channels().await.is_empty());
    }

    #[tokio::test]
    async fn in_aufgenommenen_kanaelen_wandert_auch_fremder_chat_mit() {
        let Some(pool) = make_pool("t_eng_learn_recording").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        // Ohne Aufnahme-Markierung faellt die fremde Zeile weg.
        learn.observe("partner", None, "chatter", "erste zeile", Some("m1")).await;
        let before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_learn_timeline")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(before, 0);

        // Sobald der Supervisor den Kanal als aufgenommen meldet, laeuft der
        // Chat mit — sonst gaebe es Stream-Ton ohne den zugehoerigen Chat.
        learn.set_recording(&["partner".to_string()]);
        learn.observe("partner", None, "chatter", "zweite zeile", Some("m2")).await;
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT kind, content FROM twitch_engagement_learn_timeline ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows, vec![("chat".to_string(), "zweite zeile".to_string())]);
    }

    #[tokio::test]
    async fn timeline_mischt_audio_und_chat_chronologisch() {
        let Some(pool) = make_pool("t_eng_learn_timeline").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        let base = Utc::now() - Duration::minutes(3);
        learn
            .append_transcript(&LearnTranscriptSegment {
                channel_login: "nani".to_string(),
                started_at: base,
                ended_at: base + Duration::seconds(20),
                text: "und dann geh ich rein".to_string(),
                engine: "whisper".to_string(),
                model: Some("large-v3-turbo".to_string()),
            })
            .await
            .unwrap();
        // Chat-Zeilen landen mit NOW() im Strahl, also nach dem Segment.
        learn.observe("nani", None, "owner", "wilder take", None).await;
        learn.observe("nani", None, "fremder", "haha ja", None).await;

        let entries = learn.timeline("Nani", base - Duration::minutes(1), 50).await;
        let shape: Vec<(&str, Option<&str>, &str)> = entries
            .iter()
            .map(|e| (e.kind.as_str(), e.author.as_deref(), e.content.as_str()))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("stream", None, "und dann geh ich rein"),
                ("own", Some("owner"), "wilder take"),
                ("chat", Some("fremder"), "haha ja"),
            ]
        );
    }

    #[tokio::test]
    async fn mapper_baut_sample_aus_transkript_und_chat() {
        let Some(pool) = make_pool("t_eng_learn_map").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        // Nachricht liegt weit genug zurück, damit sie fällig ist.
        let msg_ts = Utc::now() - Duration::minutes(5);
        sqlx::query(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, author, content, ts) \
             VALUES ('nani','own','owner','wilder take',$1)",
        )
        .bind(msg_ts)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, started_at, ts, content, engine) \
             VALUES ('nani','stream', $1, $2, 'und dann geh ich da einfach rein', 'whisper')",
        )
        .bind(msg_ts - Duration::seconds(40))
        .bind(msg_ts - Duration::seconds(20))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, author, content, ts) \
             VALUES ('nani','chat','fremder','was macht der da',$1)",
        )
        .bind(msg_ts - Duration::seconds(10))
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(learn.map_pending().await, 1);
        let sample = &learn.recent_samples(10, false).await[0];
        assert_eq!(sample.my_message, "wilder take");
        assert!(sample.has_stream_context);
        assert!(sample.stream_context.contains("einfach rein"));
        assert!(sample.stream_context.contains("[-20s]"));
        assert_eq!(sample.chat_context, "fremder: was macht der da");

        // Zweiter Lauf erzeugt nichts Neues (mapped_at gesetzt).
        assert_eq!(learn.map_pending().await, 0);
    }

    #[tokio::test]
    async fn mapper_wartet_auf_das_fenster_nach_der_nachricht() {
        let Some(pool) = make_pool("t_eng_learn_lag").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        sqlx::query(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, author, content) VALUES ('nani','own','owner','zu frisch')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(learn.map_pending().await, 0, "frische Nachricht wartet noch");
    }

    #[tokio::test]
    async fn sample_ohne_stream_context_wird_markiert() {
        let Some(pool) = make_pool("t_eng_learn_nostream").await else { return };
        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        sqlx::query(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, author, content, ts) \
             VALUES ('nani','own','owner','ohne audio',$1)",
        )
        .bind(Utc::now() - Duration::minutes(5))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(learn.map_pending().await, 1);
        let all = learn.recent_samples(10, false).await;
        assert!(!all[0].has_stream_context);
        // Filter auf „nur mit Audio" blendet es aus.
        assert!(learn.recent_samples(10, true).await.is_empty());
        assert_eq!(learn.sample_count().await, 1);
    }

    #[tokio::test]
    async fn warm_cache_holt_heisse_kanaele_zurueck() {
        let Some(pool) = make_pool("t_eng_learn_warm").await else { return };
        sqlx::query(
            "INSERT INTO twitch_engagement_learn_channels (channel_login, last_seen_at) \
             VALUES ('frisch', NOW()), ('alt', NOW() - INTERVAL '10 hours')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let learn = ReactionLearning::new(pool).with_owner("owner");
        learn.warm_cache().await;
        assert_eq!(learn.hot_channels(), vec!["frisch".to_string()]);
    }

    #[tokio::test]
    async fn trim_raeumt_den_zeitstrahl_aber_nicht_samples() {
        let Some(pool) = make_pool("t_eng_learn_trim").await else { return };
        let alt = Utc::now() - Duration::hours(retention_hours() + 1);
        sqlx::query(
            "INSERT INTO twitch_engagement_learn_timeline \
             (channel_login, kind, author, content, ts, created_at, mapped_at) VALUES \
             ('nani','chat','a','alte fremde zeile',$1,$1,NULL), \
             ('nani','stream',NULL,'altes segment',$1,$1,NULL), \
             ('nani','own','owner','alt und gemappt',$1,$1,$1), \
             ('nani','own','owner','alt und ungemappt',$1,$1,NULL)",
        )
        .bind(alt)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_engagement_reaction_samples (channel_login, message_ts, my_message) VALUES ('nani',$1,'bleibt')")
            .bind(alt).execute(&pool).await.unwrap();

        let learn = ReactionLearning::new(pool.clone()).with_owner("owner");
        assert_eq!(learn.trim().await, 3, "alles außer der ungemappten eigenen Zeile");
        let rest: Vec<String> =
            sqlx::query_scalar("SELECT content FROM twitch_engagement_learn_timeline")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rest, vec!["alt und ungemappt".to_string()]);
        assert_eq!(learn.sample_count().await, 1, "Samples überleben das Trimmen");
    }
}
