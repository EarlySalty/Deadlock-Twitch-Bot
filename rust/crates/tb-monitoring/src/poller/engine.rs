//! Der Poll-Loop (Python `poll_streams`/`_tick`/`_process_postings`):
//! Helix-Abgleich der getrackten Streamer + Kategorie-Sample, Live-State-
//! Transitions, Session-Lebenszyklus, Stats, Cleanup-Kadenzen.
//!
//! Außenwirkung (Discord-Postings, EventSub-Subscriptions, Raid-Refreshes,
//! Partner-Lifecycle) läuft ausschließlich über [`AnnouncementSink`] und
//! [`PollHooks`] — mit den Noop-Implementierungen ist der Loop ein reiner
//! DB-Writer und gefahrlos parallel zu Python testbar (gegen eine Test-DB;
//! gegen Prod erst beim Cutover, siehe 04-cutover-plan).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::guard::{GuardKind, GuardStore};
use crate::live_state::{LiveStateRow, LiveStateStore, LiveStateUpsert, TrackedStreamer};
use crate::poller::hooks::{
    AnnounceLiveRequest, AnnouncementSink, EndAnnouncementOutcome, EndAnnouncementRequest,
    PollHooks, ScoreRefresh, TickReport,
};
use crate::poller::settings::PollIntervalStore;
use crate::poller::source::StreamSource;
use crate::poller::tracked::{TrackedEntry, TrackedStore};
use crate::sessions::{SessionStore, SessionTracker};
use crate::stats::{StatsSample, StatsStore};
use crate::stream::{extract_stream_start, iso_seconds, StreamSnapshot};

/// Alle wievielten Tick Orphan-Cleanup + Guard-Sweep laufen.
const CLEANUP_EVERY_N_TICKS: u64 = 10;
/// Periodischer Sweep gegen verwaiste `twitch_live_state.is_live=1`-Rows.
const STALE_LIVE_SWEEP_INTERVAL: Duration = Duration::from_secs(300);
/// Live-State gilt nach 30 Minuten ohne Poll-/EventSub-Aktualisierung als stale.
const STALE_LIVE_MAX_AGE_SECS: i64 = 30 * 60;
/// Auto-Archiv-Drossel (Python: max. alle 15 Minuten).
const AUTO_ARCHIVE_THROTTLE: Duration = Duration::from_secs(900);
/// Inaktivität in Tagen, ab der ein Partner automatisch archiviert wird.
const AUTO_ARCHIVE_DAYS: i64 = 10;
/// Gemeinsame Offline-Drossel mit EventSub gegen doppelte Auto-Raid-Trigger.
const POLL_OFFLINE_RAID_THROTTLE_SECONDS: f64 = 120.0;

/// Statische Poll-Konfiguration (Env-getrieben, wie die Python-Konstanten).
#[derive(Debug, Clone)]
pub struct PollConfig {
    pub target_game: String,
    /// Sprachfilter (z. B. `["de"]`); leer = keine Filterung.
    pub language_filters: Vec<String>,
    /// Obergrenze des Kategorie-Samples (Python: `TWITCH_CATEGORY_SAMPLE_LIMIT`).
    pub category_sample_limit: usize,
    /// Stats/Sample-Kadenz in Ticks (Python: `TWITCH_LOG_EVERY_N_TICKS`).
    pub log_every_n: u64,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            target_game: "Deadlock".to_string(),
            language_filters: Vec::new(),
            category_sample_limit: 400,
            log_every_n: 1,
        }
    }
}

struct TickState {
    category_id: Option<String>,
    tick_count: u64,
    last_stale_live_sweep: Option<Instant>,
    last_archive_check: Option<Instant>,
}

pub struct PollEngine {
    source: Arc<dyn StreamSource>,
    tracked: TrackedStore,
    live_state: LiveStateStore,
    sessions: SessionStore,
    tracker: Arc<SessionTracker>,
    stats: StatsStore,
    guard: GuardStore,
    sink: Arc<dyn AnnouncementSink>,
    hooks: Arc<dyn PollHooks>,
    interval: PollIntervalStore,
    config: PollConfig,
    target_game_lower: String,
    languages: Vec<Option<String>>,
    state: Mutex<TickState>,
}

#[allow(clippy::too_many_arguments)]
impl PollEngine {
    pub fn new(
        source: Arc<dyn StreamSource>,
        tracked: TrackedStore,
        live_state: LiveStateStore,
        sessions: SessionStore,
        tracker: Arc<SessionTracker>,
        stats: StatsStore,
        guard: GuardStore,
        sink: Arc<dyn AnnouncementSink>,
        hooks: Arc<dyn PollHooks>,
        interval: PollIntervalStore,
        config: PollConfig,
    ) -> Self {
        let target_game_lower = config.target_game.trim().to_lowercase();
        // Python `_language_filter_values`: dedupliziert, lowercase; leer → [None].
        let mut languages: Vec<Option<String>> = Vec::new();
        for filter in &config.language_filters {
            let normalized = filter.trim().to_lowercase();
            if normalized.is_empty() || languages.iter().any(|l| l.as_deref() == Some(&normalized))
            {
                continue;
            }
            languages.push(Some(normalized));
        }
        if languages.is_empty() {
            languages.push(None);
        }
        Self {
            source,
            tracked,
            live_state,
            sessions,
            tracker,
            stats,
            guard,
            sink,
            hooks,
            interval,
            config,
            target_game_lower,
            languages,
            state: Mutex::new(TickState {
                category_id: None,
                tick_count: 0,
                last_stale_live_sweep: None,
                last_archive_check: None,
            }),
        }
    }

    /// Dauerschleife: Tick → Intervall aus der DB → schlafen. Stop via watch.
    pub async fn run(self: Arc<Self>, mut stop: tokio::sync::watch::Receiver<bool>) {
        self.tracker.rehydrate().await;
        loop {
            if *stop.borrow() {
                return;
            }
            self.tick().await;
            let seconds = self.interval.current_seconds().await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(seconds)) => {}
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                }
            }
        }
    }

    async fn maybe_trigger_poll_offline_raid(&self, twitch_user_id: &str, login: &str) {
        let user_id = twitch_user_id.trim();
        if user_id.is_empty() {
            return;
        }
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        match self
            .guard
            .claim(
                GuardKind::OfflineThrottle,
                user_id,
                POLL_OFFLINE_RAID_THROTTLE_SECONDS,
                now,
            )
            .await
        {
            Ok(true) => {
                let login_opt = Some(login.trim()).filter(|l| !l.is_empty());
                self.hooks.on_stream_offline_raid(user_id, login_opt).await;
            }
            Ok(false) => {
                tracing::debug!(
                    twitch_user_id = %user_id,
                    "Poller OfflineThrottle: Auto-Raid-Hook bereits durch anderen Pfad beansprucht"
                );
            }
            Err(error) => {
                tracing::warn!(%error, twitch_user_id = %user_id, "Poller OfflineThrottle-Claim fehlgeschlagen");
            }
        }
    }

    /// Ein kompletter Poll-Durchlauf. Fehler einzelner Phasen brechen den
    /// Tick nicht ab (wie Python: log + weiter).
    pub async fn tick(&self) {
        // Circuit-Breaker (B18-3): bei gesperrter Helix-App-Auth den ganzen Tick
        // überspringen, statt mit garantiert erfolglosen Requests Rate-Limits zu
        // verbrennen. Backoff übernimmt der Cooldown der Quelle (Python `_tick`
        // bricht hier ebenfalls ab — monitoring.py:1207).
        if self.source.is_auth_blocked() {
            tracing::debug!("Poll-Tick übersprungen: Helix-App-Auth gesperrt (Circuit-Breaker)");
            return;
        }

        let category_id = self.ensure_category_id().await;

        let (tracked, partner_logins) = match self.tracked.load().await {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::error!(%error, "Konnte tracked Streamer nicht aus DB lesen");
                return;
            }
        };

        let logins: Vec<String> = tracked
            .iter()
            .map(|e| e.login.clone())
            .filter(|l| !l.is_empty())
            .collect();

        // Streams der getrackten Logins — bewusst OHNE Sprachfilter: die Liste
        // ist kuratiert, Partner mit nicht-deutscher Kanal-Sprache fielen sonst
        // komplett aus Sessions/last_game/Postings raus. Der Sprachfilter gilt
        // nur fürs Kategorie-Sampling (Discovery) unten.
        let mut streams_by_login: HashMap<String, StreamSnapshot> = HashMap::new();
        let mut tracked_streams_loaded = true;
        if !logins.is_empty() {
            match self.source.streams_by_logins(&logins, None).await {
                Ok(streams) => {
                    for stream in streams {
                        let login = stream.user_login.to_lowercase();
                        if !login.is_empty() {
                            streams_by_login.insert(login, stream);
                        }
                    }
                }
                Err(error) => {
                    tracked_streams_loaded = false;
                    tracing::error!(%error, "Konnte Streams für tracked Logins nicht abrufen");
                }
            }
        }

        // Kategorie-Sample (Discovery), dedupliziert über Sprachfilter.
        let mut category_streams: Vec<StreamSnapshot> = Vec::new();
        if let Some(category_id) = &category_id {
            let mut collected: HashMap<String, StreamSnapshot> = HashMap::new();
            for language in &self.languages {
                let remaining = self
                    .config
                    .category_sample_limit
                    .saturating_sub(collected.len());
                if remaining == 0 {
                    break;
                }
                match self
                    .source
                    .streams_by_category(category_id, language.as_deref(), remaining.max(1))
                    .await
                {
                    Ok(streams) => {
                        for stream in streams {
                            let login = stream.user_login.to_lowercase();
                            if !login.is_empty() {
                                collected.entry(login).or_insert(stream);
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            %error,
                            language = language.as_deref().unwrap_or("any"),
                            "Konnte Kategorie-Streams nicht abrufen"
                        );
                    }
                }
            }
            category_streams = collected.into_values().collect();
        }

        // Backfill: getrackte Streamer, die nur im Kategorie-Sample auftauchen
        // (API-Inkonsistenz), bekommen trotzdem Stats + Session-Samples.
        let tracked_set: HashSet<String> = tracked.iter().map(|e| e.login.to_lowercase()).collect();
        for stream in &category_streams {
            let login = stream.user_login.to_lowercase();
            if !login.is_empty()
                && tracked_set.contains(&login)
                && !streams_by_login.contains_key(&login)
            {
                streams_by_login.insert(login, stream.clone());
            }
        }

        let score_refreshes = if tracked_streams_loaded {
            self.process_entries(&tracked, &streams_by_login).await
        } else {
            tracing::warn!(
                "Poll-Tick: tracked Stream-Abruf fehlgeschlagen, Offline-Transitions übersprungen"
            );
            Vec::new()
        };

        let tick_count = {
            let mut state = self.state.lock().expect("tick state lock");
            state.tick_count += 1;
            state.tick_count
        };

        if tick_count % self.config.log_every_n.max(1) == 0 {
            self.log_stats(&streams_by_login, &category_streams, &partner_logins)
                .await;
        }

        if tick_count % CLEANUP_EVERY_N_TICKS == 0 {
            let closed = self.tracker.cleanup_orphans().await;
            if closed > 0 {
                tracing::info!(closed, "Orphaned Sessions bereinigt");
            }
            // Guard-GC läuft hier statt (wie Python) bei jedem Claim.
            match self.guard.sweep_expired(epoch_now()).await {
                Ok(swept) if swept > 0 => tracing::debug!(swept, "Guard-Einträge abgeräumt"),
                Ok(_) => {}
                Err(error) => tracing::debug!(%error, "Guard-Sweep fehlgeschlagen"),
            }
        }

        self.sweep_stale_live_if_due().await;

        self.auto_archive_inactive().await;

        self.hooks
            .after_tick(TickReport {
                score_refreshes,
                category_streams,
            })
            .await;

        // EventSub-Capacity-Zeitreihe (B5-08): taktgebend jeden Tick; die
        // Sample-/Retention-Drosselung sitzt im Hook-Adapter.
        self.hooks.on_capacity_tick().await;
    }

    /// Kern der Transitions (Python `_process_postings`): pro getracktem Kanal
    /// Sessions pflegen, Announcements delegieren, Live-State-Row bauen.
    async fn process_entries(
        &self,
        tracked: &[TrackedEntry],
        streams_by_login: &HashMap<String, StreamSnapshot>,
    ) -> Vec<ScoreRefresh> {
        let now = Utc::now();
        let now_iso = iso_seconds(now);

        let tracked_refs: Vec<TrackedStreamer> = tracked
            .iter()
            .map(|e| TrackedStreamer {
                login: e.login.clone(),
                twitch_user_id: e.twitch_user_id.clone(),
            })
            .collect();
        let snapshot = match self.live_state.load_snapshot(&tracked_refs).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::error!(%error, "Konnte Live-State-Snapshot nicht laden");
                return Vec::new();
            }
        };

        let mut rows: Vec<LiveStateUpsert> = Vec::new();
        let mut refreshes: Vec<ScoreRefresh> = Vec::new();

        for entry in tracked {
            let login_lower = entry.login.trim().to_lowercase();
            if login_lower.is_empty() {
                continue;
            }
            let stream = streams_by_login.get(&login_lower);
            let prev_entry = snapshot.get(&login_lower);
            let prev_state: Option<&LiveStateRow> = prev_entry.and_then(|e| e.state.as_ref());
            let was_live = prev_state.is_some_and(|s| s.is_live.unwrap_or(0) != 0);
            let is_live = stream.is_some();
            let twitch_user_id = entry.twitch_user_id.as_deref();
            let is_archived = entry.is_archived;

            // Go-Live: 4d registriert hier die stream.offline-Subscription.
            if !was_live && is_live && entry.is_partner_active {
                if let Some(user_id) = twitch_user_id {
                    if let Err(error) = self
                        .guard
                        .release(GuardKind::OfflineThrottle, user_id)
                        .await
                    {
                        tracing::debug!(%error, twitch_user_id = %user_id, "Poller Go-Live: OfflineThrottle konnte nicht freigegeben werden");
                    }
                    self.hooks.on_stream_went_live(user_id, &login_lower).await;
                }
            }

            let previous_game = prev_state
                .and_then(|s| s.last_game.as_deref())
                .unwrap_or("")
                .trim()
                .to_string();
            let previous_game_lower = previous_game.to_lowercase();
            let was_deadlock = previous_game_lower == self.target_game_lower;
            let prev_started_at = prev_state.and_then(|s| s.last_started_at.as_deref());
            let started_at_iso = extract_stream_start(
                stream.and_then(|s| s.started_at.as_deref()),
                prev_started_at,
            )
            .map(iso_seconds);
            let previous_stream_id = prev_state
                .and_then(|s| s.last_stream_id.as_deref())
                .unwrap_or("")
                .trim()
                .to_string();
            let current_stream_id = stream
                .and_then(|s| s.id.as_deref())
                .unwrap_or("")
                .trim()
                .to_string();
            let stream_id_value = if !current_stream_id.is_empty() {
                Some(current_stream_id.clone())
            } else if !previous_stream_id.is_empty() {
                Some(previous_stream_id.clone())
            } else {
                None
            };
            let mut had_deadlock_prev =
                prev_state.is_some_and(|s| s.had_deadlock_in_session.unwrap_or(0) != 0);
            let previous_last_deadlock_seen = prev_state
                .and_then(|s| s.last_deadlock_seen_at.clone())
                .filter(|v| !v.trim().is_empty());

            // Session-Lebenszyklus.
            let mut active_session_id: Option<i64> = None;
            if let Some(stream) = stream {
                active_session_id = self
                    .tracker
                    .ensure_session(&login_lower, stream, prev_started_at, twitch_user_id, now)
                    .await;
            } else if was_live {
                self.tracker
                    .finalize(&login_lower, "offline", None, None)
                    .await;
            } else if prev_state.is_some_and(|s| s.active_session_id.is_some()) {
                self.tracker
                    .finalize(&login_lower, "stale", None, None)
                    .await;
            }

            let stream_restarted = is_live
                && !previous_stream_id.is_empty()
                && !current_stream_id.is_empty()
                && previous_stream_id != current_stream_id;
            if !was_live {
                had_deadlock_prev = false;
            } else if stream_restarted {
                had_deadlock_prev = false;
                if let Some(user_id) = twitch_user_id {
                    if entry.is_partner_active {
                        refreshes.push(ScoreRefresh {
                            twitch_user_id: user_id.to_string(),
                            login: login_lower.clone(),
                            trigger: "poll_stream_restarted",
                        });
                    }
                }
            }
            if !is_live || stream_restarted {
                self.sink.on_stream_not_live(&login_lower).await;
            }

            let message_id_previous = prev_state
                .and_then(|s| s.last_discord_message_id.clone())
                .filter(|m| !m.trim().is_empty());
            let mut message_id_to_store = message_id_previous.clone();
            let tracking_token_previous = prev_state
                .and_then(|s| s.last_tracking_token.clone())
                .filter(|t| !t.trim().is_empty());
            let mut tracking_token_to_store = tracking_token_previous.clone();

            let game_name = stream
                .map(|s| s.game_name.trim().to_string())
                .unwrap_or_default();
            let is_deadlock = is_live
                && !self.target_game_lower.is_empty()
                && game_name.to_lowercase() == self.target_game_lower;

            // Informativ inaktiver Partner streamt wieder Deadlock → Flag loeschen.
            if is_live
                && entry.is_inactivity_flagged
                && is_deadlock
                && self.hooks.on_auto_unarchive(&login_lower).await
            {
                tracing::info!(login = %login_lower, "Partner-Inaktivitaetsflag geloescht");
            }
            let _ = is_archived; // Announcement-Gates (4e) nutzen entry-Daten.

            let had_deadlock_in_session = had_deadlock_prev || is_deadlock;

            // Spielwechsel → exp-Hook (Python ruft ihn nur live→live).
            if is_live
                && was_live
                && !game_name.is_empty()
                && !previous_game.is_empty()
                && game_name.to_lowercase() != previous_game_lower
            {
                let viewer_count = stream.map(|s| s.viewer_count).unwrap_or(0);
                self.tracker
                    .on_game_transition(&login_lower, &previous_game, &game_name, viewer_count, now)
                    .await;
            }

            let had_deadlock_to_store = if is_live {
                had_deadlock_in_session
            } else {
                false
            };
            let last_title_value = stream
                .map(|s| s.title.clone())
                .or_else(|| prev_state.and_then(|s| s.last_title.clone()))
                .filter(|t| !t.is_empty());
            let last_game_value = Some(game_name.clone())
                .filter(|g| !g.is_empty())
                .or_else(|| prev_state.and_then(|s| s.last_game.clone()))
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty());
            let last_viewer_count_value = stream
                .map(|s| s.viewer_count)
                .unwrap_or_else(|| prev_state.and_then(|s| s.last_viewer_count).unwrap_or(0));
            let last_deadlock_seen_at_value = if is_deadlock {
                Some(now_iso.clone())
            } else if had_deadlock_to_store {
                previous_last_deadlock_seen.clone()
            } else {
                None
            };

            // Go-Live-Posting (Python `should_post`).
            let should_post = self.sink.ready()
                && is_deadlock
                && (!was_live || !was_deadlock || message_id_previous.is_none())
                && entry.is_partner_active;
            if should_post {
                if let Some(stream) = stream {
                    let result = self
                        .sink
                        .announce_live(AnnounceLiveRequest {
                            login: login_lower.clone(),
                            entry: entry.clone(),
                            stream: stream.clone(),
                            previous_message_id: message_id_previous.clone(),
                            previous_tracking_token: tracking_token_previous.clone(),
                            stream_id: stream_id_value.clone(),
                            started_at_iso: started_at_iso.clone(),
                            active_session_id,
                        })
                        .await;
                    if let Some(result) = result {
                        message_id_to_store = Some(result.message_id);
                        tracking_token_to_store = result.tracking_token;
                        if let Some(session_id) = active_session_id {
                            if let Err(error) = self
                                .sessions
                                .set_notification_text(session_id, &result.notification_text)
                                .await
                            {
                                tracing::debug!(%error, login = %login_lower,
                                    "notification_text konnte nicht gespeichert werden");
                            }
                        }
                    } else {
                        tracing::warn!(login = %login_lower, "Go-Live-Posting fehlgeschlagen");
                    }
                }
            }

            // Posting beenden, wenn offline oder Kategorie verlassen.
            let ended_posting =
                self.sink.ready() && message_id_previous.is_some() && (!is_live || !is_deadlock);
            if ended_posting {
                let message_id = message_id_previous.clone().expect("geprüft");
                let display_name = stream
                    .map(|s| s.user_name.clone())
                    .filter(|n| !n.is_empty())
                    .or_else(|| prev_state.map(|s| s.streamer_login.clone()))
                    .unwrap_or_else(|| entry.login.clone());
                let outcome = self
                    .sink
                    .end_announcement(EndAnnouncementRequest {
                        login: login_lower.clone(),
                        display_name,
                        message_id,
                        previous_tracking_token: tracking_token_previous.clone(),
                        last_title: last_title_value.clone(),
                        last_game: last_game_value.clone(),
                        twitch_user_id: twitch_user_id
                            .map(str::to_string)
                            .or_else(|| prev_state.map(|s| s.twitch_user_id.clone())),
                    })
                    .await;
                match outcome {
                    EndAnnouncementOutcome::Updated | EndAnnouncementOutcome::Gone => {
                        message_id_to_store = None;
                        tracking_token_to_store = None;
                    }
                    EndAnnouncementOutcome::Failed => {}
                }
            }

            // user_id-Auflösung mit Fallback auf den letzten bekannten Wert
            // (Invariante 1: ohne user_id keine Row).
            let previous_user_id = prev_state
                .map(|s| s.twitch_user_id.trim().to_string())
                .unwrap_or_default();
            let mut db_user_id = twitch_user_id.unwrap_or("").trim().to_string();
            if db_user_id.is_empty()
                && !previous_user_id.is_empty()
                && previous_user_id.to_lowercase() != login_lower
            {
                db_user_id = previous_user_id;
            }
            if db_user_id.is_empty() {
                tracing::debug!(login = %login_lower, "Live-State-Write ohne user_id übersprungen");
                continue;
            }

            rows.push(LiveStateUpsert {
                twitch_user_id: db_user_id,
                streamer_login: login_lower.clone(),
                is_live: i32::from(is_live),
                last_seen_at: now_iso.clone(),
                last_title: last_title_value,
                last_game: last_game_value,
                last_viewer_count: last_viewer_count_value,
                last_discord_message_id: message_id_to_store,
                last_tracking_token: tracking_token_to_store,
                last_stream_id: stream_id_value,
                last_started_at: started_at_iso,
                had_deadlock_in_session: i32::from(had_deadlock_to_store),
                active_session_id,
                last_deadlock_seen_at: last_deadlock_seen_at_value,
            });

            if let Some(user_id) = twitch_user_id {
                if entry.is_partner_active {
                    if !was_live && is_live {
                        refreshes.push(ScoreRefresh {
                            twitch_user_id: user_id.to_string(),
                            login: login_lower.clone(),
                            trigger: "poll_stream_online",
                        });
                    } else if was_live && !is_live {
                        self.maybe_trigger_poll_offline_raid(user_id, &login_lower)
                            .await;
                        refreshes.push(ScoreRefresh {
                            twitch_user_id: user_id.to_string(),
                            login: login_lower.clone(),
                            trigger: "poll_stream_offline",
                        });
                    }
                }
            }
        }

        if let Err(error) = self.live_state.persist(&rows).await {
            tracing::error!(%error, count = rows.len(), "Konnte Live-State-Updates nicht speichern");
        }
        refreshes
    }

    /// Stats-Zeitreihen + Session-Samples (Python `_log_stats`).
    async fn log_stats(
        &self,
        streams_by_login: &HashMap<String, StreamSnapshot>,
        category_streams: &[StreamSnapshot],
        partner_logins: &HashSet<String>,
    ) {
        let now = Utc::now();
        let sample_of = |stream: &StreamSnapshot| StatsSample {
            streamer: stream.user_login.to_lowercase(),
            viewer_count: stream.viewer_count,
            is_partner: partner_logins.contains(&stream.user_login.to_lowercase()),
            game_name: stream.game_name_opt(),
            stream_title: stream.title_opt(),
            tags: stream.tags_json(),
            language: Some(stream.language.trim().to_lowercase()).filter(|l| !l.is_empty()),
        };

        // 1) Tracked-Stats: nur Ziel-Kategorie.
        let tracked_rows: Vec<StatsSample> = streams_by_login
            .values()
            .filter(|s| s.is_in_target_category(&self.target_game_lower))
            .map(sample_of)
            .collect();
        if let Err(error) = self.stats.log_tracked(now, &tracked_rows).await {
            tracing::error!(%error, "Konnte tracked-Stats nicht loggen");
        }

        // 2) Session-Samples: alle Spiele; Kategorie-Streams zusätzlich,
        //    sofern nicht schon über die tracked-Streams erfasst.
        let mut seen: HashSet<String> = HashSet::new();
        for (login, stream) in streams_by_login {
            self.tracker.record_sample(login, stream, now).await;
            seen.insert(login.clone());
        }
        for stream in category_streams {
            let login = stream.user_login.to_lowercase();
            if !login.is_empty() && !seen.contains(&login) {
                self.tracker.record_sample(&login, stream, now).await;
                seen.insert(login);
            }
        }

        // 3) Kategorie-Stats: alle Streams der Kategorie.
        let category_rows: Vec<StatsSample> = category_streams.iter().map(sample_of).collect();
        if let Err(error) = self.stats.log_category(now, &category_rows).await {
            tracing::error!(%error, "Konnte category-Stats nicht loggen");
        }
    }

    /// Informative Inaktivitaetsmarkierung (> 10 Tage keine bekannte Stream-
    /// Aktivitaet, gedrosselt auf alle 15 Minuten). Die eigentliche Lifecycle-
    /// Op macht der Hook; sie deaktiviert Partner nicht.
    async fn auto_archive_inactive(&self) {
        {
            let mut state = self.state.lock().expect("tick state lock");
            if state
                .last_archive_check
                .is_some_and(|last| last.elapsed() < AUTO_ARCHIVE_THROTTLE)
            {
                return;
            }
            state.last_archive_check = Some(Instant::now());
        }
        let cutoff = Utc::now() - chrono::Duration::days(AUTO_ARCHIVE_DAYS);
        let candidates = match self
            .tracked
            .archive_candidates(&self.config.target_game, cutoff)
            .await
        {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::debug!(%error, "Auto-Inaktivitaet: Kandidaten nicht ladbar");
                return;
            }
        };
        for login in candidates {
            if self.hooks.on_auto_archive(&login).await {
                tracing::info!(login, "Partner automatisch als inaktiv markiert");
            }
        }
    }

    async fn sweep_stale_live_if_due(&self) {
        let now = Instant::now();
        let due = {
            let mut state = self.state.lock().expect("tick state lock");
            let due = match state.last_stale_live_sweep {
                Some(last) => now.duration_since(last) >= STALE_LIVE_SWEEP_INTERVAL,
                None => true,
            };
            if due {
                state.last_stale_live_sweep = Some(now);
            }
            due
        };
        if !due {
            return;
        }
        if let Err(error) = self
            .live_state
            .sweep_stale_live(STALE_LIVE_MAX_AGE_SECS)
            .await
        {
            tracing::debug!(%error, "Stale Live-State-Sweep fehlgeschlagen");
        }
    }

    async fn ensure_category_id(&self) -> Option<String> {
        if let Some(id) = self
            .state
            .lock()
            .expect("tick state lock")
            .category_id
            .clone()
        {
            return Some(id);
        }
        match self.source.category_id(&self.config.target_game).await {
            Ok(Some(id)) => {
                tracing::debug!(category_id = %id, game = %self.config.target_game, "Kategorie-ID ermittelt");
                self.state.lock().expect("tick state lock").category_id = Some(id.clone());
                Some(id)
            }
            Ok(None) => None,
            Err(error) => {
                tracing::error!(%error, "Konnte Twitch-Kategorie-ID nicht ermitteln");
                None
            }
        }
    }
}

fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use sqlx::postgres::PgPoolOptions;

    use super::*;
    use crate::exp_sessions::{ExpSessionStore, ExpSessionTracker};
    use crate::poller::hooks::{NoopAnnouncementSink, NoopPollHooks};
    use crate::sessions::tracker::NoFollowerSource;

    /// Quelle mit Auth-Block-Schalter: zählt jeden Helix-Aufruf, damit der Test
    /// beweisen kann, dass bei `is_auth_blocked == true` keiner stattfindet.
    struct BlockableSource {
        blocked: bool,
        helix_calls: AtomicU64,
    }

    #[async_trait::async_trait]
    impl StreamSource for BlockableSource {
        async fn streams_by_logins(
            &self,
            _logins: &[String],
            _language: Option<&str>,
        ) -> Result<Vec<StreamSnapshot>, crate::poller::source::SourceError> {
            self.helix_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
        async fn streams_by_category(
            &self,
            _category_id: &str,
            _language: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<StreamSnapshot>, crate::poller::source::SourceError> {
            self.helix_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
        async fn category_id(
            &self,
            _game_name: &str,
        ) -> Result<Option<String>, crate::poller::source::SourceError> {
            self.helix_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some("g1".to_string()))
        }
        fn is_auth_blocked(&self) -> bool {
            self.blocked
        }
    }

    /// Engine mit lazy-Pool (verbindet NIE, weil der Tick vor jedem DB-Zugriff
    /// abbricht) auf der gegebenen Quelle.
    fn engine_on(source: Arc<BlockableSource>) -> PollEngine {
        // connect_lazy stellt erst beim ersten Query eine Verbindung her — im
        // Auth-Block-Pfad passiert das nie, daher braucht der Test keine DB.
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .expect("lazy pool");
        let tracker = Arc::new(SessionTracker::new(
            SessionStore::new(pool.clone()),
            LiveStateStore::new(pool.clone()),
            ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
            Arc::new(NoFollowerSource),
            "Deadlock",
        ));
        PollEngine::new(
            source,
            TrackedStore::new(pool.clone()),
            LiveStateStore::new(pool.clone()),
            SessionStore::new(pool.clone()),
            tracker,
            StatsStore::new(pool.clone()),
            GuardStore::new(pool.clone()),
            Arc::new(NoopAnnouncementSink),
            Arc::new(NoopPollHooks),
            PollIntervalStore::new(pool.clone()),
            PollConfig::default(),
        )
    }

    #[tokio::test]
    async fn auth_blocked_ueberspringt_tick_ohne_helix_oder_db() {
        let source = Arc::new(BlockableSource {
            blocked: true,
            helix_calls: AtomicU64::new(0),
        });
        let engine = engine_on(Arc::clone(&source));
        // Würde der Guard fehlen, käme hier ein Helix- ODER DB-Zugriff (auf den
        // ungültigen Lazy-Pool → Panic/Fehler). Der Tick muss sauber zurückkehren.
        engine.tick().await;
        assert_eq!(
            source.helix_calls.load(Ordering::SeqCst),
            0,
            "bei Auth-Block darf kein Helix-Request laufen"
        );
    }
}
