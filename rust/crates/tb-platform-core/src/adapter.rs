//! Der Port zur Plattform.
//!
//! Nur der ausgehende Weg laeuft ueber den Trait. Eingehende Ereignisse kommen
//! nicht hier durch, sondern ueber den Bus, weil jede Plattform ihren eigenen
//! Empfangsweg hat (Twitch EventSub-Webhook, YouTube Polling, Kick Pusher).

use std::error::Error;
use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::platform::{ChannelRef, Platform};
use crate::stream_info::{StreamInfo, StreamInfoPatch};

/// Unter welchem Namen gesendet wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SendIdentity {
    /// Der Bot-Account.
    #[serde(rename = "bot")]
    Bot,
    /// Der Streamer selbst; braucht bei Twitch die Scopes `user:write:chat`
    /// und `user:bot` im Streamer-Token.
    #[serde(rename = "broadcaster")]
    Broadcaster,
}

impl SendIdentity {
    /// Stabile Kurzkennung, identisch mit der serde-Darstellung.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bot => "bot",
            Self::Broadcaster => "broadcaster",
        }
    }
}

impl fmt::Display for SendIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Fehler eines Plattformzugriffs.
///
/// Bewusst ohne `thiserror`: das Crate soll I/O- und abhaengigkeitsfrei
/// bleiben. Die Varianten sind so geschnitten, dass der Aufrufer daraus ohne
/// Textvergleich einen HTTP-Status bilden kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    /// Die Plattform kann das nicht oder der Adapter baut es noch nicht.
    NotSupported {
        /// Betroffene Plattform.
        platform: Platform,
        /// Betroffene Operation.
        operation: &'static str,
    },
    /// Dem hinterlegten Token fehlt ein Scope; wird zu `scope_missing`.
    MissingScope {
        /// Der fehlende Scope.
        scope: String,
    },
    /// Kein oder kein gueltiges Token fuer diesen Kanal.
    NotAuthorized,
    /// Die Plattform bremst.
    RateLimited {
        /// Wartezeit in Sekunden, falls die Plattform eine nennt.
        retry_after_seconds: Option<u64>,
    },
    /// Die Eingabe passt nicht, etwa ein zu langer Titel.
    InvalidInput(String),
    /// Die Plattform hat mit einem Fehler geantwortet.
    Upstream {
        /// HTTP-Status, falls es einen gab.
        status: Option<u16>,
        /// Meldung der Plattform.
        message: String,
    },
    /// Die Plattform war gar nicht erreichbar.
    Transport(String),
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSupported {
                platform,
                operation,
            } => write!(f, "{platform} unterstuetzt {operation} nicht"),
            Self::MissingScope { scope } => write!(f, "Scope fehlt: {scope}"),
            Self::NotAuthorized => f.write_str("kein gueltiges Token fuer diesen Kanal"),
            Self::RateLimited {
                retry_after_seconds: Some(sekunden),
            } => write!(f, "Rate-Limit, erneut in {sekunden} s"),
            Self::RateLimited {
                retry_after_seconds: None,
            } => f.write_str("Rate-Limit"),
            Self::InvalidInput(grund) => write!(f, "ungueltige Eingabe: {grund}"),
            Self::Upstream {
                status: Some(status),
                message,
            } => write!(f, "Plattformfehler {status}: {message}"),
            Self::Upstream {
                status: None,
                message,
            } => write!(f, "Plattformfehler: {message}"),
            Self::Transport(grund) => write!(f, "Verbindungsfehler: {grund}"),
        }
    }
}

impl Error for AdapterError {}

/// Ausgehender Zugriff auf eine Plattform.
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Plattform, die dieser Adapter bedient.
    fn platform(&self) -> Platform;

    /// Liest den aktuellen Kanalzustand.
    async fn stream_info(&self, channel: &ChannelRef) -> Result<StreamInfo, AdapterError>;

    /// Schreibt Titel, Kategorie oder Tags.
    async fn set_stream_info(
        &self,
        channel: &ChannelRef,
        patch: &StreamInfoPatch,
    ) -> Result<(), AdapterError>;

    /// Sendet eine Chatnachricht unter der gewuenschten Identitaet.
    async fn send_chat(
        &self,
        channel: &ChannelRef,
        text: &str,
        as_identity: SendIdentity,
    ) -> Result<(), AdapterError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimaler Adapter, der nur beweist, dass der Trait objektsicher ist und
    /// sich ohne weitere Abhaengigkeiten implementieren laesst.
    struct StummerAdapter;

    #[async_trait]
    impl PlatformAdapter for StummerAdapter {
        fn platform(&self) -> Platform {
            Platform::Twitch
        }

        async fn stream_info(&self, channel: &ChannelRef) -> Result<StreamInfo, AdapterError> {
            Ok(StreamInfo::offline(
                self.platform(),
                &channel.channel_id,
                "Pause",
            ))
        }

        async fn set_stream_info(
            &self,
            _channel: &ChannelRef,
            _patch: &StreamInfoPatch,
        ) -> Result<(), AdapterError> {
            Err(AdapterError::MissingScope {
                scope: "channel:manage:broadcast".into(),
            })
        }

        async fn send_chat(
            &self,
            _channel: &ChannelRef,
            _text: &str,
            as_identity: SendIdentity,
        ) -> Result<(), AdapterError> {
            match as_identity {
                SendIdentity::Bot => Ok(()),
                SendIdentity::Broadcaster => Err(AdapterError::MissingScope {
                    scope: "user:write:chat".into(),
                }),
            }
        }
    }

    #[test]
    fn trait_ist_objektsicher() {
        let adapter: Box<dyn PlatformAdapter> = Box::new(StummerAdapter);
        assert_eq!(adapter.platform(), Platform::Twitch);
    }

    #[test]
    fn fehlertexte_nennen_die_ursache() {
        assert_eq!(
            AdapterError::MissingScope {
                scope: "user:bot".into()
            }
            .to_string(),
            "Scope fehlt: user:bot"
        );
        assert_eq!(
            AdapterError::NotSupported {
                platform: Platform::Kick,
                operation: "set_stream_info"
            }
            .to_string(),
            "kick unterstuetzt set_stream_info nicht"
        );
    }

    #[test]
    fn sende_identitaet_ist_eingefroren() {
        assert_eq!(SendIdentity::Bot.as_str(), "bot");
        assert_eq!(SendIdentity::Broadcaster.as_str(), "broadcaster");
    }
}
