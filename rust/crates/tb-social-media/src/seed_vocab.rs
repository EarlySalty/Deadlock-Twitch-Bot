//! Initial-Vokabular für `deadlock_vocab` (Port von
//! `bot/social_media/transcription/seed_vocab.py`).
//!
//! Quellen: statische Slang-Begriffe + Heroes/Abilities/Items aus der
//! öffentlichen Deadlock-API. Build-Funktionen sind pur (JSON → [`VocabEntry`]),
//! testbar ohne HTTP. Der CLI-Einstieg + Sync-Wrapper (Admin-Tooling) ist nicht
//! portiert.

use serde_json::Value;
use sqlx::PgPool;

use crate::vocab::{self, VocabEntry};

pub const HEROES_URL: &str = "https://assets.deadlock-api.com/v2/heroes";
pub const ITEMS_URL: &str = "https://assets.deadlock-api.com/v2/items";

/// (term, canonical, aliases) — Deadlock-Slang (Python `SLANG_TERMS`).
const SLANG_TERMS: &[(&str, &str, &[&str])] = &[
    ("ult", "Ultimate", &["ulti", "ultimate", "ult"]),
    ("ulti", "Ultimate", &["ult", "ultimate"]),
    ("buyback", "Buyback", &["buy back", "rebuy"]),
    ("souls", "Souls", &["soul", "soul orbs", "soul orb"]),
    ("soul orb", "Soul Orb", &["soul orbs", "orb", "orbs"]),
    ("lane phase", "Lane Phase", &["laning phase", "lane"]),
    (
        "midboss",
        "Midboss",
        &["mid boss", "mid-boss", "patron mid"],
    ),
    ("patron", "Patron", &["base boss", "endgame boss"]),
    (
        "walker",
        "Walker",
        &["walkers", "boss tier 2", "tier 2 boss"],
    ),
    (
        "guardian",
        "Guardian",
        &["guardians", "boss tier 1", "tier 1 boss"],
    ),
    ("rejuv", "Rejuvenator", &["rejuvenator", "rejuv buff"]),
    ("zip", "Zipline", &["zipline", "zips", "ziplines"]),
    ("flex slot", "Flex Slot", &["flexslot", "flex"]),
    (
        "greens",
        "Weapon Items",
        &["green items", "green slot", "weapon items"],
    ),
    (
        "oranges",
        "Vitality Items",
        &["orange items", "orange slot", "vitality items"],
    ),
    (
        "purples",
        "Spirit Items",
        &["purple items", "purple slot", "spirit items"],
    ),
    ("gank", "Gank", &["ganking", "ganked"]),
    ("rotate", "Rotate", &["rotation", "rotating"]),
    ("farm", "Farm", &["farming", "farmed"]),
    ("split", "Splitpush", &["split push", "splitpush"]),
    ("teamfight", "Teamfight", &["team fight", "tf"]),
    ("ace", "Ace", &["aced", "team kill"]),
    ("clutch", "Clutch", &["clutched", "clutching"]),
    ("oneshot", "Oneshot", &["one shot", "1shot"]),
    ("burst", "Burst", &["burst damage"]),
];

/// Statische Slang-Einträge (category=slang, source=manual, weight=2).
pub fn slang_entries() -> Vec<VocabEntry> {
    SLANG_TERMS
        .iter()
        .map(|(term, canonical, aliases)| VocabEntry {
            term: term.trim().to_lowercase(),
            canonical: canonical.to_string(),
            category: "slang".to_string(),
            source: "manual".to_string(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            weight: 2,
            updated_at: None,
        })
        .collect()
}

/// Erstes nicht-leeres String-Feld aus den Keys.
fn first_str(obj: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| {
            obj.get(*k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(str::to_string)
}

/// Aliase aus den Keys, leere + canonical-Duplikate gefiltert.
fn collect_aliases(obj: &Value, keys: &[&str], canonical: &str) -> Vec<String> {
    let canon_lower = canonical.to_lowercase();
    keys.iter()
        .filter_map(|k| {
            obj.get(*k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .filter(|a| a.to_lowercase() != canon_lower)
        .map(str::to_string)
        .collect()
}

/// Heroes (+ Abilities) → VocabEntries (Python `_build_heroes_entries`).
pub fn build_heroes_entries(heroes: &[Value]) -> Vec<VocabEntry> {
    let mut out = Vec::new();
    for hero in heroes {
        let Some(canonical) = first_str(
            hero,
            &["name", "display_name", "english_name", "internal_name"],
        ) else {
            continue;
        };
        out.push(VocabEntry {
            term: canonical.to_lowercase(),
            canonical: canonical.clone(),
            category: "hero".to_string(),
            source: "deadlock_api".to_string(),
            aliases: collect_aliases(
                hero,
                &["internal_name", "english_name", "short_name", "alt_name"],
                &canonical,
            ),
            weight: 5,
            updated_at: None,
        });

        if let Some(abilities) = hero.get("abilities").and_then(Value::as_array) {
            for ability in abilities {
                let Some(canon_ab) = first_str(ability, &["name", "display_name", "english_name"])
                else {
                    continue;
                };
                out.push(VocabEntry {
                    term: canon_ab.to_lowercase(),
                    canonical: canon_ab.clone(),
                    category: "ability".to_string(),
                    source: "deadlock_api".to_string(),
                    aliases: collect_aliases(
                        ability,
                        &["internal_name", "english_name"],
                        &canon_ab,
                    ),
                    weight: 3,
                    updated_at: None,
                });
            }
        }
    }
    out
}

/// Items → VocabEntries (Python `_build_items_entries`).
pub fn build_items_entries(items: &[Value]) -> Vec<VocabEntry> {
    let mut out = Vec::new();
    for item in items {
        let Some(canonical) = first_str(item, &["name", "display_name", "english_name"]) else {
            continue;
        };
        out.push(VocabEntry {
            term: canonical.to_lowercase(),
            canonical: canonical.clone(),
            category: "item".to_string(),
            source: "deadlock_api".to_string(),
            aliases: collect_aliases(
                item,
                &["internal_name", "english_name", "short_name"],
                &canonical,
            ),
            weight: 4,
            updated_at: None,
        });
    }
    out
}

/// GET → Liste von Objekten (Array direkt oder `data`/`items`/`heroes`-Wrapper).
/// Fehler/Nicht-200 → leer (Python `_fetch_json`).
async fn fetch_json(http: &reqwest::Client, url: &str) -> Vec<Value> {
    let resp = match http.get(url).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!(url, status = r.status().as_u16(), "Deadlock-API non-200");
            return Vec::new();
        }
        Err(error) => {
            tracing::warn!(url, %error, "Deadlock-API request fehlgeschlagen");
            return Vec::new();
        }
    };
    let data: Value = resp.json().await.unwrap_or(Value::Null);
    match data {
        Value::Array(a) => a.into_iter().filter(Value::is_object).collect(),
        Value::Object(_) => ["data", "items", "heroes"]
            .iter()
            .find_map(|k| data.get(k).and_then(Value::as_array))
            .map(|a| a.iter().filter(|v| v.is_object()).cloned().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Lädt Heroes/Abilities + Items aus der Deadlock-API (URLs injizierbar).
pub async fn fetch_deadlock_vocab(
    http: &reqwest::Client,
    heroes_url: &str,
    items_url: &str,
) -> Vec<VocabEntry> {
    let (heroes, items) = tokio::join!(fetch_json(http, heroes_url), fetch_json(http, items_url));
    let mut out = build_heroes_entries(&heroes);
    out.extend(build_items_entries(&items));
    out
}

/// Synchronisiert das Vokabular → (geschrieben, übersprungen).
pub async fn seed_vocab(pool: &PgPool, include_slang: bool, include_api: bool) -> (usize, usize) {
    seed_vocab_with(
        pool,
        &reqwest::Client::new(),
        include_slang,
        include_api,
        HEROES_URL,
        ITEMS_URL,
    )
    .await
}

async fn seed_vocab_with(
    pool: &PgPool,
    http: &reqwest::Client,
    include_slang: bool,
    include_api: bool,
    heroes_url: &str,
    items_url: &str,
) -> (usize, usize) {
    let mut entries = Vec::new();
    if include_slang {
        entries.extend(slang_entries());
    }
    if include_api {
        entries.extend(fetch_deadlock_vocab(http, heroes_url, items_url).await);
    }
    let result = vocab::bulk_upsert_vocab_entries(pool, &entries).await;
    tracing::info!(written = result.0, skipped = result.1, "Vocab-Seed");
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn slang_und_build() {
        let slang = slang_entries();
        assert_eq!(slang.len(), 25);
        assert!(slang.iter().all(|e| e.category == "slang" && e.weight == 2));

        // Hero + Ability-Mapping, alias == canonical gefiltert.
        let heroes = vec![json!({
            "name": "Haze", "internal_name": "hero_haze", "english_name": "Haze",
            "abilities": [{"name": "Fixation", "internal_name": "haze_fixation"}]
        })];
        let entries = build_heroes_entries(&heroes);
        assert_eq!(entries.len(), 2);
        let hero = &entries[0];
        assert_eq!(hero.term, "haze");
        assert_eq!(hero.category, "hero");
        assert_eq!(hero.weight, 5);
        // english_name == canonical → gefiltert; internal_name bleibt.
        assert_eq!(hero.aliases, vec!["hero_haze".to_string()]);
        assert_eq!(entries[1].category, "ability");
        assert_eq!(entries[1].weight, 3);

        // Items.
        let items = vec![json!({"name": "Trophy Collector", "short_name": "Trophy"})];
        let it = build_items_entries(&items);
        assert_eq!(it[0].category, "item");
        assert_eq!(it[0].weight, 4);
        assert_eq!(it[0].aliases, vec!["Trophy".to_string()]);

        // Ohne name → übersprungen.
        assert!(build_items_entries(&[json!({"foo": "bar"})]).is_empty());
    }

    #[tokio::test]
    async fn fetch_deadlock_vocab_via_wrapper_und_array() {
        let server = MockServer::start().await;
        // Heroes als {"data": [...]}-Wrapper.
        Mock::given(method("GET"))
            .and(path("/heroes"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"name": "Bebop"}]})),
            )
            .mount(&server)
            .await;
        // Items als nacktes Array.
        Mock::given(method("GET"))
            .and(path("/items"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{"name": "Headshot Booster"}])),
            )
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let entries = fetch_deadlock_vocab(
            &http,
            &format!("{}/heroes", server.uri()),
            &format!("{}/items", server.uri()),
        )
        .await;
        assert!(entries
            .iter()
            .any(|e| e.canonical == "Bebop" && e.category == "hero"));
        assert!(entries
            .iter()
            .any(|e| e.canonical == "Headshot Booster" && e.category == "item"));
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, \
             category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', \
             aliases JSONB NOT NULL DEFAULT '[]'::JSONB, weight INTEGER NOT NULL DEFAULT 1, \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn seed_slang_only_schreibt_in_db() {
        let Some(pool) = make_pool("t_sm_seed").await else {
            return;
        };
        let http = reqwest::Client::new();
        let (written, skipped) =
            seed_vocab_with(&pool, &http, true, false, "http://x", "http://x").await;
        assert_eq!(written, 25);
        assert_eq!(skipped, 0);
        // Alle 25 Slang-Terme sind unique (lowercase) → 25 Zeilen.
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM deadlock_vocab WHERE category='slang'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 25);
    }
}
