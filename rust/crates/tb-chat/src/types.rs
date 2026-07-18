//! Geteilte Typen des Chat-Subsystems.
//!
//! `ChatMessageEvent` ist die deserialisierte Form des EventSub-Events
//! `channel.chat.message` (v1) — Felder nach der Twitch-Payload-Spezifikation.
//! Der Python-Chat (TwitchIO 3.x) konsumiert dasselbe Event über den
//! EventSub-WebSocket; Rust konsumiert es über den bestehenden
//! Webhook-Dispatch (tb-monitoring → tb-chat::pipeline).

use std::borrow::Cow;

use serde::Deserialize;

/// Badge eines Chatters (`event.badges[]`).
#[derive(Debug, Clone, Deserialize)]
pub struct ChatBadge {
    pub set_id: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub info: String,
}

/// Nachrichten-Fragment (`event.message.fragments[]`) — Typ `text`,
/// `cheermote`, `emote` oder `mention`.
#[derive(Debug, Clone, Deserialize)]
pub struct MessageFragment {
    #[serde(rename = "type")]
    pub fragment_type: String,
    #[serde(default)]
    pub text: String,
}

/// `event.message` von `channel.chat.message`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatMessageBody {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub fragments: Vec<MessageFragment>,
}

/// EventSub `channel.chat.message` (v1) — die für uns relevanten Felder.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatMessageEvent {
    pub broadcaster_user_id: String,
    pub broadcaster_user_login: String,
    #[serde(default)]
    pub broadcaster_user_name: String,
    pub chatter_user_id: String,
    pub chatter_user_login: String,
    #[serde(default)]
    pub chatter_user_name: String,
    pub message_id: String,
    pub message: ChatMessageBody,
    #[serde(default)]
    pub badges: Vec<ChatBadge>,
    #[serde(default)]
    pub color: String,
    // ── Twitch Shared Chat ──────────────────────────────────────────────────
    // Bei einer aktiven Shared-Chat-Session setzt Twitch diese Felder, sobald die
    // Nachricht aus einem ANDEREN Kanal der Session stammt: `source_broadcaster_*`
    // ist dann der reale Quell-Kanal, `source_message_id` die ID dort. Bei
    // Eigen-Nachrichten sind sie abwesend/None. (Python liest `source_broadcaster`
    // analog — bot.py:1500.)
    #[serde(default)]
    pub source_broadcaster_user_id: Option<String>,
    #[serde(default)]
    pub source_broadcaster_user_login: Option<String>,
    #[serde(default)]
    pub source_message_id: Option<String>,
}

impl ChatMessageEvent {
    /// Chatter ist Moderator ODER Broadcaster (Python: `is_mod`-Checks der
    /// Commands akzeptieren beide).
    pub fn is_mod_or_broadcaster(&self) -> bool {
        self.badges
            .iter()
            .any(|b| b.set_id == "moderator" || b.set_id == "broadcaster")
    }

    /// Chatter ist der Broadcaster selbst.
    pub fn is_broadcaster(&self) -> bool {
        self.chatter_user_id == self.broadcaster_user_id
            || self.badges.iter().any(|b| b.set_id == "broadcaster")
    }

    /// Nachrichtentext, getrimmt.
    pub fn text(&self) -> &str {
        self.message.text.trim()
    }

    /// Normalisiert das Event auf den **effektiven Kanal** (Twitch Shared Chat).
    ///
    /// Stammt die Nachricht aus einem fremden Quell-Kanal (`source_broadcaster_*`
    /// gesetzt), wird sie so behandelt, als käme sie aus diesem Quell-Kanal:
    /// `broadcaster_user_id`/`_login` und `message_id` werden auf die Quell-Werte
    /// gesetzt und die `source_*`-Felder geleert (idempotent). Das ist 1:1 die
    /// Python-Semantik (`bot.py:1500/1505`: `channel = source_broadcaster or
    /// broadcaster`, früh auf `message.channel` zurückgeschrieben) — so arbeitet
    /// der gesamte Downstream-Pfad (Moderation/Tracking/Promos/Commands) im
    /// richtigen Kanal statt im Host-Abonnement.
    ///
    /// Ohne Shared Chat (kein `source_broadcaster_user_id`) wird das Event
    /// unverändert **geliehen** — kein Klon, kein Overhead im Normalfall.
    pub fn with_effective_channel(&self) -> Cow<'_, ChatMessageEvent> {
        match self.source_broadcaster_user_id.as_deref() {
            Some(src_id) if !src_id.is_empty() => {
                let mut ev = self.clone();
                ev.broadcaster_user_id = src_id.to_string();
                if let Some(login) = self
                    .source_broadcaster_user_login
                    .as_deref()
                    .filter(|s| !s.is_empty())
                {
                    ev.broadcaster_user_login = login.to_string();
                }
                // Mod-Aktionen (Ban/Delete) im Quell-Kanal brauchen dessen ID.
                if let Some(mid) = self
                    .source_message_id
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                {
                    ev.message_id = mid.to_string();
                }
                ev.source_broadcaster_user_id = None;
                ev.source_broadcaster_user_login = None;
                ev.source_message_id = None;
                Cow::Owned(ev)
            }
            _ => Cow::Borrowed(self),
        }
    }
}

/// Sende-/Ban-Ergebnisse: kanonisch im Transport definiert
/// (`tb_transport_twitch::chat` — wire-true inkl. `is_sent=false`-Drop-Parsing
/// und `HttpError`-Variante für den 2-Attempt-Pfad).
pub use tb_transport_twitch::SendOutcome;

#[cfg(test)]
mod tests {
    use super::*;

    fn base_event() -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "host_a".into(),
            broadcaster_user_login: "host_a_login".into(),
            broadcaster_user_name: String::new(),
            chatter_user_id: "viewer".into(),
            chatter_user_login: "viewer_login".into(),
            chatter_user_name: String::new(),
            message_id: "msg_host".into(),
            message: ChatMessageBody {
                text: "hi".into(),
                fragments: vec![],
            },
            badges: vec![],
            color: String::new(),
            source_broadcaster_user_id: None,
            source_broadcaster_user_login: None,
            source_message_id: None,
        }
    }

    #[test]
    fn ohne_shared_chat_unveraendert_geliehen() {
        let ev = base_event();
        let norm = ev.with_effective_channel();
        assert!(
            matches!(norm, Cow::Borrowed(_)),
            "kein Klon ohne Shared Chat"
        );
        assert_eq!(norm.broadcaster_user_id, "host_a");
        assert_eq!(norm.message_id, "msg_host");
    }

    #[test]
    fn shared_chat_normalisiert_auf_quellkanal() {
        let mut ev = base_event();
        ev.source_broadcaster_user_id = Some("source_b".into());
        ev.source_broadcaster_user_login = Some("source_b_login".into());
        ev.source_message_id = Some("msg_source".into());

        let norm = ev.with_effective_channel();
        assert!(matches!(norm, Cow::Owned(_)));
        assert_eq!(norm.broadcaster_user_id, "source_b");
        assert_eq!(norm.broadcaster_user_login, "source_b_login");
        assert_eq!(norm.message_id, "msg_source");
        assert!(
            norm.source_broadcaster_user_id.is_none(),
            "Quell-Felder geleert"
        );

        // Idempotent: erneute Normalisierung ist ein No-op (geliehen).
        let again = norm.with_effective_channel();
        assert!(matches!(again, Cow::Borrowed(_)));
    }

    #[test]
    fn shared_chat_ohne_quell_login_behaelt_host_login() {
        let mut ev = base_event();
        ev.source_broadcaster_user_id = Some("source_b".into());
        let norm = ev.with_effective_channel();
        assert_eq!(norm.broadcaster_user_id, "source_b");
        assert_eq!(norm.broadcaster_user_login, "host_a_login");
    }

    #[test]
    fn deserialisiert_source_felder_optional() {
        // Mit Source-Block (Shared Chat)
        let with_source = serde_json::json!({
            "broadcaster_user_id": "a", "broadcaster_user_login": "a",
            "chatter_user_id": "v", "chatter_user_login": "v",
            "message_id": "m", "message": {"text": "hi"},
            "source_broadcaster_user_id": "b",
            "source_broadcaster_user_login": "b",
            "source_message_id": "sm"
        });
        let ev: ChatMessageEvent = serde_json::from_value(with_source).unwrap();
        assert_eq!(ev.source_broadcaster_user_id.as_deref(), Some("b"));
        assert_eq!(ev.with_effective_channel().broadcaster_user_id, "b");

        // Ohne Source-Block (Normalfall) — bleibt deserialisierbar
        let without = serde_json::json!({
            "broadcaster_user_id": "a", "broadcaster_user_login": "a",
            "chatter_user_id": "v", "chatter_user_login": "v",
            "message_id": "m", "message": {"text": "hi"}
        });
        let ev2: ChatMessageEvent = serde_json::from_value(without).unwrap();
        assert!(ev2.source_broadcaster_user_id.is_none());
    }
}
