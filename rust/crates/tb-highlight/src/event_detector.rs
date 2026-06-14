//! API-basierte Highlight-Erkennung aus den Match-Metadaten der deadlock-api.
//!
//! Port von `bot/highlight_clipper/event_detector.py`. Definiert den geteilten
//! [`HighlightEvent`]-Typ (auch vom demo-basierten Pfad genutzt) und erkennt
//! Multikills, Teamfights und Close-Fights aus `match_info["players"]`. Rein
//! und ohne I/O — vollständig deterministisch testbar.

use crate::config::{
    MULTIKILL_MIN_KILLS, MULTIKILL_THRESHOLD_SECONDS, TEAMFIGHT_MIN_KILLS,
    TEAMFIGHT_THRESHOLD_SECONDS,
};

const CLOSE_FIGHT_WINDOW_S: i64 = 40;
const MAX_PRE_ROLL_S: i64 = 35;

/// Art eines Highlight-Events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Multikill,
    Teamfight,
    CloseFight,
}

impl EventType {
    /// String-Repräsentation wie im Python-`Literal` (für Worker/Serialisierung).
    pub fn as_str(self) -> &'static str {
        match self {
            EventType::Multikill => "multikill",
            EventType::Teamfight => "teamfight",
            EventType::CloseFight => "close_fight",
        }
    }
}

/// Ein erkanntes Highlight-Fenster für den Clip-Worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightEvent {
    pub event_type: EventType,
    pub game_time_s: i64,
    pub duration_s: i64,
    pub kill_count: i64,
    pub label: String,
    /// Dynamischer Vorlauf basierend auf Time-to-Kill.
    pub pre_roll_s: i64,
}

/// Ein Todesfall aus den Metadaten (intern).
#[derive(Debug, Clone)]
struct Death {
    game_time_s: i64,
    killer_player_slot: Option<i64>,
    killed_player_slot: Option<i64>,
    time_to_kill_s: Option<f64>,
}

/// Erkennt Highlight-Events des Spielers (`account_id`) aus den Match-Metadaten.
pub fn detect_events(account_id: i64, match_info: &serde_json::Value) -> Vec<HighlightEvent> {
    let players: Vec<&serde_json::Value> = match_info
        .get("players")
        .and_then(serde_json::Value::as_array)
        .map(|a| a.iter().filter(|p| p.is_object()).collect())
        .unwrap_or_default();

    let player_slot = match find_player_slot(account_id, &players) {
        Some(slot) => slot,
        None => return Vec::new(),
    };

    let mut all_deaths = collect_deaths(&players);
    all_deaths.sort_by_key(|d| d.game_time_s);

    let player_kills: Vec<&Death> = all_deaths
        .iter()
        .filter(|d| d.killer_player_slot == Some(player_slot))
        .collect();
    let player_own_deaths: Vec<&Death> = all_deaths
        .iter()
        .filter(|d| d.killed_player_slot == Some(player_slot))
        .collect();

    let mut events: Vec<HighlightEvent> = Vec::new();

    // 1) Multikills
    for (start, end) in find_multikill_ranges(&player_kills) {
        let kills = &player_kills[start..end];
        let kill_count = kills.len() as i64;
        let first_t = kills[0].game_time_s;
        let last_t = kills[kills.len() - 1].game_time_s;
        let max_ttk = max_ttk(kills.iter().copied());
        let pre_roll = ((max_ttk as i64) + 5).min(MAX_PRE_ROLL_S);
        events.push(HighlightEvent {
            event_type: EventType::Multikill,
            game_time_s: first_t,
            duration_s: last_t - first_t,
            kill_count,
            label: format!("{} ({} Kills)", multikill_name(kill_count), kill_count),
            pre_roll_s: pre_roll,
        });
    }

    // 2) Teamfights (verkettete Todesfälle)
    for fight in find_teamfights(&all_deaths, player_slot) {
        let first_t = fight[0].game_time_s;
        let last_t = fight[fight.len() - 1].game_time_s;
        let player_fight_deaths = fight.iter().copied().filter(|d| {
            d.killer_player_slot == Some(player_slot) || d.killed_player_slot == Some(player_slot)
        });
        let max_ttk = max_ttk(player_fight_deaths);
        let pre_roll = ((max_ttk as i64) + 5).min(MAX_PRE_ROLL_S);
        events.push(HighlightEvent {
            event_type: EventType::Teamfight,
            game_time_s: first_t,
            duration_s: last_t - first_t,
            kill_count: fight.len() as i64,
            label: format!("Team Fight ({} Kills)", fight.len()),
            pre_roll_s: pre_roll,
        });
    }

    // 3) Close-Fights: Kill + eigener Tod innerhalb CLOSE_FIGHT_WINDOW_S
    events.extend(find_close_fights(&player_kills, &player_own_deaths));

    let events = deduplicate_events(events);
    let mut events = events;
    events.sort_by_key(|e| e.game_time_s);
    events
}

/// Maximale Time-to-Kill über eine Death-Sequenz (`ttk or 0`); leer → 0.
fn max_ttk<'a>(deaths: impl Iterator<Item = &'a Death>) -> f64 {
    deaths
        .map(|d| d.time_to_kill_s.unwrap_or(0.0))
        .fold(None::<f64>, |acc, v| Some(acc.map_or(v, |a| a.max(v))))
        .unwrap_or(0.0)
}

fn find_close_fights(
    player_kills: &[&Death],
    player_own_deaths: &[&Death],
) -> Vec<HighlightEvent> {
    let mut results = Vec::new();
    let mut used_death_idx = std::collections::HashSet::new();

    // Jeder Kill wird im äußeren Loop genau einmal betrachtet (break nach Match),
    // daher genügt ein used-Set für die Tode (used_kill_ids in Python redundant).
    for kill in player_kills {
        let kill_t = kill.game_time_s;
        let kill_ttk = kill.time_to_kill_s.unwrap_or(0.0);
        for (di, death) in player_own_deaths.iter().enumerate() {
            let death_t = death.game_time_s;
            let death_ttk = death.time_to_kill_s.unwrap_or(0.0);
            if (kill_t - death_t).abs() > CLOSE_FIGHT_WINDOW_S {
                continue;
            }
            if used_death_idx.contains(&di) {
                continue;
            }
            let first_t = kill_t.min(death_t);
            let last_t = kill_t.max(death_t);
            let max_ttk = kill_ttk.max(death_ttk);
            let pre_roll = ((max_ttk as i64) + 5).min(MAX_PRE_ROLL_S);
            let label = if kill_t > death_t { "Clutch Kill" } else { "Close Fight" };
            results.push(HighlightEvent {
                event_type: EventType::CloseFight,
                game_time_s: first_t,
                duration_s: last_t - first_t,
                kill_count: 1,
                label: label.to_string(),
                pre_roll_s: pre_roll,
            });
            used_death_idx.insert(di);
            break;
        }
    }
    results
}

fn deduplicate_events(events: Vec<HighlightEvent>) -> Vec<HighlightEvent> {
    let mut sorted = events;
    // Sortierung wie Python: (game_time_s aufsteigend, duration_s absteigend).
    sorted.sort_by(|a, b| {
        a.game_time_s
            .cmp(&b.game_time_s)
            .then(b.duration_s.cmp(&a.duration_s))
    });
    let mut result: Vec<HighlightEvent> = Vec::new();
    for e in sorted {
        let e_start = e.game_time_s - e.pre_roll_s;
        let e_end = e.game_time_s + e.duration_s;
        let dominated = result.iter().any(|kept| {
            let k_start = kept.game_time_s - kept.pre_roll_s;
            let k_end = kept.game_time_s + kept.duration_s;
            k_start <= e_start && k_end >= e_end
        });
        if !dominated {
            result.push(e);
        }
    }
    result
}

fn find_player_slot(account_id: i64, players: &[&serde_json::Value]) -> Option<i64> {
    for player in players {
        if as_int(player.get("account_id")) == Some(account_id) {
            return as_int(player.get("player_slot"));
        }
    }
    None
}

fn collect_deaths(players: &[&serde_json::Value]) -> Vec<Death> {
    let mut deaths = Vec::new();
    for player in players {
        let slot = as_int(player.get("player_slot"));
        let Some(details) = player.get("death_details").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for death in details {
            if !death.is_object() {
                continue;
            }
            let Some(game_time_s) = as_int(death.get("game_time_s")) else {
                continue;
            };
            deaths.push(Death {
                game_time_s,
                killer_player_slot: as_int(death.get("killer_player_slot")),
                killed_player_slot: slot,
                time_to_kill_s: death.get("time_to_kill_s").and_then(serde_json::Value::as_f64),
            });
        }
    }
    deaths
}

fn find_multikill_ranges(player_kills: &[&Death]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < player_kills.len() {
        let mut end = start + 1;
        while end < player_kills.len() {
            if player_kills[end].game_time_s - player_kills[start].game_time_s
                > MULTIKILL_THRESHOLD_SECONDS
            {
                break;
            }
            end += 1;
        }
        if end - start >= MULTIKILL_MIN_KILLS {
            ranges.push((start, end));
            start = end;
            continue;
        }
        start += 1;
    }
    ranges
}

fn find_teamfights<'a>(all_deaths: &'a [Death], player_slot: i64) -> Vec<Vec<&'a Death>> {
    if all_deaths.is_empty() {
        return Vec::new();
    }
    let mut fights: Vec<Vec<&Death>> = Vec::new();
    let mut current: Vec<&Death> = vec![&all_deaths[0]];

    for death in &all_deaths[1..] {
        if death.game_time_s - current[current.len() - 1].game_time_s <= TEAMFIGHT_THRESHOLD_SECONDS
        {
            current.push(death);
        } else {
            maybe_add_fight(&current, player_slot, &mut fights);
            current = vec![death];
        }
    }
    maybe_add_fight(&current, player_slot, &mut fights);
    fights
}

fn maybe_add_fight<'a>(deaths: &[&'a Death], player_slot: i64, out: &mut Vec<Vec<&'a Death>>) {
    let player_kills = deaths
        .iter()
        .filter(|d| d.killer_player_slot == Some(player_slot))
        .count();
    if deaths.len() >= TEAMFIGHT_MIN_KILLS && player_kills >= 1 {
        out.push(deaths.to_vec());
    }
}

/// Name eines Multikills nach Kill-Zahl (Python `_multikill_name`).
pub fn multikill_name(kill_count: i64) -> &'static str {
    match kill_count {
        2 => "Double Kill",
        3 => "Triple Kill",
        4 => "Quadra Kill",
        5 => "Penta Kill",
        _ => "Multi Kill",
    }
}

/// `int(value)`-Semantik aus Python: Zahlen werden (gegen 0) gekürzt, Strings
/// nur als reine Ganzzahl geparst (`int("12.5")` schlägt fehl → None).
fn as_int(value: Option<&serde_json::Value>) -> Option<i64> {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn multikill_namen() {
        assert_eq!(multikill_name(2), "Double Kill");
        assert_eq!(multikill_name(5), "Penta Kill");
        assert_eq!(multikill_name(6), "Multi Kill");
    }

    #[test]
    fn as_int_python_semantik() {
        assert_eq!(as_int(Some(&json!(7))), Some(7));
        assert_eq!(as_int(Some(&json!(7.9))), Some(7)); // Truncation
        assert_eq!(as_int(Some(&json!("123"))), Some(123));
        assert_eq!(as_int(Some(&json!("12.5"))), None); // int("12.5") wirft
        assert_eq!(as_int(Some(&json!(null))), None);
        assert_eq!(as_int(None), None);
    }

    #[test]
    fn detect_events_multikill_dreierserie() {
        let match_info = json!({
            "players": [
                {"account_id": 100, "player_slot": 0, "death_details": []},
                {"account_id": 200, "player_slot": 1, "death_details": [
                    {"game_time_s": 100, "killer_player_slot": 0, "time_to_kill_s": 3}]},
                {"account_id": 300, "player_slot": 2, "death_details": [
                    {"game_time_s": 105, "killer_player_slot": 0, "time_to_kill_s": 2}]},
                {"account_id": 400, "player_slot": 3, "death_details": [
                    {"game_time_s": 108, "killer_player_slot": 0}]},
            ]
        });
        let events = detect_events(100, &match_info);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.event_type, EventType::Multikill);
        assert_eq!(e.label, "Triple Kill (3 Kills)");
        assert_eq!(e.game_time_s, 100);
        assert_eq!(e.duration_s, 8);
        assert_eq!(e.pre_roll_s, 8); // int(max_ttk=3)+5
    }

    #[test]
    fn detect_events_unbekannter_spieler_leer() {
        let match_info = json!({"players": [{"account_id": 1, "player_slot": 0}]});
        assert!(detect_events(999, &match_info).is_empty());
    }

    #[test]
    fn detect_events_teamfight_ab_vier_toden() {
        // 4 verkettete Tode, Spieler (slot 0) hat ≥1 Kill → Teamfight.
        let match_info = json!({
            "players": [
                {"account_id": 100, "player_slot": 0, "death_details": [
                    {"game_time_s": 210, "killer_player_slot": 5}]},
                {"account_id": 200, "player_slot": 1, "death_details": [
                    {"game_time_s": 200, "killer_player_slot": 0}]},
                {"account_id": 300, "player_slot": 2, "death_details": [
                    {"game_time_s": 205, "killer_player_slot": 0}]},
                {"account_id": 400, "player_slot": 3, "death_details": [
                    {"game_time_s": 208, "killer_player_slot": 5}]},
            ]
        });
        let events = detect_events(100, &match_info);
        let teamfights: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::Teamfight)
            .collect();
        assert_eq!(teamfights.len(), 1);
        assert_eq!(teamfights[0].kill_count, 4);
        assert_eq!(teamfights[0].label, "Team Fight (4 Kills)");
    }

    #[test]
    fn close_fight_clutch_vs_close() {
        // Kill nach eigenem Tod → "Clutch Kill"; Kill vor Tod → "Close Fight".
        let kill = Death { game_time_s: 50, killer_player_slot: Some(0), killed_player_slot: Some(1), time_to_kill_s: Some(4.0) };
        let own_death = Death { game_time_s: 40, killer_player_slot: Some(9), killed_player_slot: Some(0), time_to_kill_s: Some(2.0) };
        let kills = vec![&kill];
        let deaths = vec![&own_death];
        let out = find_close_fights(&kills, &deaths);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "Clutch Kill"); // kill_t 50 > death_t 40
        assert_eq!(out[0].game_time_s, 40);
        assert_eq!(out[0].duration_s, 10);
        assert_eq!(out[0].pre_roll_s, 9); // int(max(4,2))+5
    }

    #[test]
    fn dedup_dominierte_events_entfernt() {
        let outer = HighlightEvent { event_type: EventType::Teamfight, game_time_s: 100, duration_s: 30, kill_count: 4, label: "A".into(), pre_roll_s: 10 };
        // inner liegt komplett im Fenster von outer → wird entfernt.
        let inner = HighlightEvent { event_type: EventType::Multikill, game_time_s: 105, duration_s: 5, kill_count: 2, label: "B".into(), pre_roll_s: 0 };
        let out = deduplicate_events(vec![outer.clone(), inner]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "A");
    }
}
