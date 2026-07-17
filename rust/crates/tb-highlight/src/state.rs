//! Persistenz der bereits verarbeiteten Matches je Streamer (`state.json`).
//!
//! Port von `bot/highlight_clipper/state.py`. Defensive Deserialisierung:
//! fehlende/kaputte Datei oder unerwartete Form → leerer State. Der `state_path`
//! ist injizierbar (Tests); produktiv [`crate::config::STATE_PATH`].
//!
//! Keys werden sortiert serialisiert — `serde_json::Map` ist ohne
//! `preserve_order` ein `BTreeMap`, und [`HighlightState`] ebenso, was Pythons
//! `json.dumps(sort_keys=True)` 1:1 entspricht.

use std::collections::BTreeMap;
use std::path::Path;

/// Gesamtzustand: Login → [`StreamerState`].
pub type HighlightState = BTreeMap<String, StreamerState>;

/// Zustand eines einzelnen Streamers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamerState {
    pub processed_matches: Vec<i64>,
    pub last_checked: i64,
}

/// Lädt den State aus `state_path`. Fehlt die Datei oder ist sie ungültig, wird
/// ein leerer State zurückgegeben (Python `load_state`).
pub fn load_state(state_path: &Path) -> HighlightState {
    let Ok(text) = std::fs::read_to_string(state_path) else {
        return HighlightState::new();
    };
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HighlightState::new();
    };
    let Some(streamers) = payload
        .get("streamers")
        .and_then(serde_json::Value::as_object)
    else {
        return HighlightState::new();
    };

    let mut result = HighlightState::new();
    for (login, data) in streamers {
        let Some(obj) = data.as_object() else {
            continue;
        };
        let processed_matches = obj
            .get("processed_matches")
            .and_then(serde_json::Value::as_array)
            .map(|arr| arr.iter().filter_map(json_as_int).collect())
            .unwrap_or_default();
        let last_checked = obj.get("last_checked").and_then(json_as_int).unwrap_or(0);
        result.insert(
            login.clone(),
            StreamerState {
                processed_matches,
                last_checked,
            },
        );
    }
    result
}

/// Schreibt den State nach `state_path` (`{"streamers": {…}}`, eingerückt,
/// Keys sortiert). Legt das übergeordnete Verzeichnis bei Bedarf an.
pub fn save_state(state_path: &Path, state: &HighlightState) -> std::io::Result<()> {
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut streamers = serde_json::Map::new();
    for (login, data) in state {
        let entry = serde_json::json!({
            "processed_matches": data.processed_matches,
            "last_checked": data.last_checked,
        });
        streamers.insert(login.clone(), entry);
    }
    let payload = serde_json::json!({ "streamers": streamers });
    let text = serde_json::to_string_pretty(&payload)?;
    std::fs::write(state_path, text)
}

/// Ob `match_id` für `login` bereits verarbeitet wurde.
pub fn is_match_processed(state: &HighlightState, login: &str, match_id: i64) -> bool {
    state
        .get(login)
        .is_some_and(|d| d.processed_matches.contains(&match_id))
}

/// Markiert `match_id` für `login` als verarbeitet und persistiert den State.
/// Legt den Streamer-Eintrag bei Bedarf an (last_checked bleibt erhalten).
pub fn mark_match_processed(
    state: &mut HighlightState,
    state_path: &Path,
    login: &str,
    match_id: i64,
) -> std::io::Result<()> {
    let entry = state.entry(login.to_string()).or_default();
    if !entry.processed_matches.contains(&match_id) {
        entry.processed_matches.push(match_id);
    }
    save_state(state_path, state)
}

/// `int(value)`-Semantik aus Python (`_is_int`-Filter + `int`): Zahlen werden
/// gegen 0 gekürzt, Strings nur als reine Ganzzahl geparst, Bools → 0/1.
fn json_as_int(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok(),
        serde_json::Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fresh_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("state.json")
    }

    #[test]
    fn load_fehlende_datei_leer() {
        let p = fresh_path("tb_hl_state_missing");
        assert!(load_state(&p).is_empty());
    }

    #[test]
    fn load_kaputtes_json_leer() {
        let p = fresh_path("tb_hl_state_broken");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ not json").unwrap();
        assert!(load_state(&p).is_empty());
        // Gültiges JSON ohne "streamers"-Objekt → ebenfalls leer.
        std::fs::write(&p, r#"{"foo": 1}"#).unwrap();
        assert!(load_state(&p).is_empty());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn save_load_roundtrip_und_normalisierung() {
        let p = fresh_path("tb_hl_state_roundtrip");
        let mut state = HighlightState::new();
        state.insert(
            "streamerb".to_string(),
            StreamerState {
                processed_matches: vec![100, 200],
                last_checked: 42,
            },
        );
        state.insert(
            "streamera".to_string(),
            StreamerState {
                processed_matches: vec![],
                last_checked: 0,
            },
        );
        save_state(&p, &state).unwrap();
        let loaded = load_state(&p);
        assert_eq!(loaded, state);

        // sortierte Keys auf der Platte (streamera vor streamerb).
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.find("streamera").unwrap() < text.find("streamerb").unwrap());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn mark_und_is_processed() {
        let p = fresh_path("tb_hl_state_mark");
        let mut state = HighlightState::new();
        assert!(!is_match_processed(&state, "nani", 7));

        mark_match_processed(&mut state, &p, "nani", 7).unwrap();
        assert!(is_match_processed(&state, "nani", 7));
        // Dedup: zweites Mark fügt nicht doppelt hinzu.
        mark_match_processed(&mut state, &p, "nani", 7).unwrap();
        assert_eq!(state["nani"].processed_matches, vec![7]);

        // Persistenz: frisch geladen ist 7 weiterhin verarbeitet.
        let reloaded = load_state(&p);
        assert!(is_match_processed(&reloaded, "nani", 7));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn load_filtert_nicht_int_matches() {
        let p = fresh_path("tb_hl_state_filter");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            r#"{"streamers": {"x": {"processed_matches": [1, "2", "abc", 3.9], "last_checked": "55"}}}"#,
        )
        .unwrap();
        let loaded = load_state(&p);
        // "2"→2, "abc"→raus, 3.9→3; last_checked "55"→55.
        assert_eq!(loaded["x"].processed_matches, vec![1, 2, 3]);
        assert_eq!(loaded["x"].last_checked, 55);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
