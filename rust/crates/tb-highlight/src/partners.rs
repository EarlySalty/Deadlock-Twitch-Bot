//! Auflösung der aktiven Partner-Streamer zu Steam-Account-IDs.
//!
//! Port der Datenschicht aus `bot/highlight_clipper/worker.py`
//! (`_get_partner_streamers` & Helfer). Drei Quellen werden kombiniert:
//! 1. Postgres `twitch_streamers_partner_state` → (login, discord_user_id),
//! 2. Postgres `core.steam_links` → discord_user_id ⇒ Steam-account_id,
//! 3. manuelle Overrides aus `data/highlight_clipper/steamids.json`.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use sqlx::PgPool;

/// Steam64-Basis-Offset (account_id = steam64 − BASE).
pub const STEAM64_BASE: i64 = 76561197960265728;
/// Standardpfad der manuellen Steam-ID-Zuordnungen.
pub const STEAMIDS_JSON_DEFAULT: &str = "data/highlight_clipper/steamids.json";

/// Liest je Discord-User die bevorzugte Steam-account_id aus Central Postgres.
/// Verifizierte Links gehen vor; danach entscheiden Aktualität und Steam-ID
/// deterministisch. Nur positive account_ids werden übernommen.
pub async fn load_steam_account_ids(
    pool: &PgPool,
    discord_ids: &[i64],
) -> Result<HashMap<i64, String>, sqlx::Error> {
    if discord_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, (i64, i64)>(
        "SELECT DISTINCT ON (discord_id) discord_id, steam_id64
         FROM core.steam_links
         WHERE discord_id = ANY($1)
         ORDER BY discord_id, verified DESC, linked_at DESC, steam_id64 ASC",
    )
    .bind(discord_ids)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(discord_id, steam_id64)| {
            let account_id = steam_id64.checked_sub(STEAM64_BASE)?;
            if account_id > 0 {
                Some((discord_id, account_id.to_string()))
            } else {
                None
            }
        })
        .collect())
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

/// Aktive Partner mit hinterlegter Discord-User-ID aus Postgres (Python
/// `_query_partner_streamers`). `discord_user_id` ist TEXT → wie Pythons `int()`
/// geparst; nicht-parsebare Zeilen werden verworfen.
pub async fn query_partner_streamers(pool: &PgPool) -> Vec<(String, i64)> {
    let rows = sqlx::query!(
        "SELECT twitch_login AS \"twitch_login!\", discord_user_id AS \"discord_user_id!\" \
         FROM twitch_streamers_partner_state \
         WHERE is_partner_active = 1 AND discord_user_id IS NOT NULL \
         ORDER BY twitch_login",
    )
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|r| {
                let login = r.twitch_login;
                let discord = r.discord_user_id;
                discord.trim().parse::<i64>().ok().map(|id| (login, id))
            })
            .collect(),
        Err(e) => {
            tracing::error!(
                error = %e,
                "HighlightClipper: Partner-Abfrage fehlgeschlagen; dieser Durchlauf verarbeitet keine Datenbank-Partner"
            );
            Vec::new()
        }
    }
}

/// Voller Orchestrator: Partner und Steam-Links aus Postgres holen und manuelle
/// Overrides anwenden (Python `_get_partner_streamers`).
pub async fn get_partner_streamers(
    pool: &PgPool,
    steamids_json_path: &Path,
) -> Vec<(String, String)> {
    let rows = query_partner_streamers(pool).await;
    let discord_ids: Vec<i64> = rows.iter().map(|(_, id)| *id).collect();
    let discord_to_account = if discord_ids.is_empty() {
        HashMap::new()
    } else {
        match load_steam_account_ids(pool, &discord_ids).await {
            Ok(ids) => ids,
            Err(error) => {
                tracing::error!(
                    %error,
                    requested = discord_ids.len(),
                    discord_ids_preview = ?discord_ids.iter().take(5).collect::<Vec<_>>(),
                    "HighlightClipper: Steam-Links-Abfrage fehlgeschlagen; nur manuelle Steam-Zuordnungen werden verarbeitet"
                );
                HashMap::new()
            }
        }
    };
    let manual = load_manual_steamids(steamids_json_path);
    combine_partners(&rows, &discord_to_account, manual)
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

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::path::PathBuf;
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        // dyn: format!-DDL im temporären Test-Schema, technisch kein sqlx-Makro.
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        // dyn: DDL im temporären Test-Schema, kein Migrations-Bezug.
        sqlx::query(
            "CREATE TABLE twitch_streamers_partner_state \
             (twitch_login TEXT, discord_user_id TEXT, is_partner_active INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE SCHEMA IF NOT EXISTS core")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS core.users (
                discord_id BIGINT PRIMARY KEY,
                username TEXT,
                global_name TEXT,
                avatar TEXT,
                first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
                last_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
                raw JSONB
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS core.steam_links (
                discord_id BIGINT NOT NULL REFERENCES core.users(discord_id) ON DELETE CASCADE,
                steam_id64 BIGINT NOT NULL,
                verified BOOLEAN NOT NULL DEFAULT false,
                linked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (discord_id, steam_id64)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn query_partner_streamers_filtert_und_parst() {
        let Some(pool) = make_pool("t9bii_partner_query").await else { return };
        // dyn: ad-hoc Test-Schema, kein Migrations-Bezug.
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state VALUES \
             ('nani', '12345', 1), \
             ('inactive', '999', 0), \
             ('nodiscord', NULL, 1), \
             ('badid', 'notanumber', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Nur 'nani': aktiv + parsebare Discord-ID. Andere fallen raus.
        assert_eq!(query_partner_streamers(&pool).await, vec![("nani".to_string(), 12345)]);
    }

    #[tokio::test]
    async fn get_partner_streamers_voller_pfad() {
        let Some(pool) = make_pool("t9bii_partner_full").await else { return };
        // dyn: ad-hoc Test-Schema, kein Migrations-Bezug.
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state VALUES \
             ('nani', '12345', 1), ('zoe', '67890', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO core.users (discord_id) VALUES (12345), (67890)
             ON CONFLICT (discord_id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO core.steam_links (discord_id, steam_id64, verified, linked_at)
             VALUES
                (12345, $1, true, '2026-07-28T10:00:00Z'),
                (12345, $2, false, '2026-07-28T11:00:00Z')
             ON CONFLICT (discord_id, steam_id64) DO UPDATE
             SET verified = EXCLUDED.verified, linked_at = EXCLUDED.linked_at",
        )
        .bind(STEAM64_BASE + 5)
        .bind(STEAM64_BASE + 99)
        .execute(&pool)
        .await
        .unwrap();

        let dir = fresh_dir("tb_hl_partners_full");
        // Manueller Override ergänzt zoe.
        let json = dir.join("steamids.json");
        std::fs::write(&json, r#"{"zoe": "4242"}"#).unwrap();

        let out = get_partner_streamers(&pool, &json).await;
        assert_eq!(
            out,
            vec![
                ("nani".to_string(), "5".to_string()),
                ("zoe".to_string(), "4242".to_string()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn steam_account_ids_queryfehler_ist_fehler() {
        let Some(pool) = make_pool("t9bii_partner_query_error").await else {
            return;
        };
        pool.close().await;

        assert!(load_steam_account_ids(&pool, &[12345]).await.is_err());
    }
}
