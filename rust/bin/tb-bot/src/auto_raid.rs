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

/// Skip-Grund der Quell-Prüfung fürs Log (Python-Reasons in `mixin.py`).
fn source_skip_reason(state: &OfflineSourceState, target_game_lower: &str) -> &'static str {
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
    let _ = target_game_lower;
    "last_game_not_eligible"
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

        // 5. Roster laden, live Partner via Helix + Session-Flags mergen.
        let roster = match self.roster.load_roster(broadcaster_id).await {
            Ok(roster) => roster,
            Err(error) => {
                tracing::error!(%error, streamer = streamer_label, "Auto-Raid: Roster nicht ladbar");
                return;
            }
        };
        let logins: Vec<String> = roster.iter().map(|p| p.twitch_login.clone()).collect();
        let streams_by_login: HashMap<String, StreamData> = match self
            .helix
            .get_streams_by_logins(&logins, None)
            .await
        {
            Ok(streams) => streams
                .iter()
                .map(|s| (s.user_login.trim().to_lowercase(), to_stream_data(s)))
                .collect(),
            Err(error) => {
                tracing::error!(%error, streamer = streamer_label, "Auto-Raid: Helix-Streams nicht ladbar");
                return;
            }
        };
        let flags = self
            .live_state
            .source_states_by_logins(&logins)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "Auto-Raid: Partner-Live-State nicht ladbar");
                HashMap::new()
            });

        let online = build_online_candidates(&roster, &streams_by_login);
        let online_count = online.len();

        // 6. Deadlock-Eligibility je Kandidat (aktiv vor kürzlich) — Partition
        //    wie `filter_eligible`, hier inline weil die Session-Flags aus der
        //    separaten Map kommen (Borrow ausserhalb des Kandidaten).
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

        // 7. Follower nur für eligible Kandidaten anreichern (Score-Tie-Break).
        for candidate in &mut eligible {
            if let Some(total) = self
                .followers
                .follower_total(Some(&candidate.twitch_user_id), &candidate.twitch_login)
                .await
            {
                candidate.stream.followers_total = total;
            }
        }

        tracing::info!(
            streamer = streamer_label,
            viewers = viewer_count,
            duration_sec = stream_duration_sec,
            online_partners = online_count,
            eligible_partners = eligible.len(),
            filtered_out,
            "Auto-Raid-Pipeline gestartet"
        );

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
    fn source_skip_reason_unterscheidet_just_chatting_faelle() {
        let base = OfflineSourceState {
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
            "last_game_not_eligible"
        );
    }
}
