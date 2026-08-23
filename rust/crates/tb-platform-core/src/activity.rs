//! Ereignisse neben dem Chat: Follow, Abo, Cheer, Raid, Streamstatus.
//!
//! Jede Variante traegt ueber [`ActivityMeta`] dieselben vier Grundangaben
//! (`platform`, `channel_id`, `occurred_at`, `dedupe_key`), damit der Bus sie
//! ohne Kenntnis der Variante einsortieren kann.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::dedupe::dedupe_key;
use crate::platform::Platform;

/// Ausloeser eines Ereignisses, also der Follower, Abonnent oder Raider.
///
/// Optional, weil `stream.online`, `stream.offline` und `channel.update`
/// keinen Ausloeser haben.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    /// Kennung des Nutzers auf der Plattform.
    pub id: String,
    /// Login des Nutzers.
    pub login: String,
    /// Anzeigename des Nutzers.
    pub display: String,
}

impl Actor {
    /// Baut einen Ausloeser.
    pub fn new(
        id: impl Into<String>,
        login: impl Into<String>,
        display: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            login: login.into(),
            display: display.into(),
        }
    }
}

/// Grundangaben, die jede Ereignisvariante traegt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityMeta {
    /// Plattform, aus der das Ereignis stammt.
    pub platform: Platform,
    /// Unveraenderliche Kanalkennung.
    pub channel_id: String,
    /// Zeitpunkt laut Plattform.
    pub occurred_at: DateTime<Utc>,
    /// Stabiler Schluessel gegen Dubletten im Nachlauf.
    pub dedupe_key: String,
    /// Ausloeser, falls das Ereignis einen hat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<Actor>,
}

impl ActivityMeta {
    /// Baut die Grundangaben mit einem bereits bekannten Dedupe-Schluessel.
    pub fn new(
        platform: Platform,
        channel_id: impl Into<String>,
        occurred_at: DateTime<Utc>,
        dedupe_key: impl Into<String>,
    ) -> Self {
        Self {
            platform,
            channel_id: channel_id.into(),
            occurred_at,
            dedupe_key: dedupe_key.into(),
            actor: None,
        }
    }

    /// Baut die Grundangaben und leitet den Dedupe-Schluessel deterministisch ab.
    ///
    /// `art` ist die Art-Kennung der Variante (siehe [`ActivityEvent::art`]),
    /// `kennzeichen` das plattformseitig eindeutige Merkmal, in der Regel die
    /// EventSub-Message-ID.
    pub fn derived(
        platform: Platform,
        channel_id: impl Into<String>,
        occurred_at: DateTime<Utc>,
        art: &str,
        kennzeichen: &str,
    ) -> Self {
        let channel_id = channel_id.into();
        let key = dedupe_key(platform, &channel_id, art, kennzeichen);
        Self {
            platform,
            channel_id,
            occurred_at,
            dedupe_key: key,
            actor: None,
        }
    }

    /// Haengt den Ausloeser an.
    #[must_use]
    pub fn with_actor(mut self, actor: Actor) -> Self {
        self.actor = Some(actor);
        self
    }
}

/// Ereignis neben dem Chat.
///
/// Der serde-Tag heisst `art`, weil `typ` bereits vom aeusseren
/// [`crate::PlatformEvent`] belegt ist. Beide Tags landen beim internen
/// Tagging in derselben JSON-Ebene, ein gleicher Name wuerde kollidieren.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "art")]
pub enum ActivityEvent {
    /// Neuer Follower.
    #[serde(rename = "follow")]
    Follow {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
    },
    /// Neues Abo.
    #[serde(rename = "subscribe")]
    Subscribe {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
        /// Stufe laut Plattform, bei Twitch `1000`, `2000`, `3000` oder `Prime`.
        tier: String,
        /// `true`, wenn das Abo geschenkt wurde.
        is_gift: bool,
    },
    /// Verlaengertes Abo mit Nachricht.
    #[serde(rename = "resub")]
    Resub {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
        /// Gesamtmonate.
        months: u32,
        /// Monate am Stueck, falls der Nutzer sie sichtbar macht.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        streak: Option<u32>,
        /// Mitgeschickte Nachricht.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Verschenkte Abos.
    #[serde(rename = "sub_gift")]
    SubGift {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
        /// Anzahl der verschenkten Abos.
        count: u32,
        /// Stufe der verschenkten Abos.
        tier: String,
    },
    /// Bits.
    #[serde(rename = "cheer")]
    Cheer {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
        /// Anzahl der Bits.
        bits: u64,
        /// Mitgeschickte Nachricht.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Eingehender Raid.
    #[serde(rename = "raid")]
    Raid {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
        /// Kanal, aus dem geraidet wurde.
        from: String,
        /// Mitgebrachte Zuschauer.
        viewers: u32,
    },
    /// Stream ist live gegangen.
    #[serde(rename = "stream_online")]
    StreamOnline {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
    },
    /// Stream ist beendet.
    #[serde(rename = "stream_offline")]
    StreamOffline {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
    },
    /// Titel oder Kategorie wurden geaendert.
    #[serde(rename = "channel_update")]
    ChannelUpdate {
        /// Grundangaben.
        #[serde(flatten)]
        meta: ActivityMeta,
        /// Neuer Titel.
        title: String,
        /// Neue Kategorie im Klartext.
        category: String,
    },
}

impl ActivityEvent {
    /// Grundangaben der Variante.
    pub fn meta(&self) -> &ActivityMeta {
        match self {
            Self::Follow { meta }
            | Self::Subscribe { meta, .. }
            | Self::Resub { meta, .. }
            | Self::SubGift { meta, .. }
            | Self::Cheer { meta, .. }
            | Self::Raid { meta, .. }
            | Self::StreamOnline { meta }
            | Self::StreamOffline { meta }
            | Self::ChannelUpdate { meta, .. } => meta,
        }
    }

    /// Plattform des Ereignisses.
    pub fn platform(&self) -> Platform {
        self.meta().platform
    }

    /// Kanalkennung des Ereignisses.
    pub fn channel_id(&self) -> &str {
        &self.meta().channel_id
    }

    /// Zeitpunkt des Ereignisses.
    pub fn occurred_at(&self) -> DateTime<Utc> {
        self.meta().occurred_at
    }

    /// Dedupe-Schluessel des Ereignisses.
    pub fn dedupe_key(&self) -> &str {
        &self.meta().dedupe_key
    }

    /// Stabile Art-Kennung, identisch mit der serde-Darstellung.
    pub const fn art(&self) -> &'static str {
        match self {
            Self::Follow { .. } => "follow",
            Self::Subscribe { .. } => "subscribe",
            Self::Resub { .. } => "resub",
            Self::SubGift { .. } => "sub_gift",
            Self::Cheer { .. } => "cheer",
            Self::Raid { .. } => "raid",
            Self::StreamOnline { .. } => "stream_online",
            Self::StreamOffline { .. } => "stream_offline",
            Self::ChannelUpdate { .. } => "channel_update",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zeitpunkt() -> DateTime<Utc> {
        "2026-08-23T20:15:00Z".parse().unwrap()
    }

    #[test]
    fn abgeleiteter_schluessel_traegt_die_art_der_variante() {
        let meta = ActivityMeta::derived(Platform::Twitch, "12345", zeitpunkt(), "follow", "m-1");
        let ereignis = ActivityEvent::Follow { meta };
        assert_eq!(ereignis.dedupe_key(), "twitch:12345:follow:m-1");
        assert_eq!(ereignis.art(), "follow");
    }

    #[test]
    fn meta_zugriff_liefert_bei_jeder_variante_dieselben_grundangaben() {
        let meta = ActivityMeta::derived(Platform::Kick, "kanal", zeitpunkt(), "cheer", "m-9");
        let ereignis = ActivityEvent::Cheer {
            meta,
            bits: 100,
            message: None,
        };
        assert_eq!(ereignis.platform(), Platform::Kick);
        assert_eq!(ereignis.channel_id(), "kanal");
        assert_eq!(ereignis.occurred_at(), zeitpunkt());
    }

    #[test]
    fn with_actor_haengt_den_ausloeser_an() {
        let meta = ActivityMeta::derived(Platform::Twitch, "12345", zeitpunkt(), "raid", "m-2")
            .with_actor(Actor::new("42", "raider", "Raider"));
        assert_eq!(meta.actor.as_ref().unwrap().login, "raider");
    }
}
