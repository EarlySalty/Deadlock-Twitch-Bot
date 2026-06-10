//! Port für die Stream-Datenquelle (Hexagonal: tb-monitoring definiert den
//! Port, der Adapter auf den Helix-Client lebt im Composition-Root `tb-bot`).

use crate::stream::StreamSnapshot;

pub type SourceError = Box<dyn std::error::Error + Send + Sync>;

#[async_trait::async_trait]
pub trait StreamSource: Send + Sync {
    /// Live-Streams für die gegebenen Logins (Helix `/streams`, gebatcht).
    async fn streams_by_logins(
        &self,
        logins: &[String],
        language: Option<&str>,
    ) -> Result<Vec<StreamSnapshot>, SourceError>;

    /// Bis zu `limit` Live-Streams einer Kategorie.
    async fn streams_by_category(
        &self,
        category_id: &str,
        language: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StreamSnapshot>, SourceError>;

    /// game_id der Ziel-Kategorie (`/search/categories`).
    async fn category_id(&self, game_name: &str) -> Result<Option<String>, SourceError>;
}

/// Kanal-Metadaten für das Go-Live-Enrichment (Helix `/channels`).
#[derive(Debug, Clone, Default)]
pub struct ChannelInfo {
    pub title: Option<String>,
    pub game_name: Option<String>,
}

/// Port für den gezielten Kanal-Lookup beim stream.online-Event — macht die
/// Kategorie event-getrieben statt poll-abhängig (wichtig für Partner, deren
/// Kanal-Sprache nicht im Poll-Sprachfilter liegt, und für Streams, die
/// kürzer als ein Poll-Intervall sind).
#[async_trait::async_trait]
pub trait ChannelInfoSource: Send + Sync {
    async fn channel_info(&self, broadcaster_id: &str)
        -> Result<Option<ChannelInfo>, SourceError>;
}
