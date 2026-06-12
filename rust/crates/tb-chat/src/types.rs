//! Geteilte Typen des Chat-Subsystems.
//!
//! `ChatMessageEvent` ist die deserialisierte Form des EventSub-Events
//! `channel.chat.message` (v1) — Felder nach der Twitch-Payload-Spezifikation.
//! Der Python-Chat (TwitchIO 3.x) konsumiert dasselbe Event über den
//! EventSub-WebSocket; Rust konsumiert es über den bestehenden
//! Webhook-Dispatch (tb-monitoring → tb-chat::pipeline).

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
#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessageBody {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub fragments: Vec<MessageFragment>,
}

/// EventSub `channel.chat.message` (v1) — die für uns relevanten Felder.
#[derive(Debug, Clone, Deserialize)]
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
}

/// Ergebnis einer Chat-Sendung (Helix `POST /chat/messages` liefert HTTP 200
/// auch bei Drop — `is_sent=false` + `drop_reason.code` müssen ausgewertet
/// werden; Python-Vertrag moderation.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Sent { message_id: String },
    Dropped { code: String, message: String },
}

impl SendOutcome {
    pub fn is_sent(&self) -> bool {
        matches!(self, Self::Sent { .. })
    }
}
