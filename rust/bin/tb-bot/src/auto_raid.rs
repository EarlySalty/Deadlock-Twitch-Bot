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

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Helix-Stream → Roster-Stream-Daten (Follower werden später nur für
/// eligible Kandidaten geholt).
fn to_stream_data(stream: &HelixStream) -> StreamData {
    StreamData {
        viewer_count: stream.viewer_count as i32,
        followers_total: 0,
        started_at: Some(stream.started_at.clone()).filter(|s| !s.trim().is_empty()),
        game_name: Some(stream.game_name.clone()).filter(|g| !g.trim().is_empty()),
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
    suppression: Arc<Mutex<ManualRaidSuppression>>,
    eligibility: OfflineEligibilityStore,
    live_state: LiveStateStore,
    roster: PartnerRosterStore,
    helix: HelixClient,
    followers: Arc<dyn FollowerCountSource>,
    pipeline: AutoRaidPipeline,
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
        target_game: &str,
    ) -> Self {
        Self {
            suppression,
            eligibility,
            live_state,
            roster,
            helix,
            followers,
            pipeline,
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
        let Some(assembled) = self.assemble_eligible_partners(broadcaster_id, now).await else {
            return;
        };

        tracing::info!(
            streamer = streamer_label,
            viewers = viewer_count,
            duration_sec = stream_duration_sec,
            online_partners = assembled.online_count,
            eligible_partners = assembled.eligible.len(),
            filtered_out = assembled.filtered_out,
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
            offline_trigger_ts: Some(offline_trigger_ts),
            reason: "auto_raid_on_offline".to_string(),
            respect_soft_raid_blacklist: true,
        };
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
        let Some(assembled) = self.assemble_eligible_partners(broadcaster_id, now).await else {
            return ManualRaidResponse::error("unavailable", "candidate_assembly_failed");
        };
        tracing::info!(
            streamer = %login,
            viewers = source.viewer_count,
            duration_sec = source.stream_duration_sec(now),
            online_partners = assembled.online_count,
            eligible_partners = assembled.eligible.len(),
            "Manual-Raid-Pipeline gestartet"
        );
        let request = AutoRaidRequest {
            broadcaster_id: broadcaster_id.to_string(),
            broadcaster_login: login.clone(),
            viewer_count: source.viewer_count,
            stream_duration_sec: source.stream_duration_sec(now),
            partners: assembled.eligible,
            category_id: self.resolve_category_id().await,
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
    /// → Eligibility-Partition (aktiv vor kürzlich) → Follower-Anreicherung.
    /// `None` bei I/O-Fehlern (bereits geloggt).
    async fn assemble_eligible_partners(
        &self,
        broadcaster_id: &str,
        now: chrono::DateTime<Utc>,
    ) -> Option<AssembledPartners> {
        let roster = match self.roster.load_roster(broadcaster_id).await {
            Ok(roster) => roster,
            Err(error) => {
                tracing::error!(%error, broadcaster_id, "Raid: Roster nicht ladbar");
                return None;
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

        Some(AssembledPartners {
            eligible,
            online_count,
            filtered_out,
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
