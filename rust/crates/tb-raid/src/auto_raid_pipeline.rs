//! Auto-Raid-Pipeline: Retry-Loop über Ziel-Auswahl → Readiness → Ausführung →
//! Pending-Registrierung, mit Strike-/Blacklist-Behandlung bei abgelehnten
//! Raids. Port von `raid/raid_pipeline.py` `RaidPipelineService.execute`.
//!
//! Die Pipeline ist der gemeinsame Kern für Auto-Raids (stream.offline) und
//! später manuelle Raids (Phase 6h). Sie bekommt die **bereits
//! eligibility-gefilterten** Online-Partner; Quell-Eligibility und
//! Deadlock-Filter passieren vorher im Aufrufer.
//!
//! Bewusst noch nicht portiert (Phase 6g, Post-Cutover): Outreach-Boost-Ziele
//! und Voice-Reaction-Conversations — beides optionale Pfade, die in Python
//! über None-bare Hooks liefen.
//!
//! Abweichungen von Python (dokumentiert):
//! - Score-Cache wird einmal pro Lauf geladen statt einmal pro Versuch
//!   (Versuche liegen Millisekunden auseinander).
//! - Kandidaten ohne Identität werden vorgefiltert statt die Pipeline
//!   abzubrechen (siehe `target_resolution`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::candidate_selection::{is_retryable_raid_error, FairnessCandidate};
use crate::partner_roster::OnlineCandidate;
use crate::pending_raids::{PendingRaid, PendingRaidStore};
use crate::raid_blacklist::RaidBlacklistStore;
use crate::raid_executor::{RaidExecutor, RaidOutcome, RaidRequest};
use crate::raid_history_store::RaidHistoryStore;
use crate::score_store::{PartnerRaidScoreRow, ScoreStore};
use crate::strikes_store::StrikesStore;
use crate::target_resolution::{resolve_fallback_target, resolve_partner_target, ResolvedTarget};
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
        }
    }

    pub async fn run(&self, req: &AutoRaidRequest) -> AutoRaidPipelineOutcome {
        let (blacklist_ids, blacklist_logins) = match self.blacklist.load_all().await {
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
        let scores: HashMap<String, PartnerRaidScoreRow> =
            match self.scores.load_many(&partner_ids).await {
                Ok(rows) => rows
                    .into_iter()
                    .map(|r| (r.twitch_user_id.clone(), r))
                    .collect(),
                Err(error) => {
                    tracing::error!(%error, "Raid-Pipeline blockiert: Score-Cache nicht ladbar");
                    return AutoRaidPipelineOutcome::Blocked {
                        error: "score_cache_unavailable".to_string(),
                    };
                }
            };

        let mut exclude_ids: HashSet<String> = [req.broadcaster_id.clone()].into();
        // Fallback-Streams werden lazy geholt und über Versuche gecacht
        // (Python `cached_de_streams`).
        let mut cached_fallback: Option<Vec<FairnessCandidate>> = None;

        for attempt in 1..=MAX_ATTEMPTS {
            let target = match self
                .resolve_target(
                    req,
                    &scores,
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
                    tracing::info!(
                        from = %req.broadcaster_login,
                        attempt,
                        reason = %req.reason,
                        "Kein gültiges Raid-Ziel gefunden"
                    );
                    return AutoRaidPipelineOutcome::NoTarget;
                }
            };

            let flow_id = format!("raid-{}", (unix_now() * 1000.0) as i64);
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

            let outcome = match self.executor.execute(&raid_request, Utc::now()).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    tracing::error!(%error, to = %target.user_login, "Raid-Ausführung: DB-Fehler");
                    return AutoRaidPipelineOutcome::Failed {
                        error: format!("db_error: {error}"),
                    };
                }
            };

            match outcome {
                RaidOutcome::Started => {
                    self.register_pending(req, &target, &flow_id, channel_raid_ready);
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
                        tracing::error!(
                            from = %req.broadcaster_login,
                            to = %target.user_login,
                            attempt,
                            %error,
                            "Raid mit nicht-wiederholbarem Fehler fehlgeschlagen"
                        );
                        return AutoRaidPipelineOutcome::Failed { error };
                    }
                    self.handle_rejected_target(&target, &error).await;
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
        blacklist_ids: &HashSet<String>,
        blacklist_logins: &HashSet<String>,
        exclude_ids: &HashSet<String>,
        cached_fallback: &mut Option<Vec<FairnessCandidate>>,
        attempt: usize,
    ) -> Option<ResolvedTarget> {
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

        let (Some(fallback), Some(category_id)) = (&self.fallback, &req.category_id) else {
            return None;
        };

        if cached_fallback.is_none() {
            tracing::info!(
                from = %req.broadcaster_login,
                attempt,
                "Keine Partner online — hole Deadlock-DE-Fallback-Streams"
            );
            let streams = match fallback
                .category_streams(category_id, FALLBACK_LANGUAGE, FALLBACK_LIMIT)
                .await
            {
                Ok(streams) => streams,
                Err(error) => {
                    tracing::error!(%error, "Fallback-Streams nicht abrufbar");
                    Vec::new()
                }
            };
            *cached_fallback = Some(streams);
        }
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

        resolve_fallback_target(
            streams,
            &recent_targets,
            &received_raids_by_id,
            blacklist_ids,
            blacklist_logins,
            exclude_ids,
        )
    }

    /// Strike-/Blacklist-Behandlung für ein Ziel, das Raids ablehnt
    /// (Python Z. 413–452): Partner werden nur übersprungen, Nicht-Partner
    /// sammeln Strikes und landen ab Strike 2 auf der Blacklist.
    async fn handle_rejected_target(&self, target: &ResolvedTarget, error: &str) {
        if target.is_partner_raid {
            tracing::warn!(
                to = %target.user_login,
                "Raid abgelehnt: Partner-Ziel erlaubt keine Raids — überspringe ohne Blacklist"
            );
            return;
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
        } else {
            tracing::info!(
                to = %target.user_login,
                strikes,
                "Raid abgelehnt: Strike vergeben, noch keine Blacklist — nächster Versuch"
            );
        }
    }

    /// Registriert den erfolgreichen Raid als Pending (Arrival-Korrelation) und
    /// räumt veraltete Pendings derselben Quelle ab (Python `register_pending_raid`).
    fn register_pending(
        &self,
        req: &AutoRaidRequest,
        target: &ResolvedTarget,
        flow_id: &str,
        channel_raid_ready: bool,
    ) {
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
        store.store(pending);
    }
}
