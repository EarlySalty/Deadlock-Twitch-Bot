//! Deadlock-Wissens-Grounding für den Engagement-Layer (Port von
//! `bot/engagement/deadlock_wiki.py`).
//!
//! Slice 5a (hier): die reinen Teile — Entity-Index-Aufbau aus den Assets-JSONs,
//! Entity-Erkennung im Chat-Text (Wortgrenzen ohne Lookaround, s. Insight im
//! Commit) und das Trimmen der Wiki-Extracts. Der HTTP-/Cache-Layer
//! (`ensure_index`/`fetch_wiki_extract`/`build_grounding_fragment`) folgt in 5b.
//!
//! Die Entity-Erkennung dient auch [`crate::deadlock_stats`] und
//! `deadlock_patches` als Grundlage.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;

/// Kürzere Namen geben als Chat-Wort zu viele Fehltreffer.
pub const MIN_NAME_LEN: usize = 4;
const MAX_EXTRACT_CHARS: usize = 700;

/// Trailing-Wiki-Sektionen, die fürs Grounding nur Rauschen sind.
const TRIM_AT: &[&str] = &[
    "== Update history ==",
    "== Navigation ==",
    "== Gallery ==",
    "== Trivia ==",
    "== Backstory ==",
    "== See also ==",
    "== References ==",
];

/// Art einer erkannten Spielsache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Hero,
    Item,
}

impl EntityKind {
    /// Deutsches Label fürs Grounding-Fragment.
    pub fn label(self) -> &'static str {
        match self {
            EntityKind::Hero => "Held",
            EntityKind::Item => "Item",
        }
    }
}

/// Ein Eintrag im Entity-Index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub lower: String,
    pub name: String,
    pub kind: EntityKind,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Wortgrenzen-Suche ohne Lookaround (mirror von `(?<!\w)needle(?!\w)`): `needle`
/// kommt vor, wenn es weder von einem Wortzeichen umgeben ist.
fn word_boundary_contains(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    for (pos, m) in haystack.match_indices(needle) {
        let before_ok = pos == 0 || !is_word_char(haystack[..pos].chars().next_back().unwrap());
        let after = pos + m.len();
        let after_ok =
            after >= haystack.len() || !is_word_char(haystack[after..].chars().next().unwrap());
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Item-Name nur, wenn es ein echter Anzeigename ist (nicht gleich `class_name`,
/// kein interner snake_case-Name).
pub fn display_item_name(entry: &serde_json::Value) -> Option<String> {
    let name = entry.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    if entry.get("class_name").and_then(serde_json::Value::as_str) == Some(name) {
        return None;
    }
    // Interner snake_case-Name (nur klein, _, Ziffern) → kein Anzeigename.
    if name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    Some(name.to_string())
}

/// Helden-Name (getrimmt, nicht leer).
pub fn hero_name(entry: &serde_json::Value) -> Option<String> {
    let name = entry.get("name")?.as_str()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Baut den Entity-Index aus den Assets-JSONs: Items zuerst (setdefault), Helden
/// überschreiben bei Namensgleichheit. Nur Namen ≥ [`MIN_NAME_LEN`]. Sortiert
/// längster Name zuerst (spezifischster Treffer gewinnt), bei Gleichstand
/// alphabetisch (Determinismus statt Python-Insertion-Order).
pub fn build_entity_index(heroes: &[serde_json::Value], items: &[serde_json::Value]) -> Vec<Entity> {
    let mut seen: HashMap<String, (String, EntityKind)> = HashMap::new();
    for it in items {
        if let Some(name) = display_item_name(it) {
            if name.chars().count() >= MIN_NAME_LEN {
                seen.entry(name.to_lowercase()).or_insert((name, EntityKind::Item));
            }
        }
    }
    for h in heroes {
        if let Some(name) = hero_name(h) {
            if name.chars().count() >= MIN_NAME_LEN {
                seen.insert(name.to_lowercase(), (name, EntityKind::Hero));
            }
        }
    }
    let mut entities: Vec<Entity> = seen
        .into_iter()
        .map(|(lower, (name, kind))| Entity { lower, name, kind })
        .collect();
    entities.sort_by(|a, b| {
        b.lower
            .chars()
            .count()
            .cmp(&a.lower.chars().count())
            .then_with(|| a.lower.cmp(&b.lower))
    });
    entities
}

/// Erkennt den spezifischsten genannten Helden/Item im Text.
pub fn detect_entity(entities: &[Entity], text: &str) -> Option<(String, EntityKind)> {
    if text.is_empty() {
        return None;
    }
    let haystack = text.to_lowercase();
    for e in entities {
        if word_boundary_contains(&haystack, &e.lower) {
            return Some((e.name.clone(), e.kind));
        }
    }
    None
}

/// Trimmt einen Wiki-Extract: Rausch-Sektionen ab, Mehrfach-Leerzeilen
/// zusammen, leere Header weg, Max-Länge mit `…`.
pub fn trim_extract(extract: &str) -> String {
    if extract.is_empty() {
        return String::new();
    }
    // Ab dem ersten Rausch-Marker abschneiden.
    let mut cut = extract.len();
    for marker in TRIM_AT {
        if let Some(idx) = extract.find(marker) {
            cut = cut.min(idx);
        }
    }
    let text = &extract[..cut];

    // 3+ aufeinanderfolgende (Whitespace-)Leerzeilen → ein einzelnes \n.
    static BLANK_RUN: OnceLock<Regex> = OnceLock::new();
    let blank_run =
        BLANK_RUN.get_or_init(|| Regex::new(r"\n[ \t]*\n[ \t]*(?:\n[ \t]*)+").expect("valide Regex"));
    let collapsed = blank_run.replace_all(text, "\n");
    let collapsed = collapsed.trim();

    // Leere "== Abschnitt ==" ohne Inhalt entfernen.
    static HEADER: OnceLock<Regex> = OnceLock::new();
    let header = HEADER.get_or_init(|| Regex::new(r"^==+ .+? ==+$").expect("valide Regex"));
    let without_headers: String = collapsed
        .lines()
        .filter(|line| !header.is_match(line.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    let text = without_headers.trim();

    if text.chars().count() > MAX_EXTRACT_CHARS {
        let truncated: String = text.chars().take(MAX_EXTRACT_CHARS - 1).collect();
        format!("{}…", truncated.trim_end())
    } else {
        text.to_string()
    }
}

const ASSETS_BASE_DEFAULT: &str = "https://assets.deadlock-api.com";
const WIKI_API_DEFAULT: &str = "https://deadlock.wiki/api.php";
const USER_AGENT: &str = "deadlock-twitch-bot/1.0 (engagement-grounding)";
const INDEX_TTL: Duration = Duration::from_secs(12 * 3600);
const PAGE_TTL: Duration = Duration::from_secs(3600);
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);

struct IndexCache {
    entities: Vec<Entity>,
    loaded_at: Option<Instant>,
}

/// Deadlock-Wissens-Grounding mit Entity-Index- (12h) und Wiki-Page-Cache (1h).
/// Hält die Caches selbst (statt Python-Modul-Globals); eine Instanz wird im
/// Pipeline-Setup geteilt. Basis-URLs sind injizierbar (Tests).
pub struct DeadlockWiki {
    assets_base: String,
    wiki_api: String,
    index: Mutex<IndexCache>,
    pages: Mutex<HashMap<String, (Instant, String)>>,
    http: reqwest::Client,
}

impl Default for DeadlockWiki {
    fn default() -> Self {
        Self::new()
    }
}

impl DeadlockWiki {
    /// Mit den produktiven Endpunkten.
    pub fn new() -> Self {
        Self::with_bases(ASSETS_BASE_DEFAULT, WIKI_API_DEFAULT)
    }

    /// Mit expliziten Basis-URLs (Tests).
    pub fn with_bases(assets_base: &str, wiki_api: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            assets_base: assets_base.trim_end_matches('/').to_string(),
            wiki_api: wiki_api.to_string(),
            index: Mutex::new(IndexCache { entities: Vec::new(), loaded_at: None }),
            pages: Mutex::new(HashMap::new()),
            http,
        }
    }

    async fn fetch_json(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.http
            .get(url)
            .query(params)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    async fn load_entity_index(&self) -> Result<Vec<Entity>, reqwest::Error> {
        let heroes = self
            .fetch_json(&format!("{}/v2/heroes", self.assets_base), &[("only_active", "true")])
            .await?;
        let items = self
            .fetch_json(&format!("{}/v2/items", self.assets_base), &[])
            .await?;
        let heroes = heroes.as_array().cloned().unwrap_or_default();
        let items = items.as_array().cloned().unwrap_or_default();
        Ok(build_entity_index(&heroes, &items))
    }

    /// Lädt den Entity-Index, falls Cache leer/abgelaufen. Netzfehler → alter
    /// (ggf. leerer) Index bleibt (Grounding bleibt dann aus).
    pub async fn ensure_index(&self) {
        {
            let cache = self.index.lock().unwrap_or_else(|p| p.into_inner());
            if !cache.entities.is_empty()
                && cache.loaded_at.is_some_and(|t| t.elapsed() < INDEX_TTL)
            {
                return;
            }
        }
        let fresh = match self.load_entity_index().await {
            Ok(f) => f,
            Err(_) => {
                tracing::warn!("DeadlockWiki: Entity-Index konnte nicht geladen werden");
                return;
            }
        };
        if !fresh.is_empty() {
            let mut cache = self.index.lock().unwrap_or_else(|p| p.into_inner());
            cache.entities = fresh;
            cache.loaded_at = Some(Instant::now());
        }
    }

    /// Erkennt den spezifischsten Helden/Item im Text gegen den aktuellen Index.
    pub fn detect(&self, text: &str) -> Option<(String, EntityKind)> {
        let cache = self.index.lock().unwrap_or_else(|p| p.into_inner());
        detect_entity(&cache.entities, text)
    }

    /// Holt den (getrimmten) Wiki-Extract zu einem Titel; 1h gecacht.
    /// Netzfehler/leer → None.
    pub async fn fetch_wiki_extract(&self, title: &str) -> Option<String> {
        let key = title.to_lowercase();
        {
            let cache = self.pages.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((loaded, trimmed)) = cache.get(&key) {
                if loaded.elapsed() < PAGE_TTL {
                    return if trimmed.is_empty() { None } else { Some(trimmed.clone()) };
                }
            }
        }
        let data = self
            .fetch_json(
                &self.wiki_api,
                &[
                    ("action", "query"),
                    ("prop", "extracts"),
                    ("explaintext", "1"),
                    ("redirects", "1"),
                    ("format", "json"),
                    ("titles", title),
                ],
            )
            .await
            .ok()?;
        let extract = data
            .get("query")
            .and_then(|q| q.get("pages"))
            .and_then(serde_json::Value::as_object)
            .and_then(|pages| {
                pages
                    .values()
                    .find_map(|page| page.get("extract").and_then(serde_json::Value::as_str))
            })
            .unwrap_or("");
        let trimmed = trim_extract(extract);
        {
            let mut cache = self.pages.lock().unwrap_or_else(|p| p.into_inner());
            cache.insert(key, (Instant::now(), trimmed.clone()));
        }
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    /// System-Prompt-Fragment mit belegten Deadlock-Fakten — oder "" wenn nichts
    /// erkannt / kein Wiki-Beleg (Python `build_grounding_fragment`).
    pub async fn build_grounding_fragment(&self, message_text: &str) -> String {
        self.ensure_index().await;
        let hit = self.detect(message_text);
        let Some((name, kind)) = hit else {
            return String::new();
        };
        let Some(extract) = self.fetch_wiki_extract(&name).await else {
            return String::new();
        };
        format!(
            "Beleg aus dem Deadlock-Wiki (offizielle Quelle). Wenn du in deiner Antwort etwas \
             über '{name}' sagst, stütze dich AUSSCHLIESSLICH auf diese Fakten — nichts dazu \
             erfinden, nichts aus dem Gedächtnis ergänzen. Stehen Details (Zahlen, Effekte) hier \
             nicht drin, sag das nicht, sondern bleib allgemein.\n[{label}: {name}]\n{extract}",
            label = kind.label(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn item_name_filtert_intern() {
        // echter Anzeigename
        assert_eq!(
            display_item_name(&json!({"name": "Trophy Collector", "class_name": "x"})),
            Some("Trophy Collector".to_string())
        );
        // name == class_name → None
        assert_eq!(
            display_item_name(&json!({"name": "citadel_weapon_x", "class_name": "citadel_weapon_x"})),
            None
        );
        // reiner snake_case → None
        assert_eq!(display_item_name(&json!({"name": "citadel_weapon_x"})), None);
    }

    #[test]
    fn index_dedup_hero_gewinnt_und_min_len() {
        let heroes = vec![json!({"name": "Haze"}), json!({"name": "GG"})]; // GG < 4 → raus
        let items = vec![
            json!({"name": "Haze", "class_name": "c"}), // wird von Hero überschrieben
            json!({"name": "Trophy Collector", "class_name": "c"}),
        ];
        let idx = build_entity_index(&heroes, &items);
        // "trophy collector" (16) vor "haze" (4); GG fehlt.
        assert_eq!(idx.len(), 2);
        assert_eq!(idx[0].lower, "trophy collector");
        let haze = idx.iter().find(|e| e.lower == "haze").unwrap();
        assert_eq!(haze.kind, EntityKind::Hero); // Hero gewann
    }

    #[test]
    fn detect_wortgrenze_und_spezifischster() {
        let idx = build_entity_index(
            &[json!({"name": "Reach"})],
            &[json!({"name": "Trophy Collector", "class_name": "c"})],
        );
        // 'reach' triggert NICHT in 'breach'
        assert_eq!(detect_entity(&idx, "the breach is wide"), None);
        // exaktes Wort triggert
        assert_eq!(detect_entity(&idx, "nice reach there").map(|(_, k)| k), Some(EntityKind::Hero));
        // Mehrwort-Item
        assert_eq!(
            detect_entity(&idx, "lohnt trophy collector?").map(|(n, _)| n),
            Some("Trophy Collector".to_string())
        );
    }

    #[test]
    fn trim_extract_schneidet_und_kuerzt() {
        let raw = "Erster Absatz.\n\n\n\nZweiter.\n== Trivia ==\nSoll weg.";
        let out = trim_extract(raw);
        assert!(out.contains("Erster Absatz."));
        assert!(out.contains("Zweiter."));
        assert!(!out.contains("Trivia"));
        assert!(!out.contains("Soll weg"));
        // Lange Extracts werden mit … gekürzt.
        let long = "a".repeat(900);
        let cut = trim_extract(&long);
        assert!(cut.chars().count() <= MAX_EXTRACT_CHARS);
        assert!(cut.ends_with('…'));
    }

    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn mount_assets(server: &MockServer, heroes: serde_json::Value, items: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path("/v2/heroes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(heroes))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(items))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn grounding_fragment_end_to_end() {
        let server = MockServer::start().await;
        mount_assets(&server, json!([{"name": "Haze"}]), json!([])).await;
        Mock::given(method("GET"))
            .and(path("/api.php"))
            .and(query_param("action", "query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "query": {"pages": {"123": {"extract": "Haze ist ein Hero mit Bullet-Dance."}}}
            })))
            .mount(&server)
            .await;

        let wiki = DeadlockWiki::with_bases(&server.uri(), &format!("{}/api.php", server.uri()));
        let frag = wiki.build_grounding_fragment("erzähl mir was über haze").await;
        assert!(frag.contains("[Held: Haze]"));
        assert!(frag.contains("Bullet-Dance"));
        assert!(frag.contains("Beleg aus dem Deadlock-Wiki"));
    }

    #[tokio::test]
    async fn grounding_leer_ohne_entity() {
        let server = MockServer::start().await;
        mount_assets(&server, json!([{"name": "Haze"}]), json!([])).await;
        let wiki = DeadlockWiki::with_bases(&server.uri(), &format!("{}/api.php", server.uri()));
        // Kein bekannter Held/Item im Text → leeres Fragment.
        assert_eq!(wiki.build_grounding_fragment("hallo zusammen wie gehts").await, "");
    }

    #[tokio::test]
    async fn index_cache_nur_einmal_geladen() {
        let server = MockServer::start().await;
        // expect(1): Assets werden nur EINMAL geholt trotz zweier ensure_index.
        Mock::given(method("GET"))
            .and(path("/v2/heroes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"name": "Haze"}])))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;
        let wiki = DeadlockWiki::with_bases(&server.uri(), &format!("{}/api.php", server.uri()));
        wiki.ensure_index().await;
        wiki.ensure_index().await; // aus dem Cache
        assert_eq!(wiki.detect("haze ist stark").map(|(n, _)| n), Some("Haze".to_string()));
        server.verify().await;
    }
}
