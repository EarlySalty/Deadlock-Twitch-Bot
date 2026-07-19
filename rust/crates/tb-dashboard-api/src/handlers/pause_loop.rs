//! Datenbankzugriff fuer die OBS-Pause-Loop-Kohorte.

#[cfg(test)]
use std::future::Future;
use std::{collections::HashSet, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::Serialize;
use sqlx::PgPool;
use tb_transport_twitch::{HelixClient, HelixClip};
use thiserror::Error;
use tokio::{sync::watch, sync::Mutex, task::JoinSet};

const DEADLOCK_CATEGORY_NAME: &str = "Deadlock";
const PAUSE_LOOP_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const PAUSE_LOOP_REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
const PAUSE_LOOP_RETRY_DELAY: Duration = Duration::from_secs(30);
const PARTNER_CLIP_TIMEOUT: Duration = Duration::from_secs(15);
const PARTNER_CLIP_LIMIT: usize = 500;
const MAX_PARALLEL_PARTNER_FETCHES: usize = 6;
const MIN_PLAYER_SAFE_DURATION: f64 = 5.0;
const MAX_PLAYER_SAFE_DURATION: f64 = 300.0;

/// Clip-Daten fuer den OBS-Pause-Loop-Player.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PauseLoopClip {
    /// Twitch-Clip-ID, getrimmt und global eindeutig im Pool.
    pub id: String,
    /// Anzeigename des Broadcasters; faellt bei leerer Helix-Antwort auf den Partner-Login zurueck.
    pub broadcaster_name: String,
    /// Clip-Titel, getrimmt.
    pub title: String,
    /// Player-sichere Clip-Dauer in Sekunden, auf 5 bis 300 Sekunden begrenzt.
    pub duration: f64,
}

/// Fehler beim Aufbau oder Abruf des Pause-Loop-Clip-Pools.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PauseLoopError {
    /// Helix konnte die Deadlock-Kategorie nicht finden.
    #[error("Deadlock-Kategorie nicht gefunden")]
    CategoryNotFound,
    /// Helix- oder Twitch-Transportfehler.
    #[error("Helix-Fehler: {0}")]
    Helix(String),
    /// Fehler beim Laden der aktiven Partner aus der Datenbank.
    #[error("Datenbank-Fehler: {0}")]
    Database(String),
    /// Ein begrenzter Refresh- oder Partnerabruf ist abgelaufen.
    #[error("Timeout bei {operation} nach {seconds}s")]
    Timeout {
        /// Betroffene Operation.
        operation: &'static str,
        /// Konfiguriertes Timeout in Sekunden.
        seconds: u64,
    },
    /// Es gab aktive Partner, aber kein Partnerabruf war erfolgreich.
    #[error("alle Partnerabrufe fehlgeschlagen ({failures}/{partners})")]
    AllPartnersFailed {
        /// Anzahl der aktiven Partner im Refresh.
        partners: usize,
        /// Anzahl fehlgeschlagener Partnerabrufe.
        failures: usize,
    },
    /// Ein Refresh lieferte nur einen Teilpool, weil mindestens ein Partnerabruf fehlschlug.
    #[error("Pause-Loop-Teilrefresh ({succeeded}/{partners} Partner erfolgreich, {failed} fehlgeschlagen)")]
    PartialRefresh {
        /// Anzahl der aktiven Partner im Refresh.
        partners: usize,
        /// Anzahl erfolgreicher Partnerabrufe.
        succeeded: usize,
        /// Anzahl fehlgeschlagener Partnerabrufe.
        failed: usize,
    },
    /// Der Hintergrund-Refresh endete, bevor er ein normales Ergebnis publizieren konnte.
    #[error("Pause-Loop-Refresh-Worker abnormal beendet: {reason}")]
    RefreshWorkerFailed {
        /// Abbruchgrund, z.B. `panic` oder `cancelled`.
        reason: &'static str,
    },
}

#[async_trait]
trait PauseLoopRefreshSource: Send + Sync {
    async fn refresh(&self) -> Result<PauseLoopRefreshReport, PauseLoopError>;
}

#[derive(Debug, Clone, PartialEq)]
struct PauseLoopRefreshReport {
    clips: Vec<PauseLoopClip>,
    partners: usize,
    succeeded: usize,
    failed: usize,
}

impl PauseLoopRefreshReport {
    #[cfg(test)]
    fn from_test_clips(clips: Vec<PauseLoopClip>) -> Self {
        Self {
            clips,
            partners: 0,
            succeeded: 0,
            failed: 0,
        }
    }
}

struct HelixPauseLoopRefreshSource {
    pool: PgPool,
    helix: HelixClient,
}

#[async_trait]
impl PauseLoopRefreshSource for HelixPauseLoopRefreshSource {
    async fn refresh(&self) -> Result<PauseLoopRefreshReport, PauseLoopError> {
        refresh_pause_loop_pool(&self.pool, &self.helix).await
    }
}

/// Routerlokaler Service fuer den OBS-Pause-Loop-Clip-Pool.
#[derive(Clone)]
pub struct PauseLoopService {
    source: Arc<dyn PauseLoopRefreshSource>,
    cache: Arc<Mutex<PauseLoopCacheState>>,
    ttl: Duration,
    refresh_timeout: Duration,
    retry_delay: Duration,
}

#[derive(Default)]
struct PauseLoopCacheState {
    entry: Option<PauseLoopCacheEntry>,
    in_flight: Option<Arc<PauseLoopInFlight>>,
    retry_not_before: Option<std::time::Instant>,
    last_error: Option<PauseLoopError>,
}

struct PauseLoopCacheEntry {
    clips: Vec<PauseLoopClip>,
    expires_at: std::time::Instant,
}

struct PauseLoopInFlight {
    result: watch::Sender<Option<Result<Vec<PauseLoopClip>, PauseLoopError>>>,
}

impl PauseLoopInFlight {
    fn new() -> Self {
        let (result, _receiver) = watch::channel(None);
        Self { result }
    }
}

impl PauseLoopService {
    /// Erstellt einen routerlokalen Pause-Loop-Service mit 15-Minuten-Cache.
    pub fn new(pool: PgPool, helix: HelixClient) -> Self {
        Self::from_source(
            Arc::new(HelixPauseLoopRefreshSource { pool, helix }),
            PAUSE_LOOP_CACHE_TTL,
            PAUSE_LOOP_REFRESH_TIMEOUT,
            PAUSE_LOOP_RETRY_DELAY,
        )
    }

    fn from_source(
        source: Arc<dyn PauseLoopRefreshSource>,
        ttl: Duration,
        refresh_timeout: Duration,
        retry_delay: Duration,
    ) -> Self {
        Self {
            source,
            cache: Arc::new(Mutex::new(PauseLoopCacheState::default())),
            ttl,
            refresh_timeout,
            retry_delay,
        }
    }

    #[cfg(test)]
    fn for_tests<F, Fut>(ttl: Duration, refresh: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<PauseLoopClip>, PauseLoopError>> + Send + 'static,
    {
        Self::for_tests_with_retry_delay(ttl, Duration::ZERO, refresh)
    }

    #[cfg(test)]
    fn for_tests_with_retry_delay<F, Fut>(ttl: Duration, retry_delay: Duration, refresh: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<PauseLoopClip>, PauseLoopError>> + Send + 'static,
    {
        struct FnPauseLoopRefreshSource<F> {
            refresh: F,
        }

        #[async_trait]
        impl<F, Fut> PauseLoopRefreshSource for FnPauseLoopRefreshSource<F>
        where
            F: Fn() -> Fut + Send + Sync,
            Fut: Future<Output = Result<Vec<PauseLoopClip>, PauseLoopError>> + Send,
        {
            async fn refresh(&self) -> Result<PauseLoopRefreshReport, PauseLoopError> {
                (self.refresh)()
                    .await
                    .map(PauseLoopRefreshReport::from_test_clips)
            }
        }

        Self::from_source(
            Arc::new(FnPauseLoopRefreshSource { refresh }),
            ttl,
            PAUSE_LOOP_REFRESH_TIMEOUT,
            retry_delay,
        )
    }

    #[cfg(test)]
    fn for_tests_reports_with_retry_delay<F, Fut>(
        ttl: Duration,
        retry_delay: Duration,
        refresh: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<PauseLoopRefreshReport, PauseLoopError>> + Send + 'static,
    {
        struct FnPauseLoopReportRefreshSource<F> {
            refresh: F,
        }

        #[async_trait]
        impl<F, Fut> PauseLoopRefreshSource for FnPauseLoopReportRefreshSource<F>
        where
            F: Fn() -> Fut + Send + Sync,
            Fut: Future<Output = Result<PauseLoopRefreshReport, PauseLoopError>> + Send,
        {
            async fn refresh(&self) -> Result<PauseLoopRefreshReport, PauseLoopError> {
                (self.refresh)().await
            }
        }

        Self::from_source(
            Arc::new(FnPauseLoopReportRefreshSource { refresh }),
            ttl,
            PAUSE_LOOP_REFRESH_TIMEOUT,
            retry_delay,
        )
    }

    /// Liefert den aktuellen Clip-Pool und refresh't kalte oder abgelaufene Eintraege singleflight.
    pub async fn clips(&self) -> Result<Vec<PauseLoopClip>, PauseLoopError> {
        let mut start_refresh: Option<(Arc<PauseLoopInFlight>, Option<Vec<PauseLoopClip>>)> = None;
        let wait_for = {
            let mut state = self.cache.lock().await;
            let now = std::time::Instant::now();

            if let Some(entry) = &state.entry {
                if entry.expires_at > now {
                    return Ok(entry.clips.clone());
                }
            }

            let stale = state.entry.as_ref().map(|entry| entry.clips.clone());

            if let Some(in_flight) = state.in_flight.clone() {
                if let Some(clips) = stale {
                    return Ok(clips);
                }
                in_flight
            } else if state
                .retry_not_before
                .is_some_and(|retry_not_before| now < retry_not_before)
            {
                if let Some(clips) = stale {
                    return Ok(clips);
                }
                if let Some(err) = state.last_error.clone() {
                    return Err(err);
                }

                let in_flight = Arc::new(PauseLoopInFlight::new());
                state.in_flight = Some(in_flight.clone());
                start_refresh = Some((in_flight.clone(), stale));
                in_flight
            } else {
                let in_flight = Arc::new(PauseLoopInFlight::new());
                state.in_flight = Some(in_flight.clone());
                start_refresh = Some((in_flight.clone(), stale));
                in_flight
            }
        };

        if let Some((in_flight, stale)) = start_refresh {
            self.spawn_refresh_worker(in_flight, stale);
        }

        wait_for_refresh(wait_for).await
    }

    fn spawn_refresh_worker(
        &self,
        in_flight: Arc<PauseLoopInFlight>,
        stale: Option<Vec<PauseLoopClip>>,
    ) {
        let source = self.source.clone();
        let cache = self.cache.clone();
        let refresh_timeout = self.refresh_timeout;
        let retry_delay = self.retry_delay;
        let ttl = self.ttl;

        tokio::spawn(async move {
            let mut guard = RefreshWorkerGuard::new(cache.clone(), in_flight.clone(), retry_delay);
            let stale_present = stale.is_some();
            let refresh_result = match tokio::time::timeout(refresh_timeout, source.refresh()).await
            {
                Ok(result) => result,
                Err(_) => Err(PauseLoopError::Timeout {
                    operation: "pause-loop-refresh",
                    seconds: refresh_timeout.as_secs(),
                }),
            };

            let response = match refresh_result {
                Ok(report) if report.failed == 0 => {
                    let partners = report.partners;
                    let succeeded = report.succeeded;
                    let failed = report.failed;
                    let clips = report.clips;
                    tracing::info!(
                        urteil = "erfolg",
                        partners,
                        succeeded,
                        failed,
                        clip_count = clips.len(),
                        "pause-loop pool refresh finished"
                    );

                    let response = Ok(clips.clone());
                    let mut state = cache.lock().await;
                    let now = std::time::Instant::now();
                    state.entry = Some(PauseLoopCacheEntry {
                        clips,
                        expires_at: now + ttl,
                    });
                    state.retry_not_before = None;
                    state.last_error = None;
                    clear_matching_inflight(&mut state, &in_flight);
                    response
                }
                Ok(report) => {
                    let partners = report.partners;
                    let succeeded = report.succeeded;
                    let failed = report.failed;
                    let partial_clips = report.clips;
                    let partial_error = PauseLoopError::PartialRefresh {
                        partners,
                        succeeded,
                        failed,
                    };
                    tracing::warn!(
                        urteil = "teilfehler",
                        partners,
                        succeeded,
                        failed,
                        clip_count = partial_clips.len(),
                        stale_present,
                        "pause-loop pool refresh finished"
                    );

                    let response = Ok(stale.clone().unwrap_or_else(|| partial_clips.clone()));
                    let mut state = cache.lock().await;
                    let now = std::time::Instant::now();
                    if !stale_present {
                        state.entry = Some(PauseLoopCacheEntry {
                            clips: partial_clips,
                            expires_at: now,
                        });
                    }
                    state.retry_not_before = Some(now + retry_delay);
                    state.last_error = Some(partial_error);
                    clear_matching_inflight(&mut state, &in_flight);
                    response
                }
                Err(err) => {
                    tracing::warn!(
                        urteil = "fehler",
                        grund = %err,
                        stale_present,
                        "pause-loop pool refresh finished"
                    );

                    let response = match stale {
                        Some(clips) => Ok(clips),
                        None => Err(err.clone()),
                    };
                    let mut state = cache.lock().await;
                    let now = std::time::Instant::now();
                    state.retry_not_before = Some(now + retry_delay);
                    state.last_error = Some(err);
                    clear_matching_inflight(&mut state, &in_flight);
                    response
                }
            };

            in_flight.result.send_replace(Some(response));
            guard.disarm();
        });
    }
}

struct RefreshWorkerGuard {
    armed: bool,
    cache: Arc<Mutex<PauseLoopCacheState>>,
    in_flight: Arc<PauseLoopInFlight>,
    retry_delay: Duration,
}

impl RefreshWorkerGuard {
    fn new(
        cache: Arc<Mutex<PauseLoopCacheState>>,
        in_flight: Arc<PauseLoopInFlight>,
        retry_delay: Duration,
    ) -> Self {
        Self {
            armed: true,
            cache,
            in_flight,
            retry_delay,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for RefreshWorkerGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let reason = if std::thread::panicking() {
            "panic"
        } else {
            "cancelled"
        };
        let err = PauseLoopError::RefreshWorkerFailed { reason };
        tracing::error!(
            urteil = "fehler",
            grund = %err,
            "pause-loop pool refresh worker ended abnormally"
        );
        publish_abnormal_worker_failure(
            self.cache.clone(),
            self.in_flight.clone(),
            self.retry_delay,
            err,
        );
    }
}

fn publish_abnormal_worker_failure(
    cache: Arc<Mutex<PauseLoopCacheState>>,
    in_flight: Arc<PauseLoopInFlight>,
    retry_delay: Duration,
    err: PauseLoopError,
) {
    if let Ok(mut state) = cache.try_lock() {
        apply_worker_failure_state(&mut state, &in_flight, retry_delay, err.clone());
        drop(state);
        in_flight.result.send_replace(Some(Err(err)));
        return;
    }

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let mut state = cache.lock().await;
            apply_worker_failure_state(&mut state, &in_flight, retry_delay, err.clone());
            drop(state);
            in_flight.result.send_replace(Some(Err(err)));
        });
    } else {
        tracing::error!(
            urteil = "fehler",
            grund = "no Tokio runtime handle for async cleanup",
            "pause-loop worker guard could not clear in-flight state"
        );
        in_flight.result.send_replace(Some(Err(err)));
    }
}

fn apply_worker_failure_state(
    state: &mut PauseLoopCacheState,
    in_flight: &Arc<PauseLoopInFlight>,
    retry_delay: Duration,
    err: PauseLoopError,
) {
    state.retry_not_before = Some(std::time::Instant::now() + retry_delay);
    state.last_error = Some(err);
    clear_matching_inflight(state, in_flight);
}

fn clear_matching_inflight(state: &mut PauseLoopCacheState, in_flight: &Arc<PauseLoopInFlight>) {
    if state
        .in_flight
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, in_flight))
    {
        state.in_flight = None;
    }
}

async fn wait_for_refresh(
    in_flight: Arc<PauseLoopInFlight>,
) -> Result<Vec<PauseLoopClip>, PauseLoopError> {
    let mut receiver = in_flight.result.subscribe();
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        if receiver.changed().await.is_err() {
            return Err(PauseLoopError::Helix(
                "pause-loop refresh worker ended without result".to_owned(),
            ));
        }
    }
}

#[derive(Debug, Error)]
enum PartnerClipFetchError {
    #[error("Helix-Fehler: {0}")]
    Helix(String),
    #[error("Timeout nach {0}s")]
    Timeout(u64),
}

struct PartnerClipFetchOutcome {
    index: usize,
    login: String,
    result: Result<Vec<PauseLoopClip>, PartnerClipFetchError>,
}

async fn refresh_pause_loop_pool(
    pool: &PgPool,
    helix: &HelixClient,
) -> Result<PauseLoopRefreshReport, PauseLoopError> {
    let game_id = helix
        .search_category_id(DEADLOCK_CATEGORY_NAME)
        .await
        .map_err(|err| PauseLoopError::Helix(err.to_string()))?
        .ok_or(PauseLoopError::CategoryNotFound)?;
    let partners = load_active_partner_broadcasters(pool)
        .await
        .map_err(|err| PauseLoopError::Database(err.to_string()))?;

    fetch_partner_clip_pool(helix, partners, &game_id).await
}

async fn fetch_partner_clip_pool(
    helix: &HelixClient,
    partners: Vec<PartnerBroadcaster>,
    game_id: &str,
) -> Result<PauseLoopRefreshReport, PauseLoopError> {
    let total_partners = partners.len();
    if total_partners == 0 {
        return Ok(PauseLoopRefreshReport {
            clips: Vec::new(),
            partners: 0,
            succeeded: 0,
            failed: 0,
        });
    }

    let mut partner_iter = partners.into_iter().enumerate();
    let mut join_set = JoinSet::new();
    let mut in_flight = 0usize;
    let mut failures = 0usize;
    let mut successes = 0usize;
    let mut successful_batches: Vec<(usize, Vec<PauseLoopClip>)> = Vec::new();

    loop {
        while in_flight < MAX_PARALLEL_PARTNER_FETCHES {
            let Some((index, partner)) = partner_iter.next() else {
                break;
            };
            let helix = helix.clone();
            let game_id = game_id.to_owned();
            join_set
                .spawn(async move { fetch_partner_clips(index, helix, partner, game_id).await });
            in_flight += 1;
        }

        if in_flight == 0 {
            break;
        }

        let Some(joined) = join_set.join_next().await else {
            failures += in_flight;
            tracing::error!(
                urteil = "task-fehler",
                grund = "join_next returned None despite tracked in-flight tasks",
                pending = in_flight,
                "pause-loop partner clip fetch task tracking failed"
            );
            break;
        };
        in_flight -= 1;
        match joined {
            Ok(outcome) => match outcome.result {
                Ok(clips) => {
                    successes += 1;
                    successful_batches.push((outcome.index, clips));
                }
                Err(err) => {
                    failures += 1;
                    tracing::warn!(
                        partner_login = %outcome.login,
                        urteil = "fehler",
                        grund = %err,
                        "pause-loop partner clip fetch failed"
                    );
                }
            },
            Err(err) => {
                failures += 1;
                tracing::warn!(
                    partner_login = "<unknown>",
                    urteil = "task-fehler",
                    grund = %err,
                    "pause-loop partner clip fetch task failed"
                );
            }
        }
    }

    if successes == 0 && failures > 0 {
        return Err(PauseLoopError::AllPartnersFailed {
            partners: total_partners,
            failures,
        });
    }

    successful_batches.sort_by_key(|(index, _)| *index);
    let clips = dedupe_pause_loop_clips(
        successful_batches
            .into_iter()
            .flat_map(|(_, clips)| clips)
            .collect(),
    );
    Ok(PauseLoopRefreshReport {
        clips,
        partners: total_partners,
        succeeded: successes,
        failed: failures,
    })
}

async fn fetch_partner_clips(
    index: usize,
    helix: HelixClient,
    partner: PartnerBroadcaster,
    game_id: String,
) -> PartnerClipFetchOutcome {
    let login = partner.twitch_login.clone();
    let result = match tokio::time::timeout(
        PARTNER_CLIP_TIMEOUT,
        helix.get_clips_by_broadcaster(&partner.twitch_user_id, PARTNER_CLIP_LIMIT),
    )
    .await
    {
        Ok(Ok(raw_clips)) => Ok(filter_partner_deadlock_clips(
            &partner.twitch_login,
            &game_id,
            raw_clips,
        )),
        Ok(Err(err)) => Err(PartnerClipFetchError::Helix(err.to_string())),
        Err(_) => Err(PartnerClipFetchError::Timeout(
            PARTNER_CLIP_TIMEOUT.as_secs(),
        )),
    };

    PartnerClipFetchOutcome {
        index,
        login,
        result,
    }
}

fn filter_partner_deadlock_clips(
    partner_login: &str,
    game_id: &str,
    raw_clips: Vec<HelixClip>,
) -> Vec<PauseLoopClip> {
    raw_clips
        .into_iter()
        .filter_map(|clip| filter_partner_deadlock_clip(partner_login, game_id, clip))
        .collect()
}

fn filter_partner_deadlock_clip(
    partner_login: &str,
    game_id: &str,
    clip: HelixClip,
) -> Option<PauseLoopClip> {
    if clip.game_id != game_id {
        return None;
    }
    let id = clip.id.trim();
    if id.is_empty() || !clip.duration.is_finite() || clip.duration <= 0.0 {
        return None;
    }
    let broadcaster_name = match clip.broadcaster_name.trim() {
        "" => partner_login.trim(),
        name => name,
    };

    Some(PauseLoopClip {
        id: id.to_owned(),
        broadcaster_name: broadcaster_name.to_owned(),
        title: clip.title.trim().to_owned(),
        duration: clip
            .duration
            .clamp(MIN_PLAYER_SAFE_DURATION, MAX_PLAYER_SAFE_DURATION),
    })
}

fn dedupe_pause_loop_clips(clips: Vec<PauseLoopClip>) -> Vec<PauseLoopClip> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(clips.len());
    for clip in clips {
        if seen.insert(clip.id.clone()) {
            deduped.push(clip);
        }
    }
    deduped
}

/// Aktiver Partner-Broadcaster, der fuer Pause-Loop-Clips beruecksichtigt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartnerBroadcaster {
    /// Getrimmter Twitch-Login.
    pub twitch_login: String,
    /// Getrimmte Twitch-User-ID.
    pub twitch_user_id: String,
}

/// Laedt aktive Partner mit gueltiger Twitch-Identitaet ohne aktive Exclusion.
pub async fn load_active_partner_broadcasters(
    pool: &PgPool,
) -> Result<Vec<PartnerBroadcaster>, sqlx::Error> {
    sqlx::query_as!(
        PartnerBroadcaster,
        r#"
        SELECT
            BTRIM(partner.twitch_login, E' \t\n\r\f') AS "twitch_login!",
            BTRIM(partner.twitch_user_id, E' \t\n\r\f') AS "twitch_user_id!"
        FROM twitch_streamers_partner_state partner
        WHERE COALESCE(partner.is_partner_active, 0) <> 0
          AND NULLIF(BTRIM(partner.twitch_login, E' \t\n\r\f'), '') IS NOT NULL
          AND NULLIF(BTRIM(partner.twitch_user_id, E' \t\n\r\f'), '') IS NOT NULL
          AND NOT EXISTS (
              SELECT 1
              FROM twitch_exclusions exclusion
              WHERE exclusion.twitch_user_id = partner.twitch_user_id
                AND exclusion.reactivated_at IS NULL
          )
        ORDER BY
            LOWER(BTRIM(partner.twitch_login, E' \t\n\r\f')),
            BTRIM(partner.twitch_login, E' \t\n\r\f'),
            BTRIM(partner.twitch_user_id, E' \t\n\r\f')
        "#
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::{
        str::FromStr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };
    use tb_transport_twitch::HelixClip;
    use tokio::sync::{oneshot, Mutex as TokioMutex};

    fn db_dsn_or_skip() -> Option<String> {
        match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => Some(dsn),
            Err(_) => {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                None
            }
        }
    }

    struct TestDb {
        schema: String,
        admin: PgPool,
        pool: PgPool,
    }

    impl TestDb {
        async fn new(schema_prefix: &str) -> Option<Self> {
            let dsn = db_dsn_or_skip()?;
            assert!(schema_prefix
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_'));
            let schema = format!("{}_{}", schema_prefix, uuid::Uuid::new_v4().simple());
            let admin = PgPoolOptions::new()
                .max_connections(1)
                .connect(&dsn)
                .await
                .expect("connect admin test-db");

            sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
                .execute(&admin)
                .await
                .expect("drop stale schema");
            sqlx::query(&format!("CREATE SCHEMA {schema}"))
                .execute(&admin)
                .await
                .expect("create schema");

            let opts = PgConnectOptions::from_str(&dsn)
                .expect("parse test dsn")
                .options([("search_path", schema.as_str())]);
            let pool = PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .expect("connect schema-bound pool");

            sqlx::raw_sql(
                r#"
                CREATE TABLE twitch_streamers_partner_state (
                    twitch_login TEXT,
                    twitch_user_id TEXT,
                    is_partner_active INTEGER
                );

                CREATE TABLE twitch_exclusions (
                    twitch_user_id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    reactivated_at TIMESTAMPTZ
                );
                "#,
            )
            .execute(&pool)
            .await
            .expect("pause loop fixture ddl");

            Some(Self {
                schema,
                admin,
                pool,
            })
        }

        async fn cleanup(self) {
            self.pool.close().await;
            sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
                .execute(&self.admin)
                .await
                .expect("drop schema");
            self.admin.close().await;
        }
    }

    async fn insert_partner(
        pool: &PgPool,
        login: Option<&str>,
        user_id: Option<&str>,
        active: Option<i32>,
    ) {
        sqlx::query(
            r#"
            INSERT INTO twitch_streamers_partner_state
                (twitch_login, twitch_user_id, is_partner_active)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(login)
        .bind(user_id)
        .bind(active)
        .execute(pool)
        .await
        .expect("insert partner fixture");
    }

    async fn insert_exclusion(pool: &PgPool, user_id: &str, reactivated: bool) {
        sqlx::query(
            r#"
            INSERT INTO twitch_exclusions (twitch_user_id, kind, reactivated_at)
            VALUES ($1, 'opt_out', CASE WHEN $2 THEN NOW() ELSE NULL END)
            "#,
        )
        .bind(user_id)
        .bind(reactivated)
        .execute(pool)
        .await
        .expect("insert exclusion fixture");
    }

    fn raw_clip(
        id: &str,
        game_id: &str,
        broadcaster_name: &str,
        title: &str,
        duration: f64,
    ) -> HelixClip {
        HelixClip {
            id: id.to_owned(),
            broadcaster_name: broadcaster_name.to_owned(),
            title: title.to_owned(),
            duration,
            game_id: game_id.to_owned(),
        }
    }

    fn pause_clip(id: &str) -> PauseLoopClip {
        PauseLoopClip {
            id: id.to_owned(),
            broadcaster_name: "Nani".to_owned(),
            title: format!("Clip {id}"),
            duration: 12.0,
        }
    }

    fn pause_report(
        clips: Vec<PauseLoopClip>,
        partners: usize,
        succeeded: usize,
        failed: usize,
    ) -> PauseLoopRefreshReport {
        PauseLoopRefreshReport {
            clips,
            partners,
            succeeded,
            failed,
        }
    }

    async fn panic_refresh(
        calls: Arc<AtomicUsize>,
    ) -> Result<PauseLoopRefreshReport, PauseLoopError> {
        calls.fetch_add(1, Ordering::SeqCst);
        panic!("refresh panic");
    }

    #[test]
    fn pause_loop_filtering_rejects_invalid_clamps_and_fills_fallback_name() {
        let clips = filter_partner_deadlock_clips(
            " partner_login ",
            "deadlock-game",
            vec![
                raw_clip("foreign", "other-game", "Name", "Foreign", 20.0),
                raw_clip("   ", "deadlock-game", "Name", "Blank", 20.0),
                raw_clip("nan", "deadlock-game", "Name", "NaN", f64::NAN),
                raw_clip("zero", "deadlock-game", "Name", "Zero", 0.0),
                raw_clip(" low ", "deadlock-game", " Name ", " Low ", 2.0),
                raw_clip("high", "deadlock-game", "Name", "High", 450.0),
                raw_clip("fallback", "deadlock-game", "   ", " Title ", 42.0),
            ],
        );

        assert_eq!(
            clips,
            vec![
                PauseLoopClip {
                    id: "low".to_owned(),
                    broadcaster_name: "Name".to_owned(),
                    title: "Low".to_owned(),
                    duration: 5.0,
                },
                PauseLoopClip {
                    id: "high".to_owned(),
                    broadcaster_name: "Name".to_owned(),
                    title: "High".to_owned(),
                    duration: 300.0,
                },
                PauseLoopClip {
                    id: "fallback".to_owned(),
                    broadcaster_name: "partner_login".to_owned(),
                    title: "Title".to_owned(),
                    duration: 42.0,
                },
            ]
        );
    }

    #[test]
    fn pause_loop_global_dedupe_keeps_first_clip_id() {
        let mut first = pause_clip("dup");
        first.title = "first".to_owned();
        let mut second = pause_clip("dup");
        second.title = "second".to_owned();

        let clips = dedupe_pause_loop_clips(vec![first.clone(), pause_clip("unique"), second]);

        assert_eq!(clips, vec![first, pause_clip("unique")]);
    }

    #[tokio::test]
    async fn pause_loop_cache_parallel_cold_misses_singleflight() {
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(TokioMutex::new(None));
        let (started_tx, started_rx) = oneshot::channel::<()>();
        *started.lock().await = Some(started_tx);
        let release = Arc::new(TokioMutex::new(None));
        let (release_tx, release_rx) = oneshot::channel::<()>();
        *release.lock().await = Some(release_rx);

        let service = PauseLoopService::for_tests(Duration::from_secs(60), {
            let calls = calls.clone();
            let started = started.clone();
            let release = release.clone();
            move || {
                let calls = calls.clone();
                let started = started.clone();
                let release = release.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    if let Some(tx) = started.lock().await.take() {
                        let _ = tx.send(());
                    }
                    let rx = release
                        .lock()
                        .await
                        .take()
                        .expect("release receiver present");
                    let _ = rx.await;
                    Ok(vec![pause_clip("singleflight")])
                }
            }
        });

        let first = tokio::spawn({
            let service = service.clone();
            async move { service.clips().await }
        });
        started_rx.await.expect("first refresh started");
        let second = tokio::spawn({
            let service = service.clone();
            async move { service.clips().await }
        });

        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release_tx.send(()).expect("release refresh");

        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();
        assert_eq!(first, vec![pause_clip("singleflight")]);
        assert_eq!(second, first);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn pause_loop_cache_aborted_cold_caller_does_not_stick_inflight() {
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(TokioMutex::new(None));
        let (started_tx, started_rx) = oneshot::channel::<()>();
        *started.lock().await = Some(started_tx);
        let release = Arc::new(TokioMutex::new(None));
        let (release_tx, release_rx) = oneshot::channel::<()>();
        *release.lock().await = Some(release_rx);

        let service = PauseLoopService::for_tests(Duration::from_secs(60), {
            let calls = calls.clone();
            let started = started.clone();
            let release = release.clone();
            move || {
                let calls = calls.clone();
                let started = started.clone();
                let release = release.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    if let Some(tx) = started.lock().await.take() {
                        let _ = tx.send(());
                    }
                    let rx = release
                        .lock()
                        .await
                        .take()
                        .expect("release receiver present");
                    let _ = rx.await;
                    Ok(vec![pause_clip("after-abort")])
                }
            }
        });

        let starter = tokio::spawn({
            let service = service.clone();
            async move { service.clips().await }
        });
        started_rx.await.expect("refresh started");
        starter.abort();
        assert!(starter.await.unwrap_err().is_cancelled());

        release_tx.send(()).expect("release refresh");

        let clips = tokio::time::timeout(Duration::from_millis(100), service.clips())
            .await
            .expect("service should recover after aborted caller")
            .unwrap();
        assert_eq!(clips, vec![pause_clip("after-abort")]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn pause_loop_cache_within_ttl_does_not_refresh() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = PauseLoopService::for_tests(Duration::from_secs(60), {
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![pause_clip("cached")])
                }
            }
        });

        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("cached")]);
        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("cached")]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn pause_loop_cache_refreshes_after_ttl() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = PauseLoopService::for_tests(Duration::from_millis(10), {
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(vec![pause_clip(&format!("clip-{call}"))])
                }
            }
        });

        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("clip-1")]);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("clip-2")]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_keeps_stale_on_refresh_error() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = PauseLoopService::for_tests(Duration::from_millis(10), {
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    if call == 1 {
                        Ok(vec![pause_clip("stale")])
                    } else {
                        Err(PauseLoopError::Helix("boom".to_owned()))
                    }
                }
            }
        });

        let stale = service.clips().await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        assert_eq!(service.clips().await.unwrap(), stale);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_stale_error_backoff_suppresses_retry_stampede() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = PauseLoopService::for_tests_with_retry_delay(
            Duration::from_millis(10),
            Duration::from_secs(60),
            {
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        if call == 1 {
                            Ok(vec![pause_clip("stale")])
                        } else {
                            Err(PauseLoopError::Helix(format!("boom-{call}")))
                        }
                    }
                }
            },
        );

        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("stale")]);
        tokio::time::sleep(Duration::from_millis(20)).await;

        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("stale")]);
        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("stale")]);
        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("stale")]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_cold_error_backoff_returns_same_error_without_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = PauseLoopService::for_tests_with_retry_delay(
            Duration::from_secs(60),
            Duration::from_secs(60),
            {
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        Err(PauseLoopError::Helix(format!("boom-{call}")))
                    }
                }
            },
        );

        let first = service.clips().await.unwrap_err();
        let second = service.clips().await.unwrap_err();

        assert_eq!(first, PauseLoopError::Helix("boom-1".to_owned()));
        assert_eq!(second, first);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn pause_loop_cache_worker_panic_publishes_error_and_backoff() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = PauseLoopService::for_tests_reports_with_retry_delay(
            Duration::from_secs(60),
            Duration::from_secs(60),
            {
                let calls = calls.clone();
                move || panic_refresh(calls.clone())
            },
        );

        let err = tokio::time::timeout(Duration::from_millis(100), service.clips())
            .await
            .expect("panic guard should publish a result")
            .unwrap_err();
        assert_eq!(err, PauseLoopError::RefreshWorkerFailed { reason: "panic" });

        let second = service.clips().await.unwrap_err();
        assert_eq!(second, err);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn pause_loop_cache_keeps_full_stale_pool_on_partial_refresh() {
        let calls = Arc::new(AtomicUsize::new(0));
        let full = vec![pause_clip("full-a"), pause_clip("full-b")];
        let partial = vec![pause_clip("partial")];
        let service = PauseLoopService::for_tests_reports_with_retry_delay(
            Duration::from_millis(10),
            Duration::from_secs(60),
            {
                let calls = calls.clone();
                let full = full.clone();
                let partial = partial.clone();
                move || {
                    let calls = calls.clone();
                    let full = full.clone();
                    let partial = partial.clone();
                    async move {
                        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        if call == 1 {
                            Ok(pause_report(full, 2, 2, 0))
                        } else {
                            Ok(pause_report(partial, 2, 1, 1))
                        }
                    }
                }
            },
        );

        assert_eq!(service.clips().await.unwrap(), full);
        tokio::time::sleep(Duration::from_millis(20)).await;

        assert_eq!(service.clips().await.unwrap(), full);
        assert_eq!(service.clips().await.unwrap(), full);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_cold_partial_is_degraded_and_retries_after_backoff() {
        let calls = Arc::new(AtomicUsize::new(0));
        let partial = vec![pause_clip("partial")];
        let fresh = vec![pause_clip("fresh")];
        let service = PauseLoopService::for_tests_reports_with_retry_delay(
            Duration::from_secs(60),
            Duration::from_millis(20),
            {
                let calls = calls.clone();
                let partial = partial.clone();
                let fresh = fresh.clone();
                move || {
                    let calls = calls.clone();
                    let partial = partial.clone();
                    let fresh = fresh.clone();
                    async move {
                        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        if call == 1 {
                            Ok(pause_report(partial, 2, 1, 1))
                        } else {
                            Ok(pause_report(fresh, 2, 2, 0))
                        }
                    }
                }
            },
        );

        assert_eq!(service.clips().await.unwrap(), partial);
        assert_eq!(service.clips().await.unwrap(), partial);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(service.clips().await.unwrap(), fresh);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_serves_stale_immediately_while_refresh_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(TokioMutex::new(None));
        let (started_tx, started_rx) = oneshot::channel::<()>();
        *started.lock().await = Some(started_tx);
        let release = Arc::new(TokioMutex::new(None));
        let (release_tx, release_rx) = oneshot::channel::<()>();
        *release.lock().await = Some(release_rx);

        let service = PauseLoopService::for_tests(Duration::from_millis(10), {
            let calls = calls.clone();
            let started = started.clone();
            let release = release.clone();
            move || {
                let calls = calls.clone();
                let started = started.clone();
                let release = release.clone();
                async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    if call == 1 {
                        return Ok(vec![pause_clip("stale")]);
                    }
                    if let Some(tx) = started.lock().await.take() {
                        let _ = tx.send(());
                    }
                    let rx = release
                        .lock()
                        .await
                        .take()
                        .expect("release receiver present");
                    let _ = rx.await;
                    Ok(vec![pause_clip("fresh")])
                }
            }
        });

        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("stale")]);
        tokio::time::sleep(Duration::from_millis(20)).await;
        let leader = tokio::spawn({
            let service = service.clone();
            async move { service.clips().await }
        });
        started_rx.await.expect("refresh started");

        let stale = tokio::time::timeout(Duration::from_millis(25), service.clips())
            .await
            .expect("stale response should not wait for refresh")
            .unwrap();
        assert_eq!(stale, vec![pause_clip("stale")]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        release_tx.send(()).expect("release refresh");
        assert_eq!(leader.await.unwrap().unwrap(), vec![pause_clip("fresh")]);
    }

    #[tokio::test]
    async fn pause_loop_cache_instances_do_not_share_entries() {
        let calls = Arc::new(AtomicUsize::new(0));
        let make_service = || {
            PauseLoopService::for_tests(Duration::from_secs(60), {
                let calls = calls.clone();
                move || {
                    let calls = calls.clone();
                    async move {
                        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        Ok(vec![pause_clip(&format!("clip-{call}"))])
                    }
                }
            })
        };
        let first = make_service();
        let second = make_service();

        assert_eq!(first.clips().await.unwrap(), vec![pause_clip("clip-1")]);
        assert_eq!(second.clips().await.unwrap(), vec![pause_clip("clip-2")]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn active_partner_query_filters_activity_and_exclusions() {
        let Some(db) = TestDb::new("pause_loop_active_exclusions").await else {
            return;
        };

        insert_partner(&db.pool, Some("active"), Some("100"), Some(1)).await;
        insert_partner(&db.pool, Some("inactive"), Some("101"), Some(0)).await;
        insert_partner(&db.pool, Some("null_active"), Some("102"), None).await;
        insert_partner(&db.pool, Some("excluded"), Some("103"), Some(1)).await;
        insert_partner(&db.pool, Some("reactivated"), Some("104"), Some(1)).await;
        insert_exclusion(&db.pool, "103", false).await;
        insert_exclusion(&db.pool, "104", true).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows,
            vec![
                PartnerBroadcaster {
                    twitch_login: "active".to_owned(),
                    twitch_user_id: "100".to_owned(),
                },
                PartnerBroadcaster {
                    twitch_login: "reactivated".to_owned(),
                    twitch_user_id: "104".to_owned(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn active_partner_query_rejects_blank_identity_and_trims_output() {
        let Some(db) = TestDb::new("pause_loop_identity_hygiene").await else {
            return;
        };

        insert_partner(&db.pool, None, Some("200"), Some(1)).await;
        insert_partner(&db.pool, Some(""), Some("201"), Some(1)).await;
        insert_partner(&db.pool, Some("   "), Some("202"), Some(1)).await;
        insert_partner(&db.pool, Some("missing_id"), None, Some(1)).await;
        insert_partner(&db.pool, Some("empty_id"), Some(""), Some(1)).await;
        insert_partner(&db.pool, Some("blank_id"), Some("  \t "), Some(1)).await;
        insert_partner(&db.pool, Some("  TrimMe  "), Some("  203  "), Some(1)).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows,
            vec![PartnerBroadcaster {
                twitch_login: "TrimMe".to_owned(),
                twitch_user_id: "203".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn active_partner_query_sorts_by_lower_trimmed_login() {
        let Some(db) = TestDb::new("pause_loop_ordering").await else {
            return;
        };

        insert_partner(&db.pool, Some(" zebra "), Some("300"), Some(1)).await;
        insert_partner(&db.pool, Some("Beta"), Some("301"), Some(1)).await;
        insert_partner(&db.pool, Some(" alpha"), Some("302"), Some(1)).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows.into_iter()
                .map(|row| row.twitch_login)
                .collect::<Vec<_>>(),
            vec!["alpha", "Beta", "zebra"]
        );
    }

    #[tokio::test]
    async fn active_partner_query_exclusion_nutzt_rohe_id_und_output_trim() {
        let Some(db) = TestDb::new("pause_loop_raw_exclusion").await else {
            return;
        };

        insert_partner(&db.pool, Some("ExcludedRaw"), Some(" 400 "), Some(1)).await;
        insert_partner(&db.pool, Some(" VisibleRaw "), Some(" 401 "), Some(1)).await;
        insert_exclusion(&db.pool, " 400 ", false).await;

        let rows = load_active_partner_broadcasters(&db.pool).await.unwrap();
        db.cleanup().await;

        assert_eq!(
            rows,
            vec![PartnerBroadcaster {
                twitch_login: "VisibleRaw".to_owned(),
                twitch_user_id: "401".to_owned(),
            }]
        );
    }
}
