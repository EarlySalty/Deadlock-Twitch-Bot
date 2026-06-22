//! Public OBS overlay for Deadlock live stats.
//!
//! Rang-/Hero-Bilder sind Deadlock-Spiel-Assets (© Valve), geladen über die
//! öffentliche deadlock-api Assets-CDN; dieser Code nutzt nur Asset-URLs und
//! keinen fremden Streamkit-Code.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Json,
};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::{Mutex, Notify};

const DEFAULT_STEAM_BOT_BASE_URL: &str = "http://127.0.0.1:8783";
const OVERLAY_CACHE_TTL: Duration = Duration::from_secs(30);
const STEAM_BOT_TIMEOUT: Duration = Duration::from_secs(8);

static OVERLAY_CACHE: OnceLock<Mutex<OverlayCache>> = OnceLock::new();

#[derive(Default)]
struct OverlayCache {
    entries: HashMap<String, CacheEntry>,
    inflight: HashMap<String, Arc<Notify>>,
}

#[derive(Clone)]
struct CacheEntry {
    inserted_at: Instant,
    body: Value,
}

#[derive(Deserialize, Default)]
pub struct OverlayQuery {
    #[serde(default)]
    streamer: Option<String>,
}

#[derive(Serialize)]
struct OverlayResponse {
    ok: bool,
    streamer: String,
    rank_name: Option<String>,
    badge_level: Option<i64>,
    delta: Option<i64>,
    wins: Option<i64>,
    losses: Option<i64>,
    winrate: Option<f64>,
    streak_kind: Option<String>,
    streak_len: Option<i64>,
    live: bool,
    hero: Option<String>,
    minutes: Option<i64>,
}

#[derive(Deserialize)]
struct SteamMmrTrend {
    #[serde(default)]
    linked: Option<bool>,
    #[serde(default, alias = "current_rank_name")]
    rank_name: Option<String>,
    #[serde(default)]
    current_badge: Option<i64>,
    #[serde(default)]
    delta: Option<i64>,
}

#[derive(Deserialize)]
struct SteamMatchHistory {
    #[serde(default)]
    linked: Option<bool>,
    #[serde(default)]
    matches: Vec<SteamMatch>,
}

#[derive(Deserialize)]
struct SteamMatch {
    #[serde(default)]
    match_result: Option<i64>,
    #[serde(default)]
    not_scored: Option<bool>,
}

#[derive(Deserialize)]
struct SteamLiveStatus {
    #[serde(default)]
    linked: Option<bool>,
    #[serde(default)]
    live: bool,
    #[serde(default)]
    hero: Option<String>,
    #[serde(default)]
    minutes: Option<i64>,
}

struct MatchSummary {
    wins: i64,
    losses: i64,
    winrate: f64,
    streak_kind: String,
    streak_len: i64,
}

fn overlay_cache() -> &'static Mutex<OverlayCache> {
    OVERLAY_CACHE.get_or_init(|| Mutex::new(OverlayCache::default()))
}

fn ok_false() -> Value {
    json!({ "ok": false })
}

fn normalize_login(streamer: Option<&str>) -> Option<String> {
    streamer
        .map(str::trim)
        .filter(|login| !login.is_empty())
        .map(str::to_lowercase)
}

async fn cached_overlay_or_fetch(pool: &PgPool, login: &str) -> Value {
    loop {
        let notify = {
            let mut cache = overlay_cache().lock().await;
            let now = Instant::now();
            if let Some(entry) = cache.entries.get(login) {
                if now.duration_since(entry.inserted_at) < OVERLAY_CACHE_TTL {
                    return entry.body.clone();
                }
            }

            if let Some(notify) = cache.inflight.get(login) {
                Some(Arc::clone(notify))
            } else {
                let notify = Arc::new(Notify::new());
                cache
                    .inflight
                    .insert(login.to_string(), Arc::clone(&notify));
                None
            }
        };

        let Some(notify) = notify else {
            break;
        };
        notify.notified().await;
    }

    let body = build_overlay_json(pool, login).await;
    let notify = {
        let mut cache = overlay_cache().lock().await;
        cache.entries.insert(
            login.to_string(),
            CacheEntry {
                inserted_at: Instant::now(),
                body: body.clone(),
            },
        );
        cache.inflight.remove(login)
    };
    if let Some(notify) = notify {
        notify.notify_waiters();
    }
    body
}

async fn build_overlay_json(pool: &PgPool, login: &str) -> Value {
    let discord_id = match resolve_discord_id(pool, login).await {
        Ok(Some(discord_id)) => discord_id,
        Ok(None) => return ok_false(),
        Err(e) => {
            tracing::error!("overlay streamer resolver fehlgeschlagen: {e}");
            return ok_false();
        }
    };

    let client = match Client::builder().timeout(STEAM_BOT_TIMEOUT).build() {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("overlay reqwest client konnte nicht gebaut werden: {e}");
            return ok_false();
        }
    };

    let trend_url = steam_bot_url("/player-mmr-trend");
    let matches_url = steam_bot_url("/player-matches");
    let live_url = steam_bot_url("/player-live");

    let trend_query = [("discord_id", discord_id.as_str()), ("days", "7")];
    let matches_query = [("discord_id", discord_id.as_str()), ("limit", "150")];
    let live_query = [("discord_id", discord_id.as_str())];

    let (trend, matches, live) = tokio::join!(
        fetch_steam_json::<SteamMmrTrend>(&client, &trend_url, &trend_query),
        fetch_steam_json::<SteamMatchHistory>(&client, &matches_url, &matches_query),
        fetch_steam_json::<SteamLiveStatus>(&client, &live_url, &live_query),
    );

    let trend = trend.filter(|value| value.linked != Some(false));
    let match_summary = matches
        .filter(|value| value.linked != Some(false))
        .and_then(|value| summarize_matches(&value.matches));
    let live = live.filter(|value| value.linked != Some(false));

    let response = OverlayResponse {
        ok: true,
        streamer: login.to_string(),
        rank_name: trend
            .as_ref()
            .and_then(|value| clean_string(&value.rank_name)),
        badge_level: trend.as_ref().and_then(|value| value.current_badge),
        delta: trend.as_ref().and_then(|value| value.delta),
        wins: match_summary.as_ref().map(|summary| summary.wins),
        losses: match_summary.as_ref().map(|summary| summary.losses),
        winrate: match_summary.as_ref().map(|summary| summary.winrate),
        streak_kind: match_summary
            .as_ref()
            .map(|summary| summary.streak_kind.clone()),
        streak_len: match_summary.as_ref().map(|summary| summary.streak_len),
        live: live.as_ref().map(|value| value.live).unwrap_or(false),
        hero: live.as_ref().and_then(|value| clean_string(&value.hero)),
        minutes: live.and_then(|value| value.minutes),
    };

    serde_json::to_value(response).unwrap_or_else(|_| ok_false())
}

async fn resolve_discord_id(pool: &PgPool, login: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT i.discord_user_id::text \
         FROM twitch_streamers s \
         JOIN twitch_streamer_identities i ON i.twitch_user_id = s.twitch_user_id \
         WHERE LOWER(s.twitch_login) = $1 \
           AND COALESCE(s.twitch_user_id, '') <> '' \
           AND COALESCE(i.discord_user_id::text, '') <> '' \
         LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;

    Ok(row
        .and_then(|(discord_id,)| discord_id)
        .map(|discord_id| discord_id.trim().to_string())
        .filter(|discord_id| !discord_id.is_empty()))
}

async fn fetch_steam_json<T>(client: &Client, url: &str, query: &[(&str, &str)]) -> Option<T>
where
    T: DeserializeOwned,
{
    let response = client.get(url).query(query).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<T>().await.ok()
}

fn steam_bot_url(path: &str) -> String {
    format!("{}{}", steam_bot_base_url(), path)
}

fn steam_bot_base_url() -> String {
    std::env::var("STEAM_BOT_RANK_URL")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim().trim_end_matches('/');
            if trimmed.is_empty() {
                None
            } else {
                Some(strip_endpoint_suffix(trimmed).to_string())
            }
        })
        .unwrap_or_else(|| DEFAULT_STEAM_BOT_BASE_URL.to_string())
}

fn strip_endpoint_suffix(value: &str) -> &str {
    for suffix in [
        "/rank",
        "/player-mmr-trend",
        "/player-matches",
        "/player-live",
    ] {
        if let Some(base) = value.strip_suffix(suffix) {
            return base.trim_end_matches('/');
        }
    }
    value
}

fn clean_string(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn summarize_matches(matches: &[SteamMatch]) -> Option<MatchSummary> {
    let scored: Vec<i64> = matches
        .iter()
        .filter(|entry| entry.not_scored != Some(true))
        .filter_map(|entry| match entry.match_result {
            Some(0 | 1) => entry.match_result,
            _ => None,
        })
        .collect();

    let first = *scored.first()?;
    let wins = scored.iter().filter(|result| **result == 1).count() as i64;
    let losses = scored.iter().filter(|result| **result == 0).count() as i64;
    let total = wins + losses;
    if total == 0 {
        return None;
    }

    let streak_len = scored.iter().take_while(|result| **result == first).count() as i64;
    let winrate = ((wins as f64 * 1000.0) / total as f64).round() / 10.0;
    let streak_kind = if first == 1 { "win" } else { "loss" }.to_string();

    Some(MatchSummary {
        wins,
        losses,
        winrate,
        streak_kind,
        streak_len,
    })
}

/// `GET /twitch/api/v2/public/overlay?streamer=<login>`
pub async fn overlay_api_handler(
    State(pool): State<PgPool>,
    Query(query): Query<OverlayQuery>,
) -> impl IntoResponse {
    let Some(login) = normalize_login(query.streamer.as_deref()) else {
        return (StatusCode::OK, Json(ok_false())).into_response();
    };

    let body = cached_overlay_or_fetch(&pool, &login).await;
    (StatusCode::OK, Json(body)).into_response()
}

/// `GET /twitch/overlay?streamer=<login>`
pub async fn overlay_html_handler() -> Html<&'static str> {
    Html(OVERLAY_HTML)
}

const OVERLAY_HTML: &str = r#"<!doctype html>
<html lang="de">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Deadlock Overlay</title>
  <style>
    html, body {
      margin: 0;
      width: 100%;
      height: 100%;
      background: transparent;
      overflow: hidden;
      font-family: Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }

    #overlay-card {
      position: fixed;
      width: 280px;
      box-sizing: border-box;
      display: none;
      padding: 14px 16px;
      border-radius: 8px;
      background: rgba(15, 15, 20, 0.78);
      color: #fff;
      border-left: 3px solid #7dd3fc;
      box-shadow: 0 14px 35px rgba(0, 0, 0, 0.34);
      backdrop-filter: blur(8px);
      font-size: 16px;
      line-height: 1.35;
      letter-spacing: 0;
    }

    #overlay-card.overlay-pos-bl {
      left: 16px;
      bottom: 16px;
    }

    #overlay-card.overlay-pos-br {
      right: 16px;
      bottom: 16px;
    }

    #overlay-card.overlay-pos-tl {
      left: 16px;
      top: 16px;
    }

    #overlay-card.overlay-pos-tr {
      right: 16px;
      top: 16px;
    }

    #overlay-card.visible {
      display: block;
      animation: overlay-fade 160ms ease-out;
    }

    .line {
      display: block;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      text-shadow: 0 1px 2px rgba(0, 0, 0, 0.55);
    }

    .asset-line {
      display: flex;
      align-items: center;
      gap: 8px;
    }

    .asset-line .text {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .rank-badge {
      width: 40px;
      height: 40px;
      flex: 0 0 auto;
      object-fit: contain;
    }

    .hero-icon {
      width: 28px;
      height: 28px;
      flex: 0 0 auto;
      border-radius: 999px;
      object-fit: contain;
      background: rgba(255, 255, 255, 0.08);
    }

    .line + .line {
      margin-top: 6px;
    }

    .live {
      color: #86efac;
      font-weight: 700;
    }

    @keyframes overlay-fade {
      from { opacity: 0; transform: translateY(4px); }
      to { opacity: 1; transform: translateY(0); }
    }
  </style>
</head>
<body>
  <!-- Rang-/Hero-Bilder sind Deadlock-Spiel-Assets (© Valve), geladen über die öffentliche deadlock-api Assets-CDN; nur Asset-URLs, kein fremder Streamkit-Code. -->
  <div id="overlay-card" aria-live="polite"></div>
  <script>
    const card = document.getElementById('overlay-card');
    const params = new URLSearchParams(window.location.search);
    const streamer = (params.get('streamer') || '').trim();
    const flags = {
      rank: params.get('rank') !== '0',
      winrate: params.get('winrate') !== '0',
      streak: params.get('streak') !== '0',
      live: params.get('live') !== '0',
    };
    const requestedPosition = params.get('pos') || 'bl';
    const position = ['bl', 'br', 'tl', 'tr'].includes(requestedPosition) ? requestedPosition : 'bl';
    card.classList.add(`overlay-pos-${position}`);
    let heroIconByName = new Map();
    let latestData = null;

    function isNumber(value) {
      return typeof value === 'number' && Number.isFinite(value);
    }

    function rankBadgeUrl(badgeLevel) {
      if (!Number.isInteger(badgeLevel)) return null;
      const tier = Math.floor(badgeLevel / 10);
      const sub = badgeLevel % 10;
      if (tier < 1 || tier > 12) return null;

      const base = `https://assets-bucket.deadlock-api.com/assets-api-res/images/ranks/rank${tier}`;
      if (sub >= 1 && sub <= 6) return `${base}/badge_lg_subrank${sub}.png`;
      if (sub === 0) return `${base}/badge_lg.png`;
      return null;
    }

    function heroIconUrl(heroName) {
      if (typeof heroName !== 'string') return null;
      return heroIconByName.get(heroName.trim().toLowerCase()) || null;
    }

    async function loadHeroAssets() {
      try {
        const response = await fetch('https://assets.deadlock-api.com/v2/heroes?only_active=true', { cache: 'force-cache' });
        if (!response.ok) return;
        const heroes = await response.json();
        if (!Array.isArray(heroes)) return;

        const next = new Map();
        for (const hero of heroes) {
          const name = typeof hero?.name === 'string' ? hero.name.trim().toLowerCase() : '';
          const images = hero?.images || {};
          const icon = images.icon_image_small || images.icon_image_small_webp;
          if (name && typeof icon === 'string' && icon.trim()) {
            next.set(name, icon.trim());
          }
        }

        heroIconByName = next;
        if (latestData) render(latestData);
      } catch (_) {
        heroIconByName = new Map();
      }
    }

    function hide() {
      latestData = null;
      card.classList.remove('visible');
      card.replaceChildren();
    }

    function line(text, className) {
      const node = document.createElement('div');
      node.className = className ? `line ${className}` : 'line';
      node.textContent = text;
      return node;
    }

    function assetLine(text, imageUrl, imageClassName, className) {
      const node = document.createElement('div');
      node.className = className ? `line asset-line ${className}` : 'line asset-line';

      if (imageUrl) {
        const image = document.createElement('img');
        image.className = imageClassName;
        image.src = imageUrl;
        image.alt = '';
        image.decoding = 'async';
        image.loading = 'lazy';
        image.onerror = () => image.remove();
        node.appendChild(image);
      }

      const label = document.createElement('span');
      label.className = 'text';
      label.textContent = text;
      node.appendChild(label);
      return node;
    }

    function render(data) {
      if (!data || data.ok !== true) {
        hide();
        return;
      }
      latestData = data;

      const rows = [];
      if (flags.rank && data.rank_name) {
        let text = `Rang: ${data.rank_name}`;
        if (isNumber(data.delta) && data.delta > 0) text += ' ▲';
        if (isNumber(data.delta) && data.delta < 0) text += ' ▼';
        rows.push(assetLine(text, rankBadgeUrl(data.badge_level), 'rank-badge'));
      }

      if (flags.winrate && isNumber(data.winrate) && isNumber(data.wins) && isNumber(data.losses)) {
        rows.push(line(`Winrate: ${data.winrate.toFixed(1)}% (${data.wins}S/${data.losses}N)`));
      }

      if (flags.streak && isNumber(data.streak_len) && data.streak_len >= 2) {
        if (data.streak_kind === 'win') {
          rows.push(line(`Serie: ${data.streak_len} Siege in Folge`));
        } else if (data.streak_kind === 'loss') {
          rows.push(line(`Serie: ${data.streak_len} Niederlagen in Folge`));
        }
      }

      if (flags.live && data.live === true) {
        const details = [];
        if (data.hero) details.push(data.hero);
        if (isNumber(data.minutes)) details.push(`${data.minutes}′`);
        const text = details.length ? `● LIVE — ${details.join(' · ')}` : '● LIVE';
        rows.push(assetLine(text, heroIconUrl(data.hero), 'hero-icon', 'live'));
      }

      if (rows.length === 0) {
        hide();
        return;
      }

      card.replaceChildren(...rows);
      card.classList.add('visible');
    }

    async function poll() {
      if (!streamer) {
        hide();
        return;
      }
      try {
        const response = await fetch(`/twitch/api/v2/public/overlay?streamer=${encodeURIComponent(streamer)}`, { cache: 'no-store' });
        if (!response.ok) {
          hide();
          return;
        }
        render(await response.json());
      } catch (_) {
        hide();
      }
    }

    loadHeroAssets();
    poll();
    setInterval(poll, 20000);
  </script>
</body>
</html>
"#;

#[cfg(test)]
async fn clear_overlay_cache_for_tests() {
    let mut cache = overlay_cache().lock().await;
    cache.entries.clear();
    cache.inflight.clear();
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use axum::body::to_bytes;
    use axum::http::{header, Request, StatusCode};
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::build_public_router;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvGuard {
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var("STEAM_BOT_RANK_URL").ok();
            std::env::set_var("STEAM_BOT_RANK_URL", value);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("STEAM_BOT_RANK_URL", previous);
            } else {
                std::env::remove_var("STEAM_BOT_RANK_URL");
            }
        }
    }

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> sqlx::PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");
        sqlx::query(
            "CREATE TABLE twitch_streamers (\
             twitch_login TEXT NOT NULL, \
             twitch_user_id TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers");
        sqlx::query(
            "CREATE TABLE twitch_streamer_identities (\
             twitch_user_id TEXT NOT NULL, \
             discord_user_id TEXT)",
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamer_identities");
        pool
    }

    async fn get_json(app: axum::Router, uri: &str) -> Value {
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn overlay_api_cache_hit_innerhalb_ttl_nutzt_keinen_zweiten_steam_abruf() {
        let dsn = db_dsn_or_skip!();
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
        super::clear_overlay_cache_for_tests().await;
        let mock_server = MockServer::start().await;
        let _env = EnvGuard::set(&mock_server.uri());
        let pool = make_pool(&dsn, "api_overlay_cache").await;

        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('StreamerX', 'tw1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, discord_user_id) \
             VALUES ('tw1', '4242')",
        )
        .execute(&pool)
        .await
        .unwrap();

        Mock::given(method("GET"))
            .and(path("/player-mmr-trend"))
            .and(query_param("discord_id", "4242"))
            .and(query_param("days", "7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "linked": true,
                "current_rank_name": "Oracle",
                "current_badge": 53,
                "delta": 3
            })))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/player-matches"))
            .and(query_param("discord_id", "4242"))
            .and(query_param("limit", "150"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "linked": true,
                "matches": [
                    { "match_result": 1 },
                    { "match_result": 1 },
                    { "match_result": 0 },
                    { "match_result": 0, "not_scored": true },
                    { "match_result": 1 }
                ]
            })))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/player-live"))
            .and(query_param("discord_id", "4242"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "linked": true,
                "live": true,
                "hero": "Haze",
                "minutes": 7
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let app = build_public_router(pool);
        let first = get_json(
            app.clone(),
            "/twitch/api/v2/public/overlay?streamer=StreamerX",
        )
        .await;
        let second = get_json(app, "/twitch/api/v2/public/overlay?streamer=streamerx").await;

        assert_eq!(first, second);
        assert_eq!(first["ok"], true);
        assert_eq!(first["streamer"], "streamerx");
        assert_eq!(first["rank_name"], "Oracle");
        assert_eq!(first["badge_level"], 53);
        assert_eq!(first["delta"], 3);
        assert_eq!(first["wins"], 3);
        assert_eq!(first["losses"], 1);
        assert_eq!(first["winrate"].as_f64().unwrap(), 75.0);
        assert_eq!(first["streak_kind"], "win");
        assert_eq!(first["streak_len"], 2);
        assert_eq!(first["live"], true);
        assert_eq!(first["hero"], "Haze");
        assert_eq!(first["minutes"], 7);

        mock_server.verify().await;
        assert_eq!(mock_server.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn overlay_api_unbekannter_streamer_liefert_ok_false_ohne_steam_abruf() {
        let dsn = db_dsn_or_skip!();
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
        super::clear_overlay_cache_for_tests().await;
        let mock_server = MockServer::start().await;
        let _env = EnvGuard::set(&mock_server.uri());
        let pool = make_pool(&dsn, "api_overlay_unknown").await;

        let app = build_public_router(pool);
        let json = get_json(app, "/twitch/api/v2/public/overlay?streamer=missing").await;

        assert_eq!(json, json!({ "ok": false }));
        assert_eq!(mock_server.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn overlay_html_route_liefert_grundstruktur_und_polling_script() {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .expect("lazy pool");
        let app = build_public_router(pool);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/twitch/overlay?streamer=nani")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();

        assert!(html.contains("background: transparent"));
        assert!(html.contains("id=\"overlay-card\""));
        assert!(html.contains("#overlay-card.overlay-pos-bl"));
        assert!(html.contains("#overlay-card.overlay-pos-br"));
        assert!(html.contains("#overlay-card.overlay-pos-tl"));
        assert!(html.contains("#overlay-card.overlay-pos-tr"));
        assert!(html.contains("rank: params.get('rank') !== '0'"));
        assert!(html.contains("winrate: params.get('winrate') !== '0'"));
        assert!(html.contains("streak: params.get('streak') !== '0'"));
        assert!(html.contains("live: params.get('live') !== '0'"));
        assert!(html.contains("['bl', 'br', 'tl', 'tr'].includes(requestedPosition)"));
        assert!(html.contains("card.classList.add(`overlay-pos-${position}`)"));
        assert!(html.contains("/twitch/api/v2/public/overlay?streamer="));
        assert!(html.contains("setInterval(poll, 20000)"));
        assert!(html.contains("Math.floor(badgeLevel / 10)"));
        assert!(html.contains("badge_lg_subrank${sub}.png"));
        assert!(html.contains("badge_lg.png"));
        assert!(html.contains("https://assets.deadlock-api.com/v2/heroes?only_active=true"));
        assert!(html.contains("icon_image_small_webp"));
        assert!(html.contains("Deadlock-Spiel-Assets (© Valve)"));
        assert!(html.contains("Rang:"));
        assert!(html.contains("Winrate:"));
        assert!(html.contains("Serie:"));
        assert!(html.contains("LIVE"));
        assert!(html.contains("if (flags.rank && data.rank_name)"));
        assert!(html.contains("if (flags.winrate && isNumber(data.winrate)"));
        assert!(html.contains("if (flags.streak && isNumber(data.streak_len)"));
        assert!(html.contains("if (flags.live && data.live === true)"));
    }
}
