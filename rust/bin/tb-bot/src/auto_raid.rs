//! Auto-Raid-Orchestrator (`on_stream_offline`): geht ein Partner offline,
//! sammelt online Partner, filtert auf Deadlock-Eligibility, scort + wählt das
//! beste Ziel und feuert den Raid. Port von `offline_raid_orchestrator.py`
//! (`prepare_offline_auto_raid_context` + `handle_streamer_offline`).
//!
//! **Auswahl-Kern** (`select_auto_raid_target`) ist reine Logik (Filter→Score→
//! Select) und unit-getestet. Die I/O-Schale (`handle_streamer_offline`) macht
//! die DB-/Helix-Reads + den Executor-Aufruf.
//!
//! Noch nicht aus `main.rs` aufgerufen (Cutover-Gate).
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use tb_raid::{
    classify_eligibility, select_by_score, DeadlockEvalInput, ScoredCandidate, SelectionResult,
};

/// Ein online Partner mit allem, was Filter + Auswahl brauchen
/// (zusammengeführt aus Roster + Helix-Stream + live_state + Score-Cache).
#[derive(Debug, Clone)]
pub struct AutoRaidCandidate {
    pub user_id: String,
    pub user_login: String,
    pub game_name: String,
    pub viewer_count: i32,
    pub started_at: String,
    /// Aus live_state: hatte die laufende Session Deadlock.
    pub had_deadlock_session: bool,
    /// Aus live_state: ISO-Zeit des letzten Deadlock-Moments.
    pub last_deadlock_seen_at: Option<String>,
    // Score-Cache-Felder:
    pub final_score: f64,
    pub today_received_raids: i32,
    pub is_live: bool,
    pub followers_total: i32,
}

/// Wählt das Auto-Raid-Ziel: Eligibility-Filter (aktiv-Deadlock bevorzugt) →
/// Score-basierte Auswahl. Reine Logik, kein I/O.
///
/// `target_game_lower` z. B. "deadlock". `now` für die Recency-Prüfung.
pub fn select_auto_raid_target(
    candidates: Vec<AutoRaidCandidate>,
    now: DateTime<Utc>,
    target_game_lower: &str,
) -> Option<ScoredCandidate> {
    // 1. Eligibility: nur Partner, die aktiv Deadlock streamen (oder kürzlich,
    //    falls keiner aktiv ist) — Python `filter_deadlock_eligible_partner_candidates`.
    let mut active = Vec::new();
    let mut recent = Vec::new();
    for c in candidates {
        let eval = DeadlockEvalInput {
            game_name: &c.game_name,
            had_deadlock_session: c.had_deadlock_session,
            last_deadlock_seen_at: c.last_deadlock_seen_at.as_deref(),
        };
        match classify_eligibility(&eval, now, target_game_lower) {
            Some(tb_raid::EligibilityBucket::Active) => active.push(c),
            Some(tb_raid::EligibilityBucket::Recent) => recent.push(c),
            None => {}
        }
    }
    let eligible = if active.is_empty() { recent } else { active };
    if eligible.is_empty() {
        return None;
    }

    // 2. In ScoredCandidate übersetzen (nur live; Auto-Raid raidet nur Live-Ziele).
    let scored: Vec<ScoredCandidate> = eligible
        .into_iter()
        .filter(|c| c.is_live)
        .map(|c| ScoredCandidate {
            user_id: c.user_id,
            user_login: c.user_login,
            final_score: c.final_score,
            today_received_raids: c.today_received_raids,
            is_live: c.is_live,
            viewer_count: c.viewer_count,
            followers_total: c.followers_total,
            started_at: c.started_at,
            raid_boost_multiplier: 1.0,
            new_partner_multiplier: 1.0,
        })
        .collect();
    if scored.is_empty() {
        return None;
    }

    // 3. Score-basierte Auswahl (Daily-Cap, Close-Threshold, Tie-Breaks).
    select_by_score(&scored).map(|result: SelectionResult| result.candidate.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-10T18:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn cand(login: &str, game: &str, score: f64, live: bool) -> AutoRaidCandidate {
        AutoRaidCandidate {
            user_id: login.to_string(),
            user_login: login.to_string(),
            game_name: game.to_string(),
            viewer_count: 10,
            started_at: "2026-06-10T17:00:00+00:00".to_string(),
            had_deadlock_session: false,
            last_deadlock_seen_at: None,
            final_score: score,
            today_received_raids: 0,
            is_live: live,
            followers_total: 100,
        }
    }

    #[test]
    fn waehlt_hoechsten_score_unter_aktiven_deadlock_partnern() {
        let cands = vec![
            cand("a", "Deadlock", 0.50, true),
            cand("b", "Deadlock", 0.90, true),
            cand("c", "Valorant", 0.99, true), // nicht eligible
        ];
        let chosen = select_auto_raid_target(cands, now(), "deadlock").unwrap();
        assert_eq!(
            chosen.user_login, "b",
            "hoechster Score unter Deadlock-Partnern"
        );
    }

    #[test]
    fn keine_eligiblen_partner_kein_ziel() {
        let cands = vec![cand("a", "Valorant", 0.99, true)];
        assert!(select_auto_raid_target(cands, now(), "deadlock").is_none());
    }

    #[test]
    fn nur_offline_eligible_kein_ziel() {
        let cands = vec![cand("a", "Deadlock", 0.99, false)]; // eligible aber nicht live
        assert!(select_auto_raid_target(cands, now(), "deadlock").is_none());
    }
}
