//! Analytics-Queries für den Twitch-Bot.
//!
//! Jede Funktion nimmt einen `&PgPool` entgegen und gibt typisierte Structs zurück.
//! Kein HTTP, kein Serde-JSON — nur reine Query-Logik.

pub mod admin_streamers;
pub mod global_ban;
pub mod streamers_crud;
pub mod bans;
pub mod network;
pub mod overview;
pub mod raids;
pub mod streamers;
pub mod system_database;
pub mod system_errors;
pub mod system_eventsub;
pub mod system_health;
