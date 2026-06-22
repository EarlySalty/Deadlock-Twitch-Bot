//! `!rank`-Kette: twitch_user_id -> discord_user_id (twitch_streamer_identities)
//! -> HTTP an den Steam-Bot (/rank) -> Chat-Antwort.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use sqlx::PgPool;

const DEFAULT_STEAM_BOT_RANK_URL: &str = "http://127.0.0.1:8783/rank";
const DEFAULT_STEAM_BOT_MATCHES_URL: &str = "http://127.0.0.1:8783/player-matches";

#[derive(Debug, Clone, Deserialize)]
pub struct RankInfo {
    pub linked: bool,
    pub rank_name: Option<String>,
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

pub async fn resolve_discord_id(pool: &PgPool, twitch_user_id: &str) -> Option<String> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT discord_user_id \
         FROM twitch_streamer_identities \
         WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    row.and_then(|(discord_user_id,)| discord_user_id)
        .map(|discord_user_id| discord_user_id.trim().to_string())
        .filter(|discord_user_id| !discord_user_id.is_empty())
}

pub async fn fetch_rank(discord_id: &str, include_stats: bool) -> Option<RankInfo> {
    let rank_url = steam_bot_rank_url();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(if include_stats { 8 } else { 5 }))
        .build()
        .ok()?;
    let mut request = client.get(rank_url).query(&[("discord_id", discord_id)]);
    if include_stats {
        request = request.query(&[("include_stats", "1")]);
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<RankInfo>().await.ok()
}

pub async fn fetch_matches(discord_id: &str) -> Option<MatchHistory> {
    let matches_url = steam_bot_matches_url();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let response = client
        .get(matches_url)
        .query(&[("discord_id", discord_id), ("limit", "150")])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<MatchHistory>().await.ok()
}

pub fn rank_reply(name: &str, info: Option<&RankInfo>) -> String {
    match info {
        Some(info) if info.linked => match &info.rank_name {
            Some(rank) => format!("Rang von {name}: {rank}"),
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
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(
                    trimmed
                        .strip_suffix("/rank")
                        .map(|base| format!("{base}/player-matches"))
                        .unwrap_or_else(|| trimmed.to_string()),
                )
            }
        })
        .unwrap_or_else(|| DEFAULT_STEAM_BOT_MATCHES_URL.to_string())
}

fn scored(m: &[MatchEntry]) -> Vec<&MatchEntry> {
    m.iter()
        .filter(|entry| entry.not_scored != Some(true))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rang_vorhanden() {
        let info = RankInfo {
            linked: true,
            rank_name: Some("Archon".into()),
            badge_level: Some(61),
            wins: None,
            losses: None,
            matches: None,
        };
        assert_eq!(rank_reply("nani", Some(&info)), "Rang von nani: Archon");
    }

    #[test]
    fn verknuepft_ohne_rang() {
        let info = RankInfo {
            linked: true,
            rank_name: None,
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
    fn winrate_reply_nicht_verknuepft() {
        let info = MatchHistory {
            linked: false,
            matches: Vec::new(),
        };

        assert!(winrate_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
        assert!(winrate_reply("nani", None).contains("noch keinen Steam-Account"));
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

        assert!(reply.contains("Sieg"));
        assert!(reply.contains("Haze"));
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
    fn mostplayed_reply_nicht_verknuepft() {
        let info = MatchHistory {
            linked: false,
            matches: Vec::new(),
        };

        assert!(mostplayed_reply("nani", Some(&info)).contains("noch keinen Steam-Account"));
        assert!(mostplayed_reply("nani", None).contains("noch keinen Steam-Account"));
    }
}
