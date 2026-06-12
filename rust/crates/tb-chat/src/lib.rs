//! tb-chat — der native Twitch-Chat-Bot (Welle B des Rust-Cutovers).
//!
//! # Architektur (Entscheid 12.6., siehe rust/docs/04-cutover-plan.md)
//!
//! Der Python-Chat (TwitchIO 3.x) liest Chat über EventSub-**WebSocket**
//! (`channel.chat.message` + `channel.chat.notification`) und sendet über
//! Helix. Rust nutzt stattdessen den bestehenden EventSub-**Webhook**-Stack
//! (tb-monitoring → `POST /eventsub/dispatch` → Pipeline): Twitch erlaubt
//! `channel.chat.message` per Webhook mit App-Token, solange der Bot-Account
//! `user:bot` und der Broadcaster `channel:bot` autorisiert hat (Letzteres
//! ist exakt der Scope-Filter des Python-`join_partner_channels`).
//! „Joinen" = Webhook-Subscription anlegen; ein WebSocket-Neubau entfällt.
//!
//! Ausgehende Aktionen (Senden, Bans, Deletes) laufen mit dem
//! **Bot-User-Token** über [`api::ChatApi`]; die Token-Ownership liegt nach
//! dem Flip exklusiv bei diesem Prozess ([`token::BotTokenManager`]).
//!
//! Verträge: `/tmp/welle-b-vertraege/*.md` (zeilengenau aus dem
//! Python-Quelltext extrahiert, 12.6.).

pub mod api;
pub mod token;
pub mod types;

pub use api::{BanOutcome, ChatApi};
pub use token::{BotTokenManager, TokenError};
pub use types::{ChatMessageEvent, SendOutcome};
