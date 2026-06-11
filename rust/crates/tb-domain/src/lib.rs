//! Reine Domänen-Typen des Twitch-Bots (kein I/O, kein sqlx).

pub mod ids;
pub mod login;
pub mod partner;

pub use ids::{StreamerLogin, TwitchUserId};
pub use login::normalize_twitch_login;
pub use partner::PartnerStatus;
