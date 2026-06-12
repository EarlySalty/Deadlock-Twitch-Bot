//! tb-transport-discord — Discord-Backend-Trait, BrokerRelay und HeadlessNoop.

pub mod backend;
pub mod noop;
pub mod relay;

pub use backend::{DiscordBackend, DiscordError, EditRichMessage, SendResult, SendRichMessage};
pub use noop::HeadlessNoop;
pub use relay::{BrokerRelay, GuildMember, ResolvedDiscordUser};
