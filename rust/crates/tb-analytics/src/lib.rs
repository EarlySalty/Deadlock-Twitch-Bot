//! Analytics-Queries für den Twitch-Bot.
//!
//! Jede Funktion nimmt einen `&PgPool` entgegen und gibt typisierte Structs zurück.
//! Kein HTTP, kein Serde-JSON — nur reine Query-Logik.

pub mod admin_streamers;
pub mod bans;
pub mod global_ban;
pub mod network;
pub mod overview;
pub mod raid_blacklist;
pub mod raids;
pub mod streamers;
pub mod streamers_crud;
pub mod system_database;
pub mod system_errors;
pub mod system_eventsub;
pub mod system_health;
