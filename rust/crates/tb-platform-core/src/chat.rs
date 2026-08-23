//! Chatnachricht in plattformneutraler Form.
//!
//! Die Nachricht ist bereits so zerlegt, dass ein Dock sie ohne
//! Twitch-spezifische Nachbearbeitung rendern kann: Badges als Liste,
//! Text/Emote/Mention/Cheermote als Fragmente mit fertigem URL-Muster.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::dedupe::dedupe_key;
use crate::platform::{ChannelRef, Platform};

/// Art-Kennung der Chatnachricht im Dedupe-Schluessel.
pub const CHAT_DEDUPE_ART: &str = "chat";

/// Ein Abzeichen des Absenders (Twitch: `badges` aus dem EventSub-Payload).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    /// Gruppe des Abzeichens, etwa `subscriber` oder `moderator`.
    pub set_id: String,
    /// Auspraegung innerhalb der Gruppe, etwa `12` fuer 12 Monate.
    pub id: String,
    /// Zusatzinfo der Plattform, bei Twitch der Monatszaehler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    /// Fertige Bild-URL, damit das Dock keine zweite Abfrage braucht.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

impl Badge {
    /// Baut ein Abzeichen ohne Zusatzinfo und ohne Bild.
    pub fn new(set_id: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            set_id: set_id.into(),
            id: id.into(),
            info: None,
            image_url: None,
        }
    }
}

/// Ein Baustein der Nachricht.
///
/// Der serde-Tag heisst `art` und nicht `typ`, weil `typ` bereits vom
/// aeusseren [`crate::PlatformEvent`] belegt ist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "art")]
pub enum Fragment {
    /// Reiner Text.
    #[serde(rename = "text")]
    Text {
        /// Der Text, so wie er im Chat steht.
        text: String,
    },
    /// Emote mit Bildquelle.
    #[serde(rename = "emote")]
    Emote {
        /// Der Textbaustein, den das Emote ersetzt.
        text: String,
        /// Plattform-Kennung des Emotes.
        emote_id: String,
        /// URL-Muster mit den Platzhaltern der Plattform, bei Twitch
        /// `{{format}}`, `{{theme_mode}}` und `{{scale}}`.
        url_template: String,
    },
    /// Erwaehnung eines anderen Nutzers.
    #[serde(rename = "mention")]
    Mention {
        /// Der Textbaustein inklusive `@`.
        text: String,
        /// Plattform-Kennung des erwaehnten Nutzers.
        user_id: String,
        /// Login des erwaehnten Nutzers.
        user_login: String,
    },
    /// Cheermote mit Bitbetrag.
    #[serde(rename = "cheermote")]
    Cheermote {
        /// Der Textbaustein, etwa `Cheer100`.
        text: String,
        /// Praefix des Cheermotes, etwa `Cheer`.
        prefix: String,
        /// Bits, die dieser Baustein traegt.
        bits: u64,
        /// Stufe des Cheermotes.
        tier: u32,
        /// URL-Muster fuer das Bild.
        url_template: String,
    },
}

impl Fragment {
    /// Reiner Text als bequemer Konstruktor.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Der sichtbare Textbaustein jeder Variante.
    pub fn as_text(&self) -> &str {
        match self {
            Self::Text { text }
            | Self::Emote { text, .. }
            | Self::Mention { text, .. }
            | Self::Cheermote { text, .. } => text,
        }
    }

    /// Stabile Art-Kennung, identisch mit der serde-Darstellung.
    pub const fn art(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Emote { .. } => "emote",
            Self::Mention { .. } => "mention",
            Self::Cheermote { .. } => "cheermote",
        }
    }
}

/// Verweis auf die beantwortete Nachricht (Twitch-Reply-Tags).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyRef {
    /// Kennung der beantworteten Nachricht.
    pub message_id: String,
    /// Kennung des urspruenglichen Absenders.
    pub sender_id: String,
    /// Login des urspruenglichen Absenders.
    pub sender_login: String,
    /// Anzeigename des urspruenglichen Absenders.
    pub sender_display: String,
    /// Text der beantworteten Nachricht, damit das Dock nichts nachladen muss.
    pub text: String,
}

/// Eine Chatnachricht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Plattform, aus der die Nachricht stammt.
    pub platform: Platform,
    /// Unveraenderliche Kanalkennung.
    pub channel_id: String,
    /// Anzeigbarer Kanalname.
    pub channel_login: String,
    /// Kennung der Nachricht auf der Plattform.
    pub message_id: String,
    /// Kennung des Absenders.
    pub sender_id: String,
    /// Login des Absenders.
    pub sender_login: String,
    /// Anzeigename des Absenders.
    pub sender_display: String,
    /// Namensfarbe als Hexwert, falls die Plattform eine liefert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Abzeichen des Absenders.
    #[serde(default)]
    pub badges: Vec<Badge>,
    /// Zerlegte Nachricht.
    pub fragments: Vec<Fragment>,
    /// Sendezeitpunkt laut Plattform.
    pub sent_at: DateTime<Utc>,
    /// `true` bei `/me`.
    #[serde(default)]
    pub is_action: bool,
    /// Beantwortete Nachricht, falls es eine gibt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyRef>,
}

impl ChatMessage {
    /// Stabiler Dedupe-Schluessel der Nachricht.
    ///
    /// Traegt dieselbe Form wie der Schluessel eines [`crate::ActivityEvent`],
    /// damit der Nachlauf beider Ereignisarten gleich entdoppelt werden kann.
    pub fn dedupe_key(&self) -> String {
        dedupe_key(
            self.platform,
            &self.channel_id,
            CHAT_DEDUPE_ART,
            &self.message_id,
        )
    }

    /// Nachrichtentext ohne Auszeichnung, aus den Fragmenten zusammengesetzt.
    pub fn plain_text(&self) -> String {
        self.fragments.iter().map(Fragment::as_text).collect()
    }

    /// Kanalverweis der Nachricht.
    pub fn channel(&self) -> ChannelRef {
        ChannelRef::new(self.platform, &self.channel_id, &self.channel_login)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nachricht() -> ChatMessage {
        ChatMessage {
            platform: Platform::Twitch,
            channel_id: "12345".into(),
            channel_login: "earlysalty".into(),
            message_id: "abc-1".into(),
            sender_id: "777".into(),
            sender_login: "zuschauer".into(),
            sender_display: "Zuschauer".into(),
            color: None,
            badges: vec![],
            fragments: vec![
                Fragment::text("moin "),
                Fragment::Emote {
                    text: "Kappa".into(),
                    emote_id: "25".into(),
                    url_template: "https://static-cdn.jtvnw.net/emoticons/v2/25/{{format}}".into(),
                },
            ],
            sent_at: "2026-08-23T20:15:00Z".parse().unwrap(),
            is_action: false,
            reply_to: None,
        }
    }

    #[test]
    fn plain_text_setzt_alle_fragmente_zusammen() {
        assert_eq!(nachricht().plain_text(), "moin Kappa");
    }

    #[test]
    fn dedupe_schluessel_haengt_an_der_nachrichten_id() {
        let a = nachricht();
        let mut b = nachricht();
        assert_eq!(a.dedupe_key(), b.dedupe_key());
        b.message_id = "abc-2".into();
        assert_ne!(a.dedupe_key(), b.dedupe_key());
    }

    #[test]
    fn kanalverweis_uebernimmt_id_und_login() {
        let kanal = nachricht().channel();
        assert_eq!(kanal.channel_id, "12345");
        assert_eq!(kanal.channel_login, "earlysalty");
        assert_eq!(kanal.platform, Platform::Twitch);
    }
}
