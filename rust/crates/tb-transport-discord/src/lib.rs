//! tb-transport-discord — Discord-Backend-Trait, BrokerRelay und HeadlessNoop.

pub mod backend;
pub mod noop;
pub mod relay;

pub use backend::{
    DeleteMessage, DiscordBackend, DiscordError, DmPurpose, EditRichMessage, SendAlertEmbed,
    SendResult, SendRichMessage, SendUserDm,
};
pub use noop::HeadlessNoop;
pub use relay::{
    BrokerRelay, ClipSubmitResult, GuildMember, InviteInfo, MessageReaction, MessageReactions,
    ResolvedDiscordUser,
};
