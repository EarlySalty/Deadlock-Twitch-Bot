//! Hero-Win/Pick-Stats als Grounding-Anker (Port von
//! `bot/engagement/deadlock_stats.py`).
//!
//! Wird ein Held erkannt (über [`crate::deadlock_wiki::DeadlockWiki`]), kommt ein
//! kurzer Anhaltspunkt rein, ob er gerade stark/überrepräsentiert ist. Bewusst
//! QUALITATIV (über/unter 50%, oft/selten gepickt) statt roher Prozente. ~6h
//! gecacht; Netzfehler → leeres Fragment (sichere Seite).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::deadlock_wiki::{DeadlockWiki, EntityKind};

const API_BASE_DEFAULT: &str = "https://api.deadlock-api.com";
const USER_AGENT: &str = "deadlock-twitch-bot/1.0 (engagement-stats)";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const STATS_TTL: Duration = Duration::from_secs(6 * 3600);

/// Grober Stärke-/Beliebtheits-Eindruck eines Helden.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroStat {
    pub name: String,
    pub wr: f64,
    pub pr_ratio: f64,
}

/// Qualitatives Winrate-Label (5 Stufen um 50%).
pub fn wr_label(wr: f64) -> &'static str {
    if wr >= 0.52 {
        "Winrate deutlich über 50%"
    } else if wr >= 0.505 {
        "Winrate leicht über 50%"
    } else if wr > 0.495 {
        "Winrate um die 50%"
    } else if wr > 0.48 {
        "Winrate leicht unter 50%"
    } else {
        "Winrate deutlich unter 50%"
    }
}

/// Qualitatives Pick-Rate-Label (relativ zum Durchschnitt).
pub fn pr_label(ratio: f64) -> &'static str {
    if ratio >= 1.25 {
        "wird grad sehr oft gespielt"
    } else if ratio >= 0.8 {
        "wird durchschnittlich oft gespielt"
    } else {
        "wird eher selten gespielt"
    }
}

fn as_i64_flex(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn json_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Baut den `name_lower → HeroStat`-Index aus Helden-Assets + hero-stats (reiner
/// Port von `_load_stats` ohne HTTP). pr_ratio = matches / Durchschnitts-matches.
pub fn build_stats_index(heroes: &[Value], stats: &[Value]) -> HashMap<String, HeroStat> {
    let mut id_to_name: HashMap<i64, String> = HashMap::new();
    for h in heroes {
        let id = h.get("id").and_then(as_i64_flex);
        let name = h.get("name").and_then(Value::as_str);
        if let (Some(id), Some(name)) = (id, name) {
            if !name.is_empty() {
                id_to_name.insert(id, name.to_string());
            }
        }
    }

    let rows: Vec<&Value> = stats
        .iter()
        .filter(|r| r.is_object() && r.get("matches").is_some_and(json_truthy))
        .collect();
    if rows.is_empty() {
        return HashMap::new();
    }
    let total: f64 = rows
        .iter()
        .map(|r| r.get("matches").and_then(as_i64_flex).unwrap_or(0) as f64)
        .sum();
    let avg_matches = total / rows.len() as f64;

    let mut out = HashMap::new();
    for r in &rows {
        let Some(name) = r.get("hero_id").and_then(as_i64_flex).and_then(|hid| id_to_name.get(&hid).cloned()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let matches = r.get("matches").and_then(as_i64_flex).unwrap_or(0);
        let wins = r.get("wins").and_then(as_i64_flex).unwrap_or(0);
        let wr = if matches != 0 { wins as f64 / matches as f64 } else { 0.0 };
        let pr_ratio = if avg_matches != 0.0 { matches as f64 / avg_matches } else { 1.0 };
        out.insert(name.to_lowercase(), HeroStat { name, wr, pr_ratio });
    }
    out
}

/// Hero-Stats-Provider mit 6h-Cache. Teilt sich die Entity-Erkennung mit
/// [`DeadlockWiki`]. Basis-URL injizierbar (Tests).
pub struct DeadlockStats {
    api_base: String,
    stats: Mutex<(Option<Instant>, HashMap<String, HeroStat>)>,
    http: reqwest::Client,
}

impl Default for DeadlockStats {
    fn default() -> Self {
        Self::new()
    }
}

impl DeadlockStats {
    pub fn new() -> Self {
        Self::with_base(API_BASE_DEFAULT)
    }

    pub fn with_base(api_base: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            api_base: api_base.trim_end_matches('/').to_string(),
            stats: Mutex::new((None, HashMap::new())),
            http,
        }
    }

    async fn fetch_json(&self, url: &str, params: &[(&str, &str)]) -> Result<Value, reqwest::Error> {
        self.http
            .get(url)
            .query(params)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    async fn load_stats(&self) -> Result<HashMap<String, HeroStat>, reqwest::Error> {
        let heroes = self
            .fetch_json(&format!("{}/v1/assets/heroes", self.api_base), &[("only_active", "true")])
            .await?;
        let stats = self
            .fetch_json(&format!("{}/v1/analytics/hero-stats", self.api_base), &[])
            .await?;
        let heroes = heroes.as_array().cloned().unwrap_or_default();
        let stats = stats.as_array().cloned().unwrap_or_default();
        Ok(build_stats_index(&heroes, &stats))
    }

    async fn ensure_stats(&self) {
        {
            let cache = self.stats.lock().unwrap_or_else(|p| p.into_inner());
            if !cache.1.is_empty() && cache.0.is_some_and(|t| t.elapsed() < STATS_TTL) {
                return;
            }
        }
        let fresh = match self.load_stats().await {
            Ok(f) => f,
            Err(_) => {
                tracing::warn!("DeadlockStats: Stats-Fetch fehlgeschlagen");
                return;
            }
        };
        if !fresh.is_empty() {
            let mut cache = self.stats.lock().unwrap_or_else(|p| p.into_inner());
            cache.1 = fresh;
            cache.0 = Some(Instant::now());
        }
    }

    /// Grober Stärke-/Beliebtheits-Anhaltspunkt zum erkannten Helden — sonst "".
    pub async fn build_stats_fragment(&self, wiki: &DeadlockWiki, message_text: &str) -> String {
        wiki.ensure_index().await;
        let Some((name, kind)) = wiki.detect(message_text) else {
            return String::new();
        };
        if kind != EntityKind::Hero {
            return String::new();
        }
        self.ensure_stats().await;
        let row = {
            let cache = self.stats.lock().unwrap_or_else(|p| p.into_inner());
            cache.1.get(&name.to_lowercase()).cloned()
        };
        let Some(row) = row else {
            return String::new();
        };
        format!(
            "Grober Stärke-Anhaltspunkt zu '{name}' (echte Aggregat-Stats): {wr}, {pr}. \
             Nimm das nur als Gefühl, ob er grad stark/meta ist — red locker drüber, lies KEINE \
             Zahlen oder Prozente vor wie eine Tabelle, und sag nie, woher du das hast.",
            wr = wr_label(row.wr),
            pr = pr_label(row.pr_ratio),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn wr_und_pr_labels() {
        assert_eq!(wr_label(0.55), "Winrate deutlich über 50%");
        assert_eq!(wr_label(0.506), "Winrate leicht über 50%");
        assert_eq!(wr_label(0.50), "Winrate um die 50%");
        assert_eq!(wr_label(0.49), "Winrate leicht unter 50%");
        assert_eq!(wr_label(0.40), "Winrate deutlich unter 50%");
        assert_eq!(pr_label(1.5), "wird grad sehr oft gespielt");
        assert_eq!(pr_label(1.0), "wird durchschnittlich oft gespielt");
        assert_eq!(pr_label(0.5), "wird eher selten gespielt");
    }

    #[test]
    fn stats_index_wr_und_pr_ratio() {
        let heroes = json!([{"id": 1, "name": "Haze"}, {"id": 2, "name": "Bebop"}]);
        let stats = json!([
            {"hero_id": 1, "matches": 100, "wins": 55}, // wr 0.55, matches 100
            {"hero_id": 2, "matches": 300, "wins": 150}, // wr 0.5, matches 300
        ]);
        let idx = build_stats_index(heroes.as_array().unwrap(), stats.as_array().unwrap());
        // avg_matches = 200 → Haze pr 0.5, Bebop pr 1.5.
        let haze = &idx["haze"];
        assert!((haze.wr - 0.55).abs() < 1e-9);
        assert!((haze.pr_ratio - 0.5).abs() < 1e-9);
        let bebop = &idx["bebop"];
        assert!((bebop.wr - 0.5).abs() < 1e-9);
        assert!((bebop.pr_ratio - 1.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn stats_fragment_end_to_end() {
        // Wiki-Server für Entity-Erkennung.
        let wiki_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/heroes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"name": "Haze"}])))
            .mount(&wiki_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&wiki_server)
            .await;
        let wiki =
            DeadlockWiki::with_bases(&wiki_server.uri(), &format!("{}/api.php", wiki_server.uri()));

        // Stats-Server.
        let stats_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/assets/heroes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id": 1, "name": "Haze"}])))
            .mount(&stats_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/analytics/hero-stats"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"hero_id": 1, "matches": 100, "wins": 60}
            ])))
            .mount(&stats_server)
            .await;
        let stats = DeadlockStats::with_base(&stats_server.uri());

        let frag = stats.build_stats_fragment(&wiki, "wie stark ist haze grad").await;
        assert!(frag.contains("'Haze'"));
        assert!(frag.contains("Winrate deutlich über 50%")); // wr 0.6
        // Einziger Held → pr_ratio 1.0 → durchschnittlich.
        assert!(frag.contains("wird durchschnittlich oft gespielt"));

        // Kein Held erkannt → leer.
        assert_eq!(stats.build_stats_fragment(&wiki, "hallo zusammen").await, "");
    }
}
