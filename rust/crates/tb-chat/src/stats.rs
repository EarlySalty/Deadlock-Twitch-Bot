//! `!rank`-Kette: twitch_user_id -> discord_user_id (twitch_streamer_identities)
//! -> HTTP an den Steam-Bot (/rank) -> Chat-Antwort.

use std::time::Duration;

use serde::Deserialize;
use sqlx::PgPool;

const DEFAULT_STEAM_BOT_RANK_URL: &str = "http://127.0.0.1:8783/rank";

#[derive(Debug, Clone, Deserialize)]
pub struct RankInfo {
    pub linked: bool,
    pub rank_name: Option<String>,
    pub badge_level: Option<i64>,
    pub wins: Option<i64>,
    pub losses: Option<i64>,
    pub matches: Option<i64>,
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
}
