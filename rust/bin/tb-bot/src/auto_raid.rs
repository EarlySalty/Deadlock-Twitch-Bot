//! Auto-Raid-Handler (`on_stream_offline`): geht ein Partner offline, prüft
//! Suppression + Eligibility + Deadlock-Quelle, sammelt die Online-Partner und
//! übergibt an die [`AutoRaidPipeline`] (tb-raid). Port der I/O-Schale aus
//! `raid/mixin.py` (Offline-Trigger) + `offline_raid_orchestrator.py`
//! (`handle_streamer_offline`); die gesamte Auswahl-/Retry-Logik liegt
//! testbar in `tb-raid`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use tb_monitoring::sessions::tracker::FollowerCountSource;
use tb_monitoring::{LiveStateStore, OfflineSourceState};
use tb_raid::{
    build_online_candidates, classify_eligibility, AutoRaidPipeline, AutoRaidPipelineOutcome,
    AutoRaidRequest, DeadlockEvalInput, EligibilityBucket, ManualRaidSuppression,
    OfflineEligibilityStore, PartnerRosterStore, StreamData,
};
use tb_transport_twitch::{HelixClient, HelixStream};

use sqlx::PgPool;

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Wartezeit zwischen Offline-Trigger und Auto-Raid. Endet ein Stream durch
/// einen manuellen Raid, kann das `channel.moderate`-Event mit dem echten Ziel
/// knapp nach `stream.offline` eintreffen. Ohne diese Pause gewinnt der
/// Auto-Raid das Rennen und schickt einen zweiten Raid hinterher.
/// Restzeit bis das Grace-Fenster seit dem Offline-Trigger voll ist. Die
/// Vorarbeit (Helix-Abfragen, Partner-Auswahl) zählt mit — meist bleibt nichts
/// zu warten. Negative/kaputte Zeitstempel ergeben das volle Fenster.
fn remaining_grace(
    offline_trigger_ts: f64,
    now_ts: f64,
    grace: std::time::Duration,
) -> std::time::Duration {
    let elapsed = now_ts - offline_trigger_ts;
    if !elapsed.is_finite() || elapsed < 0.0 {
        return grace;
    }
    grace.saturating_sub(std::time::Duration::from_secs_f64(elapsed))
}

/// Wartet die Restzeit des Grace-Fensters ab und prüft danach erneut, ob der
/// Streamer inzwischen selbst geraidet hat. `true` = Auto-Raid abbrechen.
async fn manual_raid_won_the_race(
    suppression: &Arc<Mutex<ManualRaidSuppression>>,
    broadcaster_id: &str,
    remaining: std::time::Duration,
) -> bool {
    if !remaining.is_zero() {
        tokio::time::sleep(remaining).await;
    }
    suppression
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_suppressed(broadcaster_id, None)
}

/// Helix-Stream → Roster-Stream-Daten (Follower werden später nur für
/// eligible Kandidaten geholt).
fn to_stream_data(stream: &HelixStream) -> StreamData {
    StreamData {
        viewer_count: stream.viewer_count as i32,
        followers_total: 0,
        started_at: Some(stream.started_at.clone()).filter(|s| !s.trim().is_empty()),
        game_name: Some(stream.game_name.clone()).filter(|g| !g.trim().is_empty()),
        language: Some(stream.language.trim().to_lowercase()).filter(|l| !l.is_empty()),
    }
}

fn streams_by_login_from_helix_result<E: std::fmt::Display>(
    result: Result<Vec<HelixStream>, E>,
    broadcaster_id: &str,
) -> HashMap<String, StreamData> {
    match result {
        Ok(streams) => streams
            .iter()
            .map(|s| (s.user_login.trim().to_lowercase(), to_stream_data(s)))
            .collect(),
        Err(error) => {
            tracing::warn!(
                %error,
                broadcaster_id,
                "Raid: Helix-Streams nicht ladbar, Kategorie-Fallback bleibt moeglich"
            );
            HashMap::new()
        }
    }
}

/// Skip-Grund der Quell-Prüfung fürs Log (Python-Reasons in `mixin.py`).
fn source_skip_reason(state: &OfflineSourceState, _target_game_lower: &str) -> &'static str {
    let game_lower = state
        .last_game
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let had_session = state.had_deadlock_in_session.unwrap_or(0) != 0;
    if game_lower == "just chatting" {
        if had_session {
            return "stale_deadlock_session";
        }
        return "just_chatting_without_deadlock_session";
    }
    if game_lower.is_empty() {
        return "missing_current_game";
    }
    "source_category_mismatch"
}

/// Ergebnis der Kandidaten-Assemblierung (Schritte 5–7).
struct AssembledPartners {
    eligible: Vec<tb_raid::OnlineCandidate>,
    online_count: usize,
    filtered_out: usize,
    /// Partner, die der Netzwerk-/Sprachfilter (Paket R) aussortiert hat;
    /// sie bleiben auch für den Kategorie-Ersatz ausgeschlossen.
    network_excluded_ids: Vec<String>,
    /// Sprache des Kategorie-Ersatzes (Sprachgruppe des Raiders).
    fallback_language: String,
}

struct CurrentRaidNetworkPolicy {
    pool: PgPool,
    helix: HelixClient,
}

#[async_trait::async_trait]
impl tb_raid::auto_raid_pipeline::RaidNetworkPolicy for CurrentRaidNetworkPolicy {
    async fn validate(&self, source_id: &str, target_id: &str) -> Result<(), String> {
        let ids = vec![source_id.trim().to_string(), target_id.trim().to_string()];
        let streams = self
            .helix
            .get_streams_by_user_ids(&ids, None)
            .await
            .map_err(|error| {
                tracing::error!(%error, source_id, target_id, "Raid: aktuelle Sprachen nicht ladbar");
                "network_language_unavailable".to_string()
            })?;
        validate_loaded_network_policy(&self.pool, source_id, target_id, &streams).await
    }
}

async fn validate_loaded_network_policy(
    pool: &PgPool,
    source_id: &str,
    target_id: &str,
    streams: &[HelixStream],
) -> Result<(), String> {
    let ids = vec![source_id.trim().to_string(), target_id.trim().to_string()];
    let networks = tb_db::network::load_networks(pool, &ids)
        .await
        .map_err(|error| {
            tracing::error!(%error, source_id, target_id, "Raid: aktuelles Netzwerk nicht ladbar");
            "network_unavailable".to_string()
        })?;
    validate_current_networks(source_id, target_id, streams, &networks)
}

fn validate_current_networks(
    source_id: &str,
    target_id: &str,
    streams: &[HelixStream],
    networks: &HashMap<String, tb_domain::network::StreamerNetwork>,
) -> Result<(), String> {
    let source = networks
        .get(source_id.trim())
        .ok_or_else(|| "source_network_unknown".to_string())?;
    let target = networks
        .get(target_id.trim())
        .ok_or_else(|| "target_network_unknown".to_string())?;
    let live_language = |id: &str| {
        streams
            .iter()
            .find(|stream| stream.user_id.trim() == id.trim())
            .map(|stream| stream.language.as_str())
    };
    let source_language = source.language_group(live_language(source_id));
    let target_language = target.language_group(live_language(target_id));
    tb_domain::network::raid_reach(
        source,
        source_language.as_deref(),
        target,
        target_language.as_deref(),
    )
    .map(|_| ())
    .map_err(|reason| reason.as_str().to_string())
}

fn apply_network_scope(
    broadcaster_id: &str,
    raider_live_language: Option<&str>,
    online: Vec<tb_raid::OnlineCandidate>,
    networks: Result<HashMap<String, tb_domain::network::StreamerNetwork>, sqlx::Error>,
) -> Result<(Vec<tb_raid::OnlineCandidate>, Vec<String>, String), &'static str> {
    let mut networks = networks.map_err(|error| {
        tracing::error!(%error, broadcaster_id, "Raid: Server-Netzwerk nicht ladbar");
        "network_unavailable"
    })?;
    let raider = networks
        .remove(broadcaster_id.trim())
        .ok_or("source_network_unknown")?;
    let raider_language = raider
        .language_group(raider_live_language)
        .ok_or("source_language_unknown")?;
    let outcome =
        tb_raid::filter_candidates_by_network(&raider, Some(&raider_language), online, &networks);
    for dropped in &outcome.dropped {
        tracing::debug!(
            broadcaster_id,
            ziel = %dropped.twitch_login,
            grund = dropped.reason.as_str(),
            "Raid-Ziel ausserhalb des Netzwerks"
        );
    }
    if outcome.cross_server > 0 {
        tracing::info!(
            broadcaster_id,
            ueber_servergrenzen = outcome.cross_server,
            "Raid-Pool enthält Partner anderer Server"
        );
    }
    Ok((
        outcome.kept,
        outcome
            .dropped
            .into_iter()
            .map(|dropped| dropped.twitch_user_id)
            .collect(),
        raider_language,
    ))
}

/// Quell-Zustand für den manuellen Raid — aus dem Helix-Stream (Vorrang)
/// oder dem DB-Restzustand (Fallback). Session-Flags kommen immer aus der DB
/// (Python `overlay_broadcaster_live_state_from_stream`).
#[derive(Debug)]
struct ManualSource {
    last_game: String,
    had_deadlock_session: bool,
    last_deadlock_seen_at: Option<String>,
    viewer_count: i32,
    started_at: Option<String>,
    /// Sprache des laufenden Streams (nur aus Helix; offline unbekannt).
    language: Option<String>,
}

impl ManualSource {
    fn from_stream(stream: &HelixStream, db: Option<&OfflineSourceState>) -> Self {
        Self {
            last_game: stream.game_name.trim().to_string(),
            had_deadlock_session: db
                .map(|s| s.had_deadlock_in_session.unwrap_or(0) != 0)
                .unwrap_or(false),
            last_deadlock_seen_at: db.and_then(|s| s.last_deadlock_seen_at.clone()),
            viewer_count: stream.viewer_count as i32,
            started_at: Some(stream.started_at.clone()).filter(|s| !s.trim().is_empty()),
            language: Some(stream.language.trim().to_lowercase()).filter(|l| !l.is_empty()),
        }
    }

    /// DB-Quelle: letzter bekannter Stream-Zustand — bewusst AUCH offline
    /// (Abweichung von Python): `!raid` wird gerade nach Stream-Ende
    /// gebraucht, wenn der Auto-Raid nicht gefeuert hat. Ob das Raid-Fenster
    /// noch offen ist, entscheidet die Twitch-Raid-API beim Versuch.
    /// `None` nur, wenn es nie einen Stream gab.
    fn from_db(state: &OfflineSourceState) -> Option<Self> {
        let never_streamed = state
            .last_started_at
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
            && state.last_game.as_deref().unwrap_or("").trim().is_empty();
        if never_streamed {
            return None;
        }
        Some(Self {
            last_game: state.last_game.clone().unwrap_or_default(),
            had_deadlock_session: state.had_deadlock_in_session.unwrap_or(0) != 0,
            last_deadlock_seen_at: state.last_deadlock_seen_at.clone(),
            viewer_count: state.last_viewer_count.unwrap_or(0),
            started_at: state.last_started_at.clone(),
            language: None,
        })
    }

    fn stream_duration_sec(&self, now: chrono::DateTime<Utc>) -> i32 {
        self.started_at
            .as_deref()
            .and_then(tb_raid::parse_iso_utc)
            .map(|started| (now - started).num_seconds().max(0) as i32)
            .unwrap_or(0)
    }
}

/// Antwort des manuellen Raids — Status-Strings sind der Vertrag des
/// Python-Chat-Commands (`started`, `source_not_live`, `source_not_eligible`,
/// `no_target`, `blocked`, `raid_failed`, `unavailable`).
#[derive(Debug, serde::Serialize)]
pub struct ManualRaidResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_login: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ManualRaidResponse {
    fn status(status: &str) -> Self {
        Self {
            status: status.to_string(),
            target_login: None,
            reason: None,
            error: None,
        }
    }

    fn status_with_reason(status: &str, reason: &str) -> Self {
        Self {
            reason: Some(reason.to_string()),
            ..Self::status(status)
        }
    }

    fn started(target_login: String) -> Self {
        Self {
            target_login: Some(target_login),
            ..Self::status("started")
        }
    }

    fn error(status: &str, error: &str) -> Self {
        Self {
            error: Some(error.to_string()),
            ..Self::status(status)
        }
    }
}

pub struct OfflineRaidHandler {
    offline_grace: std::time::Duration,
    suppression: Arc<Mutex<ManualRaidSuppression>>,
    eligibility: OfflineEligibilityStore,
    live_state: LiveStateStore,
    roster: PartnerRosterStore,
    helix: HelixClient,
    followers: Arc<dyn FollowerCountSource>,
    pipeline: AutoRaidPipeline,
    /// Pool für das Server-Netzwerk der Streamer (Paket R, `tb_db::network`).
    network_pool: PgPool,
    /// Spielname in Original-Schreibweise (für die Kategorie-Suche).
    target_game: String,
    target_game_lower: String,
    /// Lazy aufgelöste Helix-Kategorie-ID des Ziel-Spiels (für den DE-Fallback).
    category_id_cache: tokio::sync::Mutex<Option<String>>,
}

impl OfflineRaidHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        suppression: Arc<Mutex<ManualRaidSuppression>>,
        eligibility: OfflineEligibilityStore,
        live_state: LiveStateStore,
        roster: PartnerRosterStore,
        helix: HelixClient,
        followers: Arc<dyn FollowerCountSource>,
        pipeline: AutoRaidPipeline,
        network_pool: PgPool,
        target_game: &str,
        offline_grace: std::time::Duration,
    ) -> Self {
        let pipeline = pipeline.with_network_policy(Arc::new(CurrentRaidNetworkPolicy {
            pool: network_pool.clone(),
            helix: helix.clone(),
        }));
        Self {
            offline_grace,
            suppression,
            eligibility,
            live_state,
            roster,
            helix,
            followers,
            pipeline,
            network_pool,
            target_game: target_game.trim().to_string(),
            target_game_lower: target_game.trim().to_lowercase(),
            category_id_cache: tokio::sync::Mutex::new(None),
        }
    }

    /// Kompletter Offline-Trigger. Fehler werden geloggt, nie propagiert —
    /// der EventSub-Dispatcher darf daran nicht scheitern.
    pub async fn handle_streamer_offline(&self, broadcaster_id: &str, login: Option<&str>) {
        let offline_trigger_ts = unix_now();
        let login = login.unwrap_or("").trim().to_lowercase();
        let streamer_label = if login.is_empty() {
            broadcaster_id
        } else {
            login.as_str()
        };

        // 1. Manual-Raid-Suppression (in-memory TTL).
        let suppressed = self
            .suppression
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_suppressed(broadcaster_id, None);
        if suppressed {
            tracing::info!(
                streamer = streamer_label,
                "Auto-Raid übersprungen: kürzlich manueller/externer Raid erkannt"
            );
            return;
        }

        // 2. Quell-Eligibility (aktiver Partner + Setting + Auth).
        let elig = match self.eligibility.load(broadcaster_id).await {
            Ok(elig) => elig,
            Err(error) => {
                tracing::error!(%error, streamer = streamer_label, "Auto-Raid: Eligibility nicht ladbar");
                return;
            }
        };
        if let Some(reason) = elig.skip_reason() {
            tracing::debug!(streamer = streamer_label, reason, "Auto-Raid übersprungen");
            return;
        }

        // 3. Session-Restzustand: war die Quelle Deadlock-eligible?
        let state = match self.live_state.offline_source_state(broadcaster_id).await {
            Ok(Some(state)) => state,
            Ok(None) => {
                tracing::debug!(
                    streamer = streamer_label,
                    "Auto-Raid: kein Live-State — übersprungen"
                );
                return;
            }
            Err(error) => {
                tracing::error!(%error, streamer = streamer_label, "Auto-Raid: Live-State nicht lesbar");
                return;
            }
        };
        let now = Utc::now();
        let source_eval = DeadlockEvalInput {
            game_name: state.last_game.as_deref().unwrap_or(""),
            had_deadlock_session: state.had_deadlock_in_session.unwrap_or(0) != 0,
            last_deadlock_seen_at: state.last_deadlock_seen_at.as_deref(),
        };
        if classify_eligibility(&source_eval, now, &self.target_game_lower).is_none() {
            tracing::info!(
                streamer = streamer_label,
                last_game = state.last_game.as_deref().unwrap_or("unbekannt"),
                reason = source_skip_reason(&state, &self.target_game_lower),
                "Auto-Raid ausgelassen: Quelle nicht Deadlock-eligible"
            );
            return;
        }

        // 4. Stream-Kennzahlen aus dem Restzustand.
        let viewer_count = state.last_viewer_count.unwrap_or(0);
        let stream_duration_sec = state
            .last_started_at
            .as_deref()
            .and_then(tb_raid::parse_iso_utc)
            .map(|started| (now - started).num_seconds().max(0) as i32)
            .unwrap_or(0);

        // 5.–7. Online-Partner sammeln, filtern, anreichern (gemeinsam mit
        // dem manuellen Raid).
        let assembled = match self
            .assemble_eligible_partners(broadcaster_id, None, now)
            .await
        {
            Ok(assembled) => assembled,
            Err(reason) => {
                tracing::error!(
                    broadcaster_id,
                    reason,
                    "Auto-Raid blockiert: Kandidaten nicht sicher prüfbar"
                );
                return;
            }
        };

        tracing::info!(
            streamer = streamer_label,
            viewers = viewer_count,
            duration_sec = stream_duration_sec,
            online_partners = assembled.online_count,
            eligible_partners = assembled.eligible.len(),
            filtered_out = assembled.filtered_out,
            netzwerk_gefiltert = assembled.network_excluded_ids.len(),
            ersatz_sprache = %assembled.fallback_language,
            "Auto-Raid-Pipeline gestartet"
        );
        let eligible = assembled.eligible;

        // 8. Pipeline (Auswahl → Readiness → Raid → Pending).
        let request = AutoRaidRequest {
            broadcaster_id: broadcaster_id.to_string(),
            broadcaster_login: login.clone(),
            viewer_count,
            stream_duration_sec,
            partners: eligible,
            category_id: self.resolve_category_id().await,
            fallback_language: Some(assembled.fallback_language),
            network_excluded_ids: assembled.network_excluded_ids,
            offline_trigger_ts: Some(offline_trigger_ts),
            reason: "auto_raid_on_offline".to_string(),
            respect_soft_raid_blacklist: true,
        };

        // Letzte Prüfung direkt vor dem Raid: hat der Streamer zwischenzeitlich
        // selbst geraidet, wäre unser Raid ein zweiter — Twitch nimmt den auch
        // nach Stream-Ende noch an und schickt die Zuschauerreste woanders hin.
        let remaining = remaining_grace(offline_trigger_ts, unix_now(), self.offline_grace);
        if manual_raid_won_the_race(&self.suppression, broadcaster_id, remaining).await {
            tracing::info!(
                streamer = streamer_label,
                wartezeit_ms = remaining.as_millis(),
                reason = "manual_raid_during_grace",
                "Auto-Raid abgebrochen: Streamer hat selbst geraidet"
            );
            return;
        }

        match self.pipeline.run(&request).await {
            AutoRaidPipelineOutcome::Started { target_login, .. } => {
                tracing::info!(from = streamer_label, to = %target_login, "✅ Auto-Raid erfolgreich");
            }
            AutoRaidPipelineOutcome::NoTarget => {
                tracing::debug!(
                    streamer = streamer_label,
                    "Auto-Raid nicht durchgeführt (kein Ziel)"
                );
            }
            AutoRaidPipelineOutcome::Blocked { error }
            | AutoRaidPipelineOutcome::Failed { error } => {
                tracing::error!(streamer = streamer_label, %error, "Auto-Raid fehlgeschlagen");
            }
        }
    }

    /// Manueller Raid (`!raid`, Python `start_manual_raid`): Quelle muss live
    /// und Deadlock-eligible sein; danach dieselbe Pipeline wie der Auto-Raid
    /// mit `reason=manual_chat_command` + Suppression-Mark nach Erfolg.
    /// Die Statuswerte sind der Vertrag des Chat-Commands.
    pub async fn start_manual_raid(
        &self,
        broadcaster_id: &str,
        broadcaster_login: &str,
    ) -> ManualRaidResponse {
        let login = broadcaster_login.trim().to_lowercase();
        let now = Utc::now();

        // 1. Quell-Zustand: Helix hat Vorrang (Python `api_live`/`api_offline`),
        //    DB-Restzustand nur als Fallback bei Helix-Fehler.
        let db_state = self
            .live_state
            .offline_source_state(broadcaster_id)
            .await
            .ok()
            .flatten();
        let source = match self
            .helix
            .get_streams_by_logins(std::slice::from_ref(&login), None)
            .await
        {
            Ok(streams) => match streams
                .into_iter()
                .find(|s| s.user_login.trim().to_lowercase() == login)
            {
                Some(stream) => Some(ManualSource::from_stream(&stream, db_state.as_ref())),
                // Offline laut API: letzter Stream-Zustand aus der DB —
                // `!raid` soll gerade nach Stream-Ende funktionieren.
                None => db_state.as_ref().and_then(ManualSource::from_db),
            },
            Err(error) => {
                tracing::debug!(%error, streamer = %login, "Manual-Raid: Helix-Refresh fehlgeschlagen — DB-Fallback");
                db_state.as_ref().and_then(ManualSource::from_db)
            }
        };
        let Some(source) = source else {
            return ManualRaidResponse::status_with_reason("source_not_live", "no_known_stream");
        };

        // 2. Deadlock-Eligibility der Quelle.
        let eval = DeadlockEvalInput {
            game_name: &source.last_game,
            had_deadlock_session: source.had_deadlock_session,
            last_deadlock_seen_at: source.last_deadlock_seen_at.as_deref(),
        };
        if classify_eligibility(&eval, now, &self.target_game_lower).is_none() {
            let reason = db_state
                .as_ref()
                .map(|s| source_skip_reason(s, &self.target_game_lower))
                .unwrap_or("last_game_not_eligible");
            tracing::info!(
                streamer = %login,
                last_game = %source.last_game,
                reason,
                "Manual-Raid übersprungen: Quelle nicht Deadlock-eligible"
            );
            return ManualRaidResponse::status_with_reason("source_not_eligible", reason);
        }

        // 3. Kandidaten + Pipeline (wie Auto-Raid).
        let assembled = match self
            .assemble_eligible_partners(broadcaster_id, source.language.as_deref(), now)
            .await
        {
            Ok(assembled) => assembled,
            Err("candidate_assembly_failed") => {
                return ManualRaidResponse::error("unavailable", "candidate_assembly_failed");
            }
            Err(reason) => return ManualRaidResponse::error("blocked", reason),
        };
        tracing::info!(
            streamer = %login,
            viewers = source.viewer_count,
            duration_sec = source.stream_duration_sec(now),
            online_partners = assembled.online_count,
            eligible_partners = assembled.eligible.len(),
            netzwerk_gefiltert = assembled.network_excluded_ids.len(),
            ersatz_sprache = %assembled.fallback_language,
            "Manual-Raid-Pipeline gestartet"
        );
        let request = AutoRaidRequest {
            broadcaster_id: broadcaster_id.to_string(),
            broadcaster_login: login.clone(),
            viewer_count: source.viewer_count,
            stream_duration_sec: source.stream_duration_sec(now),
            partners: assembled.eligible,
            category_id: self.resolve_category_id().await,
            fallback_language: Some(assembled.fallback_language),
            network_excluded_ids: assembled.network_excluded_ids,
            offline_trigger_ts: None,
            reason: "manual_chat_command".to_string(),
            respect_soft_raid_blacklist: false,
        };
        match self.pipeline.run(&request).await {
            AutoRaidPipelineOutcome::Started { target_login, .. } => {
                // Python `set_manual_suppression=True`: 180 s kein Auto-Raid.
                self.suppression
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .mark(broadcaster_id, 180.0, None);
                tracing::info!(from = %login, to = %target_login, "✅ Manual-Raid gestartet");
                ManualRaidResponse::started(target_login)
            }
            AutoRaidPipelineOutcome::NoTarget => ManualRaidResponse::status("no_target"),
            AutoRaidPipelineOutcome::Blocked { error } => {
                ManualRaidResponse::error("blocked", &error)
            }
            AutoRaidPipelineOutcome::Failed { error } => {
                ManualRaidResponse::error("raid_failed", &error)
            }
        }
    }

    /// Schritte 5–7 des Raid-Triggers: Roster → Helix-Streams → Session-Flags
    /// → Netzwerk-/Sprachfilter (Paket R) → Eligibility-Partition (aktiv vor
    /// kürzlich) → Follower-Anreicherung. `raider_live_language` ist die
    /// Sprache des laufenden Streams der Quelle, falls bekannt (manueller Raid).
    /// `None` bei I/O-Fehlern (bereits geloggt).
    async fn assemble_eligible_partners(
        &self,
        broadcaster_id: &str,
        raider_live_language: Option<&str>,
        now: chrono::DateTime<Utc>,
    ) -> Result<AssembledPartners, &'static str> {
        let roster = match self.roster.load_roster(broadcaster_id).await {
            Ok(roster) => roster,
            Err(error) => {
                tracing::error!(%error, broadcaster_id, "Raid: Roster nicht ladbar");
                return Err("candidate_assembly_failed");
            }
        };
        let logins: Vec<String> = roster.iter().map(|p| p.twitch_login.clone()).collect();
        let streams_by_login = streams_by_login_from_helix_result(
            self.helix.get_streams_by_logins(&logins, None).await,
            broadcaster_id,
        );
        let flags = self
            .live_state
            .source_states_by_logins(&logins)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "Raid: Partner-Live-State nicht ladbar");
                HashMap::new()
            });

        let online = build_online_candidates(&roster, &streams_by_login);
        let online_count = online.len();

        // Netzwerk-/Sprachfilter vor der Eligibility-Partition, damit der
        // Rückfall "aktiv leer → kürzlich" nur im erlaubten Pool greift.
        let mut network_ids: Vec<String> = Vec::with_capacity(online.len() + 1);
        network_ids.push(broadcaster_id.trim().to_string());
        network_ids.extend(online.iter().map(|c| c.twitch_user_id.trim().to_string()));
        let networks = tb_db::network::load_networks(&self.network_pool, &network_ids).await;
        let (online, network_excluded_ids, fallback_language) =
            apply_network_scope(broadcaster_id, raider_live_language, online, networks)?;

        // Deadlock-Eligibility je Kandidat (aktiv vor kürzlich) — Partition
        // wie `filter_eligible`, hier inline weil die Session-Flags aus der
        // separaten Map kommen (Borrow ausserhalb des Kandidaten).
        let mut active = Vec::new();
        let mut recent = Vec::new();
        let mut filtered_out = 0usize;
        for candidate in online {
            let flag = flags.get(&candidate.twitch_login);
            let input = DeadlockEvalInput {
                game_name: candidate.stream.game_name.as_deref().unwrap_or(""),
                had_deadlock_session: flag
                    .map(|f| f.had_deadlock_in_session.unwrap_or(0) != 0)
                    .unwrap_or(false),
                last_deadlock_seen_at: flag.and_then(|f| f.last_deadlock_seen_at.as_deref()),
            };
            match classify_eligibility(&input, now, &self.target_game_lower) {
                Some(EligibilityBucket::Active) => active.push(candidate),
                Some(EligibilityBucket::Recent) => recent.push(candidate),
                None => filtered_out += 1,
            }
        }
        let mut eligible = if active.is_empty() { recent } else { active };

        // Follower nur für eligible Kandidaten anreichern (Score-Tie-Break).
        for candidate in &mut eligible {
            if let Some(total) = self
                .followers
                .follower_total(Some(&candidate.twitch_user_id), &candidate.twitch_login)
                .await
                .total
            {
                candidate.stream.followers_total = total;
            }
        }

        Ok(AssembledPartners {
            eligible,
            online_count,
            filtered_out,
            network_excluded_ids,
            fallback_language,
        })
    }

    /// Kategorie-ID des Ziel-Spiels, lazy aufgelöst und gecacht.
    async fn resolve_category_id(&self) -> Option<String> {
        if self.target_game.is_empty() {
            return None;
        }
        let mut cache = self.category_id_cache.lock().await;
        if cache.is_none() {
            match self.helix.search_category_id(&self.target_game).await {
                Ok(found) => *cache = found,
                Err(error) => {
                    tracing::debug!(%error, "Auto-Raid: Kategorie-ID nicht auflösbar");
                }
            }
        }
        cache.clone()
    }
}

#[cfg(test)]
mod grace_tests {
    use super::*;

    fn suppression() -> Arc<Mutex<ManualRaidSuppression>> {
        Arc::new(Mutex::new(ManualRaidSuppression::new()))
    }

    #[tokio::test(start_paused = true)]
    async fn manueller_raid_waehrend_der_wartezeit_stoppt_den_auto_raid() {
        let suppression = suppression();
        let observer = Arc::clone(&suppression);
        let grace = std::time::Duration::from_secs(5);

        // Das channel.moderate-Event trifft mitten im Grace-Fenster ein.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            observer.lock().unwrap().mark("1186925760", 180.0, None);
        });

        assert!(manual_raid_won_the_race(&suppression, "1186925760", grace).await);
    }

    #[tokio::test(start_paused = true)]
    async fn ohne_manuellen_raid_laeuft_der_auto_raid_weiter() {
        let suppression = suppression();
        let grace = std::time::Duration::from_secs(5);

        assert!(!manual_raid_won_the_race(&suppression, "1186925760", grace).await);
    }

    #[tokio::test(start_paused = true)]
    async fn fremder_streamer_blockiert_nicht() {
        let suppression = suppression();
        suppression.lock().unwrap().mark("999", 180.0, None);

        assert!(
            !manual_raid_won_the_race(
                &suppression,
                "1186925760",
                std::time::Duration::from_secs(1)
            )
            .await
        );
    }

    #[test]
    fn vorarbeit_zaehlt_auf_das_grace_fenster_an() {
        let grace = std::time::Duration::from_secs(5);
        // Die Helix-Runde hat schon 3,2 s gebraucht → nur der Rest wird gewartet.
        assert_eq!(
            remaining_grace(1000.0, 1003.2, grace).as_millis(),
            1800,
            "Restzeit muss 1,8 s sein"
        );
        // Vorarbeit länger als das Fenster → gar keine Wartezeit.
        assert!(remaining_grace(1000.0, 1009.0, grace).is_zero());
        // Uhr springt rückwärts → volles Fenster statt Unsinn.
        assert_eq!(remaining_grace(1000.0, 990.0, grace), grace);
        assert_eq!(remaining_grace(f64::NAN, 1000.0, grace), grace);
    }

    #[test]
    fn grace_behält_bisherigen_konfigurationsstandard() {
        // Historischer Standard bleibt in der typisierten Betriebskonfiguration erhalten.
        assert_eq!(
            tb_config::operations::BotOperations::default().auto_raid_offline_grace_seconds,
            5
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_data_mapping_filtert_leere_felder() {
        let stream = HelixStream {
            user_login: "x".to_string(),
            viewer_count: 12,
            started_at: "  ".to_string(),
            game_name: "Deadlock".to_string(),
            ..Default::default()
        };
        let data = to_stream_data(&stream);
        assert_eq!(data.viewer_count, 12);
        assert!(data.started_at.is_none(), "leere Startzeit → None");
        assert_eq!(data.game_name.as_deref(), Some("Deadlock"));
        assert!(data.language.is_none(), "leere Sprache → None");

        let english = HelixStream {
            language: " EN ".to_string(),
            ..stream
        };
        assert_eq!(to_stream_data(&english).language.as_deref(), Some("en"));
    }

    fn online(id: &str, language: &str) -> tb_raid::OnlineCandidate {
        tb_raid::OnlineCandidate {
            twitch_user_id: id.to_string(),
            twitch_login: format!("login_{id}"),
            raid_enabled: true,
            stream: StreamData {
                language: Some(language.to_string()),
                ..StreamData::default()
            },
        }
    }

    fn english_network() -> tb_domain::network::StreamerNetwork {
        use tb_domain::network::{GuildMembership, GuildSettings, StreamerNetwork};
        StreamerNetwork::new(
            vec![GuildMembership {
                guild: GuildSettings {
                    guild_id: 42,
                    language: "en".to_string(),
                    cross_server_raids: true,
                    promo_invite_url: None,
                    installed: true,
                },
                active: true,
            }],
            None,
            &GuildSettings::community_default(),
        )
    }

    #[test]
    fn netzwerkfehler_blockiert_auch_bei_bekannter_live_sprache() {
        let result = apply_network_scope(
            "1",
            Some("en"),
            vec![online("2", "en"), online("3", "de")],
            Err(sqlx::Error::PoolTimedOut),
        );
        assert_eq!(result.err(), Some("network_unavailable"));
    }

    #[test]
    fn englischer_raider_bekommt_englischen_ersatz_und_keine_deutschen_partner() {
        use tb_domain::network::{GuildSettings, StreamerNetwork};
        let community = GuildSettings::community_default();
        let networks = HashMap::from([
            ("1".to_string(), english_network()),
            ("2".to_string(), english_network()),
            ("3".to_string(), StreamerNetwork::community(&community)),
        ]);
        let (kept, excluded, language) = apply_network_scope(
            "1",
            None,
            vec![online("2", "en"), online("3", "de")],
            Ok(networks),
        )
        .expect("Netzwerkfilter");
        assert_eq!(
            kept.iter()
                .map(|c| c.twitch_login.as_str())
                .collect::<Vec<_>>(),
            vec!["login_2"]
        );
        assert_eq!(excluded, vec!["3".to_string()]);
        assert_eq!(language, "en");
    }

    #[test]
    fn deutscher_raider_behaelt_nur_deutschsprachige_partner() {
        use tb_domain::network::{GuildSettings, StreamerNetwork};
        let community = GuildSettings::community_default();
        let networks = HashMap::from([
            ("1".to_string(), StreamerNetwork::community(&community)),
            ("2".to_string(), StreamerNetwork::community(&community)),
            ("3".to_string(), StreamerNetwork::community(&community)),
        ]);
        let (kept, excluded, language) = apply_network_scope(
            "1",
            None,
            vec![online("2", "en"), online("3", "de")],
            Ok(networks),
        )
        .expect("Netzwerkfilter");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].twitch_user_id, "3");
        assert_eq!(excluded, vec!["2"]);
        assert_eq!(language, "de");
    }

    #[test]
    fn missing_source_network_blocks_assembly_and_execution() {
        assert_eq!(
            apply_network_scope("1", Some("en"), Vec::new(), Ok(HashMap::new())).err(),
            Some("source_network_unknown")
        );
        assert_eq!(
            validate_current_networks("1", "2", &[], &HashMap::new()),
            Err("source_network_unknown".to_string())
        );
        assert_eq!(
            validate_current_networks(
                "1",
                "2",
                &[],
                &HashMap::from([("1".to_string(), english_network())]),
            ),
            Err("target_network_unknown".to_string())
        );
    }

    #[tokio::test]
    async fn current_policy_reloads_undiscovered_targets_and_fails_closed_on_db_error() {
        let dsn =
            std::env::var("TB_TEST_DATABASE_URL").expect("Wegwerf-Testdatenbank erforderlich");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("Testdatenbank");
        for ddl in [
            "SET search_path TO pg_temp",
            "CREATE TEMP TABLE discord_guilds (guild_id BIGINT PRIMARY KEY, language TEXT NOT NULL, cross_server_raids BOOLEAN NOT NULL, promo_invite_url TEXT, status TEXT NOT NULL, removed_at TIMESTAMPTZ)",
            "CREATE TEMP TABLE streamer_guilds (twitch_user_id TEXT, guild_id BIGINT, active BOOLEAN, joined_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TEMP TABLE twitch_streamer_identities (twitch_user_id TEXT, broadcaster_language TEXT)",
            "INSERT INTO discord_guilds (guild_id, language, cross_server_raids, status) VALUES (42, 'en', TRUE, 'active'), (43, 'en', FALSE, 'active')",
            "INSERT INTO streamer_guilds (twitch_user_id, guild_id, active) VALUES ('1', 42, TRUE), ('2', 43, TRUE)",
            "INSERT INTO twitch_streamer_identities VALUES ('1', 'en'), ('2', 'en')",
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("Testschema");
        }
        let source_networks = tb_db::network::load_networks(&pool, &["1".to_string()]).await;
        let (partners, excluded, language) =
            apply_network_scope("1", None, Vec::new(), source_networks)
                .expect("Leere Helix-Login-Entdeckung mit gültigem Quellnetzwerk");
        assert!(partners.is_empty());
        assert!(excluded.is_empty());
        assert_eq!(language, "en");
        assert_eq!(
            validate_loaded_network_policy(&pool, "1", "2", &[]).await,
            Err("netzwerk_schalter_aus".to_string())
        );
        sqlx::query("UPDATE discord_guilds SET cross_server_raids = TRUE WHERE guild_id = 43")
            .execute(&pool)
            .await
            .expect("Zielnetzwerk einschalten");
        assert_eq!(
            validate_loaded_network_policy(&pool, "1", "2", &[]).await,
            Ok(())
        );
        sqlx::query("UPDATE discord_guilds SET cross_server_raids = FALSE WHERE guild_id = 42")
            .execute(&pool)
            .await
            .expect("Quellnetzwerk ausschalten");
        assert_eq!(
            validate_loaded_network_policy(&pool, "1", "2", &[]).await,
            Err("netzwerk_schalter_aus".to_string())
        );
        sqlx::query("UPDATE streamer_guilds SET guild_id = 42 WHERE twitch_user_id = '2'")
            .execute(&pool)
            .await
            .expect("Gemeinsamer Server");
        assert_eq!(
            validate_loaded_network_policy(&pool, "1", "2", &[]).await,
            Ok(())
        );
        let streams = [HelixStream {
            user_id: "2".to_string(),
            language: "de".to_string(),
            ..Default::default()
        }];
        assert_eq!(
            validate_loaded_network_policy(&pool, "1", "2", &streams).await,
            Err("andere_sprache".to_string())
        );
        sqlx::query("DROP TABLE discord_guilds")
            .execute(&pool)
            .await
            .expect("Netzwerk-Ladefehler");
        assert_eq!(
            validate_loaded_network_policy(&pool, "1", "2", &[]).await,
            Err("network_unavailable".to_string())
        );
        let broken = tb_db::network::load_networks(&pool, &["1".to_string()]).await;
        assert_eq!(
            apply_network_scope("1", Some("en"), Vec::new(), broken).err(),
            Some("network_unavailable")
        );
        pool.close().await;
    }

    #[test]
    fn stream_fetch_error_degradiert_zu_leerer_stream_map() {
        let streams = streams_by_login_from_helix_result::<&str>(Err("helix down"), "source_1");
        assert!(
            streams.is_empty(),
            "Helix-Fehler darf Kandidatenassemblierung nicht abbrechen"
        );
    }

    #[test]
    fn manual_source_aus_db_auch_offline_aber_nicht_ohne_stream_historie() {
        // Offline-Restzustand (is_live=0) ist eine gültige Quelle — !raid
        // wird gerade nach Stream-Ende gebraucht.
        let offline = OfflineSourceState {
            is_live: Some(0),
            last_game: Some("Deadlock".to_string()),
            had_deadlock_in_session: Some(1),
            last_deadlock_seen_at: Some("2026-06-10T18:30:00+00:00".to_string()),
            last_viewer_count: Some(12),
            last_started_at: Some("2026-06-10T17:00:00+00:00".to_string()),
        };
        let source = ManualSource::from_db(&offline).expect("offline ist gültige Quelle");
        assert_eq!(source.last_game, "Deadlock");
        assert_eq!(source.viewer_count, 12);
        assert!(source.had_deadlock_session);

        // Nie gestreamt (kein Spiel, keine Startzeit) → keine Quelle.
        let leer = OfflineSourceState {
            is_live: Some(0),
            last_game: None,
            had_deadlock_in_session: Some(0),
            last_deadlock_seen_at: None,
            last_viewer_count: None,
            last_started_at: None,
        };
        assert!(ManualSource::from_db(&leer).is_none());
    }

    #[test]
    fn source_skip_reason_unterscheidet_just_chatting_faelle() {
        let base = OfflineSourceState {
            is_live: Some(0),
            last_game: Some("Just Chatting".to_string()),
            had_deadlock_in_session: Some(1),
            last_deadlock_seen_at: None,
            last_viewer_count: None,
            last_started_at: None,
        };
        assert_eq!(
            source_skip_reason(&base, "deadlock"),
            "stale_deadlock_session"
        );

        let no_session = OfflineSourceState {
            had_deadlock_in_session: Some(0),
            ..base.clone()
        };
        assert_eq!(
            source_skip_reason(&no_session, "deadlock"),
            "just_chatting_without_deadlock_session"
        );

        let other_game = OfflineSourceState {
            last_game: Some("Valorant".to_string()),
            ..base
        };
        assert_eq!(
            source_skip_reason(&other_game, "deadlock"),
            "source_category_mismatch"
        );
    }
}
