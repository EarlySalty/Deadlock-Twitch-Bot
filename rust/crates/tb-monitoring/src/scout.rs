//! Periodischer Scout-Task für Live-Deadlock-Streams.
//!
//! Port von `bot/base.py:_scout_deadlock_channels` (945–1316 Z.).
//!
//! # Aufgabe
//!
//! Entdeckt live Streamer der Ziel-Kategorie (Deadlock, Sprache "de") und
//! registriert sie in `twitch_streamers` ohne Partner-Eintrag. Streamer, die 2
//! aufeinanderfolgende Zyklen abwesend sind, werden wieder entfernt
//! (Sessions geschlossen, Live-State gelöscht, Datenbankzeile weg).
//!
//! Für **frisch entdeckte** Kanäle wird zusätzlich
//! - eine Stream-Session **geprimt** (bevor der Chat-Bot joint), damit
//!   Viewer-/Presence-Samples vom ersten Tick an eine Session haben, und
//! - der **Chat-Bot synchronisiert**: neue Kanäle joinen, entfernte parten,
//!   Runtime-fehlende heilen (Python „Sync Chat Bot", Z. 1096–1225).
//!
//! # Design
//!
//! - **Repository** kapselt alle DB-Zugriffe; kennt kein HTTP.
//! - **ScoutTask** hält den Absent-Cycle-Counter im Arbeitsspeicher (`HashMap`)
//!   — er ist bewusst transient (kein DB-Overhead, kein Schema-Bloat, verloren bei
//!   Neustart ist akzeptabel: nach 2 Zyklen wäre der Streamer ohnehin weg).
//! - **Session-Priming** läuft über den crate-internen [`SessionTracker`];
//!   **Chat-Bot-Sync** über den [`ScoutChatSink`]-Port (Adapter im
//!   Composition-Root, Default [`NoopScoutChatSink`] = kein Chat-Effekt).
//! - Beide Kollaborateure sind **optional** (`with_session_tracker` /
//!   `with_chat_sink`): ohne sie verhält sich der Task wie der reine DB-Scout.
//! - Deaktiviert bis `TB_SCOUT_ENABLED=1` gesetzt ist.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tb_transport_twitch::{HelixClient, HelixStream};

use crate::sessions::SessionTracker;
use crate::stream::StreamSnapshot;

// ── Konstanten ─────────────────────────────────────────────────────────────────

/// Anzahl aufeinanderfolgender abwesender Zyklen bevor ein monitoring-only
/// Streamer entfernt wird. Python: `if missed_cycles >= 2`.
const ABSENT_CYCLES_BEFORE_REMOVE: u32 = 2;

/// Standard-Intervall zwischen Scout-Zyklen.
const DEFAULT_INTERVAL: Duration = Duration::from_secs(90);

/// Initiale Wartezeit nach Prozessstart (lässt den Bot erst hochfahren).
const INITIAL_DELAY: Duration = Duration::from_secs(30);

// ── Chat-Sink-Port ───────────────────────────────────────────────────────────

/// Brücke zum Chat-Bot-Runtime (Python `_twitch_chat_bot`). tb-monitoring kennt
/// den Chat-Prozess nicht direkt — der Adapter im Composition-Root setzt die
/// monitored-Kanal-Liste, joint/partet Kanäle und beantwortet die
/// Runtime-Heal-Prüfungen. Default [`NoopScoutChatSink`] = kein Chat-Effekt
/// (Verhalten des reinen DB-Scouts).
#[async_trait::async_trait]
pub trait ScoutChatSink: Send + Sync {
    /// Spiegelt die zu joinenden Logins in den Monitored-Set des Chat-Bots
    /// (Python `set_monitored_channels`). Läuft vor [`Self::join_channels`].
    async fn set_monitored_channels(&self, _logins: &[String]) {}

    /// Joint die gegebenen Kanäle (Python `join_channels`).
    async fn join_channels(&self, _logins: &[String]) {}

    /// Partet die gegebenen Kanäle (Python `part_channels`).
    async fn part_channels(&self, _logins: &[String]) {}

    /// Ist der Login bereits monitoring-only im Chat-Runtime (Python
    /// `_is_monitored_only`)? Monitoring-only-Kanäle sind **keine**
    /// Runtime-Heal-Ziele. Default `true` (Scout-Kanäle sind per Definition
    /// monitoring-only) → kein Heal.
    fn is_monitored_only(&self, _login: &str) -> bool {
        true
    }

    /// Ist die Channel-Subscription des Logins runtime-bereit (Python
    /// `is_channel_subscription_ready`)? Default `true` → kein Heal nötig.
    fn is_subscription_ready(&self, _login: &str) -> bool {
        true
    }
}

/// Chat-Sink ohne Wirkung (Wiring-Default, Tests).
pub struct NoopScoutChatSink;

impl ScoutChatSink for NoopScoutChatSink {}

/// Signalisiert echte Neuentdeckungen an optionale Scout-Folgestrecken.
#[async_trait::async_trait]
pub trait ScoutEventSink: Send + Sync {
    async fn on_new_streamer(&self, _stream: &StreamSnapshot) {}
}

pub struct NoopScoutEventSink;

impl ScoutEventSink for NoopScoutEventSink {}

/// Heal-Entscheidung — Port von `bot/chat/lurker_policy.py:should_attempt_runtime_heal`.
/// Monitoring-only-Lurker-Kanäle sind **keine** Chat-Runtime-Heal-Ziele.
pub(crate) fn should_attempt_runtime_heal(is_monitored_only: bool, is_ready: bool) -> bool {
    if is_monitored_only {
        return false;
    }
    !is_ready
}

// ── Repository ─────────────────────────────────────────────────────────────────

/// Alle DB-Zugriffe des Scout-Tasks gebündelt.
#[derive(Clone)]
pub struct ScoutRepository {
    pool: PgPool,
}

impl ScoutRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Gibt alle aktuell nur beobachteten Logins ohne Partner-Eintrag zurück.
    pub async fn load_monitored_only_logins(&self) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar!(
            "SELECT twitch_login \
             FROM twitch_streamers \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM twitch_partners p \
                 WHERE p.twitch_user_id = twitch_streamers.twitch_user_id \
                    OR LOWER(p.twitch_login) = LOWER(twitch_streamers.twitch_login) \
             )",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Trägt einen neuen Monitoring-only-Streamer ein. Gibt `true` zurück wenn
    /// er tatsächlich neu war (nicht nur ein Konflikt-Update).
    pub async fn upsert_monitored(&self, login: &str, user_id: &str) -> Result<bool, sqlx::Error> {
        let existing: Option<i32> = sqlx::query_scalar!(
            "SELECT 1 AS \"one!\" FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await?;

        if existing.is_some() {
            return Ok(false);
        }

        sqlx::query!(
            r#"
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
            VALUES ($1, $2)
            ON CONFLICT (twitch_login) DO NOTHING
            "#,
            login,
            user_id,
        )
        .execute(&self.pool)
        .await?;

        Ok(true)
    }

    /// Schließt offene Stream-Sessions für einen Streamer (auto-closed: scout-removed).
    pub async fn close_open_sessions(&self, login: &str) {
        let result = sqlx::query!(
            r#"
            UPDATE twitch_stream_sessions
            SET ended_at = NOW(),
                duration_seconds = EXTRACT(EPOCH FROM (NOW() - started_at))::int,
                notes = COALESCE(notes || '; ', '') || 'auto-closed: scout-removed'
            WHERE LOWER(streamer_login) = LOWER($1)
              AND ended_at IS NULL
            "#,
            login,
        )
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            tracing::debug!("scout: Session-Close für {login} fehlgeschlagen: {e}");
        }
    }

    /// Löscht den Live-State-Eintrag eines Streamers.
    pub async fn delete_live_state(&self, login: &str) {
        let result = sqlx::query!(
            "DELETE FROM twitch_live_state WHERE LOWER(streamer_login) = LOWER($1)",
            login,
        )
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            tracing::debug!("scout: Live-State-Delete für {login} fehlgeschlagen: {e}");
        }
    }

    /// Löscht einen Monitoring-only-Streamer und seine kaskadierenden Einträge.
    ///
    /// Safety-Guard: löscht nur wenn kein Partner-Eintrag existiert — Partner bleiben
    /// immer unberührt.
    pub async fn delete_monitored_streamer(&self, login: &str) -> Result<bool, sqlx::Error> {
        // Kaskadierende Clip-Tabellen (meist no-op für monitoring-only Streamer,
        // aber korrekt für den Fall dass Clips existieren).
        // dyn: mehrere feste Kaskaden-Deletes laufen über dieselbe Schleife.
        for sql in [
            "DELETE FROM twitch_clips_social_analytics WHERE clip_id IN \
             (SELECT id FROM twitch_clips_social_media WHERE LOWER(streamer_login) = LOWER($1))",
            "DELETE FROM twitch_clips_upload_queue WHERE clip_id IN \
             (SELECT id FROM twitch_clips_social_media WHERE LOWER(streamer_login) = LOWER($1))",
            "DELETE FROM twitch_clips_social_media WHERE LOWER(streamer_login) = LOWER($1)",
            "DELETE FROM clip_fetch_history WHERE LOWER(streamer_login) = LOWER($1)",
        ] {
            if let Err(e) = sqlx::query(sql).bind(login).execute(&self.pool).await {
                tracing::debug!("scout: Kaskaden-Delete für {login} fehlgeschlagen: {e}");
            }
        }

        let rows = sqlx::query!(
            "DELETE FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) \
             AND NOT EXISTS ( \
                 SELECT 1 FROM twitch_partners p \
                 WHERE p.twitch_user_id = twitch_streamers.twitch_user_id \
                    OR LOWER(p.twitch_login) = LOWER(twitch_streamers.twitch_login) \
             )",
            login,
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(rows > 0)
    }
}

// ── Typen ──────────────────────────────────────────────────────────────────────

/// Kompakte Statistik eines abgeschlossenen Scout-Zyklus (für tracing).
#[derive(Debug, Default)]
pub struct ScoutStats {
    pub streams_seen: u32,
    pub new_streamers: u32,
    pub removed_streamers: u32,
    /// Anzahl frisch geprimter Sessions (neue Kanäle mit Stream-Daten).
    pub primed_sessions: u32,
    /// Logins, die runtime-geheilt (rejoint) wurden.
    pub healed_streamers: u32,
}

/// HelixStream → crate-eigene Domänensicht. Spiegelt `to_snapshot` im
/// Composition-Root (wiring.rs); der Scout braucht die volle Sicht fürs
/// Session-Priming, nicht nur `login`/`user_id`.
fn to_snapshot(s: HelixStream) -> StreamSnapshot {
    StreamSnapshot {
        id: Some(s.id).filter(|v| !v.is_empty()),
        user_login: s.user_login,
        user_id: s.user_id,
        user_name: s.user_name,
        title: s.title,
        game_name: s.game_name,
        language: s.language,
        viewer_count: i32::try_from(s.viewer_count).unwrap_or(i32::MAX),
        is_mature: s.is_mature,
        tags: s.tags.unwrap_or_default(),
        started_at: Some(s.started_at).filter(|v| !v.is_empty()),
        thumbnail_url: Some(s.thumbnail_url)
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty()),
        profile_image_url: None,
    }
}

// ── Task ───────────────────────────────────────────────────────────────────────

/// Periodischer Scout-Task. Hält den Absent-Cycle-Counter in-memory.
pub struct ScoutTask {
    repo: ScoutRepository,
    helix: Arc<HelixClient>,
    game_name: String,
    language_filters: Vec<String>,
    interval: Duration,
    absent_cycles: HashMap<String, u32>,
    /// Optional: primt Sessions neuer Kanäle (Python `_prime_monitored_only_sessions`).
    session_tracker: Option<Arc<SessionTracker>>,
    /// Optional: synchronisiert den Chat-Bot (join/part/heal).
    chat: Arc<dyn ScoutChatSink>,
    events: Arc<dyn ScoutEventSink>,
}

impl ScoutTask {
    pub fn new(
        repo: ScoutRepository,
        helix: Arc<HelixClient>,
        game_name: impl Into<String>,
        language_filters: Vec<String>,
    ) -> Self {
        Self {
            repo,
            helix,
            game_name: game_name.into(),
            language_filters,
            interval: DEFAULT_INTERVAL,
            absent_cycles: HashMap::new(),
            session_tracker: None,
            chat: Arc::new(NoopScoutChatSink),
            events: Arc::new(NoopScoutEventSink),
        }
    }

    /// Session-Priming für neu entdeckte Kanäle aktivieren.
    #[must_use]
    pub fn with_session_tracker(mut self, tracker: Arc<SessionTracker>) -> Self {
        self.session_tracker = Some(tracker);
        self
    }

    /// Chat-Bot-Synchronisation (join/part/heal) aktivieren.
    #[must_use]
    pub fn with_chat_sink(mut self, chat: Arc<dyn ScoutChatSink>) -> Self {
        self.chat = chat;
        self
    }

    #[must_use]
    pub fn with_event_sink(mut self, events: Arc<dyn ScoutEventSink>) -> Self {
        self.events = events;
        self
    }

    /// Gibt den Task zurück, wenn `TB_SCOUT_ENABLED=1` gesetzt ist.
    ///
    /// Der Aufrufer behält damit die Verantwortung für Supervision und Shutdown.
    pub fn run_if_enabled(self) -> Option<impl std::future::Future<Output = ()> + Send + 'static> {
        let enabled = std::env::var("TB_SCOUT_ENABLED")
            .map(|v| v.trim() == "1")
            .unwrap_or(false);
        self.into_run(enabled)
    }

    fn into_run(
        self,
        enabled: bool,
    ) -> Option<impl std::future::Future<Output = ()> + Send + 'static> {
        if !enabled {
            tracing::info!("scout: Task deaktiviert (TB_SCOUT_ENABLED≠1)");
            return None;
        }

        tracing::info!(
            "scout: Task startet (game={}, lang={:?}, interval={}s)",
            self.game_name,
            self.language_filters,
            self.interval.as_secs(),
        );

        Some(self.run())
    }

    async fn run(mut self) {
        tokio::time::sleep(INITIAL_DELAY).await;

        loop {
            let stats = self.run_once().await;
            tracing::info!(
                "scout: Zyklus — {} Streams gesehen, {} neu, {} entfernt, {} Sessions geprimt, {} geheilt",
                stats.streams_seen,
                stats.new_streamers,
                stats.removed_streamers,
                stats.primed_sessions,
                stats.healed_streamers,
            );
            tokio::time::sleep(self.interval).await;
        }
    }

    /// Holt die aktuellen Live-Streams der Ziel-Kategorie (deduppt über
    /// Sprachfilter). `login → StreamSnapshot`.
    async fn fetch_current_streams(&self, game_id: &str) -> HashMap<String, StreamSnapshot> {
        let mut current: HashMap<String, StreamSnapshot> = HashMap::new();

        let language_list = if self.language_filters.is_empty() {
            vec![None]
        } else {
            self.language_filters
                .iter()
                .map(|l| Some(l.as_str()))
                .collect::<Vec<_>>()
        };

        for lang in language_list {
            match self.helix.get_streams_by_category(game_id, lang, 100).await {
                Ok(streams) => {
                    for s in streams {
                        let snapshot = to_snapshot(s);
                        let login = snapshot.user_login.to_lowercase();
                        if !login.is_empty() {
                            current.entry(login).or_insert(snapshot);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("scout: Streams-Fetch fehlgeschlagen (lang={lang:?}): {e}");
                }
            }
        }
        current
    }

    async fn run_once(&mut self) -> ScoutStats {
        let mut stats = ScoutStats::default();

        // ── Game-ID auflösen ──────────────────────────────────────────────────
        let game_id = match self.helix.search_category_id(&self.game_name).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                tracing::warn!("scout: Kategorie '{}' nicht gefunden", self.game_name);
                return stats;
            }
            Err(e) => {
                tracing::warn!("scout: Kategorie-Lookup fehlgeschlagen: {e}");
                return stats;
            }
        };

        // ── Aktuelle Live-Streams holen ───────────────────────────────────────
        let current_streams = self.fetch_current_streams(&game_id).await;
        stats.streams_seen = current_streams.len() as u32;

        // ── Bestehende monitoring-only Einträge laden ─────────────────────────
        let existing_monitored: HashSet<String> = match self.repo.load_monitored_only_logins().await
        {
            Ok(v) => v.into_iter().map(|l| l.to_lowercase()).collect(),
            Err(e) => {
                tracing::error!("scout: DB-Fehler beim Laden der monitoring-only Streamer: {e}");
                return stats;
            }
        };

        // ── Phase 1: Neue Streamer hinzufügen ─────────────────────────────────
        let mut new_logins: Vec<String> = Vec::new();
        for (login, snapshot) in &current_streams {
            if existing_monitored.contains(login.as_str()) {
                continue;
            }
            match self.repo.upsert_monitored(login, &snapshot.user_id).await {
                Ok(true) => {
                    tracing::debug!("scout: Neuer Monitoring-Streamer: {login}");
                    stats.new_streamers += 1;
                    new_logins.push(login.clone());
                    self.events.on_new_streamer(snapshot).await;
                }
                Ok(false) => {} // bereits vorhanden (als Partner oder monitoring)
                Err(e) => tracing::warn!("scout: DB-Fehler bei upsert für {login}: {e}"),
            }
        }

        // ── Phase 1b: Sessions neuer Kanäle primen (vor dem Chat-Join) ────────
        stats.primed_sessions = self.prime_sessions(&new_logins, &current_streams).await;

        // ── Phase 2: Absent-Cycle-Tracking + Remove ───────────────────────────
        let mut to_remove: Vec<String> = Vec::new();
        for login in &existing_monitored {
            if current_streams.contains_key(login.as_str()) {
                // Noch live → Zähler zurücksetzen
                self.absent_cycles.remove(login);
                continue;
            }
            let cycles = self.absent_cycles.entry(login.clone()).or_insert(0);
            *cycles += 1;
            if *cycles >= ABSENT_CYCLES_BEFORE_REMOVE {
                to_remove.push(login.clone());
            }
        }

        // Veraltete Zähler für Logins entfernen die nicht mehr monitoring-only sind
        self.absent_cycles
            .retain(|login, _| existing_monitored.contains(login.as_str()));

        for login in &to_remove {
            self.absent_cycles.remove(login);
            self.repo.close_open_sessions(login).await;
            self.repo.delete_live_state(login).await;

            match self.repo.delete_monitored_streamer(login).await {
                Ok(true) => {
                    tracing::info!("scout: Monitoring-Streamer entfernt: {login}");
                    stats.removed_streamers += 1;
                }
                Ok(false) => {
                    // Safety-Guard hat gegriffen: Login ist Partner → nicht löschen
                    tracing::debug!(
                        "scout: Delete für {login} abgelehnt (kein is_monitored_only=1)"
                    );
                }
                Err(e) => tracing::warn!("scout: Delete für {login} fehlgeschlagen: {e}"),
            }
        }

        // ── Phase 3: Chat-Bot synchronisieren ─────────────────────────────────
        let new_set: HashSet<&str> = new_logins.iter().map(String::as_str).collect();
        let remove_set: HashSet<&str> = to_remove.iter().map(String::as_str).collect();
        let healed = self
            .sync_chat(
                &current_streams,
                &existing_monitored,
                &new_logins,
                &to_remove,
                &new_set,
                &remove_set,
            )
            .await;
        stats.healed_streamers = healed;

        stats
    }

    /// Erstellt Stream-Sessions für frisch entdeckte Kanäle, bevor der Chat-Bot
    /// joint (Python `_prime_monitored_only_sessions`). Liefert die Anzahl
    /// geprimter Sessions. Ohne Session-Tracker no-op.
    async fn prime_sessions(
        &self,
        new_logins: &[String],
        current_streams: &HashMap<String, StreamSnapshot>,
    ) -> u32 {
        let Some(tracker) = self.session_tracker.as_ref() else {
            return 0;
        };
        if new_logins.is_empty() {
            return 0;
        }
        let now = Utc::now();
        let mut primed = 0;
        for login in new_logins {
            let Some(stream) = current_streams.get(login.as_str()) else {
                continue;
            };
            let twitch_user_id = Some(stream.user_id.trim()).filter(|v| !v.is_empty());
            if tracker
                .ensure_session(login, stream, None, twitch_user_id, now)
                .await
                .is_some()
            {
                primed += 1;
            }
        }
        if primed > 0 {
            tracing::debug!("scout: {primed}/{} neue Sessions geprimt", new_logins.len());
        }
        primed
    }

    /// Bestimmt die Heal-Ziele (monitored-only Kanäle, die noch live sind und
    /// runtime-fehlen) — Port der „Sync Chat Bot"-Heal-Schleife (Z. 1109–1144).
    /// Monitoring-only-Kanäle sind via [`should_attempt_runtime_heal`] keine
    /// Heal-Ziele, außer der Chat-Sink markiert sie als nicht-monitoring-only.
    fn heal_targets(
        &self,
        current_streams: &HashMap<String, StreamSnapshot>,
        existing_monitored: &HashSet<String>,
        new_set: &HashSet<&str>,
        remove_set: &HashSet<&str>,
    ) -> Vec<String> {
        let mut heal: Vec<String> = existing_monitored
            .iter()
            .filter(|login| current_streams.contains_key(login.as_str()))
            .filter(|login| {
                !new_set.contains(login.as_str()) && !remove_set.contains(login.as_str())
            })
            .filter(|login| {
                let is_monitored_only = self.chat.is_monitored_only(login);
                let is_ready = self.chat.is_subscription_ready(login);
                should_attempt_runtime_heal(is_monitored_only, is_ready)
            })
            .cloned()
            .collect();
        heal.sort_unstable();
        heal
    }

    /// Treibt set_monitored_channels → join_channels (neu + heal) → part_channels
    /// (entfernt) über den Chat-Sink. Liefert die Anzahl Heal-Ziele.
    async fn sync_chat(
        &self,
        current_streams: &HashMap<String, StreamSnapshot>,
        existing_monitored: &HashSet<String>,
        new_logins: &[String],
        to_remove: &[String],
        new_set: &HashSet<&str>,
        remove_set: &HashSet<&str>,
    ) -> u32 {
        let heal = self.heal_targets(current_streams, existing_monitored, new_set, remove_set);

        // Join-Ziele = neue ∪ heal (dedupliziert, Reihenfolge: neu zuerst).
        let mut join_targets: Vec<String> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for login in new_logins.iter().chain(heal.iter()) {
            if !login.is_empty() && seen.insert(login.as_str()) {
                join_targets.push(login.clone());
            }
        }

        // Der anonyme Read-Sink muss alle aktuell gefundenen Live-Kanäle sehen,
        // auch bereits bekannte Partner, die nie `is_monitored_only` sind.
        // Den bestehenden Roster behalten wir zusätzlich bis zum entprellten
        // Remove, damit ein einzelner leerer/fehlgeschlagener Helix-Fetch nicht
        // sofort alle IRC-Memberships kappt.
        let mut desired_channels: Vec<String> = current_streams
            .keys()
            .chain(existing_monitored)
            .chain(new_logins)
            .filter(|login| !remove_set.contains(login.as_str()))
            .cloned()
            .collect();
        desired_channels.sort_unstable();
        desired_channels.dedup();
        self.chat.set_monitored_channels(&desired_channels).await;

        if !join_targets.is_empty() {
            self.chat.join_channels(&join_targets).await;
            tracing::info!(
                "scout: {} Kanäle gejoint ({} neu, {} geheilt)",
                join_targets.len(),
                new_logins.len(),
                heal.len(),
            );
        }

        if !to_remove.is_empty() {
            self.chat.part_channels(to_remove).await;
            tracing::info!("scout: {} Kanäle gepartet", to_remove.len());
        }

        heal.len() as u32
    }
}

// ── Builder ────────────────────────────────────────────────────────────────────

/// Baut einen fertigen `ScoutTask` aus Pool + Helix.
///
/// `language_filters`: Leere Liste = keine Sprach-Einschränkung (alle Sprachen).
pub fn build_scout_task(
    pool: PgPool,
    helix: Arc<HelixClient>,
    game_name: impl Into<String>,
    language_filters: Vec<String>,
) -> ScoutTask {
    let repo = ScoutRepository::new(pool);
    ScoutTask::new(repo, helix, game_name, language_filters)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use sqlx::postgres::PgPoolOptions;
    use tb_transport_twitch::HelixConfig;

    use super::*;

    /// Chat-Sink, der join/part/set aufzeichnet und Heal-Prüfungen steuert.
    #[derive(Default)]
    struct RecordingChatSink {
        set: Mutex<Vec<Vec<String>>>,
        joined: Mutex<Vec<Vec<String>>>,
        parted: Mutex<Vec<Vec<String>>>,
        /// Logins, die NICHT monitoring-only sind (also Heal-fähig).
        not_monitored_only: HashSet<String>,
        /// Logins, deren Subscription NICHT runtime-bereit ist (Heal nötig).
        not_ready: HashSet<String>,
    }

    #[async_trait::async_trait]
    impl ScoutChatSink for RecordingChatSink {
        async fn set_monitored_channels(&self, logins: &[String]) {
            self.set.lock().unwrap().push(logins.to_vec());
        }
        async fn join_channels(&self, logins: &[String]) {
            self.joined.lock().unwrap().push(logins.to_vec());
        }
        async fn part_channels(&self, logins: &[String]) {
            self.parted.lock().unwrap().push(logins.to_vec());
        }
        fn is_monitored_only(&self, login: &str) -> bool {
            !self.not_monitored_only.contains(login)
        }
        fn is_subscription_ready(&self, login: &str) -> bool {
            !self.not_ready.contains(login)
        }
    }

    fn task_with_chat(chat: Arc<dyn ScoutChatSink>) -> ScoutTask {
        // Lazy-Pool: wird nie verbunden, da sync_chat/heal_targets keine DB nutzen.
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .expect("lazy pool");
        let helix =
            Arc::new(HelixClient::new(HelixConfig::new("id", "secret")).expect("helix client"));
        ScoutTask::new(ScoutRepository::new(pool), helix, "Deadlock", vec![]).with_chat_sink(chat)
    }

    fn snap(login: &str) -> StreamSnapshot {
        StreamSnapshot {
            user_login: login.to_string(),
            user_id: "1".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn aktivierter_scout_gibt_seinen_lauf_zur_externen_supervision_zurueck() {
        let task = task_with_chat(Arc::new(RecordingChatSink::default()));

        assert!(task.into_run(true).is_some());
    }

    #[tokio::test]
    async fn sync_chat_joint_neue_und_partet_entfernte() {
        let chat = Arc::new(RecordingChatSink::default());
        let task = task_with_chat(chat.clone());

        let mut current = HashMap::new();
        current.insert("neu".to_string(), snap("neu"));
        current.insert("bleibt".to_string(), snap("bleibt"));
        let existing: HashSet<String> = ["bleibt".to_string()].into_iter().collect();
        let new_logins = vec!["neu".to_string()];
        let to_remove = vec!["weg".to_string()];
        let new_set: HashSet<&str> = ["neu"].into_iter().collect();
        let remove_set: HashSet<&str> = ["weg"].into_iter().collect();

        let healed = task
            .sync_chat(
                &current,
                &existing,
                &new_logins,
                &to_remove,
                &new_set,
                &remove_set,
            )
            .await;

        assert_eq!(healed, 0, "monitoring-only Kanäle werden nicht geheilt");
        // Der vollständige Soll-Roster läuft vor join, damit bestehende live
        // monitoring-only Kanäle nach einem Prozessstart wieder gejoint werden.
        assert_eq!(
            *chat.set.lock().unwrap(),
            vec![vec!["bleibt".to_string(), "neu".to_string()]]
        );
        assert_eq!(*chat.joined.lock().unwrap(), vec![vec!["neu".to_string()]]);
        // entfernte Kanäle werden gepartet.
        assert_eq!(*chat.parted.lock().unwrap(), vec![vec!["weg".to_string()]]);
    }

    #[tokio::test]
    async fn sync_chat_nimmt_auch_bekannte_partner_in_den_live_roster() {
        let chat = Arc::new(RecordingChatSink::default());
        let task = task_with_chat(chat.clone());

        let mut current = HashMap::new();
        current.insert("partner".to_string(), snap("partner"));

        task.sync_chat(
            &current,
            &HashSet::new(),
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
        )
        .await;

        assert_eq!(
            *chat.set.lock().unwrap(),
            vec![vec!["partner".to_string()]],
            "der anonyme Read-Roster muss jeden aktuell live gefundenen Kanal enthalten"
        );
    }

    #[tokio::test]
    async fn sync_chat_heilt_nicht_monitored_only_kanal() {
        let chat = Arc::new(RecordingChatSink {
            // "partner" ist nicht monitoring-only und runtime nicht bereit → Heal.
            not_monitored_only: ["partner".to_string()].into_iter().collect(),
            not_ready: ["partner".to_string()].into_iter().collect(),
            ..Default::default()
        });
        let task = task_with_chat(chat.clone());

        let mut current = HashMap::new();
        current.insert("partner".to_string(), snap("partner"));
        let existing: HashSet<String> = ["partner".to_string()].into_iter().collect();

        let healed = task
            .sync_chat(
                &current,
                &existing,
                &[],
                &[],
                &HashSet::new(),
                &HashSet::new(),
            )
            .await;

        assert_eq!(healed, 1);
        // Heal-Ziel wird (re)gejoint.
        assert_eq!(
            *chat.joined.lock().unwrap(),
            vec![vec!["partner".to_string()]]
        );
    }

    #[tokio::test]
    async fn sync_chat_behaelt_entprellten_roster_bei_leerem_fetch() {
        let chat = Arc::new(RecordingChatSink::default());
        let task = task_with_chat(chat.clone());
        let current: HashMap<String, StreamSnapshot> = HashMap::new();
        let existing: HashSet<String> = ["bleibt".to_string()].into_iter().collect();

        task.sync_chat(
            &current,
            &existing,
            &[],
            &[],
            &HashSet::new(),
            &HashSet::new(),
        )
        .await;

        assert_eq!(*chat.set.lock().unwrap(), vec![vec!["bleibt".to_string()]]);
    }

    #[tokio::test]
    async fn sync_chat_leert_roster_ohne_kanaele() {
        let chat = Arc::new(RecordingChatSink::default());
        let task = task_with_chat(chat.clone());
        let current: HashMap<String, StreamSnapshot> = HashMap::new();
        let existing: HashSet<String> = HashSet::new();

        let healed = task
            .sync_chat(
                &current,
                &existing,
                &[],
                &[],
                &HashSet::new(),
                &HashSet::new(),
            )
            .await;

        assert_eq!(healed, 0);
        assert_eq!(*chat.set.lock().unwrap(), vec![Vec::<String>::new()]);
        assert!(chat.joined.lock().unwrap().is_empty());
        assert!(chat.parted.lock().unwrap().is_empty());
    }

    #[test]
    fn heal_policy_skips_monitored_only() {
        // Monitoring-only Kanäle sind nie Heal-Ziele (lurker_policy.py).
        assert!(!should_attempt_runtime_heal(true, false));
        assert!(!should_attempt_runtime_heal(true, true));
        // Nicht-monitoring-only: heal nur wenn nicht runtime-bereit.
        assert!(should_attempt_runtime_heal(false, false));
        assert!(!should_attempt_runtime_heal(false, true));
    }

    #[test]
    fn to_snapshot_maps_helix_fields() {
        let helix = HelixStream {
            id: "stream1".into(),
            user_id: "42".into(),
            user_login: "Dragon".into(),
            user_name: "Dragon".into(),
            game_name: "Deadlock".into(),
            title: "Ranked".into(),
            language: "de".into(),
            viewer_count: 1234,
            is_mature: true,
            tags: Some(vec!["de".into()]),
            started_at: "2026-06-16T10:00:00Z".into(),
            thumbnail_url: "https://cdn/{width}x{height}.jpg".into(),
            ..Default::default()
        };
        let snap = to_snapshot(helix);
        assert_eq!(snap.id.as_deref(), Some("stream1"));
        assert_eq!(snap.user_id, "42");
        assert_eq!(snap.viewer_count, 1234);
        assert_eq!(snap.started_at.as_deref(), Some("2026-06-16T10:00:00Z"));
        assert_eq!(
            snap.thumbnail_url.as_deref(),
            Some("https://cdn/{width}x{height}.jpg")
        );
        assert!(snap.is_mature);
    }
}
