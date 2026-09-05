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
pub mod catalog;
pub mod channel_classifier;
pub mod channel_policy;
pub mod chatter_tracking;
pub mod commands;
pub mod conversation_scam;
pub mod crew_guard;
pub mod fun_responses;
pub mod global_ban_sweep;
pub mod global_chatter_ban;
pub mod invite_question;
pub mod lfg_pitch;
pub mod lurker_policy;
pub mod mention_scoring;
pub mod moderation;
pub mod moderation_settings;
pub mod pipeline;
pub mod promo_pitch;
pub mod promos;
pub mod safe_list;
pub mod scam_pitch;
pub mod secret_sink;
pub mod spam_filter;
pub mod standard_replies;
pub mod stats;
pub mod steam_lookup;
pub mod style_score;
pub mod suppression_guard;
pub mod sus_invite;
pub mod timeout_tracking;
pub mod title_ai;
pub mod title_db;
pub mod title_jobs;
pub mod token;
pub mod types;

pub use api::{BanOutcome, ChatApi};
pub use channel_classifier::{ChannelClass, ChannelClassifier};
pub use chatter_tracking::ChatterTracker;
pub use commands::{
    AutobanEntry, CommandEngine, DiscordLinkPort, InvitePort, LastAutobanStore, RaidCommandPort,
    RaidStatusInfo, SuperModPort,
};
pub use conversation_scam::{
    ConversationScamGuard, DialogState, GuardMode, GuardSettings, MiniMaxScamJudge, ScamJudge,
    Verdict, VerdictKind,
};
pub use crew_guard::{
    screen as crew_screen, CrewGuard, CrewJudge, CrewSignal, CrewVerdict, OpenAiCrewJudge,
};
pub use fun_responses::FunResponses;
pub use global_ban_sweep::{GlobalBanSweeper, PartnerRoster};
pub use global_chatter_ban::GlobalChatterBanEnforcer;
pub use invite_question::{
    InviteQuestionInviteUrlPort, InviteQuestionJudge, InviteQuestionJudgeInput,
    InviteQuestionResponder, InviteQuestionRollup, InviteQuestionStore, InviteQuestionVerdict,
    InviteQuestionVerdictKind, MiniMaxInviteQuestionJudge, PgInviteQuestionStore,
};
pub use lfg_pitch::{
    lfg_pitch_enabled_from_env, LfgJudge, LfgJudgeInput, LfgPitchResponder, LfgVerdict,
    LfgVerdictKind, MiniMaxLfgJudge, LFG_PITCH_REPLY,
};
pub use lurker_policy::{
    is_passive_lurker_channel, should_attempt_runtime_heal, PASSIVE_LURKER_DETAIL,
    PASSIVE_LURKER_STATE,
};
pub use mention_scoring::{score_mention_patterns, MentionResolver, WHITELISTED_BOTS};
pub use moderation_settings::{ModerationSettings, ModerationSettingsCache};
pub use moderation::{
    AutoBanRequest, ChannelGuardPort, HelixChatClient, ModerationEngine, OutboundSuppressionCheck,
    OutboundSuppressionStore, TimeoutGuard,
};
pub use pipeline::{
    ChatPipeline, ChatPipelineParts, CrewRadarAlert, ModAlerter, PgHelixMentionResolver, ReviewLog,
    SCAM_PITCH_TIMEOUT_REASON,
};
pub use promo_pitch::{
    build_channel_promo_text, build_targeted_pitch_text, parse_pitch_response, pitch_filter_reject,
    ChannelPromoContext, FireworksPitchJudge, FireworksPitchTextGen, PitchJudge, PitchJudgeInput,
    PitchOccasion, PitchRejectReason, PitchResponse, PitchTextGen, TargetedPitchContext,
};
pub use promos::{
    promo_invite_fallback, NoopSuppressionCheck, PartnerChannelCheck, PitchReviewSink, PromoEngine,
    StaticInviteResolver, DEFAULT_PROMO_DISCORD_INVITE,
};
pub use safe_list::{is_safe as is_safe_account, SafeAccount, SAFE_ACCOUNTS};
pub use scam_pitch::{AccountAgePort, PitchDecision, ScamPitchDetector, SpamAiReviewer};
pub use secret_sink::{InfisicalWriter, SecretSink, SecretWriteError};
pub use spam_filter::{LearnedPatterns, SpamAction, SpamContext, SpamFilter, SpamVerdict};
pub use standard_replies::{classify_standard_reply, StandardReplies, StandardReply};
pub use suppression_guard::{
    DbManualPartnerOptOutCheck, ManualPartnerOptOutCheck, SuppressionGuardChatApi,
};
pub use sus_invite::{SusInviteCheck, SusInviteHit};
pub use timeout_tracking::{is_bot_timeout_drop, CombinedSuppression, TimeoutTrackingChatApi};
pub use token::{load_seed_tokens, BotTokenManager, SeedTokens, TokenError};
pub use types::{ChatMessageEvent, SendOutcome};
