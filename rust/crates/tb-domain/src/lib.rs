//! Reine Domänen-Typen des Twitch-Bots (kein I/O, kein sqlx).

pub mod ids;
pub mod login;
pub mod partner;
pub mod signup_block;

pub use ids::{StreamerLogin, TwitchUserId};
pub use login::normalize_twitch_login;
pub use partner::PartnerStatus;
pub use signup_block::{
    SignupBlock, PROMOTE_BLOCK_REASON, RAID_BLACKLIST_REASON_PREFIX, SIGNUP_BLOCK_BODY,
    SIGNUP_BLOCK_TITLE,
};
