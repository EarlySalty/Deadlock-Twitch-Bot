//! Orchestrierung des Highlight-Clippers (Poll-Loop über aktive Partner).
//!
//! Port von `bot/highlight_clipper/worker.py`. Aufgebaut in Teil-Slices:
//! - 9a (hier): reine Entscheidungs-Helfer ([`filter_recent_matches`],
//!   [`get_hero_id`], [`compute_clip_window`]) — ohne I/O, voll testbar.
//! - 9b/9c (folgt): Partner-Datenschicht (Postgres + Steam-SQLite + manuelle
//!   steamids.json) und der eigentliche Poll-Loop inkl. Twitch-API.
//!
//! Das ungenutzte `_score_events_with_demo` (kein Caller in Python) wird nicht
//! portiert.

use crate::config::{CLIP_POST_ROLL_SECONDS, CLIP_PRE_ROLL_SECONDS, MAX_CLIP_SECONDS};
use crate::event_detector::HighlightEvent;
use crate::state::{is_match_processed, HighlightState};

/// Ein zu verarbeitendes Match (gefiltert + normalisiert aus der Match-History).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentMatch {
    pub match_id: i64,
    pub start_time: i64,
    pub match_duration_s: i64,
}

/// Filtert die Match-History auf verarbeitbare Matches: Objekt-Form, gültige
/// `match_id`/`start_time`, jünger als 24h und noch nicht verarbeitet. Sortiert
/// aufsteigend nach `start_time` (Python `_filter_recent_matches`).
pub fn filter_recent_matches(
    matches: &[serde_json::Value],
    state: &HighlightState,
    login: &str,
    now: i64,
) -> Vec<RecentMatch> {
    let min_start = now - 86400;
    let mut filtered: Vec<RecentMatch> = Vec::new();
    for m in matches {
        let Some(obj) = m.as_object() else { continue };
        let (Some(match_id), Some(start_time)) = (
            as_int(obj.get("match_id")),
            as_int(obj.get("start_time")),
        ) else {
            continue;
        };
        if start_time <= min_start || is_match_processed(state, login, match_id) {
            continue;
        }
        filtered.push(RecentMatch {
            match_id,
            start_time,
            match_duration_s: as_int(obj.get("match_duration_s")).unwrap_or(0),
        });
    }
    filtered.sort_by_key(|m| m.start_time);
    filtered
}

/// Sucht die `hero_id` des Spielers (per `account_id == steam_id`) in den
/// Match-Metadaten (Python `_get_hero_id`).
pub fn get_hero_id(steam_id: i64, match_info: &serde_json::Value) -> Option<i64> {
    let players = match_info
        .get("players")
        .and_then(serde_json::Value::as_array)?;
    for player in players {
        if as_int(player.get("account_id")) == Some(steam_id) {
            return as_int(player.get("hero_id"));
        }
    }
    None
}

/// Berechnet das Clip-Fenster (start, end) im VOD aus dem VOD-Offset und dem
/// Event: Pre-Roll vor dem Event, Post-Roll danach, gedeckelt auf
/// `MAX_CLIP_SECONDS` (Python-Logik in `_process_match`).
pub fn compute_clip_window(vod_offset_s: i64, event: &HighlightEvent) -> (i64, i64) {
    let clip_start_s = (vod_offset_s + event.game_time_s - CLIP_PRE_ROLL_SECONDS).max(0);
    let clip_end_s = (clip_start_s + 1)
        .max(vod_offset_s + event.game_time_s + event.duration_s + CLIP_POST_ROLL_SECONDS);
    let clip_end_s = clip_end_s.min(clip_start_s + MAX_CLIP_SECONDS);
    (clip_start_s, clip_end_s)
}

/// `int(value)`-Semantik aus Python (`_as_int`): Zahlen gegen 0 gekürzt, Strings
/// nur als reine Ganzzahl, Bools → 0/1, sonst None.
fn as_int(value: Option<&serde_json::Value>) -> Option<i64> {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
        Some(serde_json::Value::Bool(b)) => Some(i64::from(*b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_detector::EventType;
    use serde_json::json;

    fn ev(game_time_s: i64, duration_s: i64) -> HighlightEvent {
        HighlightEvent {
            event_type: EventType::Multikill,
            game_time_s,
            duration_s,
            kill_count: 2,
            label: "x".into(),
            pre_roll_s: 0,
        }
    }

    #[test]
    fn filter_recent_fenster_und_sortierung() {
        let now = 1_000_000;
        let state = HighlightState::new();
        let matches = vec![
            json!({"match_id": 1, "start_time": now - 100, "match_duration_s": 1800}), // frisch
            json!({"match_id": 2, "start_time": now - 90000}),                          // > 24h alt
            json!({"match_id": 3, "start_time": now - 50}),                             // frisch, neuer
            json!("kein-objekt"),                                                       // übersprungen
            json!({"start_time": now - 10}),                                            // ohne match_id
        ];
        let out = filter_recent_matches(&matches, &state, "nani", now);
        // 1 und 3 bleiben, sortiert nach start_time (1 vor 3).
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].match_id, 1);
        assert_eq!(out[1].match_id, 3);
        assert_eq!(out[0].match_duration_s, 1800);
        assert_eq!(out[1].match_duration_s, 0); // Default ohne Feld
    }

    #[test]
    fn filter_recent_ueberspringt_verarbeitete() {
        let now = 1_000_000;
        let mut state = HighlightState::new();
        state.insert(
            "nani".into(),
            crate::state::StreamerState { processed_matches: vec![5], last_checked: 0 },
        );
        let matches = vec![json!({"match_id": 5, "start_time": now - 100})];
        assert!(filter_recent_matches(&matches, &state, "nani", now).is_empty());
    }

    #[test]
    fn hero_id_per_account() {
        let mi = json!({"players": [
            {"account_id": 111, "hero_id": 7},
            {"account_id": 222, "hero_id": 9},
        ]});
        assert_eq!(get_hero_id(222, &mi), Some(9));
        assert_eq!(get_hero_id(999, &mi), None);
        assert_eq!(get_hero_id(1, &json!({})), None);
    }

    #[test]
    fn clip_window_clamping() {
        // vod_offset 1000, event @ 50s, dur 5s. PRE=6, POST=4, MAX=40.
        // start = 1000+50-6 = 1044; end = max(1045, 1000+50+5+4=1059)=1059; cap 1044+40=1084 → 1059.
        assert_eq!(compute_clip_window(1000, &ev(50, 5)), (1044, 1059));
        // Lange Dauer wird auf MAX_CLIP gedeckelt.
        let (s, e) = compute_clip_window(0, &ev(100, 1000));
        assert_eq!(s, 94);
        assert_eq!(e, 94 + MAX_CLIP_SECONDS);
        // Negativer Start wird auf 0 geklemmt.
        let (s2, _) = compute_clip_window(0, &ev(0, 0));
        assert_eq!(s2, 0);
    }
}
