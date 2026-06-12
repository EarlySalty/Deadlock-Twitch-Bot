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
//! Module (je ein Python-Vertrag unter `/tmp/welle-b-vertraege/`):
//! [`spam_filter`] (zweistufiger Spam-Score), [`scam_pitch`]
//! (Service-Pitch-Detektor + MiniMax-Review), [`promos`] (Promo-Engine mit
//! Doppelsend-Lock), [`commands`] (die 12 Chat-Commands), [`moderation`]
//! (HelixChatClient, Auto-Ban, TimeoutGuard, Outbound-Suppression),
//! [`global_ban_sweep`] (Offline-Sweep-Executor).

pub mod api;
pub mod commands;
pub mod global_ban_sweep;
pub mod moderation;
pub mod promos;
pub mod scam_pitch;
pub mod spam_filter;
pub mod token;
pub mod types;

pub use api::{BanOutcome, ChatApi};
pub use commands::{
    AutobanEntry, CommandEngine, DiscordLinkPort, InvitePort, LastAutobanStore, RaidCommandPort,
    RaidStatusInfo, SuperModPort,
};
pub use global_ban_sweep::{GlobalBanSweeper, PartnerRoster};
pub use moderation::{
    AutoBanRequest, ChannelGuardPort, HelixChatClient, ModerationEngine, OutboundSuppressionCheck,
    OutboundSuppressionStore, TimeoutGuard,
};
pub use promos::{
    NoopSuppressionCheck, PartnerChannelCheck, PresetPicker, PromoEngine, RandomPresetPicker,
    StaticInviteResolver,
};
pub use scam_pitch::{AccountAgePort, PitchDecision, ScamPitchDetector, SpamAiReviewer};
pub use spam_filter::{LearnedPatterns, SpamAction, SpamContext, SpamFilter, SpamVerdict};
pub use token::{BotTokenManager, TokenError};
pub use types::{ChatMessageEvent, SendOutcome};
