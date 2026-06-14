//! Aktuelle Deadlock-Patchnotes als Grounding (Port von
//! `bot/engagement/deadlock_patches.py`).
//!
//! Quelle: Steam-News-API (appid 1422450), BBCode → Change-Zeilen, ~6h gecacht.
//! Zwei Einspeisungen: entity-getriggert ([`DeadlockPatches::build_patch_fragment`],
//! Helden/Item via [`crate::deadlock_wiki::DeadlockWiki`]) und ambient
//! ([`DeadlockPatches::get_patch_digest_fragment`], nur bei Patch-/Meta-Gespräch).
//! Halluzinations-sicher: nur belegte Zeilen, Quelle nie genannt.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use crate::deadlock_wiki::{word_boundary_contains, DeadlockWiki};

const STEAM_NEWS_URL_DEFAULT: &str = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/";
const APPID: i64 = 1422450;
const USER_AGENT: &str = "deadlock-twitch-bot/1.0 (engagement-patchnotes)";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const TTL: Duration = Duration::from_secs(6 * 3600);
const MIN_CHANGE_LINES: usize = 5;
const MAX_ENTITY_LINES: usize = 10;
const MAX_DIGEST_LINES: usize = 14;

/// Der zuletzt gefundene Patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestPatch {
    pub title: String,
    pub date: Option<i64>,
    pub lines: Vec<String>,
}

fn as_i64_flex(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{00A0}'),
        _ => {
            if let Some(num) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                u32::from_str_radix(num, 16).ok().and_then(char::from_u32)
            } else if let Some(num) = entity.strip_prefix('#') {
                num.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

/// Minimaler HTML-Entity-Decoder (gängige Named- + numerische Refs) — pragmatisch
/// für Steam-Patchnotes (volle Named-Tabelle nicht nötig).
fn html_unescape(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..]; // beginnt mit '&'
        if let Some(semi) = after[1..].find(';').filter(|&p| p < 12) {
            if let Some(ch) = decode_entity(&after[1..1 + semi]) {
                out.push(ch);
                rest = &after[1 + semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &after[1..];
    }
    out.push_str(rest);
    out
}

/// BBCode-Patchtext → Liste echter Change-Zeilen (führendes „- " entfernt).
pub fn bbcode_to_change_lines(body: &str) -> Vec<String> {
    let mut t = html_unescape(body).replace('\r', "\n");
    let rep = |t: &str, pat: &str, with: &str| -> String {
        Regex::new(pat).expect("static regex").replace_all(t, with).into_owned()
    };
    t = rep(&t, r"(?i)\[/?p\]", "\n");
    t = rep(&t, r"(?i)\[h[12]\](.*?)\[/h[12]\]", "\n[ $1 ]\n");
    t = rep(&t, r"(?i)\[\*\]", "\n- ");
    t = rep(&t, r"(?is)\[img\].*?\[/img\]", "");
    t = rep(&t, r"(?is)\[url=(.*?)\](.*?)\[/url\]", "$2");
    t = rep(
        &t,
        r"(?i)\[/?(?:b|i|u|list(?:=[^\]]*)?|url[^\]]*|h[1-6]|quote|code|noparse|table|tr|td|spoiler|strike)\]",
        "",
    );

    let mut changes = Vec::new();
    for raw in t.split('\n') {
        if let Some(rest) = raw.trim().strip_prefix("- ") {
            let s = rest.trim();
            if s.chars().count() >= 4 {
                changes.push(s.to_string());
            }
        }
    }
    changes
}

/// Change-Zeilen, die den Helden/das Item erwähnen (Wortgrenze), max 10.
fn lines_for_entity(name: &str, lines: &[String]) -> Vec<String> {
    let needle = name.to_lowercase();
    lines
        .iter()
        .filter(|ln| word_boundary_contains(&ln.to_lowercase(), &needle))
        .take(MAX_ENTITY_LINES)
        .cloned()
        .collect()
}

/// Wählt aus den Steam-News das erste Item mit genug Change-Zeilen.
pub fn parse_latest_patch(data: &Value) -> Option<LatestPatch> {
    let items = data
        .get("appnews")
        .and_then(|a| a.get("newsitems"))
        .and_then(Value::as_array)?;
    for it in items {
        if !it.is_object() {
            continue;
        }
        let contents = it.get("contents").and_then(Value::as_str).unwrap_or("");
        let lines = bbcode_to_change_lines(contents);
        if lines.len() >= MIN_CHANGE_LINES {
            let title = it
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Update")
                .to_string();
            let date = it.get("date").and_then(as_i64_flex);
            return Some(LatestPatch { title, date, lines });
        }
    }
    None
}

fn patch_talk_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(patch|update|hotfix|nerf|nerv|buff|gebufft|generft|generved|meta|balance|patchnotes)\w*",
        )
        .expect("static regex")
    })
}

/// Patchnotes-Provider mit 6h-Cache. Steam-News-URL injizierbar (Tests).
pub struct DeadlockPatches {
    news_url: String,
    latest: Mutex<(Option<Instant>, Option<LatestPatch>)>,
    http: reqwest::Client,
}

impl Default for DeadlockPatches {
    fn default() -> Self {
        Self::new()
    }
}

impl DeadlockPatches {
    pub fn new() -> Self {
        Self::with_url(STEAM_NEWS_URL_DEFAULT)
    }

    pub fn with_url(news_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            news_url: news_url.to_string(),
            latest: Mutex::new((None, None)),
            http,
        }
    }

    async fn fetch_latest_patch(&self) -> Result<Option<LatestPatch>, reqwest::Error> {
        let appid = APPID.to_string();
        let data: Value = self
            .http
            .get(&self.news_url)
            .query(&[
                ("appid", appid.as_str()),
                ("count", "15"),
                ("maxlength", "0"),
                ("format", "json"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(parse_latest_patch(&data))
    }

    async fn ensure_latest(&self) {
        {
            let cache = self.latest.lock().unwrap_or_else(|p| p.into_inner());
            if cache.1.is_some() && cache.0.is_some_and(|t| t.elapsed() < TTL) {
                return;
            }
        }
        let fresh = match self.fetch_latest_patch().await {
            Ok(f) => f,
            Err(_) => {
                tracing::warn!("DeadlockPatches: Patch-Fetch fehlgeschlagen");
                return;
            }
        };
        if let Some(patch) = fresh {
            let mut cache = self.latest.lock().unwrap_or_else(|p| p.into_inner());
            cache.1 = Some(patch);
            cache.0 = Some(Instant::now());
        }
    }

    fn latest_snapshot(&self) -> Option<LatestPatch> {
        self.latest.lock().unwrap_or_else(|p| p.into_inner()).1.clone()
    }

    /// Echte Patch-Änderungen zum erkannten Held/Item — oder "".
    pub async fn build_patch_fragment(&self, wiki: &DeadlockWiki, message_text: &str) -> String {
        wiki.ensure_index().await;
        let Some((name, _kind)) = wiki.detect(message_text) else {
            return String::new();
        };
        self.ensure_latest().await;
        let Some(patch) = self.latest_snapshot() else {
            return String::new();
        };
        let lines = lines_for_entity(&name, &patch.lines);
        if lines.is_empty() {
            return String::new();
        }
        let body = lines.iter().map(|ln| format!("- {ln}")).collect::<Vec<_>>().join("\n");
        format!(
            "Echte Änderungen aus dem letzten Deadlock-Patch ('{title}') zu '{name}'. \
             Du darfst die einschätzen — Buff oder Nerf, ob sich das gut/stark anfühlt — aber \
             AUSSCHLIESSLICH auf Basis dieser Zeilen, nichts dazu erfinden, und sag nie, woher du \
             das hast:\n{body}",
            title = patch.title,
        )
    }

    /// Kompakter Überblick des letzten Patches — nur bei Patch-/Meta-Gespräch.
    pub async fn get_patch_digest_fragment(&self, message_text: &str) -> String {
        if !patch_talk_re().is_match(message_text) {
            return String::new();
        }
        self.ensure_latest().await;
        let Some(patch) = self.latest_snapshot().filter(|p| !p.lines.is_empty()) else {
            return String::new();
        };
        let shown = &patch.lines[..patch.lines.len().min(MAX_DIGEST_LINES)];
        let body = shown.iter().map(|ln| format!("- {ln}")).collect::<Vec<_>>().join("\n");
        let more = patch.lines.len() - shown.len();
        let tail = if more > 0 {
            format!("\n(… und {more} weitere Änderungen)")
        } else {
            String::new()
        };
        format!(
            "Der letzte Deadlock-Patch ('{title}') hat u.a. das hier geändert (echte \
             Patch-Zeilen). Wenn jemand über den Patch oder die Meta redet, darfst du das einschätzen \
             (was ist Buff/Nerf, was tut dem Game gut/weh) — aber nur auf Basis dieser Zeilen, nichts \
             erfinden, Quelle nie nennen:\n{body}{tail}",
            title = patch.title,
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
    fn html_unescape_gaengige() {
        assert_eq!(html_unescape("a&amp;b&lt;c&gt;d&#39;e"), "a&b<c>d'e");
        assert_eq!(html_unescape("kein entity"), "kein entity");
        assert_eq!(html_unescape("&#x41;"), "A");
    }

    #[test]
    fn bbcode_zu_change_lines() {
        let body = "[h1]Helden[/h1]\n[list]\n[*][b]Haze[/b]: Damage erhöht\n[*]zu kurz\n[/list]\n[img]x[/img]\n[url=http://x]Klick[/url]";
        let lines = bbcode_to_change_lines(body);
        // "[ Helden ]" ist kein "- "-Bullet; "Haze: Damage erhöht" bleibt; "zu kurz" (7 Zeichen) bleibt.
        assert!(lines.iter().any(|l| l.contains("Haze: Damage erhöht")));
        assert!(lines.iter().any(|l| l == "zu kurz"));
        // BBCode-Tags + img-Inhalte sind weg; "Klick" ist kein "- "-Bullet → nicht drin.
        assert!(!lines.iter().any(|l| l.contains("[b]") || l.contains("img")));
        assert!(!lines.iter().any(|l| l.contains("Klick")));
    }

    #[test]
    fn lines_for_entity_wortgrenze() {
        let lines = vec![
            "Haze: Bullet Damage -5".to_string(),
            "Breach unverändert".to_string(), // 'haze' nicht in 'breach'... aber kein haze
            "haze ult cooldown +2".to_string(),
        ];
        let hits = lines_for_entity("Haze", &lines);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|l| l.to_lowercase().contains("haze")));
    }

    #[test]
    fn parse_latest_nimmt_erstes_mit_genug_zeilen() {
        let data = json!({
            "appnews": {"newsitems": [
                {"title": "Andere News", "contents": "[list][*]nur eine Zeile[/list]"},
                {"title": "Patch 1.2", "date": 1700000000, "contents":
                    "[list][*]Eins lang[*]Zwei lang[*]Drei lang[*]Vier lang[*]Fuenf lang[/list]"}
            ]}
        });
        let patch = parse_latest_patch(&data).unwrap();
        assert_eq!(patch.title, "Patch 1.2");
        assert_eq!(patch.lines.len(), 5);
        assert_eq!(patch.date, Some(1700000000));
    }

    #[tokio::test]
    async fn digest_nur_bei_patch_talk() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/news"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "appnews": {"newsitems": [{"title": "Patch X", "contents":
                    "[list][*]Haze generft[*]Bebop gebufft[*]Drei lang[*]Vier lang[*]Fuenf lang[/list]"}]}
            })))
            .mount(&server)
            .await;
        let patches = DeadlockPatches::with_url(&format!("{}/news", server.uri()));

        // Kein Patch-Talk → leer (kein Fetch nötig).
        assert_eq!(patches.get_patch_digest_fragment("hallo zusammen").await, "");
        // Patch-Talk → Digest.
        let frag = patches.get_patch_digest_fragment("wie ist die neue meta").await;
        assert!(frag.contains("Patch X"));
        assert!(frag.contains("- Haze generft"));
    }
}
