//! Auflösung der aktiven Partner-Streamer zu Steam-Account-IDs.
//!
//! Port der Datenschicht aus `bot/highlight_clipper/worker.py`
//! (`_get_partner_streamers` & Helfer). Drei Quellen werden kombiniert:
//! 1. Postgres `twitch_streamers_partner_state` → (login, discord_user_id),
//! 2. Steam-Bot-SQLite `steam_links` → discord_user_id ⇒ Steam-account_id,
//! 3. manuelle Overrides aus `data/highlight_clipper/steamids.json`.
//!
//! Slice 9b-i (hier): SQLite-Reader + manuelle Overrides + reine Kombinierung.
//! Die Postgres-Query und der async Orchestrator folgen in 9b-ii.
//!
//! SQLite wird read-only geöffnet — gleiches Muster wie
//! `tb_chat::steam_lookup` (dieselbe Datei `Deadlock-Bots/data/deadlock.sqlite3`).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rusqlite::OpenFlags;

/// Steam64-Basis-Offset (account_id = steam64 − BASE).
pub const STEAM64_BASE: i64 = 76561197960265728;
/// Standardpfad der Steam-Bot-SQLite (cross-repo, read-only).
pub const STEAM_DB_DEFAULT: &str = "/home/naniadm/Documents/Deadlock-Bots/data/deadlock.sqlite3";
/// Standardpfad der manuellen Steam-ID-Zuordnungen.
pub const STEAMIDS_JSON_DEFAULT: &str = "data/highlight_clipper/steamids.json";

/// Liest primäre Steam-account_ids aus `steam_links` für die gegebenen
/// Discord-User-IDs. Fehlende DB/Query-Fehler → leere Map (Python
/// `_load_steam_account_ids`). steam_id wird (wie Pythons `int()`) tolerant
/// geparst; nur positive account_ids werden übernommen.
pub fn load_steam_account_ids(db_path: &Path, discord_ids: &[i64]) -> HashMap<i64, String> {
    let mut result = HashMap::new();
    if discord_ids.is_empty() {
        return result;
    }
    if !db_path.exists() {
        tracing::warn!(path = %db_path.display(), "HighlightClipper: Steam-Links-DB nicht gefunden");
        return result;
    }
    let conn = match rusqlite::Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "HighlightClipper: Steam-Links-DB nicht öffenbar");
            return result;
        }
    };

    let placeholders = vec!["?"; discord_ids.len()].join(",");
    let sql = format!(
        "SELECT user_id, steam_id FROM steam_links \
         WHERE user_id IN ({placeholders}) AND primary_account = 1"
    );

    let query = (|| -> rusqlite::Result<()> {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(discord_ids.iter()), |row| {
            let user_id: i64 = row.get(0)?;
            // steam_id ist TEXT; falls doch INTEGER, tolerant umwandeln.
            let steam_id: String = row
                .get::<_, String>(1)
                .or_else(|_| row.get::<_, i64>(1).map(|i| i.to_string()))?;
            Ok((user_id, steam_id))
        })?;
        for r in rows {
            let (user_id, steam_id) = r?;
            if let Ok(sid) = steam_id.trim().parse::<i64>() {
                let account_id = sid - STEAM64_BASE;
                if account_id > 0 {
                    result.insert(user_id, account_id.to_string());
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = query {
        tracing::error!(error = %e, "HighlightClipper: Steam-Links-Abfrage fehlgeschlagen");
    }
    result
}

/// Lädt manuelle Login→account_id-Overrides aus der JSON-Datei. Fehlt/kaputt →
/// leer. Nur nicht-leere Schlüssel und „truthy" Werte (Python `if k and v`).
pub fn load_manual_steamids(path: &Path) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return result;
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text) else {
        tracing::warn!("HighlightClipper: steamids.json konnte nicht gelesen werden");
        return result;
    };
    for (k, v) in map {
        if k.is_empty() || !json_truthy(&v) {
            continue;
        }
        result.insert(k, json_to_str(&v));
    }
    result
}

/// Kombiniert Partner-Zeilen, SQLite-Auflösung und manuelle Overrides zu einer
/// Login→account_id-Liste (Python `_get_partner_streamers`-Ende). Manuelle
/// Einträge überschreiben/ergänzen.
pub fn combine_partners(
    rows: &[(String, i64)],
    discord_to_account: &HashMap<i64, String>,
    manual: BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut result: BTreeMap<String, String> = BTreeMap::new();
    for (login, discord_id) in rows {
        let login = login.trim();
        if login.is_empty() {
            continue;
        }
        if let Some(account_id) = discord_to_account.get(discord_id) {
            if !account_id.is_empty() {
                result.insert(login.to_string(), account_id.clone());
            }
        }
    }
    for (login, account_id) in manual {
        result.insert(login, account_id);
    }
    result.into_iter().collect()
}

/// Python-Wahrheitswert eines JSON-Werts (für `if v`).
fn json_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        serde_json::Value::Object(o) => !o.is_empty(),
    }
}

/// `str(v)`-Semantik aus Python für die JSON-Werte der Override-Datei.
fn json_to_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => if *b { "True".to_string() } else { "False".to_string() },
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn steam_account_ids_aus_sqlite() {
        let dir = fresh_dir("tb_hl_partners_sqlite");
        let db = dir.join("deadlock.sqlite3");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE steam_links (user_id INTEGER, steam_id TEXT, primary_account INTEGER);",
            )
            .unwrap();
            // user 10: gültig (account_id 5), primary.
            // user 20: account_id 0 (steam_id == BASE) → übersprungen.
            // user 30: primary_account=0 → von Query ausgeschlossen.
            let base = STEAM64_BASE;
            conn.execute(
                "INSERT INTO steam_links VALUES (10, ?1, 1)",
                [(base + 5).to_string()],
            )
            .unwrap();
            conn.execute("INSERT INTO steam_links VALUES (20, ?1, 1)", [base.to_string()])
                .unwrap();
            conn.execute(
                "INSERT INTO steam_links VALUES (30, ?1, 0)",
                [(base + 99).to_string()],
            )
            .unwrap();
        }
        let map = load_steam_account_ids(&db, &[10, 20, 30]);
        assert_eq!(map.get(&10), Some(&"5".to_string()));
        assert!(!map.contains_key(&20)); // account_id 0
        assert!(!map.contains_key(&30)); // nicht primary
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn steam_account_ids_fehlende_db_leer() {
        assert!(load_steam_account_ids(Path::new("/nope/x.sqlite3"), &[1]).is_empty());
        // Leere ID-Liste → leer ohne DB-Zugriff.
        assert!(load_steam_account_ids(Path::new("/nope/x.sqlite3"), &[]).is_empty());
    }

    #[test]
    fn manual_steamids_filtert_leere() {
        let dir = fresh_dir("tb_hl_partners_manual");
        let f = dir.join("steamids.json");
        std::fs::write(&f, r#"{"nani": "12345", "leer": "", "num": 678}"#).unwrap();
        let m = load_manual_steamids(&f);
        assert_eq!(m.get("nani"), Some(&"12345".to_string()));
        assert_eq!(m.get("num"), Some(&"678".to_string())); // str(678)
        assert!(!m.contains_key("leer")); // falsy value
        // Fehlende Datei → leer.
        assert!(load_manual_steamids(&dir.join("missing.json")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn combine_db_und_manual_override() {
        let rows = vec![
            ("nani".to_string(), 10),
            ("  ".to_string(), 11), // leerer Login → raus
            ("other".to_string(), 12), // ohne SQLite-Auflösung → raus
        ];
        let mut d2a = HashMap::new();
        d2a.insert(10, "500".to_string());
        let mut manual = BTreeMap::new();
        manual.insert("nani".to_string(), "999".to_string()); // override
        manual.insert("manualonly".to_string(), "777".to_string());

        let out = combine_partners(&rows, &d2a, manual);
        // nani von manual überschrieben, manualonly ergänzt, other/leer raus.
        assert_eq!(
            out,
            vec![
                ("manualonly".to_string(), "777".to_string()),
                ("nani".to_string(), "999".to_string()),
            ]
        );
    }
}
