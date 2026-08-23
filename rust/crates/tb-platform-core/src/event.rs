//! Das Drahtformat zwischen Bot, Gateway und Dock.

use serde::{Deserialize, Serialize};

use crate::activity::ActivityEvent;
use crate::chat::ChatMessage;
use crate::platform::Platform;
use crate::stream_info::StreamInfo;

/// Alles, was ueber den Bus zum Dock geht.
///
/// Die serde-Darstellung ist intern getaggt mit dem Feld `typ` und damit
/// gleichzeitig das WebSocket-Drahtformat. Sie ist eingefroren: Tagname,
/// Variantennamen und Feldnamen duerfen nicht mehr geaendert werden, weil ein
/// Dock stundenlang offen bleibt und ein aelteres Bundle weiter mitlesen muss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "typ")]
pub enum PlatformEvent {
    /// Eine Chatnachricht.
    #[serde(rename = "chat")]
    Chat(ChatMessage),
    /// Ein Ereignis neben dem Chat.
    #[serde(rename = "activity")]
    Activity(ActivityEvent),
    /// Eine Momentaufnahme der Kanalinformationen.
    #[serde(rename = "info")]
    Info(StreamInfo),
}

impl PlatformEvent {
    /// Stabile Typkennung, identisch mit der serde-Darstellung.
    pub const fn typ(&self) -> &'static str {
        match self {
            Self::Chat(_) => "chat",
            Self::Activity(_) => "activity",
            Self::Info(_) => "info",
        }
    }

    /// Plattform des Ereignisses.
    pub fn platform(&self) -> Platform {
        match self {
            Self::Chat(msg) => msg.platform,
            Self::Activity(ereignis) => ereignis.platform(),
            Self::Info(info) => info.platform,
        }
    }

    /// Kanal, an den das Ereignis gehoert. Der Bus sortiert danach.
    pub fn channel_id(&self) -> &str {
        match self {
            Self::Chat(msg) => &msg.channel_id,
            Self::Activity(ereignis) => ereignis.channel_id(),
            Self::Info(info) => &info.channel_id,
        }
    }

    /// Dedupe-Schluessel, sofern das Ereignis einer ist.
    ///
    /// [`PlatformEvent::Info`] ist ein Zustand und kein Ereignis, deshalb gibt
    /// es dort keinen Schluessel; eine neuere Momentaufnahme ersetzt die alte.
    pub fn dedupe_key(&self) -> Option<String> {
        match self {
            Self::Chat(msg) => Some(msg.dedupe_key()),
            Self::Activity(ereignis) => Some(ereignis.dedupe_key().to_string()),
            Self::Info(_) => None,
        }
    }
}

impl From<ChatMessage> for PlatformEvent {
    fn from(value: ChatMessage) -> Self {
        Self::Chat(value)
    }
}

impl From<ActivityEvent> for PlatformEvent {
    fn from(value: ActivityEvent) -> Self {
        Self::Activity(value)
    }
}

impl From<StreamInfo> for PlatformEvent {
    fn from(value: StreamInfo) -> Self {
        Self::Info(value)
    }
}
