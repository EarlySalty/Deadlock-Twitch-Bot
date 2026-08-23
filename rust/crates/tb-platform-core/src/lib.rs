//! Plattformneutrale Kerntypen fuer die eigenen OBS-Docks.
//!
//! Das Crate ist I/O-frei: keine Datenbank, kein HTTP, kein Webserver und keine
//! Abhaengigkeit auf ein anderes `tb-*`-Crate. Es haelt nur das gemeinsame
//! Vokabular, das der Bot (Schreiber), das Gateway (Verteiler) und das Dock
//! (Leser) teilen. Vorbild fuer Zuschnitt und Stil ist
//! `tb-raid/src/scope_profiles.rs`.
//!
//! Zwei Dinge sind bewusst eingefroren, weil ein Dock stundenlang offen bleibt
//! und ein aelteres Bundle weiter mitlesen koennen muss:
//!
//! 1. [`PlatformEvent`] ist intern getaggt mit dem Feld `typ` und ist damit
//!    zugleich das WebSocket-Drahtformat.
//! 2. Die inneren Aufzaehlungen [`ActivityEvent`] und [`Fragment`] tragen den
//!    Tag `art`. Sie landen beim internen Tagging in derselben JSON-Ebene wie
//!    `typ`, ein gleicher Tagname wuerde also kollidieren.
//!
//! Eingehende Ereignisse laufen nicht ueber [`PlatformAdapter`], sondern ueber
//! den Bus; der Trait deckt nur den ausgehenden Weg ab.

#![forbid(unsafe_code)]

mod activity;
mod adapter;
mod chat;
mod dedupe;
mod event;
mod platform;
mod stream_info;

pub use activity::{ActivityEvent, ActivityMeta, Actor};
pub use adapter::{AdapterError, PlatformAdapter, SendIdentity};
pub use chat::{Badge, ChatMessage, Fragment, ReplyRef, CHAT_DEDUPE_ART};
pub use dedupe::dedupe_key;
pub use event::PlatformEvent;
pub use platform::{ChannelRef, Platform};
pub use stream_info::{StreamInfo, StreamInfoPatch};
