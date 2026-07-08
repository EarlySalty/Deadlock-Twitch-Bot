//! `!rank`-Kette: twitch_user_id -> discord_user_id (twitch_streamer_identities)
//! -> HTTP an den Steam-Bot (/rank) -> Chat-Antwort.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use sqlx::PgPool;

const DEFAULT_STEAM_BOT_RANK_URL: &str = "http://127.0.0.1:8783/rank";
const DEFAULT_STEAM_BOT_MATCHES_URL: &str = "http://127.0.0.1:8783/player-matches";
const DEFAULT_STEAM_BOT_MMR_TREND_URL: &str = "http://127.0.0.1:8783/player-mmr-trend";
const DEFAULT_STEAM_BOT_LIVE_URL: &str = "http://127.0.0.1:8783/player-live";

#[derive(Debug, Clone, Deserialize)]
pub struct RankInfo {
    pub linked: bool,
    pub rank_name: Option<String>,
    pub subrank: Option<i64>,
    pub badge_level: Option<i64>,
    pub wins: Option<i64>,
    pub losses: Option<i64>,
    pub matches: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatchEntry {
    #[serde(default)]
    pub match_result: Option<i64>,
    #[serde(default)]
    pub hero_id: Option<i64>,
    #[serde(default)]
    pub hero_name: Option<String>,
    #[serde(default)]
    pub player_kills: Option<i64>,
    #[serde(default)]
    pub player_deaths: Option<i64>,
    #[serde(default)]
    pub player_assists: Option<i64>,
    #[serde(default)]
    pub not_scored: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatchHistory {
    #[serde(default)]
    pub linked: bool,
    #[serde(default)]
    pub matches: Vec<MatchEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MmrTrend {
    #[serde(default)]
    pub linked: bool,
    #[serde(default)]
    pub current_rank_name: Option<String>,
    #[serde(default)]
    pub current_badge: Option<i64>,
    #[serde(default)]
    pub delta: Option<i64>,
    #[serde(default)]
    pub days: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveStatus {
    #[serde(default)]
    pub linked: bool,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub in_deadlock: bool,
    #[serde(default)]
    pub hero: Option<String>,
    #[serde(default)]
    pub minutes: Option<i64>,
    #[serde(default)]
    pub stage: Option<String>,
}

pub async fn resolve_discord_id(pool: &PgPool, twitch_user_id: &str) -> Option<String> {
    let discord_user_id = sqlx::query_scalar!(
        "SELECT discord_user_id \
         FROM twitch_streamer_identities \
         WHERE twitch_user_id = $1",
        twitch_user_id,
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    discord_user_id
        .flatten()
        .map(|discord_user_id| discord_user_id.trim().to_string())
        .filter(|discord_user_id| !discord_user_id.is_empty())
}

pub async fn fetch_rank(discord_id: &str, include_stats: bool) -> Option<RankInfo> {
    let rank_url = steam_bot_rank_url();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(if include_stats { 8 } else { 5 }))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Steam-Bot Rank: HTTP-Client konnte nicht gebaut werden");
            return None;
        }
    };
    let mut request = client.get(rank_url).query(&[("discord_id", discord_id)]);
    if include_stats {
        request = request.query(&[("include_stats", "1")]);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot Rank: Request fehlgeschlagen");
            return None;
        }
    };
    if !response.status().is_success() {
        tracing::warn!(
            status = response.status().as_u16(),
            discord_id,
            "Steam-Bot Rank: Non-2xx"
        );
        return None;
    }
    match response.json::<RankInfo>().await {
        Ok(info) => Some(info),
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot Rank: JSON nicht lesbar");
            None
        }
    }
}

pub async fn fetch_matches(discord_id: &str) -> Option<MatchHistory> {
    let matches_url = steam_bot_matches_url();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Steam-Bot Matches: HTTP-Client konnte nicht gebaut werden");
            return None;
        }
    };
    let response = match client
        .get(matches_url)
        .query(&[("discord_id", discord_id), ("limit", "150")])
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot Matches: Request fehlgeschlagen");
            return None;
        }
    };
    if !response.status().is_success() {
        tracing::warn!(
            status = response.status().as_u16(),
            discord_id,
            "Steam-Bot Matches: Non-2xx"
        );
        return None;
    }
    match response.json::<MatchHistory>().await {
        Ok(history) => Some(history),
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot Matches: JSON nicht lesbar");
            None
        }
    }
}

pub async fn fetch_mmr_trend(discord_id: &str) -> Option<MmrTrend> {
    let trend_url = steam_bot_mmr_trend_url();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Steam-Bot MMR-Trend: HTTP-Client konnte nicht gebaut werden");
            return None;
        }
    };
    let response = match client
        .get(trend_url)
        .query(&[("discord_id", discord_id), ("days", "7")])
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot MMR-Trend: Request fehlgeschlagen");
            return None;
        }
    };
    if !response.status().is_success() {
        tracing::warn!(
            status = response.status().as_u16(),
            discord_id,
            "Steam-Bot MMR-Trend: Non-2xx"
        );
        return None;
    }
    match response.json::<MmrTrend>().await {
        Ok(trend) => Some(trend),
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot MMR-Trend: JSON nicht lesbar");
            None
        }
    }
}

pub async fn fetch_live(discord_id: &str) -> Option<LiveStatus> {
    let live_url = steam_bot_live_url();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Steam-Bot Live: HTTP-Client konnte nicht gebaut werden");
            return None;
        }
    };
    let response = match client
        .get(live_url)
        .query(&[("discord_id", discord_id)])
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot Live: Request fehlgeschlagen");
            return None;
        }
    };
    if !response.status().is_success() {
        tracing::warn!(
            status = response.status().as_u16(),
            discord_id,
            "Steam-Bot Live: Non-2xx"
        );
        return None;
    }
    match response.json::<LiveStatus>().await {
        Ok(live) => Some(live),
        Err(error) => {
            tracing::warn!(%error, discord_id, "Steam-Bot Live: JSON nicht lesbar");
            None
        }
    }
}

pub fn rank_reply(name: &str, info: Option<&RankInfo>) -> String {
    match info {
        Some(info) if info.linked => match &info.rank_name {
            Some(rank) => match info.subrank {
                Some(subrank @ 1..=6) => format!("Rang von {name}: {rank} {subrank}"),
                _ => format!("Rang von {name}: {rank}"),
            },
            None => {
                format!("{name} hat einen verknüpften Account, aber noch keinen Rang erkannt.")
            }
        },
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

/// Antwort auf `!wins`: zeigt die Karriere-Siege. Deadlocks GC liefert über
/// diesen Pfad KEINE verlässliche Gesamt-Match-Zahl (nur Siege sind sauber
/// rekonstruierbar), daher bewusst nur Siege — keine erfundene Bilanz/Winrate.
pub fn wins_reply(name: &str, info: Option<&RankInfo>) -> String {
    match info {
        Some(info) if info.linked => match info.wins {
            Some(w) => format!("{name}: {w} Siege in Deadlock."),
            None => format!("{name}: Für deinen Account liegen noch keine Sieg-Daten vor."),
        },
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

pub fn winrate_reply(name: &str, mh: Option<&MatchHistory>) -> String {
    match mh {
        Some(mh) if mh.linked => {
            let s = scored(&mh.matches);
            let w = s.iter().filter(|m| m.match_result == Some(1)).count();
            let l = s.iter().filter(|m| m.match_result == Some(0)).count();
            if w + l == 0 {
                return format!("{name}: Noch keine gewerteten Deadlock-Spiele gefunden.");
            }
            let wr = 100.0 * w as f64 / ((w + l) as f64);
            format!(
                "{name}: {wr:.1}% Winrate über die letzten {} Spiele ({w}S/{l}N).",
                w + l
            )
        }
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

pub fn mmr_reply(name: &str, t: Option<&MmrTrend>) -> String {
    match t {
        Some(t) if t.linked => match &t.current_rank_name {
            Some(rank) => {
                let d = t.days.unwrap_or(7);
                match t.delta {
                    Some(d2) if d2 > 0 => {
                        format!("{name}: Rang {rank} — {d2} Stufen hoch in den letzten {d} Tagen.")
                    }
                    Some(d2) if d2 < 0 => {
                        let abs = d2.abs();
                        format!("{name}: Rang {rank} — {abs} Stufen runter in den letzten {d} Tagen.")
                    }
                    _ => format!("{name}: Rang {rank} — stabil in den letzten {d} Tagen."),
                }
            }
            None => format!(
                "{name}: Noch keine Rang-Historie — der Trend baut sich ab jetzt auf."
            ),
        },
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

pub fn live_reply(name: &str, s: Option<&LiveStatus>) -> String {
    match s {
        Some(s) if s.linked => {
            if s.live {
                let mut reply = format!("{name} ist gerade live in Deadlock");
                if let Some(hero) = &s.hero {
                    reply.push_str(&format!(" als {hero}"));
                }
                if let Some(minutes) = s.minutes {
                    reply.push_str(&format!(" (seit {minutes} Min)"));
                }
                reply.push('.');
                reply
            } else if s.in_deadlock {
                format!("{name} ist gerade in Deadlock, aber in keinem laufenden Match.")
            } else {
                format!("{name} ist gerade nicht in Deadlock.")
            }
        }
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

pub fn lastmatch_reply(name: &str, mh: Option<&MatchHistory>) -> String {
    match mh {
        Some(mh) if mh.linked => {
            let Some(m) = mh.matches.first() else {
                return format!("{name}: Noch kein letztes Deadlock-Spiel gefunden.");
            };
            let outcome = match m.match_result {
                Some(1) => "Sieg",
                Some(0) => "Niederlage",
                _ => "Spiel",
            };
            let hero = m.hero_name.as_deref().unwrap_or("unbekanntem Hero");
            let k = m.player_kills.unwrap_or(0);
            let d = m.player_deaths.unwrap_or(0);
            let a = m.player_assists.unwrap_or(0);
            format!("{name}: Letztes Spiel — {outcome} als {hero} ({k}/{d}/{a}).")
        }
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

pub fn streak_reply(name: &str, mh: Option<&MatchHistory>) -> String {
    match mh {
        Some(mh) if mh.linked => {
            let s = scored(&mh.matches);
            let Some(first) = s.first() else {
                return format!("{name}: Noch keine gewerteten Deadlock-Spiele gefunden.");
            };
            let sign = first.match_result;
            let n = s.iter().take_while(|m| m.match_result == sign).count();
            match sign {
                Some(1) if n >= 2 => format!("{name}: {n} Siege in Folge!"),
                Some(1) => format!("{name}: Letztes Spiel gewonnen."),
                Some(0) if n >= 2 => format!("{name}: {n} Niederlagen in Folge."),
                Some(0) => format!("{name}: Letztes Spiel verloren."),
                _ => format!("{name}: Letztes Spiel ohne klares Ergebnis."),
            }
        }
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

pub fn mostplayed_reply(name: &str, mh: Option<&MatchHistory>) -> String {
    match mh {
        Some(mh) if mh.linked => {
            if mh.matches.is_empty() {
                return format!("{name}: Noch keine Deadlock-Spiele gefunden.");
            }

            let mut counts: HashMap<Option<i64>, usize> = HashMap::new();
            let mut top_hero_id = mh.matches[0].hero_id;
            let mut top_count = 0;
            for m in &mh.matches {
                let count = counts.entry(m.hero_id).or_default();
                *count += 1;
                if *count > top_count {
                    top_hero_id = m.hero_id;
                    top_count = *count;
                }
            }

            let total = mh.matches.len();
            let hero = mh
                .matches
                .iter()
                .find(|m| m.hero_id == top_hero_id)
                .and_then(|m| m.hero_name.as_deref())
                .unwrap_or("unbekannter Hero");
            format!("{name}: Meistgespielt zuletzt — {hero} ({top_count} von {total} Spielen).")
        }
        _ => format!(
            "{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        ),
    }
}

fn steam_bot_rank_url() -> String {
    std::env::var("STEAM_BOT_RANK_URL")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_else(|| DEFAULT_STEAM_BOT_RANK_URL.to_string())
}

fn steam_bot_matches_url() -> String {
    std::env::var("STEAM_BOT_RANK_URL")
        .ok()
        .map(|value| matches_url_from_rank(&value))
        .unwrap_or_else(|| DEFAULT_STEAM_BOT_MATCHES_URL.to_string())
}

fn steam_bot_mmr_trend_url() -> String {
    std::env::var("STEAM_BOT_RANK_URL")
        .ok()
        .map(|value| mmr_trend_url_from_rank(&value))
        .unwrap_or_else(|| DEFAULT_STEAM_BOT_MMR_TREND_URL.to_string())
}

fn steam_bot_live_url() -> String {
    std::env::var("STEAM_BOT_RANK_URL")
        .ok()
        .map(|value| live_url_from_rank(&value))
        .unwrap_or_else(|| DEFAULT_STEAM_BOT_LIVE_URL.to_string())
}

fn matches_url_from_rank(rank_url: &str) -> String {
    let trimmed = rank_url.trim();
    if trimmed.is_empty() {
        return DEFAULT_STEAM_BOT_MATCHES_URL.to_string();
    }

    if let Some(base) = trimmed.strip_suffix("/rank") {
        return format!("{base}/player-matches");
    }

    let base = trimmed.trim_end_matches('/');
    if base.ends_with("/player-matches") {
        base.to_string()
    } else {
        format!("{base}/player-matches")
    }
}

fn mmr_trend_url_from_rank(rank_url: &str) -> String {
    let trimmed = rank_url.trim();
    if trimmed.is_empty() {
        return DEFAULT_STEAM_BOT_MMR_TREND_URL.to_string();
    }

    if let Some(base) = trimmed.strip_suffix("/rank") {
        return format!("{base}/player-mmr-trend");
    }

    let base = trimmed.trim_end_matches('/');
    if base.ends_with("/player-mmr-trend") {
        base.to_string()
    } else {
        format!("{base}/player-mmr-trend")
    }
}

fn live_url_from_rank(rank_url: &str) -> String {
    let trimmed = rank_url.trim();
    if trimmed.is_empty() {
        return DEFAULT_STEAM_BOT_LIVE_URL.to_string();
    }

    if let Some(base) = trimmed.strip_suffix("/rank") {
        return format!("{base}/player-live");
    }

    let base = trimmed.trim_end_matches('/');
    if base.ends_with("/player-live") {
        base.to_string()
    } else {
        format!("{base}/player-live")
    }
}

fn scored(m: &[MatchEntry]) -> Vec<&MatchEntry> {
    m.iter()
        .filter(|entry| entry.not_scored != Some(true))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked_history(matches: Vec<MatchEntry>) -> MatchHistory {
        MatchHistory {
            linked: true,
            matches,
        }
    }

    fn linked_mmr(rank: Option<&str>, delta: Option<i64>, days: Option<i64>) -> MmrTrend {
        MmrTrend {
            linked: true,
            current_rank_name: rank.map(String::from),
            current_badge: Some(56),
            delta,
            days,
        }
    }

    fn linked_live(
        live: bool,
        in_deadlock: bool,
        hero: Option<&str>,
        minutes: Option<i64>,
    ) -> LiveStatus {
        LiveStatus {
            linked: true,
            live,
            in_deadlock,
            hero: hero.map(String::from),
            minutes,
            stage: None,
        }
    }

    fn entry(
        match_result: i64,
        hero_id: Option<i64>,
        hero_name: Option<&str>,
        player_kills: Option<i64>,
        player_deaths: Option<i64>,
        player_assists: Option<i64>,
        not_scored: Option<bool>,
    ) -> MatchEntry {
        MatchEntry {
            match_result: Some(match_result),
            hero_id,
            hero_name: hero_name.map(String::from),
            player_kills,
            player_deaths,
            player_assists,
            not_scored,
        }
    }

    fn result_entry(match_result: i64) -> MatchEntry {
        entry(match_result, None, None, None, None, None, None)
    }

    fn unscored_result_entry(match_result: i64) -> MatchEntry {
        entry(match_result, None, None, None, None, None, Some(true))
    }

    #[test]
    fn rang_vorhanden() {
        let info = RankInfo {
            linked: true,
            rank_name: Some("Archon".into()),
            subrank: None,
            badge_level: Some(61),
            wins: None,
            losses: None,
            matches: None,
        };
        assert_eq!(rank_reply("nani", Some(&info)), "Rang von nani: Archon");
    }

    #[test]
    fn rang_mit_subrank() {
        let info = RankInfo {
            linked: true,
            rank_name: Some("Phantom".into()),
            subrank: Some(1),
            badge_level: Some(91),
            wins: None,
            losses: None,
            matches: None,
        };
        assert_eq!(rank_reply("nani", Some(&info)), "Rang von nani: Phantom 1");
    }

    #[test]
    fn verknuepft_ohne_rang() {
        let info = RankInfo {
            linked: true,
            rank_name: None,
            subrank: None,
            badge_level: None,
            wins: None,
            losses: None,
            matches: None,
        };
        assert!(rank_reply("nani", Some(&info)).contains("noch keinen Rang"));
    }

    #[test]
    fn nicht_verknuepft() {
        let info = RankInfo {
            linked: false,
            rank_name: None,
            subrank: None,
            badge_level: None,
            wins: None,
            losses: None,
            matches: None,
        };
        assert!(rank_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
        assert!(rank_reply("nani", None).contains("noch keinen Steam-Account"));
    }

    #[test]
    fn wins_reply_mit_siegen() {
        let info = RankInfo {
            linked: true,
            rank_name: None,
            subrank: None,
            badge_level: None,
            wins: Some(1164),
            losses: None,
            matches: None,
        };

        let reply = wins_reply("nani", Some(&info));

        assert!(reply.contains("1164"));
        assert!(reply.contains("Siege"));
    }

    #[test]
    fn wins_reply_verknuepft_ohne_stats() {
        let info = RankInfo {
            linked: true,
            rank_name: None,
            subrank: None,
            badge_level: None,
            wins: None,
            losses: None,
            matches: None,
        };

        assert!(wins_reply("nani", Some(&info)).contains("noch keine Sieg-Daten"));
    }

    #[test]
    fn wins_reply_nicht_verknuepft() {
        let info = RankInfo {
            linked: false,
            rank_name: None,
            subrank: None,
            badge_level: None,
            wins: None,
            losses: None,
            matches: None,
        };

        assert!(wins_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
    }

    #[test]
    fn winrate_reply_berechnet_gewertete_spiele() {
        let matches = (0..6)
            .map(|_| MatchEntry {
                match_result: Some(1),
                hero_id: None,
                hero_name: None,
                player_kills: None,
                player_deaths: None,
                player_assists: None,
                not_scored: None,
            })
            .chain((0..4).map(|_| MatchEntry {
                match_result: Some(0),
                hero_id: None,
                hero_name: None,
                player_kills: None,
                player_deaths: None,
                player_assists: None,
                not_scored: None,
            }))
            .collect();
        let info = MatchHistory {
            linked: true,
            matches,
        };

        assert_eq!(
            winrate_reply("nani", Some(&info)),
            "nani: 60.0% Winrate über die letzten 10 Spiele (6S/4N)."
        );
    }

    #[test]
    fn winrate_reply_ignoriert_not_scored_matches() {
        let matches = (0..6)
            .map(|_| result_entry(1))
            .chain((0..4).map(|_| result_entry(0)))
            .chain((0..3).map(|i| unscored_result_entry(i % 2)))
            .collect();
        let info = linked_history(matches);

        assert_eq!(
            winrate_reply("nani", Some(&info)),
            "nani: 60.0% Winrate über die letzten 10 Spiele (6S/4N)."
        );
    }

    #[test]
    fn winrate_reply_meldet_fehlende_gewertete_spiele() {
        let only_unscored = linked_history(vec![
            unscored_result_entry(1),
            unscored_result_entry(0),
            unscored_result_entry(1),
        ]);
        let empty = linked_history(Vec::new());

        assert_eq!(
            winrate_reply("nani", Some(&only_unscored)),
            "nani: Noch keine gewerteten Deadlock-Spiele gefunden."
        );
        assert_eq!(
            winrate_reply("nani", Some(&empty)),
            "nani: Noch keine gewerteten Deadlock-Spiele gefunden."
        );
    }

    #[test]
    fn winrate_reply_nicht_verknuepft() {
        let info = MatchHistory {
            linked: false,
            matches: Vec::new(),
        };

        assert!(winrate_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
        assert!(winrate_reply("nani", None).contains("noch keinen Steam-Account"));
    }

    #[test]
    fn mmr_reply_meldet_positiven_trend() {
        let info = linked_mmr(Some("Oracle"), Some(2), Some(7));

        assert_eq!(
            mmr_reply("nani", Some(&info)),
            "nani: Rang Oracle — 2 Stufen hoch in den letzten 7 Tagen."
        );
    }

    #[test]
    fn mmr_reply_meldet_negativen_trend() {
        let info = linked_mmr(Some("Oracle"), Some(-3), Some(7));

        assert_eq!(
            mmr_reply("nani", Some(&info)),
            "nani: Rang Oracle — 3 Stufen runter in den letzten 7 Tagen."
        );
    }

    #[test]
    fn mmr_reply_meldet_stabil_bei_delta_null() {
        let info = linked_mmr(Some("Oracle"), Some(0), Some(7));

        assert_eq!(
            mmr_reply("nani", Some(&info)),
            "nani: Rang Oracle — stabil in den letzten 7 Tagen."
        );
    }

    #[test]
    fn mmr_reply_meldet_stabil_bei_fehlendem_delta() {
        let info = linked_mmr(Some("Oracle"), None, None);

        assert_eq!(
            mmr_reply("nani", Some(&info)),
            "nani: Rang Oracle — stabil in den letzten 7 Tagen."
        );
    }

    #[test]
    fn mmr_reply_meldet_fehlende_historie() {
        let info = linked_mmr(None, Some(2), Some(7));

        assert_eq!(
            mmr_reply("nani", Some(&info)),
            "nani: Noch keine Rang-Historie — der Trend baut sich ab jetzt auf."
        );
    }

    #[test]
    fn mmr_reply_nicht_verknuepft() {
        let info = MmrTrend {
            linked: false,
            current_rank_name: None,
            current_badge: None,
            delta: None,
            days: None,
        };

        assert_eq!(
            mmr_reply("nani", Some(&info)),
            "nani hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        );
        assert_eq!(
            mmr_reply("nani", None),
            "nani hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        );
    }

    #[test]
    fn live_reply_meldet_live_mit_hero_und_minuten() {
        let info = linked_live(true, true, Some("Haze"), Some(7));

        assert_eq!(
            live_reply("X", Some(&info)),
            "X ist gerade live in Deadlock als Haze (seit 7 Min)."
        );
    }

    #[test]
    fn live_reply_meldet_live_ohne_hero_und_minuten() {
        let info = linked_live(true, true, None, None);

        assert_eq!(
            live_reply("X", Some(&info)),
            "X ist gerade live in Deadlock."
        );
    }

    #[test]
    fn live_reply_meldet_in_deadlock_aber_nicht_live() {
        let info = linked_live(false, true, None, None);

        assert_eq!(
            live_reply("X", Some(&info)),
            "X ist gerade in Deadlock, aber in keinem laufenden Match."
        );
    }

    #[test]
    fn live_reply_meldet_gar_nicht_in_deadlock() {
        let info = linked_live(false, false, None, None);

        assert_eq!(
            live_reply("X", Some(&info)),
            "X ist gerade nicht in Deadlock."
        );
    }

    #[test]
    fn live_reply_nicht_verknuepft() {
        let info = LiveStatus {
            linked: false,
            live: false,
            in_deadlock: false,
            hero: None,
            minutes: None,
            stage: None,
        };

        assert_eq!(
            live_reply("X", Some(&info)),
            "X hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        );
        assert_eq!(
            live_reply("X", None),
            "X hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."
        );
    }

    #[test]
    fn lastmatch_reply_zeigt_letztes_spiel() {
        let info = MatchHistory {
            linked: true,
            matches: vec![MatchEntry {
                match_result: Some(1),
                hero_id: Some(13),
                hero_name: Some("Haze".into()),
                player_kills: Some(4),
                player_deaths: Some(10),
                player_assists: Some(9),
                not_scored: None,
            }],
        };

        let reply = lastmatch_reply("nani", Some(&info));

        assert_eq!(reply, "nani: Letztes Spiel — Sieg als Haze (4/10/9).");
    }

    #[test]
    fn lastmatch_reply_zeigt_niederlage() {
        let info = linked_history(vec![entry(
            0,
            Some(13),
            Some("Haze"),
            Some(4),
            Some(10),
            Some(9),
            None,
        )]);

        assert_eq!(
            lastmatch_reply("nani", Some(&info)),
            "nani: Letztes Spiel — Niederlage als Haze (4/10/9)."
        );
    }

    #[test]
    fn lastmatch_reply_nutzt_unbekannten_hero_ohne_hero_name() {
        let info = linked_history(vec![entry(
            1,
            Some(13),
            None,
            Some(4),
            Some(10),
            Some(9),
            None,
        )]);

        assert_eq!(
            lastmatch_reply("nani", Some(&info)),
            "nani: Letztes Spiel — Sieg als unbekanntem Hero (4/10/9)."
        );
    }

    #[test]
    fn lastmatch_reply_nimmt_das_neueste_spiel_zuerst() {
        let info = linked_history(vec![
            entry(1, Some(13), Some("Haze"), Some(4), Some(10), Some(9), None),
            entry(0, Some(6), Some("Abrams"), Some(11), Some(3), Some(7), None),
        ]);

        assert_eq!(
            lastmatch_reply("nani", Some(&info)),
            "nani: Letztes Spiel — Sieg als Haze (4/10/9)."
        );
    }

    #[test]
    fn lastmatch_reply_meldet_leere_historie() {
        let info = linked_history(Vec::new());

        assert_eq!(
            lastmatch_reply("nani", Some(&info)),
            "nani: Noch kein letztes Deadlock-Spiel gefunden."
        );
    }

    #[test]
    fn lastmatch_reply_nicht_verknuepft() {
        let info = MatchHistory {
            linked: false,
            matches: Vec::new(),
        };

        assert!(lastmatch_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
        assert!(lastmatch_reply("nani", None).contains("noch keinen Steam-Account"));
    }

    #[test]
    fn streak_reply_zaehlt_serie_ab_neuestem_spiel() {
        let info = MatchHistory {
            linked: true,
            matches: vec![
                MatchEntry {
                    match_result: Some(1),
                    hero_id: None,
                    hero_name: None,
                    player_kills: None,
                    player_deaths: None,
                    player_assists: None,
                    not_scored: None,
                },
                MatchEntry {
                    match_result: Some(1),
                    hero_id: None,
                    hero_name: None,
                    player_kills: None,
                    player_deaths: None,
                    player_assists: None,
                    not_scored: None,
                },
                MatchEntry {
                    match_result: Some(0),
                    hero_id: None,
                    hero_name: None,
                    player_kills: None,
                    player_deaths: None,
                    player_assists: None,
                    not_scored: None,
                },
            ],
        };

        assert_eq!(streak_reply("nani", Some(&info)), "nani: 2 Siege in Folge!");
    }

    #[test]
    fn streak_reply_ignoriert_not_scored_in_der_serie() {
        let info = linked_history(vec![
            result_entry(1),
            unscored_result_entry(0),
            result_entry(1),
        ]);

        assert_eq!(streak_reply("nani", Some(&info)), "nani: 2 Siege in Folge!");
    }

    #[test]
    fn streak_reply_zaehlt_niederlagenserie() {
        let info = linked_history(vec![result_entry(0), result_entry(0), result_entry(0)]);

        assert_eq!(
            streak_reply("nani", Some(&info)),
            "nani: 3 Niederlagen in Folge."
        );
    }

    #[test]
    fn streak_reply_meldet_einzelnen_sieg() {
        let info = linked_history(vec![result_entry(1), result_entry(0)]);

        assert_eq!(
            streak_reply("nani", Some(&info)),
            "nani: Letztes Spiel gewonnen."
        );
    }

    #[test]
    fn streak_reply_meldet_einzelne_niederlage() {
        let info = linked_history(vec![result_entry(0), result_entry(1)]);

        assert_eq!(
            streak_reply("nani", Some(&info)),
            "nani: Letztes Spiel verloren."
        );
    }

    #[test]
    fn streak_reply_nicht_verknuepft() {
        let info = MatchHistory {
            linked: false,
            matches: Vec::new(),
        };

        assert!(streak_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
        assert!(streak_reply("nani", None).contains("noch keinen Steam-Account"));
    }

    #[test]
    fn mostplayed_reply_zeigt_haeufigsten_hero() {
        let info = MatchHistory {
            linked: true,
            matches: vec![
                MatchEntry {
                    match_result: None,
                    hero_id: Some(13),
                    hero_name: Some("Haze".into()),
                    player_kills: None,
                    player_deaths: None,
                    player_assists: None,
                    not_scored: None,
                },
                MatchEntry {
                    match_result: None,
                    hero_id: Some(13),
                    hero_name: Some("Haze".into()),
                    player_kills: None,
                    player_deaths: None,
                    player_assists: None,
                    not_scored: None,
                },
                MatchEntry {
                    match_result: None,
                    hero_id: Some(13),
                    hero_name: Some("Haze".into()),
                    player_kills: None,
                    player_deaths: None,
                    player_assists: None,
                    not_scored: None,
                },
                MatchEntry {
                    match_result: None,
                    hero_id: Some(6),
                    hero_name: Some("Abrams".into()),
                    player_kills: None,
                    player_deaths: None,
                    player_assists: None,
                    not_scored: None,
                },
            ],
        };

        let reply = mostplayed_reply("nani", Some(&info));

        assert!(reply.contains("Haze"));
        assert!(reply.contains("3 von 4"));
    }

    #[test]
    fn mostplayed_reply_nutzt_unbekannten_hero_ohne_hero_name() {
        let info = linked_history(vec![
            entry(1, Some(13), None, None, None, None, None),
            entry(0, Some(13), None, None, None, None, None),
            entry(1, Some(6), Some("Abrams"), None, None, None, None),
        ]);

        assert_eq!(
            mostplayed_reply("nani", Some(&info)),
            "nani: Meistgespielt zuletzt — unbekannter Hero (2 von 3 Spielen)."
        );
    }

    #[test]
    fn mostplayed_reply_meldet_leere_historie() {
        let info = linked_history(Vec::new());

        assert_eq!(
            mostplayed_reply("nani", Some(&info)),
            "nani: Noch keine Deadlock-Spiele gefunden."
        );
    }

    #[test]
    fn mostplayed_reply_nicht_verknuepft() {
        let info = MatchHistory {
            linked: false,
            matches: Vec::new(),
        };

        assert!(mostplayed_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
        assert!(mostplayed_reply("nani", None).contains("noch keinen Steam-Account"));
    }

    #[test]
    fn matches_url_from_rank_leitet_player_matches_url_ab() {
        assert_eq!(
            matches_url_from_rank("http://127.0.0.1:8783/rank"),
            "http://127.0.0.1:8783/player-matches"
        );
        assert_eq!(
            matches_url_from_rank("https://x.y/rank"),
            "https://x.y/player-matches"
        );
    }

    #[test]
    fn mmr_trend_url_from_rank_leitet_player_mmr_trend_url_ab() {
        assert_eq!(
            mmr_trend_url_from_rank("http://127.0.0.1:8783/rank"),
            "http://127.0.0.1:8783/player-mmr-trend"
        );
        assert_eq!(
            mmr_trend_url_from_rank("https://x.y/rank"),
            "https://x.y/player-mmr-trend"
        );
    }

    #[test]
    fn live_url_from_rank_leitet_player_live_url_ab() {
        assert_eq!(
            live_url_from_rank("http://127.0.0.1:8783/rank"),
            "http://127.0.0.1:8783/player-live"
        );
        assert_eq!(
            live_url_from_rank("https://x.y/rank"),
            "https://x.y/player-live"
        );
    }
}
