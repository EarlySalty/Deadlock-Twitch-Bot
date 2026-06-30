//! Public OBS overlay for Deadlock live stats.
//!
//! Rang-/Hero-Bilder sind Deadlock-Spiel-Assets (© Valve), geladen über die
//! öffentliche deadlock-api Assets-CDN; dieser Code nutzt nur Asset-URLs und
//! keinen fremden Streamkit-Code.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    Json,
};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::Notify;

use crate::auth::level::DashboardAuthLevel;
use crate::handlers::spa;

const DEFAULT_STEAM_BOT_BASE_URL: &str = "http://127.0.0.1:8783";
const DEFAULT_DEADLOCK_ASSETS_BASE: &str = "https://assets.deadlock-api.com";
const OVERLAY_CACHE_TTL: Duration = Duration::from_secs(30);
const STEAM_BOT_TIMEOUT: Duration = Duration::from_secs(8);
const HERO_ASSETS_TIMEOUT: Duration = Duration::from_secs(5);
const HERO_ASSETS_TTL: Duration = Duration::from_secs(6 * 60 * 60);

static OVERLAY_CACHE: OnceLock<Mutex<OverlayCache>> = OnceLock::new();
static HERO_ICON_CACHE: OnceLock<Mutex<HeroIconCache>> = OnceLock::new();

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
    #[serde(default)]
    mode: Option<String>,
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
    today_wins: Option<i64>,
    today_losses: Option<i64>,
    today_winrate: Option<f64>,
    today_matches: Option<i64>,
    kd: Option<f64>,
    streak_kind: Option<String>,
    streak_len: Option<i64>,
    last_result: Option<String>,
    last_hero: Option<String>,
    last_kills: Option<i64>,
    last_deaths: Option<i64>,
    last_assists: Option<i64>,
    most_played_hero: Option<String>,
    most_played_count: Option<i64>,
    most_played_icon: Option<String>,
    #[serde(default)]
    recent: Vec<RecentMatch>,
    career_wins: Option<i64>,
    live: bool,
    hero: Option<String>,
    minutes: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct RecentMatch {
    result: String,
    hero: Option<String>,
    hero_icon: Option<String>,
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

#[derive(Clone, Deserialize)]
struct SteamMatch {
    #[serde(default)]
    match_result: Option<i64>,
    #[serde(default)]
    not_scored: Option<bool>,
    #[serde(default)]
    hero_name: Option<String>,
    #[serde(default)]
    start_time: i64,
    #[serde(default)]
    player_kills: i64,
    #[serde(default)]
    player_deaths: i64,
    #[serde(default)]
    player_assists: i64,
    /// Deadlock-`ECitadelGameMode`-Diskriminator: 1 = Normal/Standard,
    /// 4 = StreetBrawl. Andere Werte (Test/Sandbox/NYC/Internal) bleiben dem
    /// `all`-Modus vorbehalten. `match_mode` ist NICHT der Diskriminator.
    #[serde(default)]
    game_mode: Option<i64>,
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
    last_result: String,
    last_hero: Option<String>,
    last_kills: i64,
    last_deaths: i64,
    last_assists: i64,
    most_played_hero: Option<String>,
    most_played_count: Option<i64>,
}

#[derive(Debug, PartialEq)]
struct TodaySummary {
    wins: i64,
    losses: i64,
    winrate: f64,
    matches: i64,
}

#[derive(Default)]
struct HeroIconCache {
    fetched_at: Option<Instant>,
    /// `hero_name.to_lowercase()` → Icon-URL.
    icons: Arc<HashMap<String, String>>,
}

/// Antwortform der öffentlichen Deadlock-Assets-Helden-API.
#[derive(Deserialize)]
struct AssetHero {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    images: AssetHeroImages,
}

#[derive(Deserialize, Default)]
struct AssetHeroImages {
    #[serde(default)]
    icon_image_small: Option<String>,
    #[serde(default)]
    icon_image_small_webp: Option<String>,
}

fn overlay_cache() -> &'static Mutex<OverlayCache> {
    OVERLAY_CACHE.get_or_init(|| Mutex::new(OverlayCache::default()))
}

fn hero_icon_cache() -> &'static Mutex<HeroIconCache> {
    HERO_ICON_CACHE.get_or_init(|| Mutex::new(HeroIconCache::default()))
}

fn lock_cache<T>(cache: &Mutex<T>) -> MutexGuard<'_, T> {
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn deadlock_assets_base_url() -> String {
    std::env::var("DEADLOCK_ASSETS_BASE")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DEADLOCK_ASSETS_BASE.to_string())
}

/// Hero-Name → Icon-URL aus der öffentlichen Deadlock-Assets-API, mit langem
/// In-Memory-Cache (~6 h). Best-effort: bei Fehler/Timeout eine leere Map —
/// der Render fällt dann auf seine Buchstaben-Kachel zurück, niemals `ok:false`.
async fn hero_icon_map(client: &Client) -> Arc<HashMap<String, String>> {
    {
        let cache = lock_cache(hero_icon_cache());
        if let Some(fetched_at) = cache.fetched_at {
            if fetched_at.elapsed() < HERO_ASSETS_TTL {
                return Arc::clone(&cache.icons);
            }
        }
    }

    let map = fetch_hero_icon_map(client).await;
    let arc = Arc::new(map);
    {
        let mut cache = lock_cache(hero_icon_cache());
        cache.fetched_at = Some(Instant::now());
        cache.icons = Arc::clone(&arc);
    }
    arc
}

async fn fetch_hero_icon_map(client: &Client) -> HashMap<String, String> {
    let url = format!("{}/v2/heroes?only_active=true", deadlock_assets_base_url());
    let heroes: Vec<AssetHero> = match client
        .get(&url)
        .timeout(HERO_ASSETS_TIMEOUT)
        .send()
        .await
        .ok()
        .filter(|response| response.status().is_success())
    {
        Some(response) => response.json().await.unwrap_or_default(),
        None => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for hero in heroes {
        let Some(name) = clean_string(&hero.name) else {
            continue;
        };
        let icon = clean_string(&hero.images.icon_image_small)
            .or_else(|| clean_string(&hero.images.icon_image_small_webp));
        if let Some(icon) = icon {
            map.insert(name.to_lowercase(), icon);
        }
    }
    map
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

struct InflightGuard {
    cache: &'static Mutex<OverlayCache>,
    key: String,
    notify: Arc<Notify>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        let removed = {
            let mut cache = lock_cache(self.cache);
            let should_remove = cache
                .inflight
                .get(&self.key)
                .map(|current| Arc::ptr_eq(current, &self.notify))
                .unwrap_or(false);
            if should_remove {
                cache.inflight.remove(&self.key);
                true
            } else {
                false
            }
        };

        if removed {
            self.notify.notify_waiters();
        }
    }
}

async fn cached_or_compute<F, Fut>(
    cache: &'static Mutex<OverlayCache>,
    key: String,
    ttl: Duration,
    compute: F,
) -> Value
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Value>,
{
    enum CacheAction {
        Wait(tokio::sync::futures::OwnedNotified),
        Lead(Arc<Notify>),
    }

    loop {
        let action = {
            let mut cache = lock_cache(cache);
            let now = Instant::now();
            if let Some(entry) = cache.entries.get(&key) {
                if now.duration_since(entry.inserted_at) < ttl {
                    return entry.body.clone();
                }
            }

            if let Some(notify) = cache.inflight.get(&key) {
                CacheAction::Wait(Arc::clone(notify).notified_owned())
            } else {
                let notify = Arc::new(Notify::new());
                cache.inflight.insert(key.clone(), Arc::clone(&notify));
                CacheAction::Lead(notify)
            }
        };

        match action {
            CacheAction::Wait(notified) => notified.await,
            CacheAction::Lead(notify) => {
                let guard = InflightGuard {
                    cache,
                    key: key.clone(),
                    notify,
                };
                let body = compute().await;
                {
                    let mut cache = lock_cache(cache);
                    cache.entries.insert(
                        key.clone(),
                        CacheEntry {
                            inserted_at: Instant::now(),
                            body: body.clone(),
                        },
                    );
                }
                drop(guard);
                return body;
            }
        }
    }
}

async fn cached_overlay_or_fetch(pool: &PgPool, login: &str, mode: &str) -> Value {
    // Pro (login, mode) keyen — sonst liefert der Cache den falschen Modus.
    let key = format!("{login}|{mode}");
    cached_or_compute(overlay_cache(), key, OVERLAY_CACHE_TTL, || {
        build_overlay_json(pool, login, mode)
    })
    .await
}

async fn build_overlay_json(pool: &PgPool, login: &str, mode: &str) -> Value {
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

    let (trend, matches, live, hero_icons) = tokio::join!(
        fetch_steam_json::<SteamMmrTrend>(&client, &trend_url, &trend_query),
        fetch_steam_json::<SteamMatchHistory>(&client, &matches_url, &matches_query),
        fetch_steam_json::<SteamLiveStatus>(&client, &live_url, &live_query),
        hero_icon_map(&client),
    );

    let trend = trend.filter(|value| value.linked != Some(false));
    let history = matches.filter(|value| value.linked != Some(false));
    let live = live.filter(|value| value.linked != Some(false));

    let raw_matches: &[SteamMatch] = history
        .as_ref()
        .map(|value| value.matches.as_slice())
        .unwrap_or(&[]);
    // Modus-Filter wirkt NUR auf match-abgeleitete Stats, nicht auf rank/mmr-trend/live.
    let match_list = filter_by_mode(raw_matches, mode);
    let match_list = match_list.as_slice();
    let match_summary = summarize_matches(match_list);
    let today = summarize_today(match_list, Utc::now());
    let kd = compute_kd(match_list);
    let mut recent = build_recent(match_list, 15);

    let lookup_icon =
        |hero: Option<&str>| hero.and_then(|name| hero_icons.get(&name.to_lowercase()).cloned());

    for entry in &mut recent {
        entry.hero_icon = lookup_icon(entry.hero.as_deref());
    }
    let most_played_icon = match_summary
        .as_ref()
        .and_then(|summary| summary.most_played_hero.as_deref())
        .and_then(|hero| lookup_icon(Some(hero)));

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
        today_wins: today.as_ref().map(|summary| summary.wins),
        today_losses: today.as_ref().map(|summary| summary.losses),
        today_winrate: today.as_ref().map(|summary| summary.winrate),
        today_matches: today.as_ref().map(|summary| summary.matches),
        kd,
        streak_kind: match_summary
            .as_ref()
            .map(|summary| summary.streak_kind.clone()),
        streak_len: match_summary.as_ref().map(|summary| summary.streak_len),
        last_result: match_summary
            .as_ref()
            .map(|summary| summary.last_result.clone()),
        last_hero: match_summary
            .as_ref()
            .and_then(|summary| summary.last_hero.clone()),
        last_kills: match_summary.as_ref().map(|summary| summary.last_kills),
        last_deaths: match_summary.as_ref().map(|summary| summary.last_deaths),
        last_assists: match_summary.as_ref().map(|summary| summary.last_assists),
        most_played_hero: match_summary
            .as_ref()
            .and_then(|summary| summary.most_played_hero.clone()),
        most_played_count: match_summary
            .as_ref()
            .and_then(|summary| summary.most_played_count),
        most_played_icon,
        recent,
        career_wins: None,
        live: live.as_ref().map(|value| value.live).unwrap_or(false),
        hero: live.as_ref().and_then(|value| clean_string(&value.hero)),
        minutes: live.and_then(|value| value.minutes),
    };

    serde_json::to_value(response).unwrap_or_else(|_| ok_false())
}

async fn resolve_discord_id(pool: &PgPool, login: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<String> = sqlx::query_scalar!(
        "SELECT i.discord_user_id::text AS \"discord_user_id!\" \
         FROM twitch_streamers s \
         JOIN twitch_streamer_identities i ON i.twitch_user_id = s.twitch_user_id \
         WHERE LOWER(s.twitch_login) = $1 \
           AND COALESCE(s.twitch_user_id, '') <> '' \
           AND COALESCE(i.discord_user_id::text, '') <> '' \
         LIMIT 1",
        login
    )
    .fetch_optional(pool)
    .await?;

    Ok(row
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

/// Normalisiert den Spielmodus-Param auf `all`, `standard` oder `brawl`.
/// Unbekanntes/leeres → `all` (keine Filterung).
fn normalize_mode(mode: Option<&str>) -> &'static str {
    match mode.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("standard") => "standard",
        Some("brawl") => "brawl",
        _ => "all",
    }
}

/// Reduziert die Match-Liste auf den gewählten Spielmodus, BEVOR Stats berechnet
/// werden. `brawl` → nur Street Brawl (`game_mode == Some(4)`); `standard` →
/// alles AUSSER Street Brawl (`game_mode != Some(4)`, inkl. fehlender/unbekannter
/// game_modes — robust gegen alte Matches); `all`/unbekannt → unverändert.
/// Damit gilt stets `all == standard + brawl`. Wirkt nur auf match-abgeleitete
/// Stats, nicht auf rank/mmr-trend/live.
fn filter_by_mode(matches: &[SteamMatch], mode: &str) -> Vec<SteamMatch> {
    const STREET_BRAWL: i64 = 4;
    match mode {
        "brawl" => matches
            .iter()
            .filter(|entry| entry.game_mode == Some(STREET_BRAWL))
            .cloned()
            .collect(),
        "standard" => matches
            .iter()
            .filter(|entry| entry.game_mode != Some(STREET_BRAWL))
            .cloned()
            .collect(),
        _ => matches.to_vec(),
    }
}

/// Liefert die gewerteten Matches (`not_scored != true`, `match_result ∈ {0,1}`)
/// in Eingabe-Reihenfolge (newest-first).
fn scored_matches(matches: &[SteamMatch]) -> Vec<&SteamMatch> {
    matches
        .iter()
        .filter(|entry| entry.not_scored != Some(true))
        .filter(|entry| matches!(entry.match_result, Some(0 | 1)))
        .collect()
}

fn summarize_matches(matches: &[SteamMatch]) -> Option<MatchSummary> {
    let scored = scored_matches(matches);
    let first = scored.first()?;
    let first_result = first.match_result?;

    let wins = scored
        .iter()
        .filter(|entry| entry.match_result == Some(1))
        .count() as i64;
    let losses = scored
        .iter()
        .filter(|entry| entry.match_result == Some(0))
        .count() as i64;
    let total = wins + losses;
    if total == 0 {
        return None;
    }

    let streak_len = scored
        .iter()
        .take_while(|entry| entry.match_result == Some(first_result))
        .count() as i64;
    let winrate = ((wins as f64 * 1000.0) / total as f64).round() / 10.0;
    let streak_kind = if first_result == 1 { "win" } else { "loss" }.to_string();

    let last_result = if first_result == 1 { "win" } else { "loss" }.to_string();
    let last_hero = clean_string(&first.hero_name);
    let last_kills = first.player_kills;
    let last_deaths = first.player_deaths;
    let last_assists = first.player_assists;

    let (most_played_hero, most_played_count) = most_played(&scored);

    Some(MatchSummary {
        wins,
        losses,
        winrate,
        streak_kind,
        streak_len,
        last_result,
        last_hero,
        last_kills,
        last_deaths,
        last_assists,
        most_played_hero,
        most_played_count,
    })
}

/// Häufigster `hero_name` über das gewertete Fenster. Bei Gleichstand gewinnt der
/// zuerst (newest-first) gesehene Hero.
fn most_played(scored: &[&SteamMatch]) -> (Option<String>, Option<i64>) {
    let mut order: Vec<String> = Vec::new();
    let mut counts: HashMap<String, i64> = HashMap::new();
    for entry in scored {
        if let Some(hero) = clean_string(&entry.hero_name) {
            if !counts.contains_key(&hero) {
                order.push(hero.clone());
            }
            *counts.entry(hero).or_insert(0) += 1;
        }
    }

    let best = order
        .into_iter()
        .max_by_key(|hero| counts.get(hero).copied().unwrap_or(0));

    match best {
        Some(hero) => {
            let count = counts.get(&hero).copied().unwrap_or(0);
            (Some(hero), Some(count))
        }
        None => (None, None),
    }
}

/// Heutige Bilanz, Tagesgrenze fix `Europe/Berlin` (00:00 lokal des Berlin-Datums
/// von `now_utc`). Nur gewertete Matches mit `start_time ≥ Tagesbeginn`.
fn summarize_today(matches: &[SteamMatch], now_utc: DateTime<Utc>) -> Option<TodaySummary> {
    let now_berlin = now_utc.with_timezone(&Berlin);
    let start_of_day = Berlin
        .with_ymd_and_hms(
            now_berlin.year(),
            now_berlin.month(),
            now_berlin.day(),
            0,
            0,
            0,
        )
        .single()?;
    let cutoff = start_of_day.with_timezone(&Utc).timestamp();

    let scored = scored_matches(matches);
    let mut wins = 0i64;
    let mut losses = 0i64;
    for entry in &scored {
        if entry.start_time < cutoff {
            continue;
        }
        match entry.match_result {
            Some(1) => wins += 1,
            Some(0) => losses += 1,
            _ => {}
        }
    }

    let total = wins + losses;
    if total == 0 {
        return None;
    }

    let winrate = ((wins as f64 * 1000.0) / total as f64).round() / 10.0;
    Some(TodaySummary {
        wins,
        losses,
        winrate,
        matches: total,
    })
}

/// K/D übers gewertete Fenster: `Σkills / max(Σdeaths, 1)`, 2 Nachkommastellen.
fn compute_kd(matches: &[SteamMatch]) -> Option<f64> {
    let scored = scored_matches(matches);
    if scored.is_empty() {
        return None;
    }

    let kills: i64 = scored.iter().map(|entry| entry.player_kills).sum();
    let deaths: i64 = scored.iter().map(|entry| entry.player_deaths).sum();
    let kd = kills as f64 / deaths.max(1) as f64;
    Some((kd * 100.0).round() / 100.0)
}

/// Letzte `n` gewertete Matches, newest-first (Eingabe-Reihenfolge), `n` auf 15 gecappt.
fn build_recent(matches: &[SteamMatch], n: usize) -> Vec<RecentMatch> {
    let cap = n.min(15);
    scored_matches(matches)
        .into_iter()
        .take(cap)
        .map(|entry| RecentMatch {
            result: if entry.match_result == Some(1) {
                "win"
            } else {
                "loss"
            }
            .to_string(),
            hero: clean_string(&entry.hero_name),
            hero_icon: None,
        })
        .collect()
}

/// `GET /twitch/api/v2/public/overlay?streamer=<login>&mode=<all|standard|brawl>`
pub async fn overlay_api_handler(
    State(pool): State<PgPool>,
    Query(query): Query<OverlayQuery>,
) -> impl IntoResponse {
    let Some(login) = normalize_login(query.streamer.as_deref()) else {
        return (StatusCode::OK, Json(ok_false())).into_response();
    };

    let mode = normalize_mode(query.mode.as_deref());
    let body = cached_overlay_or_fetch(&pool, &login, mode).await;
    (StatusCode::OK, Json(body)).into_response()
}

/// `GET /twitch/overlay` — mit `streamer` das öffentliche OBS-Render, ohne Param
/// die Builder-SPA. Letztere ist auth-gegated wie die übrigen Dashboard-Seiten:
/// sonst lieferte sie bei fehlender Session nur die leere Shell aus, deren Assets
/// dann auf Login 303en → Weißbild. Der Render-Pfad bleibt öffentlich (OBS).
pub async fn overlay_html_handler(
    headers: HeaderMap,
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<OverlayQuery>,
) -> axum::response::Response {
    if normalize_login(query.streamer.as_deref()).is_some() {
        return Html(OVERLAY_HTML).into_response();
    }

    spa::serve_dashboard_v2_index_gated(&headers, &auth, &pool).await
}

const OVERLAY_HTML: &str = r##"<!doctype html>
<html lang="de">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Deadlock Overlay</title>
  <style>
    :root {
      --bg-alpha: 0.85;
      --radius: 14px;
      --shadow: 0 18px 40px rgba(0, 0, 0, 0.45);
    }

    html, body {
      margin: 0;
      width: 100%;
      height: 100%;
      background: transparent;
      overflow: hidden;
      font-family: Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }

    /* --- Themes via data-theme + CSS Custom Properties --- */
    #overlay-card[data-theme="dark"] {
      --bg: rgba(13, 15, 20, var(--bg-alpha));
      --fg: #f4f7fb;
      --muted: #9aa6b6;
      --accent: #22d3ee;
      --accent-2: #22d3ee;
      --win: #34d399;
      --loss: #fb7185;
      --border: rgba(255, 255, 255, 0.10);
      --accent-line: linear-gradient(90deg, var(--accent), transparent);
    }

    #overlay-card[data-theme="light"] {
      --bg: rgba(248, 250, 253, var(--bg-alpha));
      --fg: #0f172a;
      --muted: #475569;
      --accent: #0891b2;
      --accent-2: #0891b2;
      --win: #059669;
      --loss: #e11d48;
      --border: rgba(15, 23, 42, 0.12);
      --accent-line: linear-gradient(90deg, var(--accent), transparent);
    }

    #overlay-card[data-theme="accent"] {
      --bg: rgba(16, 12, 26, var(--bg-alpha));
      --fg: #f6f4ff;
      --muted: #b3a7cf;
      --accent: #06B6D4;
      --accent-2: #A855F7;
      --win: #34d399;
      --loss: #fb7185;
      --border: rgba(255, 255, 255, 0.12);
      --accent-line: linear-gradient(135deg, #06B6D4, #A855F7);
    }

    #overlay-card {
      position: fixed;
      box-sizing: border-box;
      display: none;
      color: var(--fg);
      font-variant-numeric: tabular-nums;
      letter-spacing: 0;
    }

    #overlay-card.overlay-pos-bl { left: 18px; bottom: 18px; }
    #overlay-card.overlay-pos-br { right: 18px; bottom: 18px; }
    #overlay-card.overlay-pos-tl { left: 18px; top: 18px; }
    #overlay-card.overlay-pos-tr { right: 18px; top: 18px; }

    #overlay-card.visible {
      display: block;
      animation: overlay-enter 180ms ease-out;
    }

    /* --- Box-Layout: Glassmorphism-Karte --- */
    #overlay-card.layout-box {
      width: 332px;
      padding: 14px 16px;
      border-radius: var(--radius);
      background: var(--bg);
      border: 1px solid var(--border);
      box-shadow: var(--shadow);
      -webkit-backdrop-filter: blur(12px) saturate(140%);
      backdrop-filter: blur(12px) saturate(140%);
      position: fixed;
      overflow: hidden;
    }

    #overlay-card.layout-box::before {
      content: "";
      position: absolute;
      inset: 0 0 auto 0;
      height: 3px;
      background: var(--accent-line);
      opacity: 0.9;
    }

    .ov-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
    }

    .ov-name {
      min-width: 0;
      font-size: 17px;
      font-weight: 700;
      color: var(--fg);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .ov-live {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      flex: 0 0 auto;
      font-size: 11px;
      font-weight: 800;
      letter-spacing: 0.10em;
      text-transform: uppercase;
      color: var(--win);
    }

    .ov-live-dot {
      width: 8px;
      height: 8px;
      border-radius: 999px;
      background: var(--win);
      box-shadow: 0 0 0 0 var(--win);
      animation: ov-pulse 1.6s ease-out infinite;
    }

    .ov-head-rule {
      height: 2px;
      margin: 9px 0 11px;
      border-radius: 2px;
      background: var(--accent-line);
      opacity: 0.75;
    }

    /* --- Stat-Raster (Box) --- */
    .ov-grid {
      display: flex;
      flex-wrap: wrap;
      align-items: stretch;
    }

    .ov-cell {
      display: flex;
      flex-direction: column;
      gap: 3px;
      padding: 2px 12px;
      flex: 1 1 auto;
      min-width: 0;
      border-left: 1px solid var(--border);
    }

    .ov-cell:first-child { padding-left: 0; border-left: 0; }

    .ov-label {
      font-size: 10px;
      font-weight: 700;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--muted);
    }

    .ov-value {
      display: flex;
      align-items: center;
      gap: 7px;
      font-size: 17px;
      font-weight: 700;
      color: var(--fg);
      white-space: nowrap;
    }

    .ov-value .ov-win { color: var(--win); }
    .ov-value .ov-loss { color: var(--loss); }
    .ov-delta-up { color: var(--win); font-size: 13px; }
    .ov-delta-down { color: var(--loss); font-size: 13px; }

    .ov-hero-name {
      overflow: hidden;
      text-overflow: ellipsis;
      max-width: 120px;
    }

    .ov-sub {
      font-size: 12px;
      font-weight: 600;
      color: var(--muted);
    }

    .ov-sub-kda {
      font-size: 13px;
      font-weight: 600;
      color: var(--muted);
    }

    .rank-badge {
      width: 40px;
      height: 40px;
      flex: 0 0 auto;
      object-fit: contain;
      filter: drop-shadow(0 2px 6px rgba(0, 0, 0, 0.4));
    }

    .ov-main-icon {
      width: 24px;
      height: 24px;
      flex: 0 0 auto;
      border-radius: 7px;
      object-fit: cover;
      background: rgba(127, 127, 127, 0.18);
    }

    /* --- Recent-Matches-Strip --- */
    .ov-recent {
      margin-top: 11px;
    }

    .ov-recent-label {
      font-size: 10px;
      font-weight: 700;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--muted);
      margin-bottom: 6px;
    }

    .ov-recent-row {
      display: flex;
      gap: 5px;
      flex-wrap: wrap;
    }

    /* Hero-Kachel: abgerundetes Quadrat mit Portrait, Sieg/Niederlage als
       dezenter farbiger Unterstrich (keine Vollfarb-Fläche). */
    .ov-tile {
      width: 26px;
      height: 26px;
      flex: 0 0 auto;
      border-radius: 7px;
      box-sizing: border-box;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      overflow: hidden;
      object-fit: cover;
      background: rgba(127, 127, 127, 0.16);
      border: 1px solid var(--border);
      border-bottom-width: 2px;
    }

    .ov-tile.win { border-bottom-color: var(--win); }
    .ov-tile.loss { border-bottom-color: var(--loss); }

    /* Fallback ohne Icon: dezente Kachel mit S/N-Buchstabe in --win/--loss. */
    .ov-tile-fallback {
      font-size: 12px;
      font-weight: 800;
      line-height: 1;
    }

    .ov-tile-fallback.win { color: var(--win); }
    .ov-tile-fallback.loss { color: var(--loss); }

    /* --- Branding --- */
    .ov-brand {
      margin-top: 12px;
      font-size: 9.5px;
      font-weight: 600;
      letter-spacing: 0.04em;
      text-transform: uppercase;
      color: var(--muted);
      opacity: 0.8;
    }

    /* --- Bar-Layout: schlanke Pille --- */
    #overlay-card.layout-bar {
      max-width: 640px;
      padding: 9px 16px;
      border-radius: 999px;
      background: var(--bg);
      border: 1px solid var(--border);
      box-shadow: var(--shadow);
      -webkit-backdrop-filter: blur(12px) saturate(140%);
      backdrop-filter: blur(12px) saturate(140%);
    }

    #overlay-card.layout-bar .ov-bar {
      display: flex;
      align-items: center;
      gap: 0;
      white-space: nowrap;
      font-size: 14px;
    }

    .ov-seg {
      display: inline-flex;
      align-items: center;
      gap: 7px;
    }

    .ov-seg + .ov-seg::before {
      content: "·";
      margin: 0 10px;
      color: var(--muted);
      opacity: 0.7;
    }

    .ov-seg .ov-seg-label {
      font-size: 10px;
      font-weight: 700;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      color: var(--muted);
    }

    .ov-seg .ov-seg-value {
      font-weight: 700;
      color: var(--fg);
    }

    .ov-seg .rank-badge { width: 26px; height: 26px; }
    .ov-seg .ov-recent-row {
      flex-wrap: nowrap;
      gap: 4px;
    }
    .ov-seg .ov-recent-row .ov-tile {
      width: 20px;
      height: 20px;
      border-radius: 6px;
    }
    .ov-seg .ov-recent-row .ov-tile-fallback { font-size: 10px; }

    @keyframes overlay-enter {
      from { opacity: 0; transform: translateY(4px); }
      to { opacity: 1; transform: translateY(0); }
    }

    @keyframes ov-pulse {
      0% { box-shadow: 0 0 0 0 var(--win); opacity: 1; }
      70% { box-shadow: 0 0 0 7px rgba(52, 211, 153, 0); opacity: 0.85; }
      100% { box-shadow: 0 0 0 0 rgba(52, 211, 153, 0); opacity: 1; }
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

    const oneOf = (key, allowed, fallback) => {
      const value = (params.get(key) || '').trim().toLowerCase();
      return allowed.includes(value) ? value : fallback;
    };
    const flag = (key, fallback) => {
      const value = params.get(key);
      if (value === null) return fallback;
      return value !== '0';
    };
    const clampInt = (key, min, max, fallback) => {
      const value = parseInt(params.get(key) || '', 10);
      if (!Number.isFinite(value)) return fallback;
      return Math.min(max, Math.max(min, value));
    };

    const theme = oneOf('theme', ['dark', 'light', 'accent'], 'dark');
    const layout = oneOf('layout', ['box', 'bar'], 'box');
    const mode = oneOf('mode', ['all', 'standard', 'brawl'], 'all');
    const position = oneOf('pos', ['bl', 'br', 'tl', 'tr'], 'tl');
    const opacity = clampInt('opacity', 0, 100, 85);
    const recentN = clampInt('recent_n', 1, 15, 10);

    const flags = {
      header: flag('header', true),
      rank: flag('rank', true),
      winrate: flag('winrate', true),
      today: flag('today', true),
      streak: flag('streak', true),
      kd: flag('kd', true),
      lastmatch: flag('lastmatch', false),
      mostplayed: flag('mostplayed', false),
      recent: flag('recent', true),
      live: flag('live', true),
      branding: flag('branding', true),
    };

    card.dataset.theme = theme;
    card.classList.add(`layout-${layout}`);
    card.classList.add(`overlay-pos-${position}`);
    card.style.setProperty('--bg-alpha', String(opacity / 100));

    let latestData = null;

    const isNumber = (value) => typeof value === 'number' && Number.isFinite(value);
    const nf1 = new Intl.NumberFormat('de-DE', { minimumFractionDigits: 1, maximumFractionDigits: 1 });
    const nf2 = new Intl.NumberFormat('de-DE', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

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

    function cleanIcon(value) {
      return typeof value === 'string' && value.trim() ? value.trim() : null;
    }

    function hide() {
      latestData = null;
      card.classList.remove('visible');
      card.replaceChildren();
    }

    function el(tag, className, text) {
      const node = document.createElement(tag);
      if (className) node.className = className;
      if (text !== undefined && text !== null) node.textContent = text;
      return node;
    }

    function fallbackTile(result) {
      return el('span', `ov-tile ov-tile-fallback ${result}`, result === 'win' ? 'S' : 'N');
    }

    function recentRow(recent) {
      const row = el('div', 'ov-recent-row');
      const items = Array.isArray(recent) ? recent.slice(0, recentN) : [];
      for (const match of items) {
        const result = match && match.result === 'win' ? 'win' : 'loss';
        const icon = cleanIcon(match && match.hero_icon);
        if (icon) {
          const image = el('img', `ov-tile ${result}`);
          image.src = icon;
          image.alt = '';
          image.decoding = 'async';
          image.loading = 'lazy';
          image.onerror = () => image.replaceWith(fallbackTile(result));
          row.appendChild(image);
        } else {
          row.appendChild(fallbackTile(result));
        }
      }
      return row.childElementCount ? row : null;
    }

    function rankValueNode(data) {
      const value = el('div', 'ov-value');
      const badge = rankBadgeUrl(data.badge_level);
      if (badge) {
        const image = el('img', 'rank-badge');
        image.src = badge;
        image.alt = '';
        image.decoding = 'async';
        image.loading = 'lazy';
        image.onerror = () => image.remove();
        value.appendChild(image);
      }
      value.appendChild(el('span', 'ov-hero-name', data.rank_name));
      if (isNumber(data.delta) && data.delta > 0) value.appendChild(el('span', 'ov-delta-up', '▲'));
      if (isNumber(data.delta) && data.delta < 0) value.appendChild(el('span', 'ov-delta-down', '▼'));
      return value;
    }

    // Liefert die aktiven Module als kuratierte Liste {label, build()}.
    function activeModules(data) {
      const mods = [];

      if (flags.rank && data.rank_name) {
        mods.push({ key: 'rank', label: 'RANG', value: () => rankValueNode(data) });
      }

      if (flags.winrate && isNumber(data.winrate) && isNumber(data.wins) && isNumber(data.losses)) {
        mods.push({
          key: 'winrate',
          label: 'WINRATE',
          value: () => {
            const value = el('div', 'ov-value');
            value.appendChild(el('span', null, `${nf1.format(data.winrate)} %`));
            value.appendChild(el('span', 'ov-sub', `${data.wins}–${data.losses}`));
            return value;
          },
        });
      }

      if (flags.today && isNumber(data.today_wins) && isNumber(data.today_losses)) {
        mods.push({
          key: 'today',
          label: 'HEUTE',
          value: () => {
            const value = el('div', 'ov-value');
            value.appendChild(el('span', 'ov-win', String(data.today_wins)));
            value.appendChild(el('span', null, '–'));
            value.appendChild(el('span', 'ov-loss', String(data.today_losses)));
            return value;
          },
        });
      }

      if (flags.streak && isNumber(data.streak_len) && data.streak_len >= 2 &&
          (data.streak_kind === 'win' || data.streak_kind === 'loss')) {
        mods.push({
          key: 'streak',
          label: 'SERIE',
          value: () => {
            const value = el('div', 'ov-value');
            const cls = data.streak_kind === 'win' ? 'ov-win' : 'ov-loss';
            value.appendChild(el('span', cls, `${data.streak_len}×`));
            return value;
          },
        });
      }

      if (flags.kd && isNumber(data.kd)) {
        mods.push({
          key: 'kd',
          label: 'K/D',
          value: () => el('div', 'ov-value', nf2.format(data.kd)),
        });
      }

      if (flags.lastmatch && (data.last_result === 'win' || data.last_result === 'loss')) {
        mods.push({
          key: 'lastmatch',
          label: 'LAST',
          value: () => {
            const value = el('div', 'ov-value');
            const cls = data.last_result === 'win' ? 'ov-win' : 'ov-loss';
            value.appendChild(el('span', cls, data.last_result === 'win' ? 'W' : 'L'));
            if (isNumber(data.last_kills) && isNumber(data.last_deaths) && isNumber(data.last_assists)) {
              value.appendChild(el('span', 'ov-sub-kda', `${data.last_kills}/${data.last_deaths}/${data.last_assists}`));
            }
            return value;
          },
        });
      }

      if (flags.mostplayed && data.most_played_hero) {
        mods.push({
          key: 'mostplayed',
          label: 'MAIN',
          value: () => {
            const value = el('div', 'ov-value');
            const icon = cleanIcon(data.most_played_icon);
            if (icon) {
              const image = el('img', 'ov-main-icon');
              image.src = icon;
              image.alt = '';
              image.decoding = 'async';
              image.loading = 'lazy';
              image.onerror = () => image.remove();
              value.appendChild(image);
            }
            value.appendChild(el('span', 'ov-hero-name', data.most_played_hero));
            return value;
          },
        });
      }

      return mods;
    }

    function buildBox(data) {
      const frag = document.createDocumentFragment();

      if (flags.header && streamer) {
        const head = el('div', 'ov-head');
        head.appendChild(el('div', 'ov-name', streamer));
        if (flags.live && data.live === true) {
          const live = el('div', 'ov-live');
          live.appendChild(el('span', 'ov-live-dot'));
          const details = [];
          if (data.hero) details.push(data.hero);
          if (isNumber(data.minutes)) details.push(`${data.minutes}′`);
          live.appendChild(el('span', null, details.length ? `LIVE · ${details.join(' · ')}` : 'LIVE'));
          head.appendChild(live);
        }
        frag.appendChild(head);
        frag.appendChild(el('div', 'ov-head-rule'));
      } else if (flags.live && data.live === true) {
        const live = el('div', 'ov-live');
        live.style.marginBottom = '10px';
        live.appendChild(el('span', 'ov-live-dot'));
        const details = [];
        if (data.hero) details.push(data.hero);
        if (isNumber(data.minutes)) details.push(`${data.minutes}′`);
        live.appendChild(el('span', null, details.length ? `LIVE · ${details.join(' · ')}` : 'LIVE'));
        frag.appendChild(live);
      }

      const mods = activeModules(data);
      if (mods.length) {
        const grid = el('div', 'ov-grid');
        for (const mod of mods) {
          const cell = el('div', 'ov-cell');
          cell.appendChild(el('div', 'ov-label', mod.label));
          cell.appendChild(mod.value());
          grid.appendChild(cell);
        }
        frag.appendChild(grid);
      }

      if (flags.recent) {
        const row = recentRow(data.recent);
        if (row) {
          const wrap = el('div', 'ov-recent');
          wrap.appendChild(el('div', 'ov-recent-label', 'Match-Verlauf'));
          wrap.appendChild(row);
          frag.appendChild(wrap);
        }
      }

      if (flags.branding) {
        frag.appendChild(el('div', 'ov-brand', 'powered by deutsche-deadlock-community.de'));
      }

      return frag.childElementCount ? frag : null;
    }

    function buildBar(data) {
      const bar = el('div', 'ov-bar');

      if (flags.header && streamer) {
        const seg = el('div', 'ov-seg');
        if (flags.live && data.live === true) seg.appendChild(el('span', 'ov-live-dot'));
        seg.appendChild(el('span', 'ov-seg-value', streamer));
        bar.appendChild(seg);
      }

      for (const mod of activeModules(data)) {
        const seg = el('div', 'ov-seg');
        seg.appendChild(el('span', 'ov-seg-label', mod.label));
        seg.appendChild(mod.value());
        bar.appendChild(seg);
      }

      if (flags.recent) {
        const row = recentRow(data.recent);
        if (row) {
          const seg = el('div', 'ov-seg');
          seg.appendChild(el('span', 'ov-seg-label', 'Match-Verlauf'));
          seg.appendChild(row);
          bar.appendChild(seg);
        }
      }

      return bar.childElementCount ? bar : null;
    }

    function render(data) {
      if (!data || data.ok !== true) {
        hide();
        return;
      }
      latestData = data;

      const content = layout === 'bar' ? buildBar(data) : buildBox(data);
      if (!content) {
        hide();
        return;
      }

      card.replaceChildren(content);
      card.classList.add('visible');
    }

    async function poll() {
      if (!streamer) {
        hide();
        return;
      }
      try {
        const response = await fetch(`/twitch/api/v2/public/overlay?streamer=${encodeURIComponent(streamer)}&mode=${mode}`, { cache: 'no-store' });
        if (!response.ok) {
          hide();
          return;
        }
        render(await response.json());
      } catch (_) {
        hide();
      }
    }

    poll();
    setInterval(poll, 20000);
  </script>
</body>
</html>
"##;

#[cfg(test)]
fn clear_overlay_cache_for_tests() {
    let mut cache = lock_cache(overlay_cache());
    cache.entries.clear();
    cache.inflight.clear();
}

#[cfg(test)]
fn clear_hero_icon_cache_for_tests() {
    let mut cache = lock_cache(hero_icon_cache());
    cache.fetched_at = None;
    cache.icons = Arc::new(HashMap::new());
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex as StdMutex, OnceLock,
    };
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use axum::body::to_bytes;
    use axum::http::{header, Request, StatusCode};
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use tokio::sync::{oneshot, Mutex, Notify};
    use tokio::time::{sleep, timeout};
    use tower::ServiceExt;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::build_public_router;

    use super::{
        build_recent, cached_or_compute, compute_kd, filter_by_mode, normalize_mode,
        scored_matches, summarize_matches, summarize_today, OverlayCache, RecentMatch, SteamMatch,
    };
    use chrono::TimeZone;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    /// `match_result` (1=Sieg/0=Niederlage), optionaler `not_scored`,
    /// `start_time` (unix-UTC), Hero-Name, K/D/A.
    fn sm(
        result: Option<i64>,
        not_scored: bool,
        start_time: i64,
        hero: &str,
        kills: i64,
        deaths: i64,
        assists: i64,
    ) -> SteamMatch {
        SteamMatch {
            match_result: result,
            not_scored: if not_scored { Some(true) } else { None },
            hero_name: if hero.is_empty() {
                None
            } else {
                Some(hero.to_string())
            },
            start_time,
            player_kills: kills,
            player_deaths: deaths,
            player_assists: assists,
            game_mode: None,
        }
    }

    /// Wie `sm`, aber mit explizitem `game_mode` (Deadlock-Diskriminator,
    /// 1 = Standard, 4 = Street Brawl).
    fn sm_mode(result: Option<i64>, game_mode: Option<i64>, hero: &str) -> SteamMatch {
        SteamMatch {
            game_mode,
            ..sm(result, false, 0, hero, 0, 0, 0)
        }
    }

    /// 2026-06-22 12:00 Europe/Berlin (Sommerzeit, UTC+2) als UTC.
    fn now_berlin_noon() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 6, 22, 10, 0, 0).unwrap()
    }

    /// Berlin-Tagesbeginn 2026-06-22 00:00 (= 2026-06-21 22:00 UTC, Sommerzeit UTC+2).
    const BERLIN_TODAY_START_UTC: i64 = 1_782_079_200; // 2026-06-21T22:00:00Z

    fn new_test_cache() -> &'static StdMutex<OverlayCache> {
        Box::leak(Box::new(StdMutex::new(OverlayCache::default())))
    }

    #[test]
    fn summarize_today_zaehlt_nur_heutige_gewertete_matches() {
        let matches = vec![
            // heute, neueste zuerst
            sm(
                Some(1),
                false,
                BERLIN_TODAY_START_UTC + 3_600,
                "Haze",
                0,
                0,
                0,
            ),
            sm(
                Some(1),
                false,
                BERLIN_TODAY_START_UTC + 100,
                "Haze",
                0,
                0,
                0,
            ),
            sm(Some(0), false, BERLIN_TODAY_START_UTC, "Haze", 0, 0, 0),
            // not_scored heute -> raus
            sm(Some(1), true, BERLIN_TODAY_START_UTC + 200, "Haze", 0, 0, 0),
            // gestern (vor Tagesbeginn) -> raus
            sm(Some(1), false, BERLIN_TODAY_START_UTC - 1, "Haze", 0, 0, 0),
            sm(
                Some(0),
                false,
                BERLIN_TODAY_START_UTC - 86_400,
                "Haze",
                0,
                0,
                0,
            ),
        ];

        let today = summarize_today(&matches, now_berlin_noon()).unwrap();
        assert_eq!(today.wins, 2);
        assert_eq!(today.losses, 1);
        assert_eq!(today.matches, 3);
        // 2/3 = 66.666... -> 66,7
        assert_eq!(today.winrate, 66.7);
    }

    #[test]
    fn summarize_today_ohne_heutige_matches_ist_none() {
        let matches = vec![
            sm(Some(1), false, BERLIN_TODAY_START_UTC - 1, "Haze", 0, 0, 0),
            sm(Some(1), true, BERLIN_TODAY_START_UTC + 5, "Haze", 0, 0, 0),
        ];
        assert_eq!(summarize_today(&matches, now_berlin_noon()), None);
    }

    #[test]
    fn compute_kd_rundet_und_haelt_deaths_null_stand() {
        let matches = vec![
            sm(Some(1), false, 0, "Haze", 10, 4, 0),
            sm(Some(0), false, 0, "Haze", 8, 6, 0),
            // not_scored -> ignoriert
            sm(Some(1), true, 0, "Haze", 100, 100, 0),
        ];
        // 18 / max(10,1) = 1.8 -> 1.80
        assert_eq!(compute_kd(&matches), Some(1.8));
    }

    #[test]
    fn compute_kd_deaths_null_teilt_durch_eins() {
        let matches = vec![sm(Some(1), false, 0, "Haze", 7, 0, 0)];
        // 7 / max(0,1) = 7.0
        assert_eq!(compute_kd(&matches), Some(7.0));
    }

    #[test]
    fn compute_kd_ohne_gewertete_matches_ist_none() {
        let matches = vec![sm(Some(1), true, 0, "Haze", 9, 1, 0)];
        assert_eq!(compute_kd(&matches), None);
    }

    #[test]
    fn build_recent_behaelt_reihenfolge_und_filtert_not_scored() {
        let matches = vec![
            sm(Some(1), false, 0, "Haze", 0, 0, 0),
            sm(Some(0), true, 0, "Abrams", 0, 0, 0), // raus
            sm(Some(0), false, 0, "Vindicta", 0, 0, 0),
            sm(Some(1), false, 0, "Seven", 0, 0, 0),
        ];
        let recent = build_recent(&matches, 10);
        assert_eq!(
            recent,
            vec![
                RecentMatch {
                    result: "win".to_string(),
                    hero: Some("Haze".to_string()),
                    hero_icon: None,
                },
                RecentMatch {
                    result: "loss".to_string(),
                    hero: Some("Vindicta".to_string()),
                    hero_icon: None,
                },
                RecentMatch {
                    result: "win".to_string(),
                    hero: Some("Seven".to_string()),
                    hero_icon: None,
                },
            ]
        );
    }

    #[test]
    fn build_recent_cappt_auf_fuenfzehn() {
        let matches: Vec<SteamMatch> = (0..20)
            .map(|i| sm(Some(i % 2), false, 0, "Haze", 0, 0, 0))
            .collect();
        // n=99 -> cap 15
        assert_eq!(build_recent(&matches, 99).len(), 15);
        // n=3 respektiert
        assert_eq!(build_recent(&matches, 3).len(), 3);
    }

    #[test]
    fn summarize_matches_liefert_last_match_und_most_played() {
        let matches = vec![
            // neuestes gewertetes Match zuerst
            sm(Some(1), false, 0, "Haze", 12, 3, 9),
            sm(Some(0), true, 0, "Seven", 1, 1, 1), // not_scored -> ignoriert
            sm(Some(0), false, 0, "Haze", 4, 8, 2),
            sm(Some(1), false, 0, "Vindicta", 6, 5, 4),
            sm(Some(1), false, 0, "Haze", 9, 2, 7),
        ];
        let summary = summarize_matches(&matches).unwrap();

        // last_match = neuestes gewertetes
        assert_eq!(summary.last_result, "win");
        assert_eq!(summary.last_hero, Some("Haze".to_string()));
        assert_eq!(summary.last_kills, 12);
        assert_eq!(summary.last_deaths, 3);
        assert_eq!(summary.last_assists, 9);

        // most_played: Haze 3x
        assert_eq!(summary.most_played_hero, Some("Haze".to_string()));
        assert_eq!(summary.most_played_count, Some(3));

        // wins=3, losses=1
        assert_eq!(summary.wins, 3);
        assert_eq!(summary.losses, 1);
        assert_eq!(summary.winrate, 75.0);
        // Streak: neuestes ist win, danach loss -> Länge 1
        assert_eq!(summary.streak_kind, "win");
        assert_eq!(summary.streak_len, 1);
    }

    #[test]
    fn normalize_mode_normalisiert_und_faellt_auf_all_zurueck() {
        assert_eq!(normalize_mode(Some("standard")), "standard");
        assert_eq!(normalize_mode(Some("  BRAWL ")), "brawl");
        assert_eq!(normalize_mode(Some("Standard")), "standard");
        assert_eq!(normalize_mode(Some("all")), "all");
        assert_eq!(normalize_mode(Some("unsinn")), "all");
        assert_eq!(normalize_mode(Some("")), "all");
        assert_eq!(normalize_mode(None), "all");
    }

    #[test]
    fn filter_by_mode_standard_schliesst_brawl_aus() {
        let matches = vec![
            sm_mode(Some(1), Some(1), "Haze"),     // Standard
            sm_mode(Some(0), Some(4), "Abrams"),   // Brawl -> raus
            sm_mode(Some(1), Some(1), "Vindicta"), // Standard
            sm_mode(Some(0), None, "Seven"),       // unbekannt -> zählt als Standard
        ];
        let filtered = filter_by_mode(&matches, "standard");
        let heroes: Vec<_> = filtered
            .iter()
            .map(|m| m.hero_name.clone().unwrap())
            .collect();
        // Standard = alles außer Street Brawl: nur Abrams (Brawl) fällt raus.
        assert_eq!(
            heroes,
            vec![
                "Haze".to_string(),
                "Vindicta".to_string(),
                "Seven".to_string()
            ]
        );
    }

    #[test]
    fn filter_by_mode_brawl_schliesst_standard_aus() {
        let matches = vec![
            sm_mode(Some(1), Some(1), "Haze"),   // Standard
            sm_mode(Some(0), Some(4), "Abrams"), // Brawl
            sm_mode(Some(1), Some(4), "Seven"),  // Brawl
            sm_mode(Some(0), None, "Vindicta"),  // unbekannt
        ];
        let filtered = filter_by_mode(&matches, "brawl");
        let heroes: Vec<_> = filtered
            .iter()
            .map(|m| m.hero_name.clone().unwrap())
            .collect();
        assert_eq!(heroes, vec!["Abrams".to_string(), "Seven".to_string()]);
    }

    #[test]
    fn filter_by_mode_all_enthaelt_beide_und_unbekannte() {
        let matches = vec![
            sm_mode(Some(1), Some(1), "Haze"),   // Standard
            sm_mode(Some(0), Some(4), "Abrams"), // Brawl
            sm_mode(Some(1), None, "Seven"),     // unbekannt
        ];
        // all
        assert_eq!(filter_by_mode(&matches, "all").len(), 3);
        // unbekannter Modus-String verhält sich wie all (keine Filterung)
        assert_eq!(filter_by_mode(&matches, "unsinn").len(), 3);
    }

    #[test]
    fn filter_by_mode_kombiniert_mit_not_scored_ausschluss() {
        // Modus-Filter wirkt VOR scored_matches; not_scored-Ausschluss bleibt danach.
        let matches = vec![
            sm_mode(Some(1), Some(1), "Haze"), // Standard, gewertet
            SteamMatch {
                not_scored: Some(true),
                ..sm_mode(Some(0), Some(1), "Abrams")
            }, // Standard, not_scored -> raus
            sm_mode(Some(0), Some(4), "Seven"), // Brawl -> vom Standard-Filter raus
            sm_mode(Some(0), Some(1), "Vindicta"), // Standard, gewertet
        ];
        let filtered = filter_by_mode(&matches, "standard");
        // Modus-Filter behält 3 Standard-Matches (inkl. not_scored)
        assert_eq!(filtered.len(), 3);
        // scored_matches entfernt das not_scored-Standard-Match -> 2 gewertete
        let scored = scored_matches(&filtered);
        assert_eq!(scored.len(), 2);
        let summary = summarize_matches(&filtered).unwrap();
        assert_eq!(summary.wins, 1);
        assert_eq!(summary.losses, 1);
    }

    #[tokio::test]
    async fn cached_or_compute_cancel_safety_raeumt_inflight_bei_abbruch() {
        let cache = new_test_cache();
        let key = "cancel-key".to_string();
        let (started_tx, started_rx) = oneshot::channel();

        let leader = tokio::spawn(cached_or_compute(
            cache,
            key.clone(),
            Duration::from_secs(30),
            move || async move {
                let _ = started_tx.send(());
                std::future::pending::<Value>().await
            },
        ));
        started_rx.await.unwrap();

        let waiter = tokio::spawn(cached_or_compute(
            cache,
            key.clone(),
            Duration::from_secs(30),
            || async { json!({ "source": "waiter" }) },
        ));
        sleep(Duration::from_millis(30)).await;
        assert!(
            !waiter.is_finished(),
            "zweiter Aufruf muss vor Leader-Abbruch warten"
        );

        leader.abort();
        assert!(leader.await.unwrap_err().is_cancelled());

        let waiter_body = timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(waiter_body, json!({ "source": "waiter" }));

        let follow_up = timeout(
            Duration::from_secs(2),
            cached_or_compute(cache, key, Duration::from_secs(30), || async {
                json!({ "source": "followup" })
            }),
        )
        .await
        .unwrap();
        assert_eq!(follow_up, json!({ "source": "waiter" }));
    }

    #[tokio::test]
    async fn cached_or_compute_dedup_happy_berechnet_nur_einmal() {
        let cache = new_test_cache();
        let key = "dedup-key".to_string();
        let counter = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let (started_tx, started_rx) = oneshot::channel();

        let first = tokio::spawn(cached_or_compute(
            cache,
            key.clone(),
            Duration::from_secs(30),
            {
                let counter = Arc::clone(&counter);
                let release = Arc::clone(&release);
                move || async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = started_tx.send(());
                    release.notified().await;
                    json!({ "value": "shared" })
                }
            },
        ));
        started_rx.await.unwrap();

        let second = tokio::spawn(cached_or_compute(cache, key, Duration::from_secs(30), {
            let counter = Arc::clone(&counter);
            move || async move {
                counter.fetch_add(1, Ordering::SeqCst);
                json!({ "value": "second" })
            }
        }));
        sleep(Duration::from_millis(30)).await;
        assert!(
            !second.is_finished(),
            "zweiter Aufruf muss waehrend Inflight warten"
        );

        release.notify_waiters();
        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();

        assert_eq!(first, json!({ "value": "shared" }));
        assert_eq!(second, first);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cached_or_compute_ttl_cached_innerhalb_ttl_und_erneuert_danach() {
        let cache = new_test_cache();
        let key = "ttl-key".to_string();
        let ttl = Duration::from_millis(60);
        let counter = Arc::new(AtomicUsize::new(0));

        let first = cached_or_compute(cache, key.clone(), ttl, {
            let counter = Arc::clone(&counter);
            move || async move {
                let value = counter.fetch_add(1, Ordering::SeqCst) + 1;
                json!({ "value": value })
            }
        })
        .await;
        let second = cached_or_compute(cache, key.clone(), ttl, {
            let counter = Arc::clone(&counter);
            move || async move {
                let value = counter.fetch_add(1, Ordering::SeqCst) + 1;
                json!({ "value": value })
            }
        })
        .await;

        assert_eq!(first, json!({ "value": 1 }));
        assert_eq!(second, first);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        sleep(Duration::from_millis(90)).await;
        let third = cached_or_compute(cache, key, ttl, {
            let counter = Arc::clone(&counter);
            move || async move {
                let value = counter.fetch_add(1, Ordering::SeqCst) + 1;
                json!({ "value": value })
            }
        })
        .await;

        assert_eq!(third, json!({ "value": 2 }));
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

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

    struct AssetsEnvGuard {
        previous: Option<String>,
    }

    impl AssetsEnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var("DEADLOCK_ASSETS_BASE").ok();
            std::env::set_var("DEADLOCK_ASSETS_BASE", value);
            Self { previous }
        }
    }

    impl Drop for AssetsEnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("DEADLOCK_ASSETS_BASE", previous);
            } else {
                std::env::remove_var("DEADLOCK_ASSETS_BASE");
            }
        }
    }

    struct DashboardDistEnvGuard {
        previous: Option<String>,
    }

    impl DashboardDistEnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var("DASHBOARD_V2_DIST_PATH").ok();
            std::env::set_var("DASHBOARD_V2_DIST_PATH", value);
            Self { previous }
        }
    }

    impl Drop for DashboardDistEnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("DASHBOARD_V2_DIST_PATH", previous);
            } else {
                std::env::remove_var("DASHBOARD_V2_DIST_PATH");
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
        super::clear_overlay_cache_for_tests();
        super::clear_hero_icon_cache_for_tests();
        let mock_server = MockServer::start().await;
        let _env = EnvGuard::set(&mock_server.uri());
        let _assets_env = AssetsEnvGuard::set(&mock_server.uri());
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
                    { "match_result": 1, "hero_name": "Haze" },
                    { "match_result": 1, "hero_name": "Haze" },
                    { "match_result": 0, "hero_name": "Vindicta" },
                    { "match_result": 0, "not_scored": true, "hero_name": "Seven" },
                    { "match_result": 1, "hero_name": "Haze" }
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
        Mock::given(method("GET"))
            .and(path("/v2/heroes"))
            .and(query_param("only_active", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "name": "Haze", "images": { "icon_image_small": "https://cdn.example/haze.png" } },
                { "name": "Vindicta", "images": { "icon_image_small_webp": "https://cdn.example/vindicta.webp" } }
            ])))
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

        // Hero-Icons server-seitig aufgelöst (kein Browser-fetch mehr).
        assert_eq!(first["most_played_hero"], "Haze");
        assert_eq!(first["most_played_icon"], "https://cdn.example/haze.png");
        let recent = first["recent"].as_array().unwrap();
        assert_eq!(recent[0]["hero"], "Haze");
        assert_eq!(recent[0]["hero_icon"], "https://cdn.example/haze.png");
        // Vindicta nur als webp hinterlegt -> webp-Fallback greift.
        let vindicta = recent
            .iter()
            .find(|m| m["hero"] == "Vindicta")
            .expect("Vindicta im Verlauf");
        assert_eq!(vindicta["hero_icon"], "https://cdn.example/vindicta.webp");

        mock_server.verify().await;
        // 3 Steam-Endpunkte + 1 Hero-Assets-Abruf (über beide Overlay-Calls gecacht).
        assert_eq!(mock_server.received_requests().await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn overlay_api_unbekannter_streamer_liefert_ok_false_ohne_steam_abruf() {
        let dsn = db_dsn_or_skip!();
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
        super::clear_overlay_cache_for_tests();
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
        // Glassmorphism / Visual-Spec
        assert!(html.contains("backdrop-filter: blur(12px) saturate(140%)"));
        assert!(html.contains("font-variant-numeric: tabular-nums"));
        assert!(html.contains("border-radius: var(--radius)"));
        // Themes via data-theme + Custom Properties
        assert!(html.contains("#overlay-card[data-theme=\"dark\"]"));
        assert!(html.contains("#overlay-card[data-theme=\"light\"]"));
        assert!(html.contains("#overlay-card[data-theme=\"accent\"]"));
        assert!(html.contains("linear-gradient(135deg, #06B6D4, #A855F7)"));
        assert!(html.contains("--bg: rgba(13, 15, 20, var(--bg-alpha))"));
        // Param-Parsing + Defaults
        assert!(html.contains("oneOf('theme', ['dark', 'light', 'accent'], 'dark')"));
        assert!(html.contains("oneOf('layout', ['box', 'bar'], 'box')"));
        assert!(html.contains("oneOf('mode', ['all', 'standard', 'brawl'], 'all')"));
        assert!(html.contains("oneOf('pos', ['bl', 'br', 'tl', 'tr'], 'tl')"));
        assert!(html.contains("clampInt('opacity', 0, 100, 85)"));
        assert!(html.contains("clampInt('recent_n', 1, 15, 10)"));
        assert!(html.contains("flag('lastmatch', false)"));
        assert!(html.contains("flag('mostplayed', false)"));
        assert!(html.contains("flag('header', true)"));
        assert!(html.contains("card.style.setProperty('--bg-alpha', String(opacity / 100))"));
        assert!(html.contains("card.dataset.theme = theme"));
        assert!(html.contains("card.classList.add(`layout-${layout}`)"));
        assert!(html.contains("card.classList.add(`overlay-pos-${position}`)"));
        // Daten-Endpoint + Polling + Assets
        assert!(html.contains("/twitch/api/v2/public/overlay?streamer="));
        // Spielmodus an den Daten-Fetch angehängt
        assert!(html.contains("&mode=${mode}"));
        assert!(html.contains("setInterval(poll, 20000)"));
        assert!(html.contains("Math.floor(badgeLevel / 10)"));
        assert!(html.contains("badge_lg_subrank${sub}.png"));
        assert!(html.contains("badge_lg.png"));
        // Hero-Icons werden server-seitig aufgelöst; kein Browser-Fetch der Hero-Map mehr.
        assert!(!html.contains("/v2/heroes"));
        assert!(!html.contains("loadHeroAssets"));
        assert!(html.contains("match && match.hero_icon"));
        assert!(html.contains("data.most_played_icon"));
        assert!(html.contains("Deadlock-Spiel-Assets (© Valve)"));
        // Deutsche Auto-Labels + Formatierung
        assert!(html.contains("'RANG'"));
        assert!(html.contains("'WINRATE'"));
        assert!(html.contains("'HEUTE'"));
        assert!(html.contains("'SERIE'"));
        assert!(html.contains("'K/D'"));
        assert!(html.contains("'LAST'"));
        assert!(html.contains("'MAIN'"));
        assert!(html.contains("'Match-Verlauf'"));
        assert!(html.contains("powered by deutsche-deadlock-community.de"));
        assert!(html.contains("Intl.NumberFormat('de-DE'"));
        // Recent-Strip (Hero-Kacheln, kein Farb-Klecks) + Live-Puls
        assert!(html.contains("ov-recent-row"));
        assert!(html.contains("ov-tile"));
        assert!(html.contains("ov-tile-fallback"));
        assert!(html.contains("ov-live-dot"));
        assert!(html.contains("@keyframes ov-pulse"));
        assert!(html.contains("function buildBox(data)"));
        assert!(html.contains("function buildBar(data)"));
    }

    #[test]
    fn overlay_html_enthaelt_theme_und_layout_zweige() {
        // OVERLAY_HTML ist statisch; theme/layout/opacity sind reine URL-Params,
        // die der eingebettete Script-Block clientseitig auf das Markup anwendet.
        // Der Render-Branch-Test prüft daher die Präsenz der Zweige im Template.
        let html = super::OVERLAY_HTML;
        // light/accent-Theme-Zweige
        assert!(html.contains("#overlay-card[data-theme=\"light\"]"));
        assert!(html.contains("--accent: #0891b2"));
        assert!(html.contains("#overlay-card[data-theme=\"accent\"]"));
        // Bar-Layout-Container
        assert!(html.contains("#overlay-card.layout-bar"));
        assert!(html.contains("border-radius: 999px"));
        // Box-Layout-Container
        assert!(html.contains("#overlay-card.layout-box"));
        // opacity wirkt auf Karten-Hintergrund via --bg-alpha
        assert!(html.contains("var(--bg-alpha)"));
        assert!(html.contains("--bg-alpha: 0.85"));
    }

    #[test]
    fn overlay_html_enthaelt_alle_modul_flags() {
        let html = super::OVERLAY_HTML;
        for flag in [
            "header",
            "rank",
            "winrate",
            "today",
            "streak",
            "kd",
            "lastmatch",
            "mostplayed",
            "recent",
            "live",
            "branding",
        ] {
            assert!(
                html.contains(&format!("flag('{flag}',")),
                "Modul-Flag {flag} fehlt im Render-Script"
            );
        }
        // Default an außer lastmatch/mostplayed
        assert!(html.contains("flag('header', true)"));
        assert!(html.contains("flag('recent', true)"));
        assert!(html.contains("flag('branding', true)"));
        assert!(html.contains("flag('lastmatch', false)"));
        assert!(html.contains("flag('mostplayed', false)"));
    }

    #[tokio::test]
    async fn overlay_html_route_ohne_streamer_ohne_session_leitet_zum_login() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().await;
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tb_overlay_spa_index_test_{unique}"));
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(
            root.join("index.html"),
            r#"<!doctype html><html><head><script type="module" src="/twitch/dashboard-v2/assets/app.js"></script></head><body><div id="root"></div></body></html>"#,
        )
        .await
        .unwrap();
        let _dist_env = DashboardDistEnvGuard::set(root.to_str().unwrap());

        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .expect("lazy pool");
        let app = build_public_router(pool);

        let render_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/twitch/overlay?streamer=nani")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(render_resp.status(), StatusCode::OK);
        let render_body = to_bytes(render_resp.into_body(), usize::MAX).await.unwrap();
        let render_html = String::from_utf8(render_body.to_vec()).unwrap();
        assert!(render_html.contains("id=\"overlay-card\""));
        assert!(render_html.contains("background: transparent"));

        // Ohne Streamer-Param UND ohne Session (Test-Router hat keine
        // DashboardAuthState-Extension → DashboardAuthLevel::None) leitet die
        // Builder-Shell zum Login um, statt die leere SPA-Shell auszuliefern
        // (deren auth-gegatete Assets würden sonst 303en → Weißbild).
        let spa_resp = app
            .oneshot(
                Request::builder()
                    .uri("/twitch/overlay")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            spa_resp.status().is_redirection(),
            "erwartet Login-Redirect, war {}",
            spa_resp.status()
        );
        let location = spa_resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            location.contains("/twitch/auth/login"),
            "Redirect-Ziel war {location}"
        );

        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
