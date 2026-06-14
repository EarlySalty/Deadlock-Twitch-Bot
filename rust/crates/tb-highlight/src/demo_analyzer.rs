//! Demo-basiertes Highlight-Scoring — liest Kills/Abilities/Health direkt aus
//! dem Replay (über die [`crate::boon`]-Parser) und bewertet sie.
//!
//! Port von `bot/highlight_clipper/demo_analyzer.py` (Scoring-Teil; die
//! `_run_boon`/`_parse_*`-Schicht ist Slice 3). Die per-Kill-Momentbildung,
//! in Python in `detect_all_events` und `analyze_match` dupliziert, ist hier in
//! [`build_moment`] zusammengeführt. Das ungenutzte `_parse_kills` und das
//! caller-lose `analyze_match` (importiert, nie aufgerufen) sind portiert bzw.
//! markiert.

use std::collections::HashSet;
use std::path::Path;

use crate::boon::{self, TICK_RATE};
use crate::event_detector::{multikill_name, EventType, HighlightEvent};

/// Combo-Fenster vor dem Kill in Sekunden (Python `_COMBO_WINDOW_S`).
const COMBO_WINDOW_S: f64 = 10.0;

/// Abilities, die als „High-Impact" gelten (Ultimates/starke CC).
const HIGH_IMPACT: &[&str] = &[
    "citadel_ability_uppercut",
    "citadel_ability_hook",
    "citadel_ability_flying_strike",
    "citadel_ability_infinity_slash",
    "citadel_ability_power_slash",
    "citadel_ability_lightning_ball",
    "citadel_ability_storm_cloud",
    "ability_ult_combo",
    "ability_power_surge",
    "drifter_darkness",
    "ability_vampirebat_batswarm",
    "ability_necro_zombiewall",
    "ability_incendiary_projectile",
];

/// Ein bewerteter Kill-Moment aus der Demo.
#[derive(Debug, Clone, PartialEq)]
pub struct KillMoment {
    pub game_time_s: f64,
    pub tick: i64,
    pub combo_abilities: Vec<String>,
    pub combo_score: i64,
    pub has_high_impact: bool,
    pub combo_label: String,
    /// Health-Prozent beim Kill (1.0 = 100%).
    pub health_pct: f64,
    /// True wenn `health_pct < 0.35`.
    pub is_clutch: bool,
}

impl KillMoment {
    /// Gesamtpunktzahl — health-basiert, Kombos nur als Zusatz (Python-Property).
    pub fn excitement_score(&self) -> i64 {
        let mut score = 0;
        if self.health_pct < 0.20 {
            score += 5; // Extrem niedrige HP — ultra clutch
        } else if self.health_pct < 0.35 {
            score += 3; // Gefährlich niedrig — echter Clutch
        } else if self.health_pct < 0.50 {
            score += 1; // Etwas unter Druck
        }
        if self.has_high_impact && self.combo_score >= 2 {
            score += 1;
        }
        score
    }
}

/// Baut einen [`KillMoment`] aus Kill-Tick, Spieler-Abilities und Health-Prozent.
/// Gemeinsame Logik von [`detect_all_events`] und [`analyze_match`].
fn build_moment(kill_tick: i64, player_abilities: &[(i64, String)], health_pct: f64) -> KillMoment {
    let window = (COMBO_WINDOW_S * TICK_RATE as f64) as i64;
    let combo: Vec<String> = player_abilities
        .iter()
        .filter(|(t, _)| kill_tick - window <= *t && *t <= kill_tick)
        .map(|(_, name)| name.clone())
        .collect();
    let has_high_impact = combo.iter().any(|a| HIGH_IMPACT.contains(&a.as_str()));
    let combo_label = build_combo_label(&combo);
    let combo_score = combo.len() as i64;
    KillMoment {
        game_time_s: kill_tick as f64 / TICK_RATE as f64,
        tick: kill_tick,
        combo_abilities: combo,
        combo_score,
        has_high_impact,
        combo_label,
        health_pct,
        is_clutch: health_pct < 0.35,
    }
}

/// Vollständige demo-basierte Event-Erkennung ohne API: liest Kills direkt aus
/// dem Replay und bewertet Combo + Health.
pub async fn detect_all_events(
    boon_path: &Path,
    demo_path: &Path,
    hero_id: i64,
    twitch_login: &str,
) -> Vec<KillMoment> {
    let abilities = boon::abilities(boon_path, demo_path).await;
    let raw_kills = boon::kills_from_demo(boon_path, demo_path, twitch_login).await;
    if raw_kills.is_empty() {
        tracing::warn!(login = twitch_login, "HighlightClipper: Keine Kills im Demo gefunden");
        return Vec::new();
    }

    let player_abilities = filter_player_abilities(abilities, hero_id);
    let kill_ticks: Vec<i64> = raw_kills.iter().map(|k| k.tick).collect();
    let entity_idx = boon::find_player_entity(boon_path, demo_path, &kill_ticks).await;

    let mut moments = Vec::new();
    for kill in &raw_kills {
        let health_pct = min_health(boon_path, demo_path, entity_idx, kill.tick).await;
        moments.push(build_moment(kill.tick, &player_abilities, health_pct));
    }
    moments
}

/// Extrahiert KillMoments für bekannte Kill-Zeiten (Sekunden). In Python
/// importiert, aber aktuell ohne Aufrufer — der Vollständigkeit halber portiert.
pub async fn analyze_match(
    boon_path: &Path,
    demo_path: &Path,
    hero_id: i64,
    kill_times_s: &[f64],
) -> Vec<KillMoment> {
    let abilities = boon::abilities(boon_path, demo_path).await;
    let player_abilities = filter_player_abilities(abilities, hero_id);
    let kill_ticks: Vec<i64> = kill_times_s
        .iter()
        .map(|t| (t * TICK_RATE as f64).round() as i64)
        .collect();
    let entity_idx = boon::find_player_entity(boon_path, demo_path, &kill_ticks).await;

    let mut moments = Vec::new();
    for kill_tick in kill_ticks {
        let health_pct = min_health(boon_path, demo_path, entity_idx, kill_tick).await;
        moments.push(build_moment(kill_tick, &player_abilities, health_pct));
    }
    moments
}

fn filter_player_abilities(abilities: Vec<boon::AbilityCast>, hero_id: i64) -> Vec<(i64, String)> {
    abilities
        .into_iter()
        .filter(|(_, hid, _)| *hid == hero_id)
        .map(|(t, _, name)| (t, name))
        .collect()
}

/// Minimum-Health im Fight-Fenster (Kill −5s … +15s, 5s-Schritte); kein
/// Entity-Index → 1.0.
async fn min_health(
    boon_path: &Path,
    demo_path: &Path,
    entity_idx: Option<i64>,
    kill_tick: i64,
) -> f64 {
    match entity_idx {
        Some(idx) => {
            boon::get_min_health_in_window(
                boon_path,
                demo_path,
                idx,
                (kill_tick - 320).max(0),
                kill_tick + 960,
                320,
            )
            .await
        }
        None => 1.0,
    }
}

/// Konvertiert Demo-KillMoments zu [`HighlightEvent`]s: Multikills (≥2 Kills in
/// 15s) und Solo-Clutch-Kills (`excitement_score ≥ max(min_score, 1)`).
pub fn moments_to_events(moments: &[KillMoment], min_score: i64) -> Vec<HighlightEvent> {
    if moments.is_empty() {
        return Vec::new();
    }
    let mut sorted_m: Vec<&KillMoment> = moments.iter().collect();
    sorted_m.sort_by_key(|m| m.tick);

    let mut events: Vec<HighlightEvent> = Vec::new();
    let mut used: HashSet<i64> = HashSet::new();
    let cluster_window = 15 * TICK_RATE;

    // 1) Multikills: ≥2 Kills in 15s
    for i in 0..sorted_m.len() {
        let m = sorted_m[i];
        if used.contains(&m.tick) {
            continue;
        }
        let mut cluster: Vec<&KillMoment> = vec![m];
        for m2 in &sorted_m[i + 1..] {
            if m2.tick - cluster[0].tick <= cluster_window {
                cluster.push(m2);
            } else {
                break;
            }
        }
        if cluster.len() >= 2 {
            // Python max(): erster maximaler Moment bei Gleichstand.
            let best = cluster
                .iter()
                .copied()
                .reduce(|a, b| if b.excitement_score() > a.excitement_score() { b } else { a })
                .unwrap();
            let max_combo = cluster.iter().map(|x| x.combo_score).max().unwrap();
            let pre_roll = (max_combo * 3 + 15).min(35);
            let count = cluster.len() as i64;
            events.push(HighlightEvent {
                event_type: EventType::Multikill,
                game_time_s: cluster[0].game_time_s as i64,
                duration_s: (cluster[cluster.len() - 1].game_time_s - cluster[0].game_time_s) as i64,
                kill_count: count,
                label: format!(
                    "{} ({} Kills) — {}",
                    multikill_name(count),
                    count,
                    best.combo_label
                ),
                pre_roll_s: pre_roll,
            });
            for c in &cluster {
                used.insert(c.tick);
            }
        }
    }

    // 2) Solo-Clutch-Kills
    for m in &sorted_m {
        if used.contains(&m.tick) {
            continue;
        }
        if m.excitement_score() < min_score.max(1) {
            continue;
        }
        let hp = (m.health_pct * 100.0) as i64;
        let hp_tag = if m.health_pct < 0.20 {
            format!("🔴 {hp}%HP")
        } else if m.health_pct < 0.35 {
            format!("🟠 {hp}%HP")
        } else if m.health_pct < 0.50 {
            format!("🟡 {hp}%HP")
        } else {
            String::new()
        };

        let mut label_parts: Vec<String> = Vec::new();
        label_parts.push(if m.is_clutch { "Clutch Kill" } else { "Kill" }.to_string());
        if !hp_tag.is_empty() {
            label_parts.push(hp_tag);
        }
        if !m.combo_label.is_empty() && m.combo_label != "Kill" {
            label_parts.push(m.combo_label.clone());
        }
        let pre_roll = (m.combo_score * 3 + 15).min(35);
        events.push(HighlightEvent {
            event_type: EventType::CloseFight,
            game_time_s: m.game_time_s as i64,
            duration_s: 0,
            kill_count: 1,
            label: label_parts.join(" — "),
            pre_roll_s: pre_roll,
        });
        used.insert(m.tick);
    }

    events.sort_by_key(|e| e.game_time_s);
    events
}

fn pretty_ability(name: &str) -> Option<&'static str> {
    Some(match name {
        "citadel_ability_hook" => "Hook",
        "citadel_ability_sticky_bomb" => "Bomb",
        "citadel_ability_uppercut" => "Uppercut",
        "citadel_ability_power_slash" => "Power Slash",
        "citadel_ability_flying_strike" => "Flying Strike",
        "citadel_ability_infinity_slash" => "Infinity Slash",
        "citadel_ability_lightning_ball" => "Lightning Ball",
        "citadel_ability_storm_cloud" => "Storm Cloud",
        "ability_ult_combo" => "Ultimate",
        "ability_power_surge" => "Power Surge",
        "ability_nano_dash" => "Dash",
        "ability_flame_dash" => "Flame Dash",
        "drifter_darkness" => "Darkness",
        "ability_vampirebat_batswarm" => "Bat Swarm",
        "ability_throw_sand" => "Sand Throw",
        "ability_intimidate" => "Intimidate",
        _ => return None,
    })
}

/// Title-Case wie Pythons `str.title()`: erster Buchstabe je Wort groß, Rest klein.
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Erzeugt ein lesbares Combo-Label wie „Hook → Bomb → Uppercut" (dedupliziert).
fn build_combo_label(abilities: &[String]) -> String {
    if abilities.is_empty() {
        return "Kill".to_string();
    }
    let mut seen: Vec<String> = Vec::new();
    for a in abilities {
        let label = pretty_ability(a).map(str::to_string).unwrap_or_else(|| {
            title_case(
                &a.replace("citadel_ability_", "")
                    .replace("ability_", "")
                    .replace('_', " "),
            )
        });
        if !seen.contains(&label) {
            seen.push(label);
        }
    }
    seen.join(" → ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn km(tick: i64, health_pct: f64, high_impact: bool, combo_score: i64, label: &str) -> KillMoment {
        KillMoment {
            game_time_s: tick as f64 / TICK_RATE as f64,
            tick,
            combo_abilities: Vec::new(),
            combo_score,
            has_high_impact: high_impact,
            combo_label: label.to_string(),
            health_pct,
            is_clutch: health_pct < 0.35,
        }
    }

    #[test]
    fn excitement_score_schwellen() {
        assert_eq!(km(0, 0.15, false, 0, "").excitement_score(), 5);
        assert_eq!(km(0, 0.30, false, 0, "").excitement_score(), 3);
        assert_eq!(km(0, 0.45, false, 0, "").excitement_score(), 1);
        assert_eq!(km(0, 0.80, false, 0, "").excitement_score(), 0);
        // High-Impact-Bonus nur mit combo_score >= 2.
        assert_eq!(km(0, 0.80, true, 2, "").excitement_score(), 1);
        assert_eq!(km(0, 0.80, true, 1, "").excitement_score(), 0);
        assert_eq!(km(0, 0.15, true, 2, "").excitement_score(), 6);
    }

    #[test]
    fn combo_label_pretty_fallback_dedup() {
        assert_eq!(build_combo_label(&[]), "Kill");
        assert_eq!(
            build_combo_label(&["citadel_ability_hook".into(), "citadel_ability_uppercut".into()]),
            "Hook → Uppercut"
        );
        // Fallback-Title-Case für unbekannte Ability.
        assert_eq!(build_combo_label(&["ability_unknown_thing".into()]), "Unknown Thing");
        // Duplikate werden zusammengefasst.
        assert_eq!(
            build_combo_label(&["citadel_ability_hook".into(), "citadel_ability_hook".into()]),
            "Hook"
        );
    }

    #[test]
    fn moments_to_events_multikill() {
        // 3 Kills innerhalb 960 Ticks (15s) → ein Multikill.
        let moments = vec![
            km(1000, 1.0, false, 1, "Hook"),
            km(1100, 1.0, false, 1, "Hook"),
            km(1200, 1.0, false, 1, "Hook"),
        ];
        let events = moments_to_events(&moments, 2);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.event_type, EventType::Multikill);
        assert_eq!(e.kill_count, 3);
        assert_eq!(e.label, "Triple Kill (3 Kills) — Hook");
        assert_eq!(e.game_time_s, 15); // int(1000/64)
        assert_eq!(e.duration_s, 3); // int(1200/64 - 1000/64)
        assert_eq!(e.pre_roll_s, 18); // min(1*3+15, 35)
    }

    #[test]
    fn moments_to_events_solo_clutch() {
        let moments = vec![km(640, 0.15, false, 0, "Uppercut")];
        let events = moments_to_events(&moments, 2);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.event_type, EventType::CloseFight);
        assert_eq!(e.label, "Clutch Kill — 🔴 15%HP — Uppercut");
        assert_eq!(e.game_time_s, 10); // int(640/64)
        assert_eq!(e.pre_roll_s, 15); // min(0*3+15, 35)
    }

    #[test]
    fn moments_to_events_schwacher_kill_verworfen() {
        // excitement_score 0 < max(2,1) → kein Event.
        let moments = vec![km(640, 0.90, false, 0, "Kill")];
        assert!(moments_to_events(&moments, 2).is_empty());
    }
}
