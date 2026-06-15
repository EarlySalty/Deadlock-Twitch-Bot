//! Session-Lebenszyklus-Orchestrierung (Python `_SessionsMixin`):
//! ensure → sample → finalize, mit In-Memory-Cache der offenen Sessions
//! (Start-Rehydrierung aus der DB) und den dünnen `exp_*`-Hooks.
//!
//! Nicht portierter Python-Seiteneffekt des Finalize: IRC-Lurker-Experiment-
//! Finalize (Chat). Das Partner-Raid-Score-Tracking-Resolve (Raid-Subsystem,
//! B7 `raid-scores-tracking-1`) läuft jetzt über den entkoppelten
//! [`RaidTrackingResolver`]-Port — die Composition-Root verdrahtet ihn gegen
//! `tb-raid`, sodass tb-monitoring nicht an die Raid-Tabellen koppelt.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};

use crate::exp_sessions::ExpSessionTracker;
use crate::live_state::LiveStateStore;
use crate::stream::{extract_stream_start, StreamSnapshot};

use super::metrics;
use super::store::{FinalizeUpdate, NewSession, SessionStore};

/// Liefert die Follower-Gesamtzahl eines Kanals (Helix in Prod, Stub in Tests).
/// `None` = nicht ermittelbar (best-effort, wie Python).
#[async_trait::async_trait]
pub trait FollowerCountSource: Send + Sync {
    async fn follower_total(&self, twitch_user_id: Option<&str>, login: &str) -> Option<i32>;
}

/// Quelle, die nie Follower-Zahlen liefert (Wiring-Default, Tests).
pub struct NoFollowerSource;

#[async_trait::async_trait]
impl FollowerCountSource for NoFollowerSource {
    async fn follower_total(&self, _twitch_user_id: Option<&str>, _login: &str) -> Option<i32> {
        None
    }
}

/// Löst beim Session-Finalize die offenen Partner-Raid-Score-Tracking-Zeilen der
/// Session auf (B7 `raid-scores-tracking-1`). Entkoppelt tb-monitoring von den
/// Raid-Tabellen — die echte Impl in der Composition-Root delegiert an
/// `tb_raid::ScoreTrackingStore::resolve_for_session`. Best-effort: liefert die
/// Anzahl aufgelöster Zeilen, schluckt Fehler (wie Python).
#[async_trait::async_trait]
pub trait RaidTrackingResolver: Send + Sync {
    async fn resolve_for_session(
        &self,
        twitch_user_id: Option<&str>,
        streamer_login: &str,
        session_id: i64,
        session_ended_at: DateTime<Utc>,
    ) -> i64;
}

/// Resolver ohne Wirkung (Wiring-Default ohne Raid-Subsystem, Tests).
pub struct NoRaidTrackingResolver;

#[async_trait::async_trait]
impl RaidTrackingResolver for NoRaidTrackingResolver {
    async fn resolve_for_session(
        &self,
        _twitch_user_id: Option<&str>,
        _streamer_login: &str,
        _session_id: i64,
        _session_ended_at: DateTime<Utc>,
    ) -> i64 {
        0
    }
}

pub struct SessionTracker {
    store: SessionStore,
    live_state: LiveStateStore,
    exp: ExpSessionTracker,
    followers: std::sync::Arc<dyn FollowerCountSource>,
    raid_resolver: std::sync::Arc<dyn RaidTrackingResolver>,
    target_game_lower: String,
    cache: Mutex<HashMap<String, i64>>,
}

impl SessionTracker {
    pub fn new(
        store: SessionStore,
        live_state: LiveStateStore,
        exp: ExpSessionTracker,
        followers: std::sync::Arc<dyn FollowerCountSource>,
        target_game: &str,
    ) -> Self {
        Self {
            store,
            live_state,
            exp,
            followers,
            raid_resolver: std::sync::Arc::new(NoRaidTrackingResolver),
            target_game_lower: target_game.trim().to_lowercase(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Verdrahtet den Raid-Score-Tracking-Resolver (Composition-Root, B7).
    /// Ohne diesen Aufruf bleibt der No-op-Resolver aktiv.
    pub fn with_raid_resolver(
        mut self,
        resolver: std::sync::Arc<dyn RaidTrackingResolver>,
    ) -> Self {
        self.raid_resolver = resolver;
        self
    }

    /// Cache aus offenen DB-Sessions neu aufbauen (Prozess-Start).
    pub async fn rehydrate(&self) {
        match self.store.list_open().await {
            Ok(open) => {
                let mut cache = self.cache.lock().expect("cache lock");
                cache.clear();
                for session in open {
                    let login = session.streamer_login.trim().to_lowercase();
                    if !login.is_empty() {
                        cache.insert(login, session.id);
                    }
                }
            }
            Err(error) => {
                tracing::debug!(%error, "Konnte offene Twitch-Sessions nicht laden");
            }
        }
    }

    /// Aktive Session-ID: Cache zuerst, sonst DB-Lookup (füllt den Cache).
    pub async fn active_session_id(&self, login: &str) -> Option<i64> {
        let login = login.to_lowercase();
        if let Some(id) = self.cache.lock().expect("cache lock").get(&login).copied() {
            return Some(id);
        }
        match self.store.find_open_id(&login).await {
            Ok(Some(id)) => {
                self.cache.lock().expect("cache lock").insert(login, id);
                Some(id)
            }
            Ok(None) => None,
            Err(error) => {
                tracing::debug!(%error, login, "Lookup offene Session fehlgeschlagen");
                None
            }
        }
    }

    /// Sorgt für eine offene Session (Python `_ensure_stream_session`):
    /// Stream-Neustart (andere stream_id) finalisiert die alte Session;
    /// bestehende Sessions werden ggf. adoptiert (Scout-Backfill).
    pub async fn ensure_session(
        &self,
        login: &str,
        stream: &StreamSnapshot,
        previous_started_at: Option<&str>,
        twitch_user_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<i64> {
        let login = login.to_lowercase();
        let stream_id = stream
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let mut session_id = self.active_session_id(&login).await;
        if let Some(id) = session_id {
            let current = self.store.stream_id_of(id).await.ok().flatten();
            if let (Some(current), Some(new_id)) = (current.as_deref(), stream_id) {
                if !current.is_empty() && current != new_id {
                    self.finalize(&login, "restarted", None, None).await;
                    session_id = None;
                }
            }
        }
        if let Some(id) = session_id {
            if let Err(error) = self
                .store
                .adopt_incomplete(
                    id,
                    stream.viewer_count,
                    stream.game_name_opt().as_deref(),
                    stream.is_in_target_category(&self.target_game_lower),
                    stream.title_opt().as_deref(),
                )
                .await
            {
                tracing::debug!(%error, login, "Konnte unvollständige Session nicht adoptieren");
            }
            return Some(id);
        }

        let followers_start = self.followers.follower_total(twitch_user_id, &login).await;
        let started_at =
            extract_stream_start(stream.started_at.as_deref(), previous_started_at).unwrap_or(now);

        let new = NewSession {
            streamer_login: login.clone(),
            stream_id: stream_id.map(str::to_string),
            started_at,
            viewer_count: stream.viewer_count,
            followers_start,
            title: stream.title.trim().to_string(),
            language: stream.language.trim().to_string(),
            is_mature: stream.is_mature,
            tags: stream.tags_joined(),
            game_name: stream.game_name_opt(),
            had_deadlock: stream.is_in_target_category(&self.target_game_lower),
        };
        let outcome = match self.store.start_session(&new).await {
            Ok(outcome) => outcome,
            Err(error) => {
                tracing::debug!(%error, login, "Konnte neue Twitch-Session nicht speichern");
                return None;
            }
        };
        let id = outcome.session_id();
        self.cache
            .lock()
            .expect("cache lock")
            .insert(login.clone(), id);
        self.exp.on_session_start(&login, stream, started_at).await;
        Some(id)
    }

    /// Viewer-Sample aufzeichnen (Python `_record_session_sample`) + exp-Hook.
    pub async fn record_sample(&self, login: &str, stream: &StreamSnapshot, now: DateTime<Utc>) {
        let login = login.to_lowercase();
        let Some(session_id) = self.active_session_id(&login).await else {
            return;
        };
        match self
            .store
            .record_sample(session_id, stream.viewer_count, now)
            .await
        {
            Ok(true) => self.exp.on_session_sample(&login, stream, now).await,
            Ok(false) => {}
            Err(error) => {
                tracing::debug!(%error, login, "Konnte Session-Sample nicht speichern");
            }
        }
    }

    /// Spielwechsel an die exp-Schicht durchreichen (Aufrufer: Poll-Loop).
    pub async fn on_game_transition(
        &self,
        login: &str,
        from_game: &str,
        to_game: &str,
        viewer_count: i32,
        now: DateTime<Utc>,
    ) {
        self.exp
            .on_game_transition(login, from_game, to_game, viewer_count, now)
            .await;
    }

    /// Session abschließen (Python `_finalize_stream_session`).
    /// `true` = abgeschlossen, `false` = keine offene Session / Fehler.
    pub async fn finalize(
        &self,
        login: &str,
        reason: &str,
        session_id: Option<i64>,
        ended_at: Option<DateTime<Utc>>,
    ) -> bool {
        let login = login.to_lowercase();
        let resolved = match session_id {
            Some(id) => Some(id),
            None => self.active_session_id(&login).await,
        };
        let Some(session_id) = resolved else {
            return false;
        };
        let now = ended_at.unwrap_or_else(Utc::now);

        let source = match self.store.finalize_source(session_id).await {
            Ok(Some(source)) => source,
            Ok(None) => {
                self.drop_cached(&login, session_id);
                return false;
            }
            Err(error) => {
                tracing::debug!(%error, login, "Konnte Session nicht laden für Abschluss");
                return false;
            }
        };
        let duration_seconds = (now - source.started_at).num_seconds().max(0) as i32;

        let samples = self
            .store
            .viewer_samples(session_id)
            .await
            .unwrap_or_default();
        let start_viewers = source.start_viewers.unwrap_or(0);
        let aggregates = metrics::final_aggregates(
            &samples,
            metrics::Aggregates {
                end_viewers: source.end_viewers.unwrap_or(0),
                peak_viewers: source.peak_viewers.unwrap_or(0),
                avg_viewers: source.avg_viewers.unwrap_or(0.0),
                samples: source.samples.unwrap_or(0),
            },
        );
        let retention_5 = metrics::retention_at(&samples, 5, start_viewers);
        let retention_10 = metrics::retention_at(&samples, 10, start_viewers);
        let retention_20 = metrics::retention_at(&samples, 20, start_viewers);
        let dropoff = metrics::max_dropoff(&samples, start_viewers);

        let (unique_chatters, first_time_chatters) =
            match self.store.chatter_counts(session_id).await {
                Ok(counts) => counts,
                Err(error) => {
                    tracing::debug!(%error, login, "Chatter-Zählung fehlgeschlagen");
                    (0, 0)
                }
            };
        let returning_chatters = (unique_chatters - first_time_chatters).max(0);

        let state = self.live_state.finalize_state(&login).await.ok().flatten();
        let twitch_user_id = state.as_ref().and_then(|s| s.twitch_user_id.clone());
        let last_game = state.as_ref().and_then(|s| s.last_game.clone());
        let had_deadlock_state = state
            .as_ref()
            .map(|s| s.had_deadlock_in_session.unwrap_or(0) != 0)
            .unwrap_or(false);

        let mut followers_end = self
            .followers
            .follower_total(twitch_user_id.as_deref(), &login)
            .await;
        let followers_start = source.followers_start;
        let follower_delta = match (followers_start, followers_end) {
            (Some(start), Some(end)) if end == 0 && start > 0 => {
                // API lieferte 0 ohne User-Token → als fehlende Daten behandeln.
                followers_end = None;
                None
            }
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        };

        let last_game_lower = last_game
            .as_deref()
            .map(|g| g.trim().to_lowercase())
            .unwrap_or_default();
        let had_deadlock_session = had_deadlock_state
            || (!self.target_game_lower.is_empty() && last_game_lower == self.target_game_lower);

        let update = FinalizeUpdate {
            session_id,
            streamer_login: login.clone(),
            ended_at: now,
            duration_seconds,
            end_viewers: aggregates.end_viewers,
            peak_viewers: aggregates.peak_viewers,
            avg_viewers: aggregates.avg_viewers,
            samples: aggregates.samples,
            retention_5m: retention_5,
            retention_10m: retention_10,
            retention_20m: retention_20,
            dropoff_pct: dropoff.as_ref().map(|d| d.pct),
            dropoff_label: dropoff.map(|d| d.label).unwrap_or_default(),
            unique_chatters: unique_chatters as i32,
            first_time_chatters: first_time_chatters as i32,
            returning_chatters: returning_chatters as i32,
            followers_end,
            follower_delta,
            notes: reason.to_string(),
            had_deadlock_in_session: had_deadlock_session,
            fallback_game_name: last_game,
        };
        match self.store.apply_finalize(&update).await {
            Ok(true) => {}
            Ok(false) => {
                // Doppel-Finalize-Race: anderer Pfad war schneller —
                // Kennzahlen nicht überschreiben, nur Cache aufräumen.
                tracing::debug!(login, session_id, "Session bereits abgeschlossen");
                self.drop_cached(&login, session_id);
                return false;
            }
            Err(error) => {
                tracing::debug!(%error, login, "Konnte Session-Abschluss nicht speichern");
                return false;
            }
        }

        self.drop_cached(&login, session_id);

        // Partner-Raid-Score-Tracking auflösen (B7 raid-scores-tracking-1):
        // sonst bleiben Deadlock-Raid-Zeilen dauerhaft offen (resolved_at NULL).
        // Reihenfolge wie Python: nach erfolgreichem Finalize, vor dem exp-Hook
        // unkritisch — best-effort, blockiert den Finalize nicht.
        self.raid_resolver
            .resolve_for_session(twitch_user_id.as_deref(), &login, session_id, now)
            .await;

        self.exp
            .on_session_finalize(&login, follower_delta, now)
            .await;
        true
    }

    /// Verwaiste offene Sessions schließen (Python `_cleanup_orphaned_sessions`).
    /// Liefert die Anzahl geschlossener Sessions.
    pub async fn cleanup_orphans(&self) -> usize {
        let (zero_sample, stale) = match self.store.orphan_candidates().await {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::debug!(%error, "Orphaned-Session-Cleanup fehlgeschlagen");
                return 0;
            }
        };
        let mut closed = 0;
        let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let candidates = zero_sample
            .into_iter()
            .map(|c| (c, "auto-closed: orphaned session (no samples, open > 24h)"))
            .chain(
                stale
                    .into_iter()
                    .map(|c| (c, "auto-closed: stale session (last viewer data > 1h ago)")),
            );
        for (candidate, reason) in candidates {
            if !seen.insert(candidate.id) {
                continue;
            }
            let login = candidate.streamer_login.trim().to_lowercase();
            if login.is_empty() {
                continue;
            }
            if self
                .finalize(
                    &login,
                    reason,
                    Some(candidate.id),
                    Some(candidate.finalized_at),
                )
                .await
            {
                closed += 1;
            }
        }
        closed
    }

    fn drop_cached(&self, login: &str, session_id: i64) {
        let mut cache = self.cache.lock().expect("cache lock");
        if cache.get(login).copied() == Some(session_id) {
            cache.remove(login);
        }
    }
}
