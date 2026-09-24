//! Rank lookup: verified local Steam link first; public Deadlock API second.
//! Steam-name search is only a labelled candidate, never a persisted identity link.
//! API contract: https://api.deadlock-api.com/openapi.json (2026-09-17).
use crate::{command_target::CommandTarget, stats};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use sqlx::PgPool;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OnceCell};

const STEAM64_BASE: u64 = 76_561_197_960_265_728;
const PUBLIC_BASE: &str = "https://api.deadlock-api.com/v1";
const UNAVAILABLE: &str =
    "Die Spielstatistik kann ich gerade nicht abrufen. Versuch es gleich nochmal.";
const SEARCH_LIMIT: usize = 100;

/// Explicit prefix avoids confusing a numeric Twitch login with a Steam ID.
pub(crate) fn parse_steam_id(args: &str) -> Result<Option<u32>, ()> {
    let args = args.trim();
    if !args
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("steam:"))
    {
        return Ok(None);
    }
    let value = &args[6..];
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(());
    }
    let value: u64 = value.parse().map_err(|_| ())?;
    let account = if value >= STEAM64_BASE {
        value - STEAM64_BASE
    } else {
        value
    };
    u32::try_from(account)
        .ok()
        .filter(|id| *id > 0)
        .map(Some)
        .ok_or(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApiError {
    Protected,
    NotFound,
    Limited,
    Unavailable,
}

struct CacheEntry {
    created: Instant,
    cell: Arc<OnceCell<Result<Value, ApiError>>>,
    ttl: Duration,
}
#[derive(Default)]
struct Cache {
    entries: HashMap<String, CacheEntry>,
    requests: VecDeque<Instant>,
    blocked_until: Option<Instant>,
}

pub(crate) struct RankLookup {
    client: reqwest::Client,
    public_base: String,
    steam_rank_url: String,
    cache: Mutex<Cache>,
}
impl Default for RankLookup {
    fn default() -> Self {
        Self::with_urls(PUBLIC_BASE, &stats::steam_bot_rank_url())
    }
}
impl RankLookup {
    pub(crate) fn with_urls(public_base: &str, steam_rank_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            public_base: public_base.trim_end_matches('/').into(),
            steam_rank_url: steam_rank_url.into(),
            cache: Mutex::new(Cache::default()),
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, ApiError> {
        // Only fixed API paths plus validated numeric IDs are accepted by callers.
        let request = self
            .client
            .get(format!("{}{path}", self.public_base))
            .query(params)
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| ApiError::Unavailable)?;
        let key = request.url().to_string();
        let cell = {
            let mut cache = self.cache.lock().await;
            let now = Instant::now();
            cache.entries.retain(|_, entry| {
                let ttl = if entry.cell.get().is_some_and(|result| result.is_err()) {
                    Duration::from_secs(15)
                } else {
                    entry.ttl
                };
                now.duration_since(entry.created) < ttl
            });
            if let Some(entry) = cache.entries.get(&key) {
                entry.cell.clone()
            } else {
                if cache.entries.len() >= 256 {
                    if let Some(oldest) = cache
                        .entries
                        .iter()
                        .min_by_key(|(_, entry)| entry.created)
                        .map(|(key, _)| key.clone())
                    {
                        cache.entries.remove(&oldest);
                    }
                }
                let cell = Arc::new(OnceCell::new());
                let ttl = Duration::from_secs(if path == "/assets/ranks" { 3600 } else { 120 });
                cache.entries.insert(
                    key,
                    CacheEntry {
                        created: now,
                        cell: cell.clone(),
                        ttl,
                    },
                );
                cell
            }
        };
        // Concurrent requests for the same account/name share one HTTP call.
        let value = cell
            .get_or_init(|| async {
                {
                    let mut cache = self.cache.lock().await;
                    let now = Instant::now();
                    cache
                        .requests
                        .retain(|at| now.duration_since(*at) < Duration::from_secs(60));
                    if cache.blocked_until.is_some_and(|until| now < until)
                        || cache.requests.len() >= 16
                    {
                        return Err(ApiError::Limited);
                    }
                    cache.requests.push_back(now);
                }
                let response = self
                    .client
                    .execute(request)
                    .await
                    .map_err(|_| ApiError::Unavailable)?;
                match response.status().as_u16() {
                    403 => return Err(ApiError::Protected),
                    404 => return Err(ApiError::NotFound),
                    429 => {
                        let seconds = response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| value.parse::<u64>().ok())
                            .unwrap_or(60)
                            .clamp(1, 300);
                        self.cache.lock().await.blocked_until =
                            Some(Instant::now() + Duration::from_secs(seconds));
                        return Err(ApiError::Limited);
                    }
                    200 => {}
                    _ => return Err(ApiError::Unavailable),
                }
                response
                    .json::<Value>()
                    .await
                    .map_err(|_| ApiError::Unavailable)
            })
            .await
            .clone()?;
        serde_json::from_value(value).map_err(|_| ApiError::Unavailable)
    }

    pub(crate) async fn twitch_reply(
        &self,
        pool: &PgPool,
        target: &CommandTarget,
        explicit: bool,
    ) -> String {
        let link = match crate::player_links::load(pool, &target.user_id).await {
            Ok(link) => link,
            Err(_) => return UNAVAILABLE.into(),
        };
        if link.as_ref().is_some_and(|l| !l.lookup_enabled) {
            return crate::player_links::DISCONNECTED_REPLY.into();
        }
        let revision = link.as_ref().map(|l| l.revision);
        let text = match link.as_ref().filter(|l| l.steam_id64.is_some()) {
            Some(link) => {
                let Some(id) = link.account_id() else { return UNAVAILABLE.into(); };
                self.account_reply(id, &target.name, false).await
            }
            None => self.legacy_reply(pool, target, explicit).await,
        };
        // Recheck all sources, including legacy/name fallback, after network I/O.
        match crate::player_links::load(pool, &target.user_id).await {
            Ok(Some(current)) if !current.lookup_enabled => crate::player_links::DISCONNECTED_REPLY.into(),
            Ok(current) if current.as_ref().map(|l| l.revision) == revision => text,
            Ok(_) => "Die Steam-Zuordnung wurde geändert. Bitte den Rang erneut abfragen.".into(),
            Err(_) => UNAVAILABLE.into(),
        }
    }

    /// Zentrale Discord-Steam-Verknüpfung laut Steam-Bot:
    /// `Some((Deadlock-Account-ID, verified))`, `None` ohne Verknüpfung.
    pub(crate) async fn linked_account(
        &self,
        discord_id: &str,
    ) -> Result<Option<(u32, bool)>, stats::StatsError> {
        let info = stats::fetch_rank_at(&self.steam_rank_url, discord_id, false).await?;
        central_link(&info)
    }

    async fn legacy_reply(&self, pool: &PgPool, target: &CommandTarget, explicit: bool) -> String {
        let discord_id = match stats::resolve_discord_id(pool, &target.user_id).await {
            Ok(id) => id,
            Err(_) => return UNAVAILABLE.into(),
        };
        let Some(discord_id) = discord_id else {
            return if explicit {
                self.name_reply(&target.login).await
            } else {
                stats::rank_reply(&target.name, None)
            };
        };
        let primary = stats::fetch_rank_at(&self.steam_rank_url, &discord_id, false).await;
        if let Ok(info) = &primary {
            if info.linked
                && info.verified
                && info.is_steam_friend
                && info
                    .rank_name
                    .as_ref()
                    .is_some_and(|name| !name.trim().is_empty())
            {
                return stats::rank_reply(&target.name, Some(info));
            }
            if !info.linked && !explicit {
                return stats::rank_reply(&target.name, Some(info));
            }
        }
        // Never search for a different person when a real link exists, or when
        // a lookup failure prevents proving whether a link exists.
        let link_check = match primary.as_ref() {
            Ok(info) => central_link(info),
            Err(_) => Err(stats::StatsError),
        };
        match link_check {
            Ok(Some((id, true))) => self.account_reply(id, &target.name, false).await,
            Ok(Some((_, false))) => format!(
                "{} hat Steam verknüpft, aber die Steam-Verknüpfung ist noch nicht bestätigt.",
                target.name
            ),
            Ok(None) => match primary {
                Ok(info) if !info.linked && explicit => self.name_reply(&target.login).await,
                Ok(info) => stats::rank_reply(&target.name, Some(&info)),
                Err(_) => UNAVAILABLE.into(),
            },
            Err(_) => UNAVAILABLE.into(),
        }
    }

    async fn name_reply(&self, login: &str) -> String {
        let profiles = match self
            .get::<Vec<SteamProfile>>(
                "/players/steam-search",
                &[
                    ("search_query", login),
                    ("limit", "100"),
                    ("min_matches_played_last_30d", "0"),
                    ("matches_played_weight", "0"),
                ],
            )
            .await
        {
            Ok(profiles) => profiles,
            Err(ApiError::NotFound) => Vec::new(),
            Err(error) => return api_error_reply(error),
        };
        match select_profile(login, &profiles) {
            ProfileSelection::Exact(profile) => self.account_reply(profile.account_id, &profile.personaname, true).await,
            ProfileSelection::None => format!("Für @{login} gibt es noch keine Steam-Verknüpfung. Direkt geht !rank steam:<Account-ID>, sonst hier verbinden: {}", crate::player_links::CONNECT_URL),
            ProfileSelection::Ambiguous(candidates) => {
                let suggestions = candidates.into_iter().take(3).map(|profile|
                    format!("{} (!rank steam:{})", chat_label(&profile.personaname), profile.account_id)
                ).collect::<Vec<_>>().join(" | ");
                format!("@{login} lässt sich keinem Steam-Account eindeutig zuordnen. Mögliche Treffer: {suggestions}. Kein Rang automatisch zugeordnet.")
            }
        }
    }

    pub(crate) async fn account_reply(
        &self,
        account_id: u32,
        name: &str,
        name_only: bool,
    ) -> String {
        let subject = if name_only {
            format!(
                "Steam-Namensfund {} (ID {account_id}; Twitch-Zuordnung unbestätigt)",
                chat_label(name)
            )
        } else {
            chat_label(name)
        };
        let rank = match self.get::<PublicRank>(&format!("/players/{account_id}/rank"), &[]).await {
            Ok(rank) => rank,
            Err(ApiError::Protected) => return format!("{subject}: Die Deadlock API gibt für diesen Account keine öffentlichen Rangdaten frei."),
            Err(ApiError::NotFound) => return format!("{subject}: Keine öffentlichen Rangdaten gefunden."),
            Err(error) => return api_error_reply(error),
        };
        if rank.badge == 0 && rank.rank == 0 && rank.subrank == 0 {
            return format!("{subject}: Kein Rang aus einem aktuellen Ranked-Match verfügbar (Deadlock API). Das bedeutet nicht automatisch, dass der Account noch nie gerankt war.");
        }
        if rank.rank == 0
            || !(1..=6).contains(&rank.subrank)
            || rank
                .rank
                .checked_mul(10)
                .and_then(|tier| tier.checked_add(rank.subrank))
                != Some(rank.badge)
        {
            return UNAVAILABLE.into();
        }
        let ranks = self
            .get::<Vec<RankAsset>>("/assets/ranks", &[])
            .await
            .unwrap_or_default();
        let rank_name = ranks
            .iter()
            .find(|asset| asset.tier == rank.rank && !asset.name.trim().is_empty())
            .map(|asset| chat_label(&asset.name))
            .unwrap_or_else(|| format!("Rangstufe {}", rank.rank));
        let date = rank
            .last_match
            .as_ref()
            .and_then(|value| value.get("start_time"))
            .and_then(Value::as_i64)
            .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
            .map(|date| format!(" vom {}", date.format("%d.%m.%Y")))
            .unwrap_or_default();
        format!("Rang von {subject}: {rank_name} {} (Deadlock API; letztes erfasstes Ranked-Match{date}).", rank.subrank)
    }
}

fn api_error_reply(error: ApiError) -> String {
    match error {
        ApiError::Limited => {
            "Die Deadlock API ist gerade ausgelastet. Bitte versuch es in einer Minute nochmal."
                .into()
        }
        _ => UNAVAILABLE.into(),
    }
}
fn chat_label(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() && *c != '@')
        .take(40)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Zentrale Discord-Steam-Verknüpfung aus der `/rank`-Antwort des Steam-Bots:
/// `Some((Deadlock-Account-ID, verified))`, `None` ohne Verknüpfung. Fehler,
/// wenn die Verknüpfung nicht bewiesen werden kann.
fn central_link(info: &stats::RankInfo) -> Result<Option<(u32, bool)>, stats::StatsError> {
    if !info.linked {
        return Ok(None);
    }
    let steam64 = info
        .steam_id
        .as_deref()
        .ok_or(stats::StatsError)?
        .parse::<u64>()
        .map_err(|_| stats::StatsError)?;
    let id = steam64
        .checked_sub(STEAM64_BASE)
        .and_then(|id| u32::try_from(id).ok())
        .filter(|id| *id > 0)
        .ok_or(stats::StatsError)?;
    Ok(Some((id, info.verified)))
}

#[derive(Debug, Deserialize)]
struct SteamProfile {
    account_id: u32,
    personaname: String,
}
#[derive(Deserialize)]
struct PublicRank {
    badge: u32,
    rank: u32,
    subrank: u32,
    #[serde(default)]
    last_match: Option<Value>,
}
#[derive(Deserialize)]
struct RankAsset {
    tier: u32,
    name: String,
}
enum ProfileSelection<'a> {
    Exact(&'a SteamProfile),
    Ambiguous(Vec<&'a SteamProfile>),
    None,
}
fn select_profile<'a>(login: &str, profiles: &'a [SteamProfile]) -> ProfileSelection<'a> {
    let unique: BTreeMap<u32, &SteamProfile> = profiles
        .iter()
        .filter(|p| p.account_id > 0)
        .map(|p| (p.account_id, p))
        .collect();
    let exact: Vec<_> = unique
        .values()
        .copied()
        .filter(|profile| profile.personaname.trim().eq_ignore_ascii_case(login))
        .collect();
    if exact.len() == 1 && profiles.len() < SEARCH_LIMIT {
        return ProfileSelection::Exact(exact[0]);
    }
    if unique.is_empty() {
        return ProfileSelection::None;
    }
    ProfileSelection::Ambiguous(if exact.is_empty() {
        unique.values().copied().collect()
    } else {
        exact
    })
}

#[cfg(test)]
mod tests;
