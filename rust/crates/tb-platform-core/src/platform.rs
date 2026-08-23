//! Plattform-Kennung und Kanalverweis.
//!
//! Reines Typmodul ohne I/O — Vorbild `tb-raid/src/scope_profiles.rs`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Streaming-Plattform, aus der ein Ereignis stammt bzw. an die gesendet wird.
///
/// Die serde-Namen sind Teil des Drahtformats und duerfen nicht mehr geaendert
/// werden; deshalb stehen sie explizit da statt ueber `rename_all`
/// (`rename_all = "snake_case"` wuerde aus `YouTube` ein `you_tube` machen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Platform {
    /// Twitch — einzige Plattform, die im MVP wirklich gebaut wird.
    #[serde(rename = "twitch")]
    Twitch,
    /// YouTube Live — Platzhalter fuer den spaeteren Adapter.
    #[serde(rename = "youtube")]
    YouTube,
    /// Kick — Platzhalter fuer den spaeteren Adapter.
    #[serde(rename = "kick")]
    Kick,
}

impl Platform {
    /// Stabile Kurzkennung, identisch mit der serde-Darstellung.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Twitch => "twitch",
            Self::YouTube => "youtube",
            Self::Kick => "kick",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Verweis auf genau einen Kanal einer Plattform.
///
/// `channel_id` ist die plattforminterne, unveraenderliche Kennung (bei Twitch
/// die numerische User-ID), `channel_login` der anzeigbare Kanalname. Beide
/// werden getragen, weil Helix je nach Endpunkt das eine oder das andere will.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelRef {
    /// Plattform des Kanals.
    pub platform: Platform,
    /// Unveraenderliche Kanalkennung der Plattform.
    pub channel_id: String,
    /// Anzeigbarer Kanalname (Login).
    pub channel_login: String,
}

impl ChannelRef {
    /// Baut einen Kanalverweis.
    pub fn new(
        platform: Platform,
        channel_id: impl Into<String>,
        channel_login: impl Into<String>,
    ) -> Self {
        Self {
            platform,
            channel_id: channel_id.into(),
            channel_login: channel_login.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plattform_kurzkennung_ist_eingefroren() {
        assert_eq!(Platform::Twitch.as_str(), "twitch");
        assert_eq!(Platform::YouTube.as_str(), "youtube");
        assert_eq!(Platform::Kick.as_str(), "kick");
    }

    #[test]
    fn display_entspricht_kurzkennung() {
        assert_eq!(Platform::YouTube.to_string(), "youtube");
    }
}
