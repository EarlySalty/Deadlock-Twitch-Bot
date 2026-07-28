//! Datenbankzugriff fuer die OBS-Pause-Loop-Kohorte.

#[cfg(test)]
use std::future::Future;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use tb_transport_twitch::{HelixClient, HelixClip};
use thiserror::Error;
use tokio::{sync::watch, sync::Mutex, task::JoinSet};
use tower_http::cors::CorsLayer;

const DEADLOCK_CATEGORY_NAME: &str = "Deadlock";
const PAUSE_LOOP_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const PAUSE_LOOP_REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
const PAUSE_LOOP_RETRY_DELAY: Duration = Duration::from_secs(30);
const PARTNER_CLIP_TIMEOUT: Duration = Duration::from_secs(15);
const PARTNER_CLIP_LIMIT: usize = 500;
const MAX_PARALLEL_PARTNER_FETCHES: usize = 6;
const MIN_PLAYER_SAFE_DURATION: f64 = 5.0;
const MAX_PLAYER_SAFE_DURATION: f64 = 300.0;
const PAUSE_LOOP_PLAYER_HTML: &str = include_str!("pause_loop_player.html");
const PAUSE_LOOP_RETRY_AFTER_SECONDS: &str = "30";
const PAUSE_LOOP_STALE_HEADER: &str = "x-pause-loop-stale";
/// CSP des Players. `connect-src` braucht die Twitch-Web-GQL-API, weil der Player
/// die MP4-Quelle jedes Clips dort aufloest; `media-src https:` deckt die
/// wechselnden, signierten Twitch-CDN-Hosts ab, die diese Aufloesung liefert.
const PAUSE_LOOP_CSP: &str = concat!(
    "default-src 'none'; ",
    "script-src 'self' 'unsafe-inline'; ",
    "style-src 'self' 'unsafe-inline'; ",
    "connect-src 'self' https://gql.twitch.tv; ",
    "media-src https:; ",
    "frame-ancestors 'self'; ",
    "base-uri 'none'; ",
    "form-action 'none'"
);

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

#[derive(Debug, Clone, PartialEq)]
struct PauseLoopCachedClip {
    clip: PauseLoopClip,
    partner_twitch_user_id: Option<String>,
}

impl PauseLoopCachedClip {
    fn with_partner(clip: PauseLoopClip, partner_twitch_user_id: &str) -> Self {
        let partner_twitch_user_id = trim_ascii_whitespace(partner_twitch_user_id);
        Self {
            clip,
            partner_twitch_user_id: (!partner_twitch_user_id.is_empty())
                .then(|| partner_twitch_user_id.to_owned()),
        }
    }

    #[cfg(test)]
    fn without_partner(clip: PauseLoopClip) -> Self {
        Self {
            clip,
            partner_twitch_user_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PauseLoopServiceSnapshot {
    clips: Vec<PauseLoopClip>,
    stale: bool,
}

impl PauseLoopServiceSnapshot {
    fn from_cached(clips: &[PauseLoopCachedClip], stale: bool) -> Self {
        Self {
            clips: clips.iter().map(|clip| clip.clip.clone()).collect(),
            stale,
        }
    }
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
    /// Pause-Loop ist ohne Helix-Credentials absichtlich nicht aktiv.
    #[error("Pause-Loop-Helix nicht konfiguriert")]
    NotConfigured,
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
    clips: Vec<PauseLoopCachedClip>,
    partners: usize,
    succeeded: usize,
    failed: usize,
    active_partner_user_ids: Vec<String>,
    successful_partner_user_ids: HashSet<String>,
}

impl PauseLoopRefreshReport {
    #[cfg(test)]
    fn from_test_clips(clips: Vec<PauseLoopClip>) -> Self {
        Self {
            clips: clips
                .into_iter()
                .map(PauseLoopCachedClip::without_partner)
                .collect(),
            partners: 0,
            succeeded: 0,
            failed: 0,
            active_partner_user_ids: Vec::new(),
            successful_partner_user_ids: HashSet::new(),
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

struct UnavailablePauseLoopRefreshSource;

#[async_trait]
impl PauseLoopRefreshSource for UnavailablePauseLoopRefreshSource {
    async fn refresh(&self) -> Result<PauseLoopRefreshReport, PauseLoopError> {
        Err(PauseLoopError::NotConfigured)
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
    clips: Vec<PauseLoopCachedClip>,
    expires_at: std::time::Instant,
}

struct PauseLoopInFlight {
    result: watch::Sender<Option<Result<PauseLoopServiceSnapshot, PauseLoopError>>>,
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

    #[cfg(test)]
    async fn expire_cache_entry_for_tests(&self) {
        let mut state = self.cache.lock().await;
        let entry = state.entry.as_mut().expect("cache entry present");
        entry.expires_at = std::time::Instant::now() - Duration::from_millis(1);
    }

    #[cfg(test)]
    async fn release_retry_for_tests(&self) {
        let mut state = self.cache.lock().await;
        state.retry_not_before = Some(std::time::Instant::now() - Duration::from_millis(1));
    }

    #[cfg(test)]
    async fn wait_for_in_flight_finished_for_tests(&self) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let done = {
                    let state = self.cache.lock().await;
                    state.in_flight.is_none()
                };
                if done {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pause-loop refresh should finish");
    }

    #[cfg(test)]
    async fn wait_for_in_flight_waiters_for_tests(&self, min_waiters: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let waiters = {
                    let state = self.cache.lock().await;
                    state
                        .in_flight
                        .as_ref()
                        .map(|in_flight| in_flight.result.receiver_count())
                        .unwrap_or_default()
                };
                if waiters >= min_waiters {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pause-loop refresh waiters should attach");
    }

    /// Liefert den aktuellen Clip-Pool und refresh't kalte oder abgelaufene Eintraege singleflight.
    pub async fn clips(&self) -> Result<Vec<PauseLoopClip>, PauseLoopError> {
        Ok(self.snapshot().await?.clips)
    }

    async fn snapshot(&self) -> Result<PauseLoopServiceSnapshot, PauseLoopError> {
        enum CacheDecision {
            Return(Result<PauseLoopServiceSnapshot, PauseLoopError>),
            Wait(Arc<PauseLoopInFlight>),
            StartAndWait {
                in_flight: Arc<PauseLoopInFlight>,
                stale: Option<Vec<PauseLoopCachedClip>>,
            },
            StartAndReturnStale {
                in_flight: Arc<PauseLoopInFlight>,
                stale: Vec<PauseLoopCachedClip>,
            },
        }

        let decision = {
            let mut state = self.cache.lock().await;
            let now = std::time::Instant::now();

            if let Some(entry) = &state.entry {
                if entry.expires_at > now {
                    return Ok(PauseLoopServiceSnapshot::from_cached(&entry.clips, false));
                }
            }

            let stale = state.entry.as_ref().map(|entry| entry.clips.clone());

            if let Some(in_flight) = state.in_flight.clone() {
                if let Some(clips) = stale {
                    CacheDecision::Return(Ok(PauseLoopServiceSnapshot::from_cached(&clips, true)))
                } else {
                    CacheDecision::Wait(in_flight)
                }
            } else if state
                .retry_not_before
                .is_some_and(|retry_not_before| now < retry_not_before)
            {
                if let Some(clips) = stale {
                    CacheDecision::Return(Ok(PauseLoopServiceSnapshot::from_cached(&clips, true)))
                } else if let Some(err) = state.last_error.clone() {
                    CacheDecision::Return(Err(err))
                } else {
                    let in_flight = Arc::new(PauseLoopInFlight::new());
                    state.in_flight = Some(in_flight.clone());
                    CacheDecision::StartAndWait {
                        in_flight,
                        stale: None,
                    }
                }
            } else {
                let in_flight = Arc::new(PauseLoopInFlight::new());
                state.in_flight = Some(in_flight.clone());
                if let Some(stale) = stale {
                    CacheDecision::StartAndReturnStale { in_flight, stale }
                } else {
                    CacheDecision::StartAndWait {
                        in_flight,
                        stale: None,
                    }
                }
            }
        };

        match decision {
            CacheDecision::Return(result) => result,
            CacheDecision::Wait(in_flight) => wait_for_refresh(in_flight).await,
            CacheDecision::StartAndWait { in_flight, stale } => {
                self.spawn_refresh_worker(in_flight.clone(), stale);
                wait_for_refresh(in_flight).await
            }
            CacheDecision::StartAndReturnStale { in_flight, stale } => {
                let snapshot = PauseLoopServiceSnapshot::from_cached(&stale, true);
                self.spawn_refresh_worker(in_flight, Some(stale));
                Ok(snapshot)
            }
        }
    }

    fn spawn_refresh_worker(
        &self,
        in_flight: Arc<PauseLoopInFlight>,
        stale: Option<Vec<PauseLoopCachedClip>>,
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

                    let response = Ok(PauseLoopServiceSnapshot::from_cached(&clips, false));
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
                    let merged_clips = merge_degraded_refresh_clips(&report, stale.as_deref());
                    let partial_error = degraded_refresh_error(&report);
                    tracing::warn!(
                        urteil = "teilfehler",
                        partners,
                        succeeded,
                        failed,
                        clip_count = merged_clips.len(),
                        stale_present,
                        "pause-loop pool refresh finished"
                    );

                    let response = if succeeded == 0 && !stale_present {
                        Err(partial_error.clone())
                    } else {
                        Ok(PauseLoopServiceSnapshot::from_cached(&merged_clips, true))
                    };
                    let mut state = cache.lock().await;
                    let now = std::time::Instant::now();
                    if response.is_ok() {
                        state.entry = Some(PauseLoopCacheEntry {
                            clips: merged_clips,
                            expires_at: now,
                        });
                    } else {
                        state.entry = None;
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

                    let response = Err(err.clone());
                    let mut state = cache.lock().await;
                    let now = std::time::Instant::now();
                    state.entry = None;
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

/// Baut den oeffentlichen Pause-Loop-Router mit routerlokalem Service/Cache.
pub fn build_pause_loop_router(pool: PgPool, helix: HelixClient) -> Router {
    build_pause_loop_router_from_service(PauseLoopService::new(pool, helix))
}

pub(crate) fn build_unavailable_pause_loop_router() -> Router {
    build_pause_loop_router_from_service(PauseLoopService::from_source(
        Arc::new(UnavailablePauseLoopRefreshSource),
        PAUSE_LOOP_CACHE_TTL,
        PAUSE_LOOP_REFRESH_TIMEOUT,
        PAUSE_LOOP_RETRY_DELAY,
    ))
}

fn build_pause_loop_router_from_service(service: PauseLoopService) -> Router {
    Router::new()
        .route(
            "/twitch/api/v2/public/pause-loop-clips",
            get(pause_loop_clips_handler),
        )
        .route("/twitch/pause-loop", get(pause_loop_player_handler))
        .with_state(service)
        .layer(CorsLayer::permissive())
}

#[cfg(test)]
fn build_pause_loop_router_with_service(service: PauseLoopService) -> Router {
    build_pause_loop_router_from_service(service)
}

async fn pause_loop_clips_handler(State(service): State<PauseLoopService>) -> Response {
    match service.snapshot().await {
        Ok(snapshot) => {
            let mut response = Json(snapshot.clips).into_response();
            let headers = response.headers_mut();
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            if snapshot.stale {
                headers.insert(PAUSE_LOOP_STALE_HEADER, HeaderValue::from_static("1"));
            }
            response
        }
        Err(err) => {
            tracing::warn!(
                urteil = "fehler",
                grund = %err,
                "pause-loop public clip request failed"
            );
            let mut response = (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "pause_loop_unavailable" })),
            )
                .into_response();
            let headers = response.headers_mut();
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            headers.insert(
                header::RETRY_AFTER,
                HeaderValue::from_static(PAUSE_LOOP_RETRY_AFTER_SECONDS),
            );
            response
        }
    }
}

async fn pause_loop_player_handler() -> Response {
    let mut response = Html(PAUSE_LOOP_PLAYER_HTML).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-robots-tag",
        HeaderValue::from_static("noindex, nofollow"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("SAMEORIGIN"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(PAUSE_LOOP_CSP),
    );
    response
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
    state.entry = None;
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
) -> Result<PauseLoopServiceSnapshot, PauseLoopError> {
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

fn degraded_refresh_error(report: &PauseLoopRefreshReport) -> PauseLoopError {
    if report.succeeded == 0 && report.failed > 0 {
        PauseLoopError::AllPartnersFailed {
            partners: report.partners,
            failures: report.failed,
        }
    } else {
        PauseLoopError::PartialRefresh {
            partners: report.partners,
            succeeded: report.succeeded,
            failed: report.failed,
        }
    }
}

fn merge_degraded_refresh_clips(
    report: &PauseLoopRefreshReport,
    stale: Option<&[PauseLoopCachedClip]>,
) -> Vec<PauseLoopCachedClip> {
    let active_partner_ids = report
        .active_partner_user_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let successful_partner_ids = &report.successful_partner_user_ids;

    let mut fresh_by_partner: HashMap<String, Vec<PauseLoopCachedClip>> = HashMap::new();
    for clip in report.clips.iter().cloned() {
        let Some(partner_id) = clip.partner_twitch_user_id.as_deref() else {
            continue;
        };
        if successful_partner_ids.contains(partner_id) {
            fresh_by_partner
                .entry(partner_id.to_owned())
                .or_default()
                .push(clip);
        }
    }

    let mut stale_by_partner: HashMap<String, Vec<PauseLoopCachedClip>> = HashMap::new();
    for clip in stale.unwrap_or(&[]).iter().cloned() {
        let Some(partner_id) = clip.partner_twitch_user_id.as_deref() else {
            continue;
        };
        if active_partner_ids.contains(partner_id) && !successful_partner_ids.contains(partner_id) {
            stale_by_partner
                .entry(partner_id.to_owned())
                .or_default()
                .push(clip);
        }
    }

    let mut merged = Vec::new();
    for partner_id in &report.active_partner_user_ids {
        if successful_partner_ids.contains(partner_id) {
            if let Some(clips) = fresh_by_partner.remove(partner_id) {
                merged.extend(clips);
            }
        } else if let Some(clips) = stale_by_partner.remove(partner_id) {
            merged.extend(clips);
        }
    }
    dedupe_cached_pause_loop_clips(merged)
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
    user_id: String,
    result: Result<Vec<PauseLoopCachedClip>, PartnerClipFetchError>,
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
            active_partner_user_ids: Vec::new(),
            successful_partner_user_ids: HashSet::new(),
        });
    }

    let active_partner_user_ids = partners
        .iter()
        .map(|partner| partner.twitch_user_id.clone())
        .collect::<Vec<_>>();
    let mut partner_iter = partners.into_iter().enumerate();
    let mut join_set = JoinSet::new();
    let mut in_flight = 0usize;
    let mut failures = 0usize;
    let mut successes = 0usize;
    let mut successful_partner_user_ids = HashSet::new();
    let mut successful_batches: Vec<(usize, Vec<PauseLoopCachedClip>)> = Vec::new();

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
                    successful_partner_user_ids.insert(outcome.user_id);
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

    successful_batches.sort_by_key(|(index, _)| *index);
    let clips = dedupe_cached_pause_loop_clips(
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
        active_partner_user_ids,
        successful_partner_user_ids,
    })
}

async fn fetch_partner_clips(
    index: usize,
    helix: HelixClient,
    partner: PartnerBroadcaster,
    game_id: String,
) -> PartnerClipFetchOutcome {
    let login = partner.twitch_login.clone();
    let user_id = partner.twitch_user_id.clone();
    let result = match tokio::time::timeout(
        PARTNER_CLIP_TIMEOUT,
        helix.get_clips_by_broadcaster(&partner.twitch_user_id, PARTNER_CLIP_LIMIT),
    )
    .await
    {
        Ok(Ok(raw_clips)) => Ok(filter_partner_deadlock_clips(
            &partner.twitch_login,
            &partner.twitch_user_id,
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
        user_id,
        result,
    }
}

fn filter_partner_deadlock_clips(
    partner_login: &str,
    partner_twitch_user_id: &str,
    game_id: &str,
    raw_clips: Vec<HelixClip>,
) -> Vec<PauseLoopCachedClip> {
    raw_clips
        .into_iter()
        .filter_map(|clip| {
            filter_partner_deadlock_clip(partner_login, partner_twitch_user_id, game_id, clip)
        })
        .collect()
}

fn filter_partner_deadlock_clip(
    partner_login: &str,
    partner_twitch_user_id: &str,
    game_id: &str,
    clip: HelixClip,
) -> Option<PauseLoopCachedClip> {
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

    Some(PauseLoopCachedClip::with_partner(
        PauseLoopClip {
            id: id.to_owned(),
            broadcaster_name: broadcaster_name.to_owned(),
            title: clip.title.trim().to_owned(),
            duration: clip
                .duration
                .clamp(MIN_PLAYER_SAFE_DURATION, MAX_PLAYER_SAFE_DURATION),
        },
        partner_twitch_user_id,
    ))
}

fn dedupe_cached_pause_loop_clips(clips: Vec<PauseLoopCachedClip>) -> Vec<PauseLoopCachedClip> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(clips.len());
    for clip in clips {
        if seen.insert(clip.clip.id.clone()) {
            deduped.push(clip);
        }
    }
    deduped
}

fn trim_ascii_whitespace(value: &str) -> &str {
    value.trim_matches(|ch: char| ch.is_ascii_whitespace())
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
              WHERE BTRIM(exclusion.twitch_user_id, E' \t\n\r\f') = BTRIM(partner.twitch_user_id, E' \t\n\r\f')
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
    use axum::{
        body::{to_bytes, Body},
        http::{header, HeaderMap, Method, Request, StatusCode},
        Router,
    };
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::{
        str::FromStr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };
    use tb_transport_twitch::{HelixClip, HelixConfig};
    use tokio::sync::{oneshot, Mutex as TokioMutex};
    use tower::ServiceExt;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn route_bytes(
        app: Router,
        method: Method,
        uri: &str,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::ORIGIN, "https://obs.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, headers, body)
    }

    fn helix_client(server: &MockServer) -> HelixClient {
        HelixClient::new(HelixConfig {
            client_id: "cid".to_owned(),
            client_secret: "sec".to_owned(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

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

    fn cached_pause_clip(id: &str, partner_id: &str) -> PauseLoopCachedClip {
        PauseLoopCachedClip::with_partner(pause_clip(id), partner_id)
    }

    fn public_clips(clips: &[PauseLoopCachedClip]) -> Vec<PauseLoopClip> {
        PauseLoopServiceSnapshot::from_cached(clips, false).clips
    }

    fn pause_report(
        clips: Vec<PauseLoopCachedClip>,
        active_partner_user_ids: &[&str],
        successful_partner_user_ids: &[&str],
        failed: usize,
    ) -> PauseLoopRefreshReport {
        PauseLoopRefreshReport {
            clips,
            partners: active_partner_user_ids.len(),
            succeeded: successful_partner_user_ids.len(),
            failed,
            active_partner_user_ids: active_partner_user_ids
                .iter()
                .map(|partner_id| (*partner_id).to_owned())
                .collect(),
            successful_partner_user_ids: successful_partner_user_ids
                .iter()
                .map(|partner_id| (*partner_id).to_owned())
                .collect(),
        }
    }

    fn full_pause_report(
        clips: Vec<PauseLoopCachedClip>,
        active_partner_user_ids: &[&str],
    ) -> PauseLoopRefreshReport {
        pause_report(clips, active_partner_user_ids, active_partner_user_ids, 0)
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
            " 42 ",
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
            public_clips(&clips),
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
        let mut first = cached_pause_clip("dup", "A");
        first.clip.title = "first".to_owned();
        let mut second = cached_pause_clip("dup", "B");
        second.clip.title = "second".to_owned();

        let clips = dedupe_cached_pause_loop_clips(vec![
            first.clone(),
            cached_pause_clip("unique", "B"),
            second,
        ]);

        assert_eq!(clips, vec![first, cached_pause_clip("unique", "B")]);
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

        service.wait_for_in_flight_waiters_for_tests(2).await;
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
        service.expire_cache_entry_for_tests().await;
        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("clip-1")]);
        service.wait_for_in_flight_finished_for_tests().await;
        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("clip-2")]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_clears_stale_after_unvalidated_refresh_error() {
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
                            Err(PauseLoopError::Helix("boom".to_owned()))
                        }
                    }
                }
            },
        );

        let stale = service.clips().await.unwrap();
        service.expire_cache_entry_for_tests().await;

        assert_eq!(service.clips().await.unwrap(), stale);
        service.wait_for_in_flight_finished_for_tests().await;
        assert_eq!(
            service.clips().await.unwrap_err(),
            PauseLoopError::Helix("boom".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_unvalidated_error_backoff_suppresses_retry_stampede() {
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
        service.expire_cache_entry_for_tests().await;

        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("stale")]);
        service.wait_for_in_flight_finished_for_tests().await;
        let first = service.clips().await.unwrap_err();
        let second = service.clips().await.unwrap_err();
        let third = service.clips().await.unwrap_err();
        assert_eq!(first, PauseLoopError::Helix("boom-2".to_owned()));
        assert_eq!(second, first);
        assert_eq!(third, first);
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
    async fn pause_loop_cache_partial_refresh_does_not_keep_full_stale_pool() {
        let calls = Arc::new(AtomicUsize::new(0));
        let full = vec![
            cached_pause_clip("old-a", "A"),
            cached_pause_clip("old-b", "B"),
            cached_pause_clip("old-c", "C"),
        ];
        let merged = vec![
            cached_pause_clip("fresh-b", "B"),
            cached_pause_clip("old-c", "C"),
        ];
        let full_public = public_clips(&full);
        let merged_public = public_clips(&merged);
        let service = PauseLoopService::for_tests_reports_with_retry_delay(
            Duration::from_millis(10),
            Duration::from_secs(60),
            {
                let calls = calls.clone();
                let full = full.clone();
                let merged = merged.clone();
                move || {
                    let calls = calls.clone();
                    let full = full.clone();
                    let merged = merged.clone();
                    async move {
                        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        if call == 1 {
                            Ok(full_pause_report(full, &["A", "B", "C"]))
                        } else {
                            Ok(pause_report(
                                vec![merged[0].clone()],
                                &["B", "C"],
                                &["B"],
                                1,
                            ))
                        }
                    }
                }
            },
        );

        assert_eq!(service.clips().await.unwrap(), full_public);
        service.expire_cache_entry_for_tests().await;

        assert_eq!(service.clips().await.unwrap(), full_public);
        service.wait_for_in_flight_finished_for_tests().await;
        assert_eq!(service.clips().await.unwrap(), merged_public);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_cache_cold_partial_is_degraded_and_retries_after_backoff() {
        let calls = Arc::new(AtomicUsize::new(0));
        let partial = vec![cached_pause_clip("partial", "A")];
        let fresh = vec![cached_pause_clip("fresh", "A")];
        let partial_public = public_clips(&partial);
        let fresh_public = public_clips(&fresh);
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
                            Ok(pause_report(partial, &["A", "B"], &["A"], 1))
                        } else {
                            Ok(full_pause_report(fresh, &["A", "B"]))
                        }
                    }
                }
            },
        );

        assert_eq!(service.clips().await.unwrap(), partial_public);
        assert_eq!(service.clips().await.unwrap(), partial_public);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        service.release_retry_for_tests().await;
        assert_eq!(service.clips().await.unwrap(), partial_public);
        service.wait_for_in_flight_finished_for_tests().await;
        assert_eq!(service.clips().await.unwrap(), fresh_public);
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
        service.expire_cache_entry_for_tests().await;
        let stale = tokio::time::timeout(Duration::from_millis(25), service.clips())
            .await
            .expect("first stale response after TTL should not wait for refresh")
            .unwrap();
        assert_eq!(stale, vec![pause_clip("stale")]);
        started_rx.await.expect("refresh started");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        release_tx.send(()).expect("release refresh");
        service.wait_for_in_flight_finished_for_tests().await;
        assert_eq!(service.clips().await.unwrap(), vec![pause_clip("fresh")]);
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
    async fn active_partner_query_normalisiert_partner_und_exclusion_id_gleich() {
        let Some(db) = TestDb::new("pause_loop_normalized_exclusion").await else {
            return;
        };

        insert_partner(&db.pool, Some("ExcludedRaw"), Some(" 400 "), Some(1)).await;
        insert_partner(&db.pool, Some(" VisibleRaw "), Some(" 401 "), Some(1)).await;
        insert_exclusion(&db.pool, "400", false).await;

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

    #[tokio::test]
    async fn pause_loop_public_json_success_exact_array_no_store_and_cors() {
        let clips = vec![PauseLoopClip {
            id: "clip-one".to_owned(),
            broadcaster_name: "Nani".to_owned(),
            title: "Erster Clip".to_owned(),
            duration: 12.5,
        }];
        let service = PauseLoopService::for_tests(Duration::from_secs(60), {
            let clips = clips.clone();
            move || {
                let clips = clips.clone();
                async move { Ok(clips) }
            }
        });

        let (status, headers, body) = route_bytes(
            build_pause_loop_router_with_service(service),
            Method::GET,
            "/twitch/api/v2/public/pause-loop-clips",
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(headers.get("access-control-allow-origin").unwrap(), "*");
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{
                "id": "clip-one",
                "broadcaster_name": "Nani",
                "title": "Erster Clip",
                "duration": 12.5
            }])
        );
    }

    #[tokio::test]
    async fn pause_loop_public_json_error_is_generic_with_retry_after() {
        let service = PauseLoopService::for_tests(Duration::from_secs(60), || async {
            Err(PauseLoopError::Helix(
                "secret upstream detail must stay server-side".to_owned(),
            ))
        });

        let (status, headers, body) = route_bytes(
            build_pause_loop_router_with_service(service),
            Method::GET,
            "/twitch/api/v2/public/pause-loop-clips",
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(headers.get(header::RETRY_AFTER).unwrap(), "30");
        let text = std::str::from_utf8(&body).unwrap();
        assert!(!text.contains("secret upstream detail"));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"error": "pause_loop_unavailable"}));
    }

    #[tokio::test]
    async fn pause_loop_unavailable_router_serves_html_and_generic_json_503() {
        let app = build_unavailable_pause_loop_router();

        let (html_status, html_headers, html_body) =
            route_bytes(app.clone(), Method::GET, "/twitch/pause-loop").await;
        assert_eq!(html_status, StatusCode::OK);
        assert_eq!(html_headers.get("x-frame-options").unwrap(), "SAMEORIGIN");
        assert!(html_headers.get("content-security-policy").is_some());
        assert!(std::str::from_utf8(&html_body)
            .unwrap()
            .contains("/twitch/api/v2/public/pause-loop-clips"));

        let (api_status, api_headers, api_body) =
            route_bytes(app, Method::GET, "/twitch/api/v2/public/pause-loop-clips").await;
        assert_eq!(api_status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(api_headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(api_headers.get(header::RETRY_AFTER).unwrap(), "30");
        assert_eq!(api_headers.get("access-control-allow-origin").unwrap(), "*");
        let text = std::str::from_utf8(&api_body).unwrap();
        assert!(!text.contains("NotConfigured"));
        assert!(!text.contains("nicht konfiguriert"));
        let json: serde_json::Value = serde_json::from_slice(&api_body).unwrap();
        assert_eq!(json, serde_json::json!({"error": "pause_loop_unavailable"}));
    }

    #[tokio::test]
    async fn pause_loop_html_contract_headers_and_static_player_guards() {
        let service =
            PauseLoopService::for_tests(Duration::from_secs(60), || async { Ok(Vec::new()) });

        let (status, headers, body) = route_bytes(
            build_pause_loop_router_with_service(service),
            Method::GET,
            "/twitch/pause-loop",
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(headers
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(headers.get("x-robots-tag").unwrap(), "noindex, nofollow");
        assert_eq!(headers.get("x-frame-options").unwrap(), "SAMEORIGIN");
        let csp = headers
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        for needle in [
            "script-src 'self' 'unsafe-inline'",
            "style-src 'self' 'unsafe-inline'",
            "connect-src 'self' https://gql.twitch.tv",
            "media-src https:",
            "frame-ancestors 'self'",
        ] {
            assert!(csp.contains(needle), "CSP fehlt: {needle}");
        }

        let html = std::str::from_utf8(&body).unwrap();
        for needle in [
            "width:100vw",
            "height:100vh",
            "/twitch/api/v2/public/pause-loop-clips",
            // MP4-Direktwiedergabe: das Twitch-Embed startet ohne Klick nicht.
            "https://gql.twitch.tv/gql",
            "playbackAccessToken",
            "createElement('video')",
            "video.play()",
            "addEventListener('ended'",
            "url.searchParams.set('sig'",
            "url.protocol !== 'https:'",
            "function shuffleQueue",
            "function avoidImmediateRepeat(items)",
            "for (let i = items.length - 1; i > 0; i -= 1)",
            "const FETCH_TIMEOUT_MS = 12000",
            "const RESOLVE_TIMEOUT_MS = 12000",
            "new AbortController()",
            "signal: controller.signal",
            "controller.abort()",
            "window.clearTimeout(abortTimer)",
            "const playedThisCycle = new Set()",
            "function reconcileQueue(nextPool)",
            "nextById.has(clip.id)",
            "nextById.get(clip.id)",
            "playedThisCycle.has(clip.id)",
            "clip.id === activeId",
            "queuedIds.has(clip.id)",
            "queue.push(...additions)",
            "if (nextPool.length === 0)",
            "playedThisCycle.clear()",
            "res.ok",
            "res.headers.get('x-pause-loop-stale') === '1'",
            "Number.isFinite",
            "MAX_RESOLVE_FAILURES",
            "textContent",
        ] {
            assert!(html.contains(needle), "HTML/JS-Vertrag fehlt: {needle}");
        }
        assert_eq!(
            html.matches("avoidImmediateRepeat(queue);").count(),
            2,
            "Grenzschutz muss sowohl beim normalen Zyklus als auch nach leerem Pool greifen"
        );

        // Ein entnommener Clip wird vorgeladen, bevor er laeuft: in diesem Fenster ist
        // er weder in der Queue noch aktiv. Wird er erst beim Abspielen als verbraucht
        // markiert, reiht ein Pool-Refresh ihn erneut ein und er laeuft doppelt.
        assert_eq!(
            html.matches("playedThisCycle.add(").count(),
            1,
            "Clip darf nur an einer Stelle als verbraucht markiert werden"
        );
        let mark = html.find("playedThisCycle.add(").unwrap();
        let take_next = html.find("function takeNextClip()").unwrap();
        let after_take_next = html.find("function describeClip(clip)").unwrap();
        assert!(
            mark > take_next && mark < after_take_next,
            "Markierung muss beim Entnehmen in takeNextClip passieren, nicht erst beim Abspielen"
        );
        assert!(!html.contains("innerHTML"));
        // Das klickpflichtige Twitch-Embed darf nicht zurueckkehren.
        assert!(!html.contains("clips.twitch.tv/embed"));
        assert!(!html.contains("<iframe"));
    }

    #[tokio::test]
    async fn pause_loop_json_marks_immediate_stale_then_503_after_unvalidated_error() {
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
                        if call == 1 {
                            Ok(vec![pause_clip("stale-ok")])
                        } else {
                            Err(PauseLoopError::Helix("upstream-secret-detail".to_owned()))
                        }
                    }
                }
            },
        );
        let app = build_pause_loop_router_with_service(service.clone());

        let (first_status, first_headers, first_body) = route_bytes(
            app.clone(),
            Method::GET,
            "/twitch/api/v2/public/pause-loop-clips",
        )
        .await;
        assert_eq!(first_status, StatusCode::OK);
        assert!(first_headers.get("x-pause-loop-stale").is_none());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&first_body).unwrap(),
            serde_json::json!([{
                "id": "stale-ok",
                "broadcaster_name": "Nani",
                "title": "Clip stale-ok",
                "duration": 12.0
            }])
        );

        service.expire_cache_entry_for_tests().await;

        let (second_status, second_headers, second_body) = route_bytes(
            app.clone(),
            Method::GET,
            "/twitch/api/v2/public/pause-loop-clips",
        )
        .await;
        assert_eq!(second_status, StatusCode::OK);
        assert_eq!(second_headers.get("x-pause-loop-stale").unwrap(), "1");
        let second_text = std::str::from_utf8(&second_body).unwrap();
        assert!(!second_text.contains("upstream-secret-detail"));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&second_body).unwrap(),
            serde_json::from_slice::<serde_json::Value>(&first_body).unwrap()
        );

        service.wait_for_in_flight_finished_for_tests().await;

        let (third_status, third_headers, third_body) =
            route_bytes(app, Method::GET, "/twitch/api/v2/public/pause-loop-clips").await;
        assert_eq!(third_status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            third_headers.get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert_eq!(third_headers.get(header::RETRY_AFTER).unwrap(), "30");
        let third_text = std::str::from_utf8(&third_body).unwrap();
        assert!(!third_text.contains("upstream-secret-detail"));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&third_body).unwrap(),
            serde_json::json!({"error": "pause_loop_unavailable"})
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pause_loop_routes_reject_non_get_with_405() {
        let service =
            PauseLoopService::for_tests(Duration::from_secs(60), || async { Ok(Vec::new()) });
        let app = build_pause_loop_router_with_service(service);

        for uri in [
            "/twitch/api/v2/public/pause-loop-clips",
            "/twitch/pause-loop",
        ] {
            let (status, _, _) = route_bytes(app.clone(), Method::POST, uri).await;
            assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{uri}");
        }
    }

    #[tokio::test]
    async fn pause_loop_pipeline_db_helix_filters_dedupes_and_normalizes() {
        let Some(db) = TestDb::new("pause_loop_pipeline").await else {
            return;
        };
        insert_partner(&db.pool, Some(" ActivePartner "), Some(" 111 "), Some(1)).await;
        insert_partner(&db.pool, Some("InactivePartner"), Some("222"), Some(0)).await;
        insert_partner(&db.pool, Some("ExcludedPartner"), Some("333"), Some(1)).await;
        insert_exclusion(&db.pool, "333", false).await;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/search/categories"))
            .and(query_param("query", "Deadlock"))
            .and(query_param("first", "25"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "deadlock-game", "name": "Deadlock"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("first", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {
                        "id": " keep-a ",
                        "broadcaster_name": " Active Name ",
                        "title": " First ",
                        "duration": 4.0,
                        "game_id": "deadlock-game"
                    },
                    {
                        "id": "foreign",
                        "broadcaster_name": "Active Name",
                        "title": "Wrong Game",
                        "duration": 20.0,
                        "game_id": "other-game"
                    },
                    {
                        "id": "keep-a",
                        "broadcaster_name": "Active Name",
                        "title": "Duplicate",
                        "duration": 20.0,
                        "game_id": "deadlock-game"
                    },
                    {
                        "id": "keep-b",
                        "broadcaster_name": "   ",
                        "title": " High ",
                        "duration": 450.0,
                        "game_id": "deadlock-game"
                    }
                ],
                "pagination": {}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let (status, _, body) = route_bytes(
            build_pause_loop_router(db.pool.clone(), helix_client(&server)),
            Method::GET,
            "/twitch/api/v2/public/pause-loop-clips",
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {
                    "id": "keep-a",
                    "broadcaster_name": "Active Name",
                    "title": "First",
                    "duration": 5.0
                },
                {
                    "id": "keep-b",
                    "broadcaster_name": "ActivePartner",
                    "title": "High",
                    "duration": 300.0
                }
            ])
        );

        server.verify().await;
        let clip_broadcasters = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/helix/clips")
            .filter_map(|request| {
                request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "broadcaster_id")
                    .map(|(_, value)| value.into_owned())
            })
            .collect::<Vec<_>>();
        assert_eq!(clip_broadcasters, vec!["111".to_owned()]);

        db.cleanup().await;
    }
}
