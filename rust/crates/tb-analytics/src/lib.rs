//! Analytics-Queries für den Twitch-Bot.
//!
//! Jede Funktion nimmt einen `&PgPool` entgegen und gibt typisierte Structs zurück.
//! Kein HTTP, kein Serde-JSON — nur reine Query-Logik.
// Für große json!-Antworten (chat-analytics ~40 Felder + verschachteltes dataQuality).
#![recursion_limit = "256"]

pub mod admin_affiliate;
pub mod admin_billing;
pub mod affiliate_claim_window;
pub mod affiliate_commission;
pub mod affiliate_gutschrift;
pub mod affiliate_pii;
pub mod admin_config;
pub mod admin_streamers;
pub mod ai_analysis;
pub mod ai_history;
pub mod bans;
pub mod billing;
pub mod category_activity;
pub mod chat_analytics;
pub mod chat_analytics_lexicon;
pub mod chat_deep_minimax;
pub mod chat_content_analysis;
pub mod chat_content_lexicon;
pub mod chat_hype_timeline;
pub mod chat_social_graph;
pub mod coaching;
pub mod engagement_metrics;
pub mod exp_analytics;
pub mod global_ban;
pub mod market;
pub mod monetization;
pub mod network;
pub mod overview;
pub mod partner_access;
pub mod peer_group;
pub mod plan;
pub mod raw_chat_status;
pub mod tag_analysis;
pub mod watch_time;
pub mod post_stream;
pub mod promo_mode;
pub mod raid_blacklist;
pub mod raid_history;
pub mod raids;
pub mod self_explainer_log;
pub mod streamer_link;
pub mod ads_schedule_collector;
pub mod subs_snapshot_collector;
pub mod streamers;
pub mod streamers_crud;
pub mod stripe;
pub mod system_database;
pub mod system_errors;
pub mod system_eventsub;
pub mod system_health;
pub mod system_oauth_scopes;
pub mod telemetry_routes;
pub mod trial;
