//! Ausschließlich die acht abschaltbaren Statistikbefehle.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatCommand {
    Rank,
    Wins,
    Winrate,
    Mmr,
    Live,
    Lastmatch,
    Streak,
    Mostplayed,
}

impl StatCommand {
    pub const ALL: [Self; 8] = [
        Self::Rank,
        Self::Wins,
        Self::Winrate,
        Self::Mmr,
        Self::Live,
        Self::Lastmatch,
        Self::Streak,
        Self::Mostplayed,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Rank => "rank",
            Self::Wins => "wins",
            Self::Winrate => "winrate",
            Self::Mmr => "mmr",
            Self::Live => "live",
            Self::Lastmatch => "lastmatch",
            Self::Streak => "streak",
            Self::Mostplayed => "mostplayed",
        }
    }

    pub fn from_chat(command: &str) -> Option<Self> {
        match command {
            "!rank" => Some(Self::Rank),
            "!wins" => Some(Self::Wins),
            "!winrate" => Some(Self::Winrate),
            "!mmr" | "!climb" => Some(Self::Mmr),
            "!live" => Some(Self::Live),
            "!lastmatch" | "!last" => Some(Self::Lastmatch),
            "!streak" => Some(Self::Streak),
            "!mostplayed" | "!main" => Some(Self::Mostplayed),
            _ => None,
        }
    }
}

/// Pro Prozess höchstens zwei gleiche Datenbankwarnungen je Woche; die nächste
/// Meldung zählt die unterdrückten Wiederholungen mit. Kein Chat löst Logspam aus.
pub(crate) fn warn_read_failure(error: &sqlx::Error) {
    use std::{
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    static WARNINGS: OnceLock<Mutex<(Option<Instant>, u64)>> = OnceLock::new();
    let mut window = WARNINGS
        .get_or_init(|| Mutex::new((None, 0)))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();
    if window
        .0
        .is_some_and(|last| now.duration_since(last) < Duration::from_secs(4 * 24 * 60 * 60))
    {
        window.1 = window.1.saturating_add(1);
        return;
    }
    tracing::warn!(%error, wiederholungen = window.1, "Statistikbefehl-Einstellung konnte nicht gelesen werden");
    *window = (Some(now), 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geschuetzte_befehle_sind_nicht_schaltbar() {
        for command in [
            "!raid",
            "!traid",
            "!discord",
            "!dldc",
            "!dlde",
            "!invite",
            "!commands",
            "!help",
            "!ping",
            "!health",
            "!status",
            "!bot",
        ] {
            assert_eq!(StatCommand::from_chat(command), None, "{command}");
            assert!(serde_json::from_value::<StatCommand>(serde_json::json!(
                command.trim_start_matches('!')
            ))
            .is_err());
        }
    }
}
