//! Kern-Datentypen der Engagement-Pipeline (Port der Dataclasses/Enum aus
//! `bot/engagement/pipeline.py`).

/// Ausgang eines Engagement-Durchlaufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Modell hat geantwortet, Text geht in den Chat.
    Spoke,
    /// Bewusst still (Pre-Filter, leere Modellantwort, Starter-Repeat).
    Silent,
    /// Anti-Burst-Sperre.
    AntiBurst,
    /// Anti-Flood-Sperre.
    FloodGuard,
    /// User hat sich abgemeldet.
    Optout,
    /// Channel nicht aktiv (kein Setting / disabled / kein operativer Partner /
    /// nicht live mit Deadlock).
    Disabled,
    /// Fehler im Modell-/Provider-Aufruf.
    ProviderError,
}

impl Decision {
    /// String-Repräsentation wie Pythons `Decision`-Werte (für DB-Log).
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Spoke => "spoke",
            Decision::Silent => "silent",
            Decision::AntiBurst => "anti_burst",
            Decision::FloodGuard => "flood_guard",
            Decision::Optout => "optout",
            Decision::Disabled => "disabled",
            Decision::ProviderError => "provider_error",
        }
    }
}

/// Engagement-Konfiguration eines Channels (`twitch_engagement_settings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngagementSettings {
    pub channel_login: String,
    pub enabled: bool,
    pub steam_id: Option<String>,
    pub persona_override: Option<String>,
    pub tabu_topics: Vec<String>,
}

/// Eine eingehende Chat-Nachricht, die die Pipeline bewertet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessage {
    pub channel_login: String,
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub content: String,
    pub message_id: Option<String>,
}

/// Ergebnis eines Durchlaufs inkl. Telemetrie-Feldern fürs Log.
#[derive(Debug, Clone, PartialEq)]
pub struct HandleResult {
    pub decision: Decision,
    pub response_text: Option<String>,
    pub model: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub referenced_thread_ids: Option<Vec<i64>>,
}

impl HandleResult {
    /// Ergebnis nur mit Entscheidung, alle Telemetrie-Felder leer.
    pub fn new(decision: Decision) -> Self {
        Self {
            decision,
            response_text: None,
            model: None,
            prompt_tokens: None,
            completion_tokens: None,
            latency_ms: None,
            referenced_thread_ids: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_strings() {
        assert_eq!(Decision::Spoke.as_str(), "spoke");
        assert_eq!(Decision::AntiBurst.as_str(), "anti_burst");
        assert_eq!(Decision::ProviderError.as_str(), "provider_error");
    }

    #[test]
    fn handle_result_new_leer() {
        let r = HandleResult::new(Decision::Disabled);
        assert_eq!(r.decision, Decision::Disabled);
        assert!(r.response_text.is_none());
        assert!(r.referenced_thread_ids.is_none());
    }
}
