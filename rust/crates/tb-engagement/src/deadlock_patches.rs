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

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;

use crate::deadlock_wiki::{word_boundary_contains, DeadlockWiki};
use crate::steam_web_api::{Observation, Reservation, Settlement, SteamWebApiLedger};

const STEAM_NEWS_URL_DEFAULT: &str = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/";
const APPID: i64 = 1422450;
const USER_AGENT: &str = "deadlock-twitch-bot/1.0 (engagement-patchnotes)";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const TTL: Duration = Duration::from_secs(6 * 3600);
const RETRY_FLOOR: Duration = Duration::from_secs(60);
const FAILURE_BACKOFF: Duration = Duration::from_secs(15 * 60);
const LEDGER_CALLER: &str = "twitch_engagement_patches";
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
    cache: Mutex<PatchCache>,
    refresh: tokio::sync::Mutex<()>,
    ledger: SteamWebApiLedger,
    http: Option<reqwest::Client>,
}

#[derive(Default)]
struct PatchCache {
    latest: Option<LatestPatch>,
    refreshed_at: Option<Instant>,
    next_attempt: Option<Instant>,
}

enum Fetch {
    Patch(Option<LatestPatch>),
    Failed,
}

fn until(time: DateTime<Utc>) -> Duration {
    (time - Utc::now()).to_std().unwrap_or(Duration::ZERO)
}

impl Default for DeadlockPatches {
    fn default() -> Self {
        Self::new()
    }
}

impl DeadlockPatches {
    pub fn new() -> Self {
        Self::with_endpoints(
            STEAM_NEWS_URL_DEFAULT,
            SteamWebApiLedger::from_env(LEDGER_CALLER),
        )
    }

    pub fn with_endpoints(news_url: &str, ledger: SteamWebApiLedger) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok();
        Self {
            news_url: news_url.to_string(),
            cache: Mutex::new(PatchCache::default()),
            refresh: tokio::sync::Mutex::new(()),
            ledger,
            http,
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, PatchCache> {
        self.cache.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn refresh_due(&self) -> bool {
        let cache = self.cache();
        let fresh = cache.latest.is_some() && cache.refreshed_at.is_some_and(|t| t.elapsed() < TTL);
        !fresh && cache.next_attempt.is_none_or(|t| Instant::now() >= t)
    }

    fn defer(&self, wait: Duration) {
        let at = Instant::now() + wait.max(RETRY_FLOOR);
        let mut cache = self.cache();
        cache.next_attempt = Some(cache.next_attempt.map_or(at, |t| t.max(at)));
    }

    async fn fetch_latest_patch(&self, reservation_id: i64) -> (Observation, Fetch) {
        let Some(http) = &self.http else {
            return (Observation::dispatched(reservation_id), Fetch::Failed);
        };
        let appid = APPID.to_string();
        let sent = http
            .get(&self.news_url)
            .query(&[
                ("appid", appid.as_str()),
                ("count", "15"),
                ("maxlength", "0"),
                ("format", "json"),
            ])
            .send()
            .await;
        let Ok(response) = sent else {
            return (Observation::dispatched(reservation_id), Fetch::Failed);
        };
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let observation =
            Observation::from_response(reservation_id, status.as_u16(), retry_after.as_deref());
        if !status.is_success() {
            return (observation, Fetch::Failed);
        }
        match response.json::<Value>().await {
            Ok(data) => (observation, Fetch::Patch(parse_latest_patch(&data))),
            Err(_) => (observation, Fetch::Failed),
        }
    }

    async fn ensure_latest(&self) {
        let _refresh = self.refresh.lock().await;
        match self.ledger.settle_pending().await {
            Ok(Settlement::Clear) => {}
            Ok(Settlement::Confirmed(cooldown)) => {
                if let Some(cooldown) = cooldown.filter(|c| *c > Utc::now()) {
                    self.defer(until(cooldown));
                    return;
                }
            }
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "DeadlockPatches: offene Steam-Beobachtung unbestätigt, Abruf gesperrt"
                );
                self.defer(FAILURE_BACKOFF);
                return;
            }
        }
        if !self.refresh_due() {
            return;
        }
        if self.http.is_none() {
            tracing::warn!("DeadlockPatches: Steam-HTTP-Client nicht verfügbar, kein Abruf");
            self.defer(FAILURE_BACKOFF);
            return;
        }
        let reservation_id = match self.ledger.reserve().await {
            Ok(Reservation::Granted(id)) => id,
            Ok(Reservation::Denied(retry_at)) => {
                self.defer(until(retry_at));
                return;
            }
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "DeadlockPatches: Steam-Kontingent nicht erreichbar, kein Abruf"
                );
                self.defer(FAILURE_BACKOFF);
                return;
            }
        };
        if let Err(error) = self.ledger.record_dispatch(reservation_id).await {
            tracing::warn!(
                ?error,
                "DeadlockPatches: Steam-Beobachtung nicht speicherbar, kein Abruf"
            );
            self.defer(FAILURE_BACKOFF);
            return;
        }
        let (observation, fetched) = self.fetch_latest_patch(reservation_id).await;
        if let Err(error) = self.ledger.record_pending(&observation).await {
            tracing::warn!(
                ?error,
                "DeadlockPatches: Steam-Beobachtung konnte nicht aktualisiert werden"
            );
        }
        let settled = self.ledger.settle(&observation).await;
        match settled {
            Ok(Settlement::Confirmed(cooldown)) => {
                if let Some(cooldown) = cooldown.filter(|c| *c > Utc::now()) {
                    self.defer(until(cooldown));
                }
            }
            Ok(Settlement::Clear) => {}
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "DeadlockPatches: Steam-Beobachtung unbestätigt, weitere Abrufe gesperrt"
                );
                self.defer(FAILURE_BACKOFF);
                return;
            }
        }
        match fetched {
            Fetch::Patch(Some(patch)) => {
                let mut cache = self.cache();
                cache.latest = Some(patch);
                cache.refreshed_at = Some(Instant::now());
            }
            Fetch::Patch(None) => self.defer(TTL),
            Fetch::Failed => {
                tracing::warn!("DeadlockPatches: Patch-Fetch fehlgeschlagen");
                self.defer(FAILURE_BACKOFF);
            }
        }
    }

    fn latest_snapshot(&self) -> Option<LatestPatch> {
        self.cache().latest.clone()
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
    use wiremock::matchers::{body_json, header, method, path};
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

    const PATCH_BODY: &str =
        "[list][*]Haze generft[*]Bebop gebufft[*]Drei lang[*]Vier lang[*]Fuenf lang[/list]";
    const FUTURE: &str = "2099-01-01T00:00:00Z";

    fn news(title: &str) -> Value {
        json!({"appnews": {"newsitems": [{"title": title, "contents": PATCH_BODY}]}})
    }

    fn ledger(server: &MockServer, pending: &std::path::Path) -> SteamWebApiLedger {
        SteamWebApiLedger::new(
            &server.uri(),
            Some("k".to_string()),
            "twitch_engagement_patches",
            pending,
        )
    }

    fn patches_for(server: &MockServer, pending: &std::path::Path) -> DeadlockPatches {
        DeadlockPatches::with_endpoints(&format!("{}/news", server.uri()), ledger(server, pending))
    }

    async fn mount_reserve(server: &MockServer, response: ResponseTemplate, times: u64) {
        Mock::given(method("POST"))
            .and(path("/steam-web-api/reserve"))
            .and(header("X-Internal-Token", "k"))
            .and(body_json(
                json!({"caller": "twitch_engagement_patches", "caller_class": "standard"}),
            ))
            .respond_with(response)
            .expect(times)
            .mount(server)
            .await;
    }

    fn granted(id: i64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(
            json!({"ok": true, "granted": true, "reservation_id": id, "reserved_at": FUTURE}),
        )
    }

    fn observed(cooldown: Option<String>) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "response_at": FUTURE, "cooldown_until": cooldown, "duplicate": false
        }))
    }

    async fn mount_observe(
        server: &MockServer,
        body: Value,
        response: ResponseTemplate,
        times: u64,
    ) {
        Mock::given(method("POST"))
            .and(path("/steam-web-api/observe"))
            .and(header("X-Internal-Token", "k"))
            .and(body_json(body))
            .respond_with(response)
            .expect(times)
            .mount(server)
            .await;
    }

    async fn mount_news(server: &MockServer, response: ResponseTemplate, times: u64) {
        Mock::given(method("GET"))
            .and(path("/news"))
            .respond_with(response)
            .expect(times)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn digest_nur_bei_patch_talk() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        mount_reserve(&server, granted(1), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 1, "http_status": 200}),
            observed(None),
            1,
        )
        .await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(news("Patch X")),
            1,
        )
        .await;
        let patches = patches_for(&server, &pending);

        assert_eq!(
            patches.get_patch_digest_fragment("hallo zusammen").await,
            ""
        );
        let frag = patches
            .get_patch_digest_fragment("wie ist die neue meta")
            .await;
        assert!(frag.contains("Patch X"));
        assert!(frag.contains("- Haze generft"));
        let again = patches.get_patch_digest_fragment("und der patch?").await;
        assert!(again.contains("Patch X"));
        assert!(!pending.exists());
    }

    #[tokio::test]
    async fn gleichzeitige_cache_misses_reservieren_einmal() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        mount_reserve(&server, granted(2), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 2, "http_status": 200}),
            observed(None),
            1,
        )
        .await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(news("Patch Y")),
            1,
        )
        .await;
        let patches = patches_for(&server, &dir.path().join("pending.json"));
        let (a, b) = tokio::join!(
            patches.get_patch_digest_fragment("meta?"),
            patches.get_patch_digest_fragment("patch?")
        );
        assert!(a.contains("Patch Y") && b.contains("Patch Y"));
    }

    #[tokio::test]
    async fn ablehnung_verhindert_abruf_bis_retry_at() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let denied = ResponseTemplate::new(200).set_body_json(
            json!({"ok": true, "granted": false, "reason": "cooldown", "retry_at": FUTURE}),
        );
        mount_reserve(&server, denied, 1).await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(news("Patch")),
            0,
        )
        .await;
        let patches = patches_for(&server, &dir.path().join("pending.json"));
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert_eq!(patches.get_patch_digest_fragment("patch?").await, "");
        assert!(patches.cache().next_attempt.is_some());
    }

    #[tokio::test]
    async fn kontingent_fehler_fuehren_nie_zum_abruf() {
        let fehler = [
            ResponseTemplate::new(401).set_body_json(json!({"ok": false, "error": "key_invalid"})),
            ResponseTemplate::new(403).set_body_json(json!({"ok": false, "error": "key_missing"})),
            ResponseTemplate::new(503)
                .set_body_json(json!({"ok": false, "error": "ledger_unavailable"})),
            ResponseTemplate::new(404),
            ResponseTemplate::new(200).set_body_string("kein json"),
            ResponseTemplate::new(200).set_body_json(json!({"ok": true, "granted": true})),
            ResponseTemplate::new(200).set_body_json(json!({"ok": true, "granted": false})),
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": false, "granted": true, "reservation_id": 1})),
            ResponseTemplate::new(200).set_delay(Duration::from_secs(8)),
        ];
        for antwort in fehler {
            let server = MockServer::start().await;
            let dir = tempfile::tempdir().unwrap();
            mount_reserve(&server, antwort, 1).await;
            mount_news(
                &server,
                ResponseTemplate::new(200).set_body_json(news("Patch")),
                0,
            )
            .await;
            let patches = patches_for(&server, &dir.path().join("pending.json"));
            assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
            assert_eq!(patches.get_patch_digest_fragment("patch?").await, "");
        }
    }

    #[tokio::test]
    async fn ohne_schluessel_kein_abruf() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let patches = DeadlockPatches::with_endpoints(
            &format!("{}/news", server.uri()),
            SteamWebApiLedger::new(
                &server.uri(),
                None,
                "twitch_engagement_patches",
                dir.path().join("p.json"),
            ),
        );
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn steam_429_meldet_retry_after_und_haelt_cooldown() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        mount_reserve(&server, granted(5), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 5, "http_status": 429, "retry_after": "90"}),
            observed(Some(FUTURE.to_string())),
            1,
        )
        .await;
        mount_news(
            &server,
            ResponseTemplate::new(429).insert_header("Retry-After", "90"),
            1,
        )
        .await;
        let patches = patches_for(&server, &dir.path().join("pending.json"));
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert_eq!(patches.get_patch_digest_fragment("patch?").await, "");
        assert!(patches.cache().next_attempt.is_some());
    }

    #[tokio::test]
    async fn transportfehler_meldet_null_status() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        mount_reserve(&server, granted(6), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 6, "http_status": null}),
            observed(None),
            1,
        )
        .await;
        let patches = DeadlockPatches::with_endpoints(
            "http://127.0.0.1:1/news",
            ledger(&server, &dir.path().join("p.json")),
        );
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert_eq!(patches.get_patch_digest_fragment("patch?").await, "");
    }

    #[tokio::test]
    async fn verlorene_beobachtung_sperrt_auch_nach_neustart() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("state/pending.json");
        mount_reserve(&server, granted(7), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 7, "http_status": 503}),
            ResponseTemplate::new(503),
            2,
        )
        .await;
        mount_news(&server, ResponseTemplate::new(503), 1).await;
        assert_eq!(
            patches_for(&server, &pending)
                .get_patch_digest_fragment("meta?")
                .await,
            ""
        );
        assert!(pending.exists());

        let neu = patches_for(&server, &pending);
        assert_eq!(neu.get_patch_digest_fragment("patch?").await, "");
        assert!(pending.exists());
        server.verify().await;
        server.reset().await;

        mount_observe(
            &server,
            json!({"reservation_id": 7, "http_status": 503}),
            observed(None),
            1,
        )
        .await;
        mount_reserve(&server, granted(8), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 8, "http_status": 200}),
            observed(None),
            1,
        )
        .await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(news("Patch Z")),
            1,
        )
        .await;
        let spaeter = patches_for(&server, &pending);
        assert!(spaeter
            .get_patch_digest_fragment("meta?")
            .await
            .contains("Patch Z"));
        assert!(!pending.exists());
    }

    #[tokio::test]
    async fn konflikt_sperrt_weitere_abrufe() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        std::fs::write(
            &pending,
            r#"{"reservation_id":9,"http_status":null,"ready":true}"#,
        )
        .unwrap();
        mount_observe(
            &server,
            json!({"reservation_id": 9, "http_status": null}),
            ResponseTemplate::new(409)
                .set_body_json(json!({"ok": false, "error": "conflicting_report"})),
            2,
        )
        .await;
        mount_reserve(&server, granted(10), 0).await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(news("Patch K")),
            0,
        )
        .await;
        let patches = patches_for(&server, &pending);
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert_eq!(patches.get_patch_digest_fragment("patch?").await, "");
        assert!(pending.exists());
        server.verify().await;
    }

    #[tokio::test]
    async fn nicht_speicherbare_reservierung_sendet_nichts_an_steam() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        std::fs::create_dir(dir.path().join("pending.tmp")).unwrap();
        mount_reserve(&server, granted(15), 1).await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(news("Patch")),
            0,
        )
        .await;
        let patches = patches_for(&server, &pending);
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        server.verify().await;
    }

    #[tokio::test]
    async fn neustart_mit_unbekanntem_request_status_sperrt() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        ledger(&server, &pending).record_dispatch(13).await.unwrap();
        let patches = patches_for(&server, &pending);
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert!(server.received_requests().await.unwrap().is_empty());
        assert!(pending.exists());
    }

    #[tokio::test]
    async fn patch_bleibt_ohne_beobachtungsbestaetigung_aus() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        mount_reserve(&server, granted(14), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 14, "http_status": 200}),
            ResponseTemplate::new(503),
            1,
        )
        .await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(news("Unbestätigt")),
            1,
        )
        .await;
        let patches = patches_for(&server, &pending);
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert!(patches.cache().latest.is_none());
        assert!(pending.exists());
    }

    #[tokio::test]
    async fn beschaedigte_offene_beobachtung_sperrt() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        std::fs::write(&pending, "kaputt").unwrap();
        let patches = patches_for(&server, &pending);
        assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn fehler_frischt_alten_patch_nicht_auf_und_wartet() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        mount_reserve(&server, granted(11), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 11, "http_status": 500}),
            observed(None),
            1,
        )
        .await;
        mount_news(&server, ResponseTemplate::new(500), 1).await;
        let patches = patches_for(&server, &dir.path().join("pending.json"));
        let alt = Instant::now()
            .checked_sub(TTL + Duration::from_secs(1))
            .unwrap();
        {
            let mut cache = patches.cache();
            cache.latest = Some(LatestPatch {
                title: "Alter Patch".into(),
                date: None,
                lines: vec!["Haze alt".into()],
            });
            cache.refreshed_at = Some(alt);
        }
        assert!(patches
            .get_patch_digest_fragment("meta?")
            .await
            .contains("Alter Patch"));
        assert!(patches
            .get_patch_digest_fragment("patch?")
            .await
            .contains("Alter Patch"));
        let cache = patches.cache();
        assert_eq!(cache.refreshed_at, Some(alt));
        assert!(cache.next_attempt.unwrap() >= Instant::now() + Duration::from_secs(800));
    }

    #[tokio::test]
    async fn leere_antwort_erzeugt_keine_schleife() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        mount_reserve(&server, granted(12), 1).await;
        mount_observe(
            &server,
            json!({"reservation_id": 12, "http_status": 200}),
            observed(None),
            1,
        )
        .await;
        mount_news(
            &server,
            ResponseTemplate::new(200).set_body_json(json!({"appnews": {"newsitems": []}})),
            1,
        )
        .await;
        let patches = patches_for(&server, &dir.path().join("pending.json"));
        for _ in 0..3 {
            assert_eq!(patches.get_patch_digest_fragment("meta?").await, "");
        }
    }
}
