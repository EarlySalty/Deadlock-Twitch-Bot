//! Auto-Raid-Pipeline: Retry-Loop über Ziel-Auswahl → Readiness → Ausführung →
//! Pending-Registrierung, mit Strike-/Blacklist-Behandlung bei abgelehnten
//! Raids. Port von `raid/raid_pipeline.py` `RaidPipelineService.execute`.
//!
//! Die Pipeline ist der gemeinsame Kern für Auto-Raids (stream.offline) und
//! später manuelle Raids (Phase 6h). Sie bekommt die **bereits
//! eligibility-gefilterten** Online-Partner; Quell-Eligibility und
//! Deadlock-Filter passieren vorher im Aufrufer.
//!
//! Outreach-Boost (6g) ist portiert: frisch kontaktierte Outreach-Empfänger
//! haben Vorrang vor Partnern und werden nach dem Raid per CAS als verbraucht
//! markiert. Bewusst noch nicht portiert: Voice-Reaction-Conversations
//! (Discord-Pfad, folgt mit der Broker-Erweiterung).
//!
//! Abweichungen von Python (dokumentiert):
//! - Score-Cache wird einmal pro Lauf geladen statt einmal pro Versuch
//!   (Versuche liegen Millisekunden auseinander).
//! - Kandidaten ohne Identität werden vorgefiltert statt die Pipeline
//!   abzubrechen (siehe `target_resolution`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use serde_json::{json, Value};
use tb_observability::{
    AnalyticsDecision, AnalyticsObservabilityService, RaidObservabilityService,
};

use crate::candidate_selection::{is_retryable_raid_error, FairnessCandidate, FOLLOWERS_UNKNOWN};
use crate::outreach_boost::{OutreachBoostStore, OUTREACH_BOOST_LOOKBACK_HOURS};
use crate::partner_roster::OnlineCandidate;
use crate::pending_raids::{PendingRaid, PendingRaidStore};
use crate::raid_blacklist::RaidBlacklistStore;
use crate::raid_executor::{RaidExecutor, RaidOutcome, RaidRequest};
use crate::raid_history_store::RaidHistoryStore;
use crate::score_store::{PartnerRaidScoreRow, ScoreStore};
use crate::strikes_store::StrikesStore;
use crate::target_resolution::{
    filter_fallback_pool, resolve_boost_target, resolve_partner_target, select_fallback_from_pool,
    ResolvedTarget,
};
use crate::util::{parse_iso_utc, unix_now};

/// Maximale Ziel-Versuche pro Lauf (Python `max_attempts = 3`).
const MAX_ATTEMPTS: usize = 3;
/// Ab so vielen Strikes wird ein Nicht-Partner-Ziel geblacklistet.
const STRIKE_BLACKLIST_THRESHOLD: i32 = 2;
/// Cooldown für den Fairness-Fallback (Python `recent_raid_cooldown_days = 7`).
const RECENT_RAID_COOLDOWN_DAYS: i32 = 7;
/// Sprache + Limit des Kategorie-Fallbacks (Python `language="de", limit=50`).
const FALLBACK_LANGUAGE: &str = "de";
const FALLBACK_LIMIT: usize = 50;

/// Port: Live-Streams der Ziel-Kategorie für den Fairness-Fallback
/// (echte Impl: Helix `get_streams_by_category` in der Composition-Root).
#[async_trait::async_trait]
pub trait FallbackStreamSource: Send + Sync {
    async fn category_streams(
        &self,
        category_id: &str,
        language: &str,
        limit: usize,
    ) -> Result<Vec<FairnessCandidate>, String>;
}

/// Port: stellt vor dem Raid-Start die `channel.raid`-Subscription fürs Ziel
/// sicher (Python `ensure_raid_arrival_subscription_ready`, best-effort).
#[async_trait::async_trait]
pub trait ArrivalReadiness: Send + Sync {
    async fn ensure_ready(&self, to_broadcaster_id: &str, to_broadcaster_login: &str) -> bool;
}

/// Port: reichert den gefilterten Fallback-Pool mit echten Follower-Zahlen an
/// (Python `attach_followers_totals(pool)` in `select_fairest_candidate`,
/// candidate_selection.py Z. 377–378). Ohne diese Anreicherung bleibt die
/// 3. Tie-Break-Ebene (Follower) tot, weil alle Kandidaten auf dem
/// [`FOLLOWERS_UNKNOWN`](crate::candidate_selection::FOLLOWERS_UNKNOWN)-Sentinel
/// stehen. Best-effort: ein Kandidat ohne abrufbare Zahl behält den Sentinel
/// und sortiert ans Ende.
#[async_trait::async_trait]
pub trait FollowerEnricher: Send + Sync {
    /// Setzt `followers_total` jedes Kandidaten auf die echte Helix-Zahl,
    /// sofern abrufbar. Nicht-ermittelbare Kandidaten bleiben unverändert.
    async fn enrich(&self, pool: &mut [FairnessCandidate]);

    /// Wie [`FollowerEnricher::enrich`], aber mit strukturierter Diagnose für
    /// Analytics-Observability. Bestehende Implementierungen behalten ueber den
    /// Default ihr bisheriges Verhalten.
    ///
    /// WIRING-TODO(P3.10): bin/tb-bot Follower-Enricher-Impl soll diese Methode
    /// mit echten Helix-Result-Feldern (`http_status`, `error_code`) ueberschreiben.
    async fn enrich_with_observability(
        &self,
        pool: &mut [FairnessCandidate],
    ) -> FollowersEnrichmentObservation {
        self.enrich(pool).await;
        FollowersEnrichmentObservation::ok(
            pool.len(),
            pool.iter()
                .filter(|candidate| candidate.followers_total != FOLLOWERS_UNKNOWN)
                .count(),
        )
    }
}

fn elapsed_ms(start: Instant) -> i64 {
    let micros = start.elapsed().as_micros();
    ((micros.saturating_add(999) / 1000).max(1)) as i64
}

fn no_target_details(
    attempt: usize,
    selection_ms: i64,
    total_ms: i64,
    reason: &str,
) -> BTreeMap<String, Value> {
    let mut details = common_attempt_details(attempt, selection_ms, 0, reason);
    details.insert("total_ms".to_string(), json!(total_ms));
    details
}

fn invalid_target_details(
    attempt: usize,
    selection_ms: i64,
    candidates_count: i32,
    total_ms: i64,
    reason: &str,
) -> BTreeMap<String, Value> {
    let mut details = common_attempt_details(attempt, selection_ms, candidates_count, reason);
    details.insert("total_ms".to_string(), json!(total_ms));
    details
}

fn attempt_selected_details(
    attempt: usize,
    selection_ms: i64,
    candidates_count: i32,
    reason: &str,
    is_partner_raid: bool,
) -> BTreeMap<String, Value> {
    let mut details = common_attempt_details(attempt, selection_ms, candidates_count, reason);
    details.insert("is_partner_raid".to_string(), json!(is_partner_raid));
    details
}

fn raid_started_details(
    attempt: usize,
    selection_ms: i64,
    api_call_ms: i64,
    total_ms: i64,
    candidates_count: i32,
    reason: &str,
) -> BTreeMap<String, Value> {
    let mut details = common_attempt_details(attempt, selection_ms, candidates_count, reason);
    details.insert("api_call_ms".to_string(), json!(api_call_ms));
    details.insert("total_ms".to_string(), json!(total_ms));
    details
}

fn raid_failed_details(
    attempt: usize,
    selection_ms: i64,
    api_call_ms: i64,
    total_ms: i64,
    candidates_count: i32,
    reason: &str,
    error: String,
) -> BTreeMap<String, Value> {
    let mut details = common_attempt_details(attempt, selection_ms, candidates_count, reason);
    details.insert("api_call_ms".to_string(), json!(api_call_ms));
    details.insert("total_ms".to_string(), json!(total_ms));
    details.insert("error".to_string(), json!(error));
    details
}

fn common_attempt_details(
    attempt: usize,
    selection_ms: i64,
    candidates_count: i32,
    reason: &str,
) -> BTreeMap<String, Value> {
    let mut details = BTreeMap::new();
    details.insert("attempt".to_string(), json!(attempt));
    details.insert("max_attempts".to_string(), json!(MAX_ATTEMPTS));
    details.insert("selection_ms".to_string(), json!(selection_ms));
    details.insert("candidates_count".to_string(), json!(candidates_count));
    details.insert("reason".to_string(), json!(reason));
    details
}

/// Strukturierte Diagnose des Follower-Enrichments fuer
/// `AnalyticsObservabilityService::log_decision`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowersEnrichmentObservation {
    pub decision: String,
    pub reason: String,
    pub request_attempted: Option<bool>,
    pub request_result: String,
    pub http_status: Option<i64>,
    pub scope_state: BTreeMap<String, Value>,
    pub runtime_state: BTreeMap<String, Value>,
    pub extra: BTreeMap<String, Value>,
}

impl FollowersEnrichmentObservation {
    pub fn ok(candidate_count: usize, enriched_count: usize) -> Self {
        let mut extra = BTreeMap::new();
        extra.insert("candidate_count".to_string(), json!(candidate_count));
        extra.insert("enriched_count".to_string(), json!(enriched_count));
        Self {
            decision: "terminal_decision".to_string(),
            reason: "followers_enriched".to_string(),
            request_attempted: Some(true),
            request_result: "ok".to_string(),
            http_status: None,
            scope_state: BTreeMap::new(),
            runtime_state: BTreeMap::new(),
            extra,
        }
    }

    pub fn http_error(http_status: i64, error_code: impl Into<String>) -> Self {
        let mut extra = BTreeMap::new();
        extra.insert("error_code".to_string(), json!(error_code.into()));
        Self {
            decision: "terminal_decision".to_string(),
            reason: "followers_http_error".to_string(),
            request_attempted: Some(true),
            request_result: "http_error".to_string(),
            http_status: Some(http_status),
            scope_state: BTreeMap::new(),
            runtime_state: BTreeMap::new(),
            extra,
        }
    }

    pub fn request_error(error_code: impl Into<String>) -> Self {
        let mut extra = BTreeMap::new();
        extra.insert("error_code".to_string(), json!(error_code.into()));
        Self {
            decision: "terminal_decision".to_string(),
            reason: "followers_request_error".to_string(),
            request_attempted: Some(true),
            request_result: "request_error".to_string(),
            http_status: None,
            scope_state: BTreeMap::new(),
            runtime_state: BTreeMap::new(),
            extra,
        }
    }
}

/// Daten einer früher eingegangenen, verwaisten `channel.chat.notification`,
/// die beim Registrieren eines passenden Pendings nachgespielt wird.
///
/// Spiegelt das Orphan-Payload-Dict aus Python
/// (`raid_tracking_runtime.py:504-513`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanChatNotification {
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub from_broadcaster_login: String,
    pub viewer_count: i32,
    pub from_broadcaster_id: Option<String>,
    pub message_id: Option<String>,
    pub event_timestamp: Option<String>,
}

/// Port: Korrelations-Replay einer verwaisten `channel.chat.notification` beim
/// Registrieren eines passenden Pendings (P2.30).
///
/// Port von Python `register_pending_raid` (raid_tracking_runtime.py:477-514):
/// Beim Registrieren wird ein passendes Orphan-Signal aus dem Cache gezogen
/// (`pop_orphan`) und über `on_chat_raid_notification` erneut durch die
/// Korrelation geschickt, damit der Raid bereits zum Registrierzeitpunkt
/// pending-korreliert bestätigt wird (statt erst nach ~15 s Grace als
/// unabhängiges Arrival promotet zu werden).
///
/// Die konkrete Impl (echter Orphan-Store + `on_chat_raid_notification`) lebt im
/// Composition-Root (tb-bot `raid_arrival_wiring.rs`) — siehe WIRING-TODO.
#[async_trait::async_trait]
pub trait OrphanReplay: Send + Sync {
    /// Zieht ein passendes Orphan-Signal (falls vorhanden) und gibt es zur
    /// Wiedereinspielung zurück. Liefert `None`, wenn kein Orphan vorliegt.
    async fn pop_orphan(
        &self,
        to_broadcaster_id: &str,
        from_broadcaster_login: &str,
    ) -> Option<OrphanChatNotification>;

    /// Spielt das gezogene Orphan-Signal erneut durch die Chat-Raid-Korrelation
    /// (`on_chat_raid_notification`), nachdem das Pending registriert wurde.
    async fn replay(&self, orphan: OrphanChatNotification);
}

/// Eingabe eines Pipeline-Laufs.
#[derive(Debug, Clone)]
pub struct AutoRaidRequest {
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub viewer_count: i32,
    pub stream_duration_sec: i32,
    /// Online-Partner nach Deadlock-Eligibility-Filter.
    pub partners: Vec<OnlineCandidate>,
    /// Kategorie-ID für den DE-Fallback (None → kein Fallback).
    pub category_id: Option<String>,
    /// Unix-Zeitpunkt des Offline-Triggers (für Arrival-Latenz-Metriken).
    pub offline_trigger_ts: Option<f64>,
    /// History-Grund, z. B. `auto_raid_on_offline`.
    pub reason: String,
    /// Auto-Raids respektieren die weiche Raid-Blacklist; manuelle Raids nur
    /// harte globale Bans.
    pub respect_soft_raid_blacklist: bool,
}

/// Ergebnis eines Pipeline-Laufs (Python Status-Dict).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoRaidPipelineOutcome {
    Started {
        target_login: String,
        is_partner_raid: bool,
    },
    NoTarget,
    /// Vorbedingung fehlgeschlagen (z. B. Blacklist nicht ladbar) — kein Versuch.
    Blocked {
        error: String,
    },
    Failed {
        error: String,
    },
}

pub struct AutoRaidPipeline {
    blacklist: RaidBlacklistStore,
    scores: ScoreStore,
    history: RaidHistoryStore,
    strikes: StrikesStore,
    executor: RaidExecutor,
    pending: Arc<Mutex<PendingRaidStore>>,
    readiness: Arc<dyn ArrivalReadiness>,
    fallback: Option<Arc<dyn FallbackStreamSource>>,
    /// Follower-Anreicherung für den Fallback-Pool (Python
    /// `attach_followers_totals`) — `None` lässt die Follower-Tie-Break-Ebene
    /// auf dem Sentinel (degradiert auf `started_at`, wie ohne Helix-Follower).
    follower_enricher: Option<Arc<dyn FollowerEnricher>>,
    /// Outreach-Boost-Ziele (Phase 6g) — `None` deaktiviert den Boost-Pfad.
    outreach: Option<OutreachBoostStore>,
    /// Orphan-Replay beim Registrieren (P2.30) — `None` lässt den Replay aus
    /// (Verhalten wie vor P2.30: Orphan wird erst nach Grace als unabhängiges
    /// Arrival promotet). Wird per [`AutoRaidPipeline::with_orphan_replay`]
    /// gesetzt; die konkrete Impl verdrahtet das Composition-Root (WIRING-TODO).
    orphan_replay: Option<Arc<dyn OrphanReplay>>,
    /// Strukturierte Raid-Flow-Events. `None` behaelt das bisherige Verhalten.
    observability_raid: Option<Arc<RaidObservabilityService>>,
    /// Analytics-Decision-Events fuer Followers-Enrichment. `None` = kein Log.
    observability_analytics: Option<Arc<AnalyticsObservabilityService>>,
}

impl AutoRaidPipeline {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        blacklist: RaidBlacklistStore,
        scores: ScoreStore,
        history: RaidHistoryStore,
        strikes: StrikesStore,
        executor: RaidExecutor,
        pending: Arc<Mutex<PendingRaidStore>>,
        readiness: Arc<dyn ArrivalReadiness>,
        fallback: Option<Arc<dyn FallbackStreamSource>>,
        follower_enricher: Option<Arc<dyn FollowerEnricher>>,
        outreach: Option<OutreachBoostStore>,
    ) -> Self {
        Self {
            blacklist,
            scores,
            history,
            strikes,
            executor,
            pending,
            readiness,
            fallback,
            follower_enricher,
            outreach,
            orphan_replay: None,
            observability_raid: None,
            observability_analytics: None,
        }
    }

    /// Aktiviert den Orphan-Replay beim Registrieren (P2.30). Aufruf im
    /// Composition-Root nach `new(..)`; ohne ihn bleibt das alte Verhalten.
    #[must_use]
    pub fn with_orphan_replay(mut self, orphan_replay: Arc<dyn OrphanReplay>) -> Self {
        self.orphan_replay = Some(orphan_replay);
        self
    }

    /// Verdrahtet optionale Observability-Services. Injiziert in der
    /// Composition-Root `bin/tb-bot/src/main.rs` (AutoRaidPipeline-Aufbau)
    /// (P2.42/P3.8/P2.45/P3.14).
    #[must_use]
    pub fn with_observability(
        mut self,
        raid: Option<Arc<RaidObservabilityService>>,
        analytics: Option<Arc<AnalyticsObservabilityService>>,
    ) -> Self {
        self.observability_raid = raid;
        self.observability_analytics = analytics;
        self
    }

    pub async fn run(&self, req: &AutoRaidRequest) -> AutoRaidPipelineOutcome {
        let flow_start = Instant::now();
        let blacklist_sets = if req.respect_soft_raid_blacklist {
            self.blacklist.load_all().await
        } else {
            self.blacklist.load_hard_bans().await
        };
        let (blacklist_ids, blacklist_logins) = match blacklist_sets {
            Ok(sets) => sets,
            Err(error) => {
                tracing::error!(%error, "Raid-Pipeline blockiert: Blacklist nicht ladbar");
                return AutoRaidPipelineOutcome::Blocked {
                    error: "blacklist_unavailable".to_string(),
                };
            }
        };

        // Score-Cache einmal für alle Partner-Kandidaten laden.
        let partner_ids: Vec<&str> = req
            .partners
            .iter()
            .map(|p| p.twitch_user_id.as_str())
            .collect();
        let scores: HashMap<String, PartnerRaidScoreRow> = match self
            .scores
            .load_many(&partner_ids)
            .await
        {
            Ok(rows) => rows
                .into_iter()
                .map(|r| (r.twitch_user_id.clone(), r))
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "Score-Cache nicht ladbar — fahre ohne Partner-Scores fort");
                HashMap::new()
            }
        };

        // Outreach-Boost-Ziele einmal pro Lauf laden (Python lädt vor dem Loop).
        let boost_logins: HashSet<String> = match &self.outreach {
            Some(store) => store
                .load_boost_logins(OUTREACH_BOOST_LOOKBACK_HOURS)
                .await
                .unwrap_or_else(|error| {
                    tracing::debug!(%error, "Outreach-Boost-Loader fehlgeschlagen");
                    HashSet::new()
                }),
            None => HashSet::new(),
        };

        let mut exclude_ids: HashSet<String> = [req.broadcaster_id.clone()].into();
        // Fallback-Streams werden lazy geholt und über Versuche gecacht
        // (Python `cached_de_streams`).
        let mut cached_fallback: Option<Vec<FairnessCandidate>> = None;
        let observability_flow_id = self
            .observability_raid
            .as_ref()
            .map(|service| service.next_flow_id("raid"));

        for attempt in 1..=MAX_ATTEMPTS {
            let attempt_start = Instant::now();
            let target = match self
                .resolve_target(
                    req,
                    &scores,
                    &boost_logins,
                    &blacklist_ids,
                    &blacklist_logins,
                    &exclude_ids,
                    &mut cached_fallback,
                    attempt,
                )
                .await
            {
                Some(target) => target,
                None => {
                    if let Some(flow_id) = observability_flow_id.as_deref() {
                        self.emit_raid_observability_event(
                            flow_id,
                            "no_target",
                            "no_target",
                            req,
                            None,
                            no_target_details(
                                attempt,
                                elapsed_ms(attempt_start),
                                elapsed_ms(flow_start),
                                req.reason.as_str(),
                            ),
                        );
                    }
                    tracing::info!(
                        from = %req.broadcaster_login,
                        attempt,
                        reason = %req.reason,
                        "Kein gültiges Raid-Ziel gefunden"
                    );
                    return AutoRaidPipelineOutcome::NoTarget;
                }
            };

            let selection_ms = elapsed_ms(attempt_start);
            let flow_id = observability_flow_id
                .clone()
                .unwrap_or_else(|| format!("raid-{}", (unix_now() * 1000.0) as i64));
            if target.user_id.trim().is_empty() || target.user_login.trim().is_empty() {
                self.emit_raid_observability_event(
                    &flow_id,
                    "invalid_target",
                    "invalid_target_identity",
                    req,
                    Some(&target),
                    invalid_target_details(
                        attempt,
                        selection_ms,
                        target.candidates_count,
                        elapsed_ms(flow_start),
                        req.reason.as_str(),
                    ),
                );
                return AutoRaidPipelineOutcome::NoTarget;
            }
            if let Some(service) = &self.observability_raid {
                service.increment_counter("raid_flow_started_total", 1);
            }
            self.emit_raid_observability_event(
                &flow_id,
                "attempt_selected",
                "candidate_selected",
                req,
                Some(&target),
                attempt_selected_details(
                    attempt,
                    selection_ms,
                    target.candidates_count,
                    req.reason.as_str(),
                    target.is_partner_raid,
                ),
            );
            tracing::info!(
                from = %req.broadcaster_login,
                to = %target.user_login,
                attempt,
                candidates = target.candidates_count,
                partner = target.is_partner_raid,
                reason = %req.reason,
                "Führe Raid-Versuch aus"
            );

            let channel_raid_ready = self
                .readiness
                .ensure_ready(&target.user_id, &target.user_login)
                .await;

            let raid_request = RaidRequest {
                from_broadcaster_id: req.broadcaster_id.clone(),
                from_broadcaster_login: req.broadcaster_login.clone(),
                to_broadcaster_id: target.user_id.clone(),
                to_broadcaster_login: target.user_login.clone(),
                viewer_count: req.viewer_count,
                stream_duration_sec: req.stream_duration_sec,
                target_stream_started_at: target.started_at.as_deref().and_then(parse_iso_utc),
                candidates_count: target.candidates_count,
                reason: req.reason.clone(),
            };

            let api_start = Instant::now();
            let execution = self.executor.execute(&raid_request, Utc::now()).await;
            let api_call_ms = elapsed_ms(api_start);
            let total_ms = elapsed_ms(flow_start);
            let outcome = match execution {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.emit_raid_observability_event(
                        &flow_id,
                        "raid_failed",
                        "db_error",
                        req,
                        Some(&target),
                        raid_failed_details(
                            attempt,
                            selection_ms,
                            api_call_ms,
                            total_ms,
                            target.candidates_count,
                            req.reason.as_str(),
                            error.to_string(),
                        ),
                    );
                    tracing::error!(%error, to = %target.user_login, "Raid-Ausführung: DB-Fehler");
                    return AutoRaidPipelineOutcome::Failed {
                        error: format!("db_error: {error}"),
                    };
                }
            };

            match outcome {
                RaidOutcome::Started => {
                    self.register_pending(req, &target, &flow_id, channel_raid_ready)
                        .await;
                    if target.is_outreach_boost {
                        self.consume_outreach_boost(&target.user_login).await;
                    }
                    self.emit_raid_observability_event(
                        &flow_id,
                        "raid_started",
                        "success",
                        req,
                        Some(&target),
                        raid_started_details(
                            attempt,
                            selection_ms,
                            api_call_ms,
                            total_ms,
                            target.candidates_count,
                            req.reason.as_str(),
                        ),
                    );
                    tracing::info!(
                        from = %req.broadcaster_login,
                        to = %target.user_login,
                        attempt,
                        reason = %req.reason,
                        "Raid gestartet"
                    );
                    return AutoRaidPipelineOutcome::Started {
                        target_login: target.user_login,
                        is_partner_raid: target.is_partner_raid,
                    };
                }
                RaidOutcome::Failed(error) => {
                    exclude_ids.insert(target.user_id.clone());
                    if !is_retryable_raid_error(&error) {
                        self.emit_raid_observability_event(
                            &flow_id,
                            "raid_failed",
                            "non_retryable",
                            req,
                            Some(&target),
                            raid_failed_details(
                                attempt,
                                selection_ms,
                                api_call_ms,
                                total_ms,
                                target.candidates_count,
                                req.reason.as_str(),
                                error.clone(),
                            ),
                        );
                        tracing::error!(
                            from = %req.broadcaster_login,
                            to = %target.user_login,
                            attempt,
                            %error,
                            "Raid mit nicht-wiederholbarem Fehler fehlgeschlagen"
                        );
                        return AutoRaidPipelineOutcome::Failed { error };
                    }
                    let blacklist_decision = self.handle_rejected_target(&target, &error).await;
                    self.emit_raid_observability_event(
                        &flow_id,
                        "raid_failed_retryable",
                        blacklist_decision,
                        req,
                        Some(&target),
                        raid_failed_details(
                            attempt,
                            selection_ms,
                            api_call_ms,
                            total_ms,
                            target.candidates_count,
                            req.reason.as_str(),
                            error,
                        ),
                    );
                }
            }
        }

        AutoRaidPipelineOutcome::Failed {
            error: "no_valid_target_after_retries".to_string(),
        }
    }

    /// Ziel eines Versuchs: Partner-Pfad, sonst lazy DE-Kategorie-Fallback.
    #[allow(clippy::too_many_arguments)]
    async fn resolve_target(
        &self,
        req: &AutoRaidRequest,
        scores: &HashMap<String, PartnerRaidScoreRow>,
        boost_logins: &HashSet<String>,
        blacklist_ids: &HashSet<String>,
        blacklist_logins: &HashSet<String>,
        exclude_ids: &HashSet<String>,
        cached_fallback: &mut Option<Vec<FairnessCandidate>>,
        attempt: usize,
    ) -> Option<ResolvedTarget> {
        // Prio 1 (Python Z. 144–178): Outreach-Boost-Ziel unter den
        // Kategorie-Streams — VOR dem Partner-Pfad.
        if !boost_logins.is_empty() {
            self.ensure_fallback_streams(req, cached_fallback, attempt)
                .await;
            if let Some(target) = resolve_boost_target(
                cached_fallback.as_deref().unwrap_or(&[]),
                boost_logins,
                blacklist_ids,
                blacklist_logins,
                exclude_ids,
            ) {
                tracing::info!(
                    from = %req.broadcaster_login,
                    to = %target.user_login,
                    pool = target.candidates_count,
                    "Outreach-Boost-Ziel gewählt"
                );
                return Some(target);
            }
        }

        let partner = resolve_partner_target(
            &req.partners,
            scores,
            blacklist_ids,
            blacklist_logins,
            exclude_ids,
        );
        if partner.stats.cache_misses > 0 || partner.stats.stale_not_live > 0 {
            tracing::info!(
                from = %req.broadcaster_login,
                considered = partner.stats.considered,
                cache_misses = partner.stats.cache_misses,
                stale_not_live = partner.stats.stale_not_live,
                "Partner-Auswahl: Score-Cache unvollständig"
            );
        }
        if let Some(target) = partner.target {
            if let Some(reason) = partner.reason {
                tracing::info!(
                    to = %target.user_login,
                    selection_reason = reason.as_str(),
                    "Partner-Raid-Ziel gewählt"
                );
            }
            return Some(target);
        }

        self.ensure_fallback_streams(req, cached_fallback, attempt)
            .await;
        let streams = cached_fallback.as_deref().unwrap_or(&[]);
        if streams.is_empty() {
            return None;
        }

        let recent_targets = self
            .history
            .get_recent_raid_targets(&req.broadcaster_id, RECENT_RAID_COOLDOWN_DAYS)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "Recent-Raid-Targets nicht ladbar — ohne Cooldown");
                HashSet::new()
            });
        let stream_ids: Vec<&str> = streams.iter().map(|s| s.user_id.as_str()).collect();
        let received_raids_by_id: HashMap<String, i32> =
            match self.scores.load_many(&stream_ids).await {
                Ok(rows) => rows
                    .into_iter()
                    .map(|r| (r.twitch_user_id.clone(), r.received_successful_raids_total))
                    .collect(),
                Err(error) => {
                    tracing::debug!(%error, "Raid-Totals nicht ladbar — Fairness ohne Totals");
                    HashMap::new()
                }
            };

        // Filter → Follower-Anreicherung (nur auf dem gefilterten Pool, nicht
        // allen 50 Streams) → Tie-Break. Python: `attach_followers_totals(pool)`
        // vor der Sortierung in `select_fairest_candidate`.
        let mut pool = filter_fallback_pool(streams, blacklist_ids, blacklist_logins, exclude_ids);
        if let Some(enricher) = &self.follower_enricher {
            if self.observability_analytics.is_some() {
                let observation = enricher.enrich_with_observability(&mut pool).await;
                self.emit_followers_observability_decision(req, observation);
            } else {
                enricher.enrich(&mut pool).await;
            }
        }
        select_fallback_from_pool(&pool, &recent_targets, &received_raids_by_id)
    }

    /// Holt die Kategorie-Streams einmalig (lazy, über Versuche gecacht —
    /// Python `cached_de_streams`). No-op ohne Fallback-Quelle/Kategorie.
    async fn ensure_fallback_streams(
        &self,
        req: &AutoRaidRequest,
        cached_fallback: &mut Option<Vec<FairnessCandidate>>,
        attempt: usize,
    ) {
        if cached_fallback.is_some() {
            return;
        }
        let (Some(fallback), Some(category_id)) = (&self.fallback, &req.category_id) else {
            return;
        };
        tracing::info!(
            from = %req.broadcaster_login,
            attempt,
            "Hole Deadlock-DE-Kategorie-Streams (Boost/Fallback)"
        );
        let streams = match fallback
            .category_streams(category_id, FALLBACK_LANGUAGE, FALLBACK_LIMIT)
            .await
        {
            Ok(streams) => streams,
            Err(error) => {
                tracing::error!(%error, "Kategorie-Streams nicht abrufbar");
                Vec::new()
            }
        };
        *cached_fallback = Some(streams);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_raid_observability_event(
        &self,
        flow_id: &str,
        step: &str,
        decision: &str,
        req: &AutoRaidRequest,
        target: Option<&ResolvedTarget>,
        details: BTreeMap<String, Value>,
    ) {
        let Some(service) = &self.observability_raid else {
            return;
        };
        service.emit_event(
            "raid",
            flow_id,
            step,
            decision,
            Some(&req.broadcaster_login),
            Some(&req.broadcaster_id),
            target.map(|target| target.user_login.as_str()),
            target.map(|target| target.user_id.as_str()),
            details,
        );
    }

    fn emit_followers_observability_decision(
        &self,
        req: &AutoRaidRequest,
        observation: FollowersEnrichmentObservation,
    ) {
        let Some(service) = &self.observability_analytics else {
            return;
        };
        let flow_id = service.next_flow_id("followers");
        service.log_decision(AnalyticsDecision {
            flow_id,
            flow: "followers".to_string(),
            login: req.broadcaster_login.clone(),
            session_id: None,
            decision: observation.decision,
            reason: observation.reason,
            request_attempted: observation.request_attempted,
            request_result: observation.request_result,
            http_status: observation.http_status,
            scope_state: observation.scope_state,
            runtime_state: observation.runtime_state,
            extra: observation.extra,
        });
    }

    /// Strike-/Blacklist-Behandlung für ein Ziel, das Raids ablehnt
    /// (Python Z. 413–452): Partner werden nur übersprungen, Nicht-Partner
    /// sammeln Strikes und landen ab Strike 2 auf der Blacklist.
    async fn handle_rejected_target(&self, target: &ResolvedTarget, error: &str) -> &'static str {
        if target.is_partner_raid {
            tracing::warn!(
                to = %target.user_login,
                "Raid abgelehnt: Partner-Ziel erlaubt keine Raids — überspringe ohne Blacklist"
            );
            return "skip_blacklist";
        }
        let strikes = match self
            .strikes
            .increment(Some(&target.user_id), &target.user_login, error)
            .await
        {
            Ok(count) => count,
            Err(db_error) => {
                // Python-Fallback: ohne Strike-Zähler gilt das Ziel als reif.
                tracing::warn!(%db_error, to = %target.user_login, "Strike-Zähler nicht erreichbar — werte als Schwelle");
                STRIKE_BLACKLIST_THRESHOLD
            }
        };
        if strikes >= STRIKE_BLACKLIST_THRESHOLD {
            tracing::warn!(
                to = %target.user_login,
                strikes,
                "Raid abgelehnt: Ziel blockiert Raids — Blacklist + nächster Versuch"
            );
            if let Err(db_error) = self
                .blacklist
                .add(Some(&target.user_id), &target.user_login, error, Utc::now())
                .await
            {
                tracing::error!(%db_error, to = %target.user_login, "Blacklist-Eintrag fehlgeschlagen");
            }
            "retry"
        } else {
            tracing::info!(
                to = %target.user_login,
                strikes,
                "Raid abgelehnt: Strike vergeben, noch keine Blacklist — nächster Versuch"
            );
            "retry_no_blacklist"
        }
    }

    /// Markiert das Outreach-Boost-Ziel als verbraucht (CAS; Python
    /// `mark_outreach_boost_used`). Best-effort — der Raid lief bereits.
    async fn consume_outreach_boost(&self, target_login: &str) {
        let Some(store) = &self.outreach else { return };
        match store.mark_used(target_login).await {
            Ok(true) => {
                tracing::info!(to = %target_login, "Outreach-Boost verbraucht");
            }
            Ok(false) => {
                tracing::debug!(to = %target_login, "Outreach-Boost war bereits verbraucht");
            }
            Err(error) => {
                tracing::error!(%error, to = %target_login, "Outreach-Boost-Markierung fehlgeschlagen");
            }
        }
    }

    /// Registriert den erfolgreichen Raid als Pending (Arrival-Korrelation),
    /// räumt veraltete Pendings derselben Quelle ab und spielt — falls aktiviert —
    /// eine zuvor verwaiste `channel.chat.notification` nach (Python
    /// `register_pending_raid`, raid_tracking_runtime.py:477-514).
    async fn register_pending(
        &self,
        req: &AutoRaidRequest,
        target: &ResolvedTarget,
        flow_id: &str,
        channel_raid_ready: bool,
    ) {
        // Pending speichern (synchroner Store-Abschnitt, Lock nicht über await halten).
        {
            let mut store = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            store.supersede_from_source(&req.broadcaster_login, &target.user_id);
            let mut pending = PendingRaid::new(&req.broadcaster_login, &target.user_id);
            pending.is_partner_raid = target.is_partner_raid;
            pending.registered_viewer_count = req.viewer_count;
            pending.offline_trigger_ts = req.offline_trigger_ts;
            pending.raid_flow_id = Some(flow_id.to_string());
            pending.channel_raid_ready = Some(channel_raid_ready);
            pending.target_stream_data = target.target_stream_data.clone();
            store.store(pending);
        }

        // P2.30: Orphan-Replay NACH dem Store — so findet das wiedereingespielte
        // Chat-Signal das soeben registrierte Pending und korreliert es sofort,
        // statt erst nach Grace als unabhängiges Arrival promotet zu werden.
        if let Some(replay) = &self.orphan_replay {
            if let Some(orphan) = replay
                .pop_orphan(&target.user_id, &req.broadcaster_login)
                .await
            {
                tracing::info!(
                    from = %req.broadcaster_login,
                    to = %target.user_login,
                    "Pending raid matcht früheres channel.chat.notification-Signal — Replay"
                );
                replay.replay(orphan).await;
            }
        }
    }
}
