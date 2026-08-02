//! Kern-Datentypen der Engagement-Pipeline (Port der Dataclasses/Enum aus
//! `bot/engagement/pipeline.py`).

/// Ausgang eines Engagement-Durchlaufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Modell hat geantwortet, Text geht in den Chat.
    Spoke,
    /// Modell hat geantwortet, aber der Output-Modus ist `shadow`: der Text
    /// wurde erzeugt und gestaged/markiert, geht aber NICHT in den Twitch-Chat
    /// (für späteres Discord-Review). Kein Python-Pendant — neue Funktion aus
    /// dem Block-19-Grillme (Engagement-KI mit Shadow-Modus).
    Shadowed,
    /// Wie [`Shadowed`](Self::Shadowed), aber aus dem Smalltalk-Testmodus in
    /// einem fremden Kanal. Eigener Wert, weil der Shadow-Review jede
    /// `shadowed`-Zeile nach Discord forwardet: Testantworten wuerden dort
    /// zwischen den Partner-Vorschlaegen landen und wie welche aussehen.
    Tested,
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
            Decision::Shadowed => "shadowed",
            Decision::Tested => "tested",
            Decision::Silent => "silent",
            Decision::AntiBurst => "anti_burst",
            Decision::FloodGuard => "flood_guard",
            Decision::Optout => "optout",
            Decision::Disabled => "disabled",
            Decision::ProviderError => "provider_error",
        }
    }
}

/// Output-Modus der Engagement-KI eines Channels.
///
/// Neu aus dem Block-19-Grillme (Python kannte nur `enabled` als bool). Steuert,
/// was mit einer erzeugten KI-Antwort passiert. **Default ist [`Off`](Self::Off)**
/// (der deaktivierte Zustand): Dashboard-Toggle und Shadow→Discord-Auslieferung
/// kommen in separaten Tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Kein KI-Output: die Pipeline erzeugt erst gar keine Antwort (no-op).
    #[default]
    Off,
    /// Antwort wird erzeugt und gestaged/markiert, aber NICHT in den Chat
    /// gesendet (für späteres Discord-Review).
    Shadow,
    /// Antwort wird normal in den Twitch-Chat gesendet.
    Live,
    /// Antwort wird für fremde Kanäle erzeugt und ausschließlich ausgewertet.
    /// Das Partner-Gate entfällt, der Twitch-Sendepfad bleibt gesperrt.
    Test,
}

impl OutputMode {
    /// String-Repräsentation für die DB-Spalte `output_mode`.
    pub fn as_str(self) -> &'static str {
        match self {
            OutputMode::Off => "off",
            OutputMode::Shadow => "shadow",
            OutputMode::Live => "live",
            OutputMode::Test => "test",
        }
    }

    /// Parst den DB-Wert. Unbekannte/leere Werte fallen sicher auf
    /// [`Off`](Self::Off) zurück (fail-safe: im Zweifel kein Output).
    pub fn from_db(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "live" => OutputMode::Live,
            "shadow" => OutputMode::Shadow,
            "test" => OutputMode::Test,
            _ => OutputMode::Off,
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
    /// Output-Modus der KI-Antwort (`off`/`shadow`/`live`). Default `off`.
    pub output_mode: OutputMode,
}

/// Eine eingehende Chat-Nachricht, die die Pipeline bewertet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessage {
    pub channel_login: String,
    /// Stabile Twitch-ID des **Kanals** (IRC-Tag `room-id`, EventSub
    /// `broadcaster_user_id`). Nicht mit [`twitch_user_id`](Self::twitch_user_id)
    /// verwechseln — das ist der Chatter.
    ///
    /// Sie liegt an jeder Nachricht an und ist deshalb der billige Weg zur
    /// Identität eines Kanals: den Kanal später aus seinem Namen
    /// zurückzurechnen kostet eine Auflösung pro Query und überlebt eine
    /// Umbenennung nur, solange die Alias-Historie eindeutig ist.
    pub channel_user_id: String,
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub content: String,
    pub message_id: Option<String>,
}

/// Ergebnis eines Durchlaufs inkl. Telemetrie-Feldern fürs Log.
#[derive(Debug, Clone, PartialEq)]
pub struct HandleResult {
    pub decision: Decision,
    /// Text, der in den Twitch-Chat gesendet werden soll. Nur im `live`-Modus
    /// gesetzt — der tb-bot-Sendepfad sendet ausschließlich, wenn dieses Feld
    /// belegt ist. Im `shadow`-Modus bleibt es bewusst `None` (siehe
    /// [`shadow_text`](Self::shadow_text)).
    pub response_text: Option<String>,
    /// Erzeugte KI-Antwort, die im `shadow`-Modus NICHT gesendet, sondern
    /// gestaged/markiert wird (für späteres Discord-Review). Getrennt von
    /// [`response_text`](Self::response_text), damit der bestehende
    /// tb-bot-Sendepfad shadow-Antworten nie versehentlich in den Chat schickt.
    pub shadow_text: Option<String>,
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
            shadow_text: None,
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
        assert_eq!(Decision::Shadowed.as_str(), "shadowed");
        assert_eq!(Decision::Tested.as_str(), "tested");
        assert_eq!(Decision::AntiBurst.as_str(), "anti_burst");
        assert_eq!(Decision::ProviderError.as_str(), "provider_error");
    }

    #[test]
    fn handle_result_new_leer() {
        let r = HandleResult::new(Decision::Disabled);
        assert_eq!(r.decision, Decision::Disabled);
        assert!(r.response_text.is_none());
        assert!(r.shadow_text.is_none());
        assert!(r.referenced_thread_ids.is_none());
    }

    #[test]
    fn output_mode_default_ist_off() {
        // Default-AUS-Garantie aus dem Grillme: ohne explizite Config kein Output.
        assert_eq!(OutputMode::default(), OutputMode::Off);
    }

    #[test]
    fn output_mode_roundtrip_db_string() {
        for m in [
            OutputMode::Off,
            OutputMode::Shadow,
            OutputMode::Live,
            OutputMode::Test,
        ] {
            assert_eq!(OutputMode::from_db(m.as_str()), m);
        }
        assert_eq!(OutputMode::Off.as_str(), "off");
        assert_eq!(OutputMode::Shadow.as_str(), "shadow");
        assert_eq!(OutputMode::Live.as_str(), "live");
        assert_eq!(OutputMode::Test.as_str(), "test");
    }

    #[test]
    fn output_mode_from_db_failsafe_off() {
        // Unbekannte/leere/grossgeschriebene Werte → fail-safe Off (kein Output).
        assert_eq!(OutputMode::from_db(""), OutputMode::Off);
        assert_eq!(OutputMode::from_db("   "), OutputMode::Off);
        assert_eq!(OutputMode::from_db("bogus"), OutputMode::Off);
        assert_eq!(OutputMode::from_db("LIVE"), OutputMode::Live);
        assert_eq!(OutputMode::from_db("  Shadow "), OutputMode::Shadow);
        assert_eq!(OutputMode::from_db(" Test "), OutputMode::Test);
    }
}
