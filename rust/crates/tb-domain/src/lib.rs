//! Reine Domänen-Typen des Twitch-Bots (kein I/O, kein sqlx).

pub mod ids;
pub mod partner;

pub use ids::{StreamerLogin, TwitchUserId};
pub use partner::PartnerStatus;
