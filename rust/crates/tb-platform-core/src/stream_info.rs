//! Zustand eines Kanals: Titel, Kategorie, Tags, Livestatus.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::platform::Platform;

/// Momentaufnahme der Kanalinformationen.
///
/// Kein Ereignis, sondern ein Zustand: das Stream-Info-Dock belegt seine Felder
/// damit vor und bekommt bei jeder Aenderung eine neue Momentaufnahme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfo {
    /// Plattform des Kanals.
    pub platform: Platform,
    /// Unveraenderliche Kanalkennung.
    pub channel_id: String,
    /// Aktueller Titel.
    pub title: String,
    /// Kennung der Kategorie, bei Twitch die `game_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// Kategorie im Klartext.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_name: Option<String>,
    /// Gesetzte Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// `true`, solange der Kanal sendet.
    pub is_live: bool,
    /// Beginn der laufenden Sendung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    /// Zuschauerzahl, falls die Plattform sie liefert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewers: Option<u32>,
}

impl StreamInfo {
    /// Baut eine Momentaufnahme eines Kanals, der gerade nicht sendet.
    pub fn offline(
        platform: Platform,
        channel_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            platform,
            channel_id: channel_id.into(),
            title: title.into(),
            category_id: None,
            category_name: None,
            tags: Vec::new(),
            is_live: false,
            started_at: None,
            viewers: None,
        }
    }
}

/// Aenderungswunsch an den Kanalinformationen.
///
/// Jedes `None` heisst "nicht anfassen". Der Adapter schickt nur die gesetzten
/// Felder an die Plattform, bei Twitch als `PATCH /channels`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfoPatch {
    /// Neuer Titel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Neue Kategorie als Kennung, bei Twitch die `game_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// Neue Tagliste, ersetzt die bisherige vollstaendig.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl StreamInfoPatch {
    /// `true`, wenn der Patch gar nichts aendern wuerde.
    ///
    /// Der Aufrufer soll in dem Fall gar nicht erst zur Plattform gehen.
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.category_id.is_none() && self.tags.is_none()
    }

    /// Setzt den Titel.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Setzt die Kategorie.
    #[must_use]
    pub fn with_category_id(mut self, category_id: impl Into<String>) -> Self {
        self.category_id = Some(category_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leerer_patch_wird_erkannt() {
        assert!(StreamInfoPatch::default().is_empty());
        assert!(!StreamInfoPatch::default().with_title("neu").is_empty());
    }

    #[test]
    fn offline_momentaufnahme_hat_keinen_startzeitpunkt() {
        let info = StreamInfo::offline(Platform::Twitch, "12345", "Pause");
        assert!(!info.is_live);
        assert!(info.started_at.is_none());
        assert!(info.viewers.is_none());
    }
}
