//! boon-Wrapper — ruft das vorkompilierte Source-2-Demo-Parser-Binary auf und
//! parst dessen Text-/JSON-Ausgabe.
//!
//! Port der `_run_boon`/`_parse_*`-Schicht aus `demo_analyzer.py`. Die reinen
//! Parser sind bewusst vom Subprocess getrennt, damit sie ohne das Binary
//! deterministisch testbar sind. Fehler (Binary fehlt, Timeout, Crash)
//! degradieren auf leere Ausgabe bzw. leeres Ergebnis (Python `return ""`).
//! Das ungenutzte `_parse_kills` (ohne Caller) wird bewusst nicht portiert.

use std::path::Path;
use std::time::Duration;

use regex::Regex;

/// Timeout pro boon-Aufruf (Python `timeout=60`).
const BOON_TIMEOUT: Duration = Duration::from_secs(60);

/// Tickrate der Demos (Python `_TICK_RATE`).
pub const TICK_RATE: i64 = 64;

/// Ein Ability-Cast aus `boon abilities`: (tick, hero_id, ability_name).
pub type AbilityCast = (i64, i64, String);

/// Ein Kill aus `boon events`: Tick + optionaler attacker_pawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoKill {
    pub tick: i64,
    pub pawn: Option<i64>,
}

/// Führt das boon-Binary mit `args` aus und gibt stdout zurück. Fehlendes
/// Binary, Timeout oder Crash → leerer String (Python `_run_boon`).
pub async fn run_boon(boon_path: &Path, args: &[&str]) -> String {
    if !boon_path.exists() {
        tracing::error!(path = %boon_path.display(), "HighlightClipper: boon Binary nicht gefunden");
        return String::new();
    }
    let mut cmd = tokio::process::Command::new(boon_path);
    cmd.args(args);
    match tokio::time::timeout(BOON_TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "HighlightClipper: boon fehlgeschlagen");
            String::new()
        }
        Err(_) => {
            tracing::error!("HighlightClipper: boon Timeout");
            String::new()
        }
    }
}

// ---- reine Parser (ohne Binary testbar) -------------------------------------

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Parst `boon abilities`: Zeilen `tick hero_id name …`. Nur Zeilen, deren erste
/// zwei Felder reine Ziffern sind (Python `isdigit`, schließt negatives aus).
pub fn parse_abilities(output: &str) -> Vec<AbilityCast> {
    let mut out = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && is_digits(parts[0]) && is_digits(parts[1]) {
            if let (Ok(tick), Ok(hid)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
                out.push((tick, hid, parts[2].to_string()));
            }
        }
    }
    out
}

/// Parst `boon events` und sammelt die Kills, deren `attackername` (case-
/// insensitive) dem Login entspricht. Statemachine 1:1 zu `_parse_kills_from_demo`
/// (erlaubt Tick 0 via `is not None`).
pub fn parse_kills_from_demo(output: &str, twitch_login: &str) -> Vec<DemoKill> {
    let death_re = Regex::new(r"^\[tick (\d+)\] player_death").expect("static regex");
    let name_re = Regex::new(r"^\s+attackername: (.+)").expect("static regex");
    let pawn_re = Regex::new(r"^\s+attacker_pawn: (-?\d+)").expect("static regex");
    let login_lower = twitch_login.to_lowercase();

    let mut kills = Vec::new();
    let mut current_tick: Option<i64> = None;
    let mut in_death = false;
    let mut current_attacker = String::new();
    let mut current_pawn: Option<i64> = None;

    for line in output.lines() {
        if let Some(c) = death_re.captures(line) {
            current_tick = c.get(1).and_then(|m| m.as_str().parse::<i64>().ok());
            in_death = true;
            current_attacker = String::new();
            current_pawn = None;
            continue;
        }
        if in_death {
            if line.trim() == "--" || (line.starts_with("[tick") && !line.contains("player_death"))
            {
                if current_attacker.to_lowercase() == login_lower {
                    if let Some(tick) = current_tick {
                        kills.push(DemoKill {
                            tick,
                            pawn: current_pawn,
                        });
                    }
                }
                in_death = false;
                continue;
            }
            if let Some(c) = name_re.captures(line) {
                current_attacker = c
                    .get(1)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default();
            }
            if let Some(c) = pawn_re.captures(line) {
                current_pawn = c.get(1).and_then(|m| m.as_str().parse::<i64>().ok());
            }
        }
    }
    kills
}

/// Parst `boon events` und liefert den Entity-Index des Spielers über bekannte
/// Kill-Ticks (Toleranz 200 Ticks ≈ 3s). Maske `(pawn & 0xFFFFFFFF) & 0x7FFF`.
/// Schließt Tick 0 aus (Python truthy `if current_tick`).
pub fn parse_player_entity(output: &str, kill_ticks: &[i64]) -> Option<i64> {
    let death_re = Regex::new(r"^\[tick (\d+)\] player_death").expect("static regex");
    let pawn_re = Regex::new(r"^\s+attacker_pawn: (-?\d+)").expect("static regex");
    let tolerance: i64 = 200;

    let mut current_tick: Option<i64> = None;
    let mut in_death = false;

    for line in output.lines() {
        if let Some(c) = death_re.captures(line) {
            current_tick = c.get(1).and_then(|m| m.as_str().parse::<i64>().ok());
            in_death = true;
            continue;
        }
        if in_death {
            if line.trim() == "--" {
                in_death = false;
                continue;
            }
            if let Some(ct) = current_tick {
                if ct != 0 && kill_ticks.iter().any(|&kt| (ct - kt).abs() < tolerance) {
                    if let Some(c) = pawn_re.captures(line) {
                        if let Some(pawn) = c.get(1).and_then(|m| m.as_str().parse::<i64>().ok()) {
                            return Some((((pawn as u64) & 0xFFFF_FFFF) & 0x7FFF) as i64);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Parst die `boon entities … --json`-Ausgabe und liefert (health, max_health)
/// des Entitys. Python: hp darf 0 sein, max_hp muss truthy (≠0) sein.
pub fn parse_health_entities(json: &str, entity_idx: i64) -> Option<(i64, i64)> {
    if json.is_empty() {
        return None;
    }
    let ents: serde_json::Value = serde_json::from_str(json).ok()?;
    for e in ents.as_array()? {
        if e.get("index").and_then(serde_json::Value::as_i64) != Some(entity_idx) {
            continue;
        }
        let Some(f) = e.get("fields") else { continue };
        let hp = f.get("m_iHealth").and_then(serde_json::Value::as_i64);
        let max_hp = f.get("m_iMaxHealth").and_then(serde_json::Value::as_i64);
        if let (Some(hp), Some(max_hp)) = (hp, max_hp) {
            if max_hp != 0 {
                return Some((hp, max_hp));
            }
        }
    }
    None
}

// ---- async Compose-Funktionen (Subprocess + Parser) -------------------------

/// `boon abilities <demo>` → Ability-Casts.
pub async fn abilities(boon_path: &Path, demo_path: &Path) -> Vec<AbilityCast> {
    let demo = demo_path.to_string_lossy();
    let out = run_boon(boon_path, &["abilities", &demo]).await;
    parse_abilities(&out)
}

/// `boon events <demo>` → Kills des Spielers (attackername == login).
pub async fn kills_from_demo(
    boon_path: &Path,
    demo_path: &Path,
    twitch_login: &str,
) -> Vec<DemoKill> {
    let demo = demo_path.to_string_lossy();
    let out = run_boon(boon_path, &["events", &demo]).await;
    parse_kills_from_demo(&out, twitch_login)
}

/// `boon events <demo>` → Entity-Index des Spielers über bekannte Kill-Ticks.
pub async fn find_player_entity(
    boon_path: &Path,
    demo_path: &Path,
    kill_ticks: &[i64],
) -> Option<i64> {
    let demo = demo_path.to_string_lossy();
    let out = run_boon(boon_path, &["events", &demo]).await;
    parse_player_entity(&out, kill_ticks)
}

/// `boon entities <demo> --tick T --filter CitadelPlayerPawn --json` → (hp, max_hp).
pub async fn get_health_at_tick(
    boon_path: &Path,
    demo_path: &Path,
    entity_idx: i64,
    tick: i64,
) -> Option<(i64, i64)> {
    let demo = demo_path.to_string_lossy();
    let tick_s = tick.to_string();
    let out = run_boon(
        boon_path,
        &[
            "entities",
            &demo,
            "--tick",
            &tick_s,
            "--filter",
            "CitadelPlayerPawn",
            "--json",
        ],
    )
    .await;
    parse_health_entities(&out, entity_idx)
}

/// Minimum Health-Prozent im Tick-Fenster `[start, end)` in `step`-Schritten
/// (Python `_get_min_health_in_window`). Kein Datenpunkt → 1.0.
pub async fn get_min_health_in_window(
    boon_path: &Path,
    demo_path: &Path,
    entity_idx: i64,
    start_tick: i64,
    end_tick: i64,
    step: i64,
) -> f64 {
    let mut min_pct = 1.0_f64;
    if step <= 0 {
        return min_pct;
    }
    let mut tick = start_tick;
    while tick < end_tick {
        if let Some((hp, max_hp)) = get_health_at_tick(boon_path, demo_path, entity_idx, tick).await
        {
            if max_hp > 0 {
                let pct = (hp as f64 / max_hp as f64).min(1.0);
                if pct < min_pct {
                    min_pct = pct;
                }
            }
        }
        tick += step;
    }
    min_pct
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_abilities_filtert_nicht_ziffern() {
        let out = "100 5 citadel_ability_hook extra\n\
                   150 5 citadel_ability_uppercut\n\
                   header line ohne ziffern\n\
                   -1 5 neg_keine_ziffer\n\
                   200 7 ability_ult_combo\n";
        let a = parse_abilities(out);
        assert_eq!(
            a,
            vec![
                (100, 5, "citadel_ability_hook".to_string()),
                (150, 5, "citadel_ability_uppercut".to_string()),
                (200, 7, "ability_ult_combo".to_string()),
            ]
        );
    }

    #[test]
    fn kills_from_demo_login_case_insensitive() {
        let out = "[tick 1000] player_death\n\
                   \tattackername: HeroPlayer\n\
                   \tattacker_pawn: 12345\n\
                   --\n\
                   [tick 2000] player_death\n\
                   \tattackername: Enemy\n\
                   \tattacker_pawn: 999\n\
                   --\n";
        let kills = parse_kills_from_demo(out, "heroplayer");
        assert_eq!(
            kills,
            vec![DemoKill {
                tick: 1000,
                pawn: Some(12345)
            }]
        );
    }

    #[test]
    fn kills_from_demo_terminator_durch_andere_tick_zeile() {
        // player_death endet auch durch eine Nicht-death-Tick-Zeile.
        let out = "[tick 500] player_death\n\
                   \tattackername: HeroPlayer\n\
                   \tattacker_pawn: 42\n\
                   [tick 510] some_other_event\n";
        let kills = parse_kills_from_demo(out, "heroplayer");
        assert_eq!(
            kills,
            vec![DemoKill {
                tick: 500,
                pawn: Some(42)
            }]
        );
    }

    #[test]
    fn player_entity_naehe_und_maske() {
        let out = "[tick 1000] player_death\n\
                   \tattacker_pawn: 12345\n\
                   --\n";
        // |1000-1050|=50 < 200 → Treffer; 12345 & 0x7FFF = 12345.
        assert_eq!(parse_player_entity(out, &[1050]), Some(12345));
        // Außerhalb der Toleranz → None.
        assert_eq!(parse_player_entity(out, &[5000]), None);
    }

    #[test]
    fn player_entity_maske_schneidet_hohe_bits() {
        let out = "[tick 100] player_death\n\tattacker_pawn: 16777221\n--\n";
        // 16777221 & 0xFFFFFFFF & 0x7FFF = 5.
        assert_eq!(parse_player_entity(out, &[100]), Some(5));
    }

    #[test]
    fn player_entity_tick_null_ausgeschlossen() {
        let out = "[tick 0] player_death\n\tattacker_pawn: 7\n--\n";
        assert_eq!(parse_player_entity(out, &[0]), None);
    }

    #[test]
    fn health_entities_findet_und_filtert() {
        let json = r#"[{"index": 12345, "fields": {"m_iHealth": 250, "m_iMaxHealth": 1000}}]"#;
        assert_eq!(parse_health_entities(json, 12345), Some((250, 1000)));
        assert_eq!(parse_health_entities(json, 999), None);
        // max_hp 0 → None (Python truthy-Check).
        let json0 = r#"[{"index": 1, "fields": {"m_iHealth": 5, "m_iMaxHealth": 0}}]"#;
        assert_eq!(parse_health_entities(json0, 1), None);
        assert_eq!(parse_health_entities("", 1), None);
    }

    #[tokio::test]
    async fn run_boon_fehlendes_binary_leer() {
        let out = run_boon(Path::new("/nonexistent/boon-binary"), &["events", "x"]).await;
        assert_eq!(out, "");
    }
}
