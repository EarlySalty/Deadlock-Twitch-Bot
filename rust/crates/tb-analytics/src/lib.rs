//! Analytics-Queries für den Twitch-Bot.
//!
//! Jede Funktion nimmt einen `&PgPool` entgegen und gibt typisierte Structs zurück.
//! Kein HTTP, kein Serde-JSON — nur reine Query-Logik.

pub mod admin_billing;
pub mod admin_config;
pub mod admin_streamers;
pub mod bans;
pub mod global_ban;
pub mod market;
pub mod network;
pub mod overview;
pub mod partner_access;
pub mod plan;
pub mod post_stream;
pub mod promo_mode;
pub mod raid_blacklist;
pub mod raids;
pub mod self_explainer_log;
pub mod streamer_link;
pub mod streamers;
pub mod streamers_crud;
pub mod system_database;
pub mod system_errors;
pub mod system_eventsub;
pub mod system_health;
pub mod telemetry_routes;
pub mod trial;
