//! Steam-/Rang-/Live-Lookup für den `!title`-Generator (B11). Port von
//! `bot/title_generator/steam_lookup.py` — liest die SQLite-DB des Steam-Bots
//! (`steam_links` + `live_player_state`) read-only.
//!
//! BEWUSSTE ABWEICHUNG zu Python: Python fragt `steam_links.discord_user_id` ab
//! und nutzt einen veralteten Default-Pfad (`~/Documents/Deadlock/service/…`) —
//! beides existiert im aktuellen Steam-Bot-Rust-Schema/Layout NICHT mehr (die
//! Spalte heißt `user_id`; die DB liegt unter
//! `Deadlock-Bots/data/deadlock.sqlite3`). Pythons Enrichment ist dadurch
//! effektiv tot (liefert immer None). Hier wird das KORREKTE Schema + der echte
//! Pfad genutzt, damit das (mod-only, immer aktive) `!title`-Feature die Rang-/
//! Live-Daten tatsächlich bekommt. Fehlt DB/Tabelle/Zeile → None (sauberer
//! Fallback, das Feature funktioniert auch ohne Enrichment weiter).
//!
//! Funktionen sind synchron (rusqlite); der Aufrufer entscheidet über
//! Off-Thread-Ausführung. Read-only-Open, daher nebenwirkungsfrei.

use rusqlite::OpenFlags;

/// Echter Steam-Bot-SQLite-Pfad (Steam-Bot `DEADLOCK_DB_PATH`-Default).
const STEAM_DB_DEFAULT: &str = "/home/naniadm/Documents/Deadlock-Bots/data/deadlock.sqlite3";

/// Deadlock-Rangnamen 0..11 (Python `_RANK_NAMES`; 10 und 11 = Eternus).
fn rank_name(rank_num: i64) -> &'static str {
    match rank_num {
        0 => "Obscurus",
        1 => "Seeker",
        2 => "Alchemist",
        3 => "Arcanist",
        4 => "Ritualist",
        5 => "Emissary",
        6 => "Archon",
        7 => "Oracle",
        8 => "Phantom",
        9 => "Ascendant",
        10 | 11 => "Eternus",
        _ => "Unknown",
    }
}

/// Rang-Info eines Discord-Users (Python `get_rank_for_discord_user`).
#[derive(Debug, Clone)]
pub struct RankInfo {
    pub rank_name: String,
    pub rank_num: i64,
    pub subrank: i64,
    pub rank_display: String,
}

/// Live-In-Game-Zustand (Python `get_live_state_for_discord_user`).
#[derive(Debug, Clone)]
pub struct LiveState {
    pub in_match: bool,
    pub hero: Option<String>,
    pub party_hint: Option<String>,
    pub stage: Option<String>,
}

/// Pfad zur Steam-Bot-SQLite: env `STEAM_BOT_DB_PATH` (Python-Parität, falls
/// gesetzt) sonst der echte Steam-Bot-Default.
pub fn steam_db_path() -> String {
    std::env::var("STEAM_BOT_DB_PATH").unwrap_or_else(|_| STEAM_DB_DEFAULT.to_string())
}

fn open_ro(db_path: &str) -> Option<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

/// Rang-Info für einen Discord-User; `None` wenn kein Link / DB nicht lesbar.
pub fn get_rank_for_discord_user(db_path: &str, user_id: i64) -> Option<RankInfo> {
    let conn = open_ro(db_path)?;
    let (rank, subrank) = conn
        .query_row(
            "SELECT deadlock_rank, deadlock_subrank FROM steam_links WHERE user_id = ?1 LIMIT 1",
            [user_id],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
        )
        .ok()?;
    let rank_num = rank.unwrap_or(0);
    let subrank = subrank.unwrap_or(0);
    let name = rank_name(rank_num);
    // Python: f"{name} {subrank or ''}".strip() — subrank 0/None → nur Name.
    let rank_display = if subrank != 0 {
        format!("{name} {subrank}")
    } else {
        name.to_string()
    };
    Some(RankInfo {
        rank_name: name.to_string(),
        rank_num,
        subrank,
        rank_display,
    })
}

/// Live-Zustand falls aktuell in Deadlock, sonst `None` (Python
/// `get_live_state_for_discord_user`: `not in_deadlock_now` → None).
pub fn get_live_state_for_discord_user(db_path: &str, user_id: i64) -> Option<LiveState> {
    let conn = open_ro(db_path)?;
    let (in_deadlock, in_match, hero, party_hint, stage) = conn
        .query_row(
            "SELECT lps.in_deadlock_now, lps.in_match_now_strict, lps.deadlock_hero, \
                    lps.deadlock_party_hint, lps.deadlock_stage \
             FROM steam_links sl \
             JOIN live_player_state lps ON sl.steam_id = lps.steam_id \
             WHERE sl.user_id = ?1 LIMIT 1",
            [user_id],
            |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .ok()?;
    if in_deadlock.unwrap_or(0) == 0 {
        return None;
    }
    Some(LiveState {
        in_match: in_match.unwrap_or(0) != 0,
        hero,
        party_hint,
        stage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db(name: &str) -> String {
        let path = format!("/tmp/tb_steam_test_{name}.sqlite3");
        let _ = std::fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE steam_links (user_id INTEGER, steam_id TEXT, deadlock_rank INTEGER, deadlock_subrank INTEGER);\
             CREATE TABLE live_player_state (steam_id TEXT, in_deadlock_now INTEGER, in_match_now_strict INTEGER, deadlock_hero TEXT, deadlock_party_hint TEXT, deadlock_stage TEXT);",
        )
        .unwrap();
        path
    }

    #[test]
    fn rank_lookup_namen_und_subrank() {
        let path = make_test_db("rank");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO steam_links (user_id, steam_id, deadlock_rank, deadlock_subrank) \
             VALUES (123, 's1', 6, 3), (124, 's2', NULL, NULL)",
            [],
        )
        .unwrap();

        let r = get_rank_for_discord_user(&path, 123).unwrap();
        assert_eq!(r.rank_name, "Archon");
        assert_eq!(r.subrank, 3);
        assert_eq!(r.rank_display, "Archon 3");

        // NULL rank → 0 → Obscurus; subrank 0/NULL → nur Name.
        let r2 = get_rank_for_discord_user(&path, 124).unwrap();
        assert_eq!(r2.rank_display, "Obscurus");

        // Unbekannter User → None; fehlende DB → None.
        assert!(get_rank_for_discord_user(&path, 999).is_none());
        assert!(get_rank_for_discord_user("/tmp/tb_steam_nichtda.sqlite3", 123).is_none());
    }

    #[test]
    fn live_state_nur_wenn_in_deadlock() {
        let path = make_test_db("live");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO steam_links (user_id, steam_id) VALUES (200, 'sid')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO live_player_state \
             (steam_id, in_deadlock_now, in_match_now_strict, deadlock_hero, deadlock_party_hint, deadlock_stage) \
             VALUES ('sid', 1, 1, 'Haze', 'solo', 'laning')",
            [],
        )
        .unwrap();

        let ls = get_live_state_for_discord_user(&path, 200).unwrap();
        assert!(ls.in_match);
        assert_eq!(ls.hero.as_deref(), Some("Haze"));
        assert_eq!(ls.stage.as_deref(), Some("laning"));

        // in_deadlock_now = 0 → None.
        conn.execute(
            "UPDATE live_player_state SET in_deadlock_now = 0 WHERE steam_id = 'sid'",
            [],
        )
        .unwrap();
        assert!(get_live_state_for_discord_user(&path, 200).is_none());
    }
}
