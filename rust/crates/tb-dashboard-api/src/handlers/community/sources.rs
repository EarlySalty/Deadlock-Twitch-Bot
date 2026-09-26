//! Bounded, cached read-only adapters. Credentials never reach the browser.
use super::matching::{history_mode, GameMode, PlayerProfile, RANK_NAMES};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::{Arc, OnceLock}, time::{Duration, Instant}};
use tokio::sync::{Mutex, Semaphore};

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_millis(2200))
        .connect_timeout(Duration::from_millis(700)).redirect(reqwest::redirect::Policy::none())
        .build().expect("community HTTP client"))
}

#[derive(Default, Deserialize)]
struct Rank {
    #[serde(default)] linked: bool,
    #[serde(default)] verified: bool,
    #[serde(default)] is_steam_friend: bool,
    rank_name: Option<String>, subrank: Option<u8>, rank_updated_at: Option<i64>,
}
#[derive(Default, Deserialize)]
struct Matches {
    #[serde(default)] linked: bool,
    #[serde(default)] stale: bool,
    updated_at: Option<i64>,
    #[serde(default)] matches: Vec<Value>,
}

fn fresh(timestamp: Option<i64>, now: i64, max_age: i64) -> bool {
    timestamp.is_some_and(|at| at <= now + 5 && at >= now - max_age)
}

fn project_profile(rank: Option<Rank>, matches: Option<Matches>, now: i64) -> PlayerProfile {
    let mut profile = PlayerProfile { source_status: "unavailable".into(), ..Default::default() };
    let Some(rank) = rank else { return profile; };
    if !rank.linked || !rank.verified || !rank.is_steam_friend {
        profile.source_status = "link_unconfirmed".into(); return profile;
    }
    profile.source_status = "partial".into();
    if fresh(rank.rank_updated_at, now, 14 * 86400) {
        if let Some((index, name)) = rank.rank_name.as_deref().and_then(|name| RANK_NAMES.iter().enumerate().find(|(_, value)| value.eq_ignore_ascii_case(name))) {
            profile.rank_name = Some((*name).into()); profile.rank_tier = Some((index+1) as u8);
            profile.subrank = rank.subrank.filter(|n| (1..=6).contains(n)); profile.rank_updated_at = rank.rank_updated_at;
        }
    }
    if let Some(matches) = matches.filter(|m| m.linked && !m.stale && fresh(m.updated_at, now, 900)) {
        (profile.mode, profile.mode_samples) = history_mode(&matches.matches, now);
        profile.mode_source = profile.mode.map(|_| "steam_history".into());
        profile.history_updated_at = matches.updated_at;
        profile.source_status = "ok".into();
    }
    profile
}

type CacheEntry = Arc<Mutex<Option<(Instant, PlayerProfile)>>>;

pub async fn player_profile(discord_id: &str) -> PlayerProfile {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    static LIMIT: OnceLock<Semaphore> = OnceLock::new();
    if !valid_id(discord_id) { return PlayerProfile { source_status:"not_linked".into(),..Default::default() }; }
    let entry = {
        let mut cache = CACHE.get_or_init(Default::default).lock().await;
        // Only configured partner identities enter this cache. Still bound memory.
        if cache.len() >= 1024 && !cache.contains_key(discord_id) { cache.clear(); }
        cache.entry(discord_id.to_string()).or_insert_with(|| Arc::new(Mutex::new(None))).clone()
    };
    // Per-identity single flight; never hold the map lock over network I/O.
    let mut cached = entry.lock().await;
    if let Some((expires, value)) = cached.as_ref().filter(|(expires,_)| *expires > Instant::now()) {
        let _ = expires; return value.clone();
    }
    let _permit = LIMIT.get_or_init(|| Semaphore::new(4)).acquire().await.expect("community semaphore open");
    let rank_url = std::env::var("STEAM_BOT_RANK_URL").ok().filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8783/rank".into());
    let base = rank_url.trim_end_matches('/').strip_suffix("/rank").unwrap_or(rank_url.trim_end_matches('/'));
    let matches_url = format!("{base}/player-matches");
    let rank_fetch = async { client().get(&rank_url).query(&[("discord_id",discord_id),("cached_only","1")]).send().await?.error_for_status()?.json::<Rank>().await };
    let matches_fetch = async { client().get(&matches_url).query(&[("discord_id",discord_id),("limit","30"),("cached_only","1")]).send().await?.error_for_status()?.json::<Matches>().await };
    let (rank, matches): (Result<Rank,reqwest::Error>,Result<Matches,reqwest::Error>) = tokio::join!(rank_fetch,matches_fetch);
    if rank.is_err() || matches.is_err() { tracing::debug!("Community Steam enrichment partly unavailable"); }
    let result = project_profile(rank.ok(),matches.ok(),Utc::now().timestamp());
    let ttl = if result.source_status == "ok" { 300 } else { 60 };
    *cached = Some((Instant::now()+Duration::from_secs(ttl),result.clone()));
    result
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lobby {
    pub channel_id: String,
    pub name: String,
    pub member_count: usize,
    pub user_limit: Option<u32>,
    pub mode: Option<GameMode>,
    pub intent: Option<String>,
    pub rank_average: Option<f64>,
    pub rank_samples: usize,
    pub requester_present: bool,
    pub is_streamer_vc: bool,
    #[serde(default)] pub joinable: bool,
    #[serde(default)] pub slots_free: Option<u32>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct LobbyResult {
    pub status: String,
    pub captured_at: Option<i64>,
    pub lobbies: Vec<Lobby>,
}
impl LobbyResult {
    fn unavailable(status: &str) -> Self { Self {status: status.into(), captured_at:None,lobbies:vec![]} }
}
#[derive(Deserialize)]
struct Envelope { ok: bool, result: Option<Directory> }
#[derive(Deserialize)]
struct Directory { captured_at: i64, lobbies: Vec<Lobby> }

pub fn valid_id(id: &str) -> bool { (1..=20).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_digit()) && id.parse::<u64>().is_ok_and(|v| v>0) }

fn project_directory(mut data: Directory, now: i64) -> LobbyResult {
    if !fresh(Some(data.captured_at),now,60) { return LobbyResult::unavailable("stale"); }
    data.lobbies.retain(|l| valid_id(&l.channel_id) && l.member_count > 0 && l.member_count <= 1000);
    data.lobbies.truncate(100);
    for lobby in &mut data.lobbies {
        lobby.name = lobby.name.chars().filter(|c| !c.is_control()).take(100).collect();
        lobby.user_limit = lobby.user_limit.filter(|n| *n > 0);
        lobby.slots_free = lobby.user_limit.map(|limit| limit.saturating_sub(lobby.member_count as u32));
        lobby.joinable = lobby.slots_free != Some(0);
        if lobby.rank_samples == 0 || !lobby.rank_average.is_some_and(|rank| rank.is_finite() && (1.0..=11.6).contains(&rank)) {
            lobby.rank_average = None; lobby.rank_samples = 0;
        }
    }
    LobbyResult {status:"ok".into(),captured_at:Some(data.captured_at),lobbies:data.lobbies}
}

/// `discord_id` comes exclusively from the *viewer's* server-side identity,
/// never from a selected streamer or a browser-supplied Discord ID.
pub async fn lobbies(discord_id: Option<&str>) -> LobbyResult {
    let Some(id) = discord_id.filter(|id| valid_id(id)) else { return LobbyResult::unavailable("link_required"); };
    let token = ["MASTER_BROKER_TOKEN","MAIN_BOT_INTERNAL_TOKEN","TWITCH_INTERNAL_API_TOKEN"].iter()
        .find_map(|key| std::env::var(key).ok().filter(|s| !s.trim().is_empty()));
    let Some(token) = token else { return LobbyResult::unavailable("unavailable"); };
    let Ok(config) = tb_config::runtime::settings() else { return LobbyResult::unavailable("unavailable"); };
    let base = &config.broker.base_url;
    let response = client().post(format!("{}/internal/master/v1/discord/community-lobbies",base.trim_end_matches('/')))
        .header("X-Internal-Token",token).json(&serde_json::json!({"guild_id":"1289721245281292288","user_id":id})).send().await;
    let Ok(response) = response else { return LobbyResult::unavailable("unavailable"); };
    if response.status() == reqwest::StatusCode::FORBIDDEN { return LobbyResult::unavailable("membership_unconfirmed"); }
    let Ok(response) = response.error_for_status() else { return LobbyResult::unavailable("unavailable"); };
    match response.json::<Envelope>().await {
        Ok(Envelope {ok:true,result:Some(data)}) => project_directory(data,Utc::now().timestamp()),
        _ => LobbyResult::unavailable("unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn stale_and_unverified_ranks_are_not_published() {
        let now=1_800_000_000;
        let rank=Rank {linked:true,verified:true,is_steam_friend:true,rank_name:Some("Oracle".into()),subrank:Some(3),rank_updated_at:Some(now-15*86400)};
        assert_eq!(project_profile(Some(rank),None,now).rank_tier,None);
        let rank=Rank {linked:true,rank_name:Some("Oracle".into()),rank_updated_at:Some(now),..Default::default()};
        assert_eq!(project_profile(Some(rank),None,now).rank_tier,None);
    }
    #[test] fn fresh_confirmed_rank_is_available_without_match_history() {
        let now=1_800_000_000;
        let rank=Rank {linked:true,verified:true,is_steam_friend:true,rank_name:Some("Oracle".into()),subrank:Some(3),rank_updated_at:Some(now)};
        let result=project_profile(Some(rank),None,now);
        assert_eq!(result.rank_tier,Some(8)); assert_eq!(result.mode,None); assert_eq!(result.source_status,"partial");
    }
    #[test] fn stale_or_future_directory_is_closed() {
        assert_eq!(project_directory(Directory{captured_at:100,lobbies:vec![]},200).status,"stale");
        assert_eq!(project_directory(Directory{captured_at:300,lobbies:vec![]},200).status,"stale");
    }
    #[test] fn full_lobby_cannot_be_advertised_as_joinable() {
        let lobby: Lobby=serde_json::from_value(serde_json::json!({"channel_id":"1326984426906714236","name":"Voice","member_count":6,"user_limit":6,"mode":null,"intent":null,"rank_average":null,"rank_samples":0,"requester_present":false,"is_streamer_vc":true})).unwrap();
        let data=project_directory(Directory{captured_at:200,lobbies:vec![lobby]},200);
        assert!(!data.lobbies[0].joinable); assert_eq!(data.lobbies[0].slots_free,Some(0));
    }
}
