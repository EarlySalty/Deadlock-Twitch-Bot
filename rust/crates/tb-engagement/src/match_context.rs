//! Live-Match-Kontext (Deadlock) für den Engagement-Layer (Port von
//! `bot/engagement/match_context.py`).
//!
//! Die Pipeline liest den Snapshot synchron aus `twitch_channel_match_state`
//! ([`MatchContext::get_match_state`]) und hängt einen kurzen „Streamer spielt
//! aktuell X"-Hint in den System-Prompt. Der Hintergrund-Poll (API → DB,
//! `poll_match_state`) folgt in 12b.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

const DEADLOCK_API_BASE: &str = "https://api.deadlock-api.com/v1";
const ASSETS_API_BASE: &str = "https://assets.deadlock-api.com";
const HERO_TTL: Duration = Duration::from_secs(6 * 3600);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LIVE_MAX_AGE_SEC: i64 = 90 * 60;

/// Snapshot des aktuellen Match-States eines Channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchSnapshot {
    pub channel_login: String,
    pub hero_id: Option<i64>,
    pub hero_name: Option<String>,
    pub match_id: Option<String>,
    pub match_started_at: Option<DateTime<Utc>>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub is_live: bool,
}

impl MatchSnapshot {
    /// Prompt-Hint, wenn der Streamer gerade spielt; sonst "".
    pub fn to_prompt_fragment(&self) -> String {
        self.fragment_at(Utc::now())
    }

    /// Wie [`Self::to_prompt_fragment`], aber mit explizitem „jetzt" (testbar).
    fn fragment_at(&self, now: DateTime<Utc>) -> String {
        if !self.is_live {
            return String::new();
        }
        let hero = match self.hero_name.as_deref().filter(|n| !n.is_empty()) {
            Some(name) => name.to_string(),
            None => match self.hero_id {
                Some(id) => format!("Hero #{id}"),
                None => "einem unbekannten Hero".to_string(),
            },
        };
        let duration = match self.match_started_at {
            Some(started) => {
                let elapsed_min = (now - started).num_seconds() / 60;
                format!(" Match läuft seit ~{elapsed_min} Min.")
            }
            None => String::new(),
        };
        format!("Streamer spielt aktuell {hero}.{duration}")
    }
}

/// Match-Kontext-Provider mit Hero-Cache (6h) und injizierbaren API-Basis-URLs.
pub struct MatchContext {
    pool: PgPool,
    deadlock_base: String,
    assets_base: String,
    hero_cache: Mutex<(Option<Instant>, HashMap<i64, String>)>,
    http: reqwest::Client,
}

impl MatchContext {
    pub fn new(pool: PgPool) -> Self {
        Self::with_bases(pool, DEADLOCK_API_BASE, ASSETS_API_BASE)
    }

    /// Mit expliziten API-Basis-URLs (Tests).
    pub fn with_bases(pool: PgPool, deadlock_base: &str, assets_base: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            pool,
            deadlock_base: deadlock_base.trim_end_matches('/').to_string(),
            assets_base: assets_base.trim_end_matches('/').to_string(),
            hero_cache: Mutex::new((None, HashMap::new())),
            http,
        }
    }

    /// Lädt den Match-Snapshot eines Channels aus der DB (oder None).
    pub async fn get_match_state(&self, channel_login: &str) -> Option<MatchSnapshot> {
        let row = sqlx::query!(
            r#"SELECT channel_login AS "channel_login!", hero_id, hero_name, match_id,
                    match_started_at, last_synced_at AS "last_synced_at?", is_live AS "is_live!"
             FROM twitch_channel_match_state WHERE channel_login = $1"#,
            channel_login
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;
        Some(MatchSnapshot {
            channel_login: row.channel_login,
            hero_id: row.hero_id.map(i64::from),
            hero_name: row.hero_name,
            match_id: row.match_id.filter(|s| !s.is_empty()),
            match_started_at: row.match_started_at,
            last_synced_at: row.last_synced_at,
            is_live: row.is_live,
        })
    }

    async fn fetch_last_match(&self, steam_id: &str) -> Option<Value> {
        let url = format!("{}/players/{}/match-history", self.deadlock_base, steam_id);
        let data: Value = self
            .http
            .get(&url)
            .query(&[("limit", "1")])
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        data.as_array()?.first().filter(|v| v.is_object()).cloned()
    }

    async fn fetch_heroes(&self) -> HashMap<i64, String> {
        let url = format!("{}/v2/heroes", self.assets_base);
        let resp = self
            .http
            .get(&url)
            .query(&[("only_active", "true")])
            .send()
            .await;
        let data: Value = match resp.and_then(reqwest::Response::error_for_status) {
            Ok(r) => match r.json().await {
                Ok(d) => d,
                Err(_) => return HashMap::new(),
            },
            Err(_) => {
                tracing::warn!("MatchContext: Hero-Liste konnte nicht geladen werden");
                return HashMap::new();
            }
        };
        let mut out = HashMap::new();
        if let Some(arr) = data.as_array() {
            for item in arr {
                let hid = item.get("id").and_then(as_i64_flex);
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("display_name").and_then(Value::as_str));
                if let (Some(hid), Some(name)) = (hid, name) {
                    if !name.is_empty() {
                        out.insert(hid, name.to_string());
                    }
                }
            }
        }
        out
    }

    async fn ensure_hero_cache(&self) -> HashMap<i64, String> {
        {
            let cache = self.hero_cache.lock().unwrap_or_else(|p| p.into_inner());
            if !cache.1.is_empty() && cache.0.is_some_and(|t| t.elapsed() < HERO_TTL) {
                return cache.1.clone();
            }
        }
        let fresh = self.fetch_heroes().await;
        if !fresh.is_empty() {
            let mut cache = self.hero_cache.lock().unwrap_or_else(|p| p.into_inner());
            cache.0 = Some(Instant::now());
            cache.1 = fresh.clone();
            return fresh;
        }
        self.hero_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .1
            .clone()
    }

    async fn upsert_match_state(
        &self,
        channel_login: &str,
        m: &ExtractedMatch,
        hero_name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO twitch_channel_match_state \
             (channel_login, hero_id, hero_name, match_id, match_started_at, last_synced_at, is_live) \
             VALUES ($1, $2, $3, $4, $5, NOW(), $6) \
             ON CONFLICT (channel_login) DO UPDATE SET \
               hero_id = EXCLUDED.hero_id, hero_name = EXCLUDED.hero_name, \
               match_id = EXCLUDED.match_id, match_started_at = EXCLUDED.match_started_at, \
               last_synced_at = NOW(), is_live = EXCLUDED.is_live",
            channel_login,
            m.hero_id.map(|h| h as i32),
            hero_name,
            m.match_id.as_deref(),
            m.match_started_at,
            m.is_live
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Pollt die deadlock-api für das aktuelle Match, persistiert es und liefert
    /// den Snapshot. Kein Steam-ID/kein Match → bestehender Snapshot bleibt.
    pub async fn poll_match_state(
        &self,
        channel_login: &str,
        steam_id: &str,
    ) -> Option<MatchSnapshot> {
        if steam_id.is_empty() {
            return None;
        }
        let item = match self.fetch_last_match(steam_id).await {
            Some(i) => i,
            None => return self.get_match_state(channel_login).await,
        };
        let extracted = extract_match_fields(&item, Utc::now());
        let hero_name = match extracted.hero_id {
            Some(id) => self.ensure_hero_cache().await.get(&id).cloned(),
            None => None,
        };
        if let Err(error) = self
            .upsert_match_state(channel_login, &extracted, hero_name.as_deref())
            .await
        {
            tracing::warn!(
                %error,
                channel = %channel_login,
                "Match-State konnte nicht gespeichert werden"
            );
        }
        self.get_match_state(channel_login).await
    }
}

/// Aus einem Match-History-Item extrahierte Felder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtractedMatch {
    hero_id: Option<i64>,
    match_id: Option<String>,
    match_started_at: Option<DateTime<Utc>>,
    is_live: bool,
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

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        other => other.to_string(),
    }
}

/// Erstes Feld mit „truthy" Wert aus der Schlüssel-Liste (Python `a or b or …`).
fn first_truthy<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|k| item.get(*k).filter(|v| json_truthy(v)))
}

/// `_parse_ts`: Unix-Int/Float → Zeitstempel, ISO-String → rfc3339.
fn parse_ts(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::Number(n) => {
            let secs = n.as_i64().or_else(|| n.as_f64().map(|f| f as i64))?;
            DateTime::from_timestamp(secs, 0)
        }
        Value::String(s) => DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc)),
        _ => None,
    }
}

/// Reine Extraktion + `is_live`-Heuristik aus einem Match-Item (Python-Logik in
/// `poll_match_state`): live, wenn Start da, KEIN End-Feld, KEINE Dauer und
/// 0 < Alter < 90 Min.
fn extract_match_fields(item: &Value, now: DateTime<Utc>) -> ExtractedMatch {
    let hero_id = item.get("hero_id").and_then(as_i64_flex);
    let match_id = item
        .get("match_id")
        .filter(|v| !v.is_null())
        .map(value_to_string);
    let match_started_at = first_truthy(
        item,
        &["start_time", "match_start", "started_at", "start_time_iso"],
    )
    .and_then(parse_ts);
    let end_present =
        first_truthy(item, &["end_time", "match_end", "ended_at", "end_time_iso"]).is_some();
    let has_duration = first_truthy(item, &["duration_s", "duration"]).is_some();

    let is_live = match match_started_at {
        Some(started) if !end_present && !has_duration => {
            let age = (now - started).num_seconds();
            age > 0 && age < LIVE_MAX_AGE_SEC
        }
        _ => false,
    };
    ExtractedMatch {
        hero_id,
        match_id,
        match_started_at,
        is_live,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn snap(
        is_live: bool,
        hero_name: Option<&str>,
        hero_id: Option<i64>,
        started: Option<DateTime<Utc>>,
    ) -> MatchSnapshot {
        MatchSnapshot {
            channel_login: "nani".to_string(),
            hero_id,
            hero_name: hero_name.map(str::to_string),
            match_id: None,
            match_started_at: started,
            last_synced_at: None,
            is_live,
        }
    }

    #[test]
    fn fragment_varianten() {
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        // nicht live → leer
        assert_eq!(snap(false, Some("Haze"), None, None).fragment_at(now), "");
        // live mit Name + Dauer
        let s = snap(
            true,
            Some("Haze"),
            Some(5),
            Some(now - Duration::minutes(30)),
        );
        assert_eq!(
            s.fragment_at(now),
            "Streamer spielt aktuell Haze. Match läuft seit ~30 Min."
        );
        // live ohne Name, mit id
        let s2 = snap(true, None, Some(7), None);
        assert_eq!(s2.fragment_at(now), "Streamer spielt aktuell Hero #7.");
        // live ohne Name + ohne id
        let s3 = snap(true, None, None, None);
        assert_eq!(
            s3.fragment_at(now),
            "Streamer spielt aktuell einem unbekannten Hero."
        );
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
            "CREATE TABLE twitch_channel_match_state (\
             channel_login TEXT PRIMARY KEY, hero_id INT, hero_name TEXT, match_id TEXT, \
             match_started_at TIMESTAMPTZ, last_synced_at TIMESTAMPTZ NOT NULL, \
             is_live BOOLEAN NOT NULL DEFAULT FALSE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn get_match_state_aus_db() {
        let Some(pool) = make_pool("t_eng_match").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_channel_match_state \
             (channel_login, hero_id, hero_name, match_id, match_started_at, last_synced_at, is_live) \
             VALUES ('nani', 5, 'Haze', 'm1', NOW(), NOW(), TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let ctx = MatchContext::new(pool);
        let s = ctx.get_match_state("nani").await.unwrap();
        assert_eq!(s.hero_id, Some(5));
        assert_eq!(s.hero_name.as_deref(), Some("Haze"));
        assert!(s.is_live);
        // unbekannter Channel → None
        assert!(ctx.get_match_state("other").await.is_none());
    }

    #[test]
    fn parse_ts_int_und_iso() {
        assert_eq!(
            parse_ts(&json!(1_700_000_000)),
            DateTime::from_timestamp(1_700_000_000, 0)
        );
        assert!(parse_ts(&json!("2021-05-01T00:00:00Z")).is_some());
        assert_eq!(parse_ts(&json!("garbage")), None);
        assert_eq!(parse_ts(&json!(true)), None);
    }

    #[test]
    fn extract_is_live_heuristik() {
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let recent = 1_700_000_000 - 600; // 10 Min vor now
                                          // live: Start da, kein End, keine Dauer.
        let e = extract_match_fields(
            &json!({"hero_id": 5, "match_id": "m1", "start_time": recent}),
            now,
        );
        assert!(e.is_live);
        assert_eq!(e.hero_id, Some(5));
        assert_eq!(e.match_id.as_deref(), Some("m1"));
        // Dauer gesetzt → nicht live.
        assert!(
            !extract_match_fields(&json!({"start_time": recent, "duration_s": 1800}), now).is_live
        );
        // End gesetzt → nicht live.
        assert!(
            !extract_match_fields(
                &json!({"start_time": recent, "end_time": recent + 1800}),
                now
            )
            .is_live
        );
        // zu alt (> 90 Min) → nicht live.
        assert!(!extract_match_fields(&json!({"start_time": 1_700_000_000 - 7000}), now).is_live);
    }

    #[tokio::test]
    async fn poll_setzt_live_und_hero() {
        let Some(pool) = make_pool("t_eng_match_poll").await else {
            return;
        };
        let dl = MockServer::start().await;
        let recent = Utc::now().timestamp() - 600;
        Mock::given(method("GET"))
            .and(path("/players/76561/match-history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"hero_id": 5, "match_id": "m1", "start_time": recent}
            ])))
            .mount(&dl)
            .await;
        let assets = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/heroes"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{"id": 5, "name": "Haze"}])),
            )
            .mount(&assets)
            .await;

        let ctx = MatchContext::with_bases(pool.clone(), &dl.uri(), &assets.uri());
        let snap = ctx.poll_match_state("nani", "76561").await.unwrap();
        assert!(snap.is_live);
        assert_eq!(snap.hero_id, Some(5));
        assert_eq!(snap.hero_name.as_deref(), Some("Haze"));
    }
}
