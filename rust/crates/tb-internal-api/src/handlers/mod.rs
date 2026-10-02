pub mod chat_command;
pub mod clip_contest;
pub mod common;
pub mod community_points;
pub mod diagnose;
pub mod discord_invite;
pub mod eventsub;
pub mod global_ban;
pub mod healthz;
pub mod legacy_proxy;
pub mod market_share;
pub mod partner_signup_block;
pub mod patch_announcement;
pub mod python_stubs;
pub mod raid;
pub mod raid_blacklist;
pub mod raid_oauth;
pub mod reauth_all;
pub mod scam_guard;
pub mod scout_community;
pub mod self_explainer_log;
pub mod session_detail;
pub mod spam_learning;
pub mod stats_native;
pub mod streamer_analytics_native;
pub mod streamer_link;
pub mod streamers;
pub mod telemetry_routes;

/// Gemeinsame Sperre fuer DB-Tests, die Migrationen laufen lassen: die
/// Migrationen legen globale Rollen an, parallel laufende Testmodule stoeren
/// sich sonst gegenseitig ("tuple concurrently updated").
#[cfg(test)]
pub(crate) static TEST_MIGRATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
