//! FromRow-Structs für read-only Zugriffe. Typen folgen dem **echten** Prod-Schema
//! (Timestamps = text → String; Bool = integer → i32; bigint → i64).

use sqlx::FromRow;

/// Auszug aus `twitch_streamers` (PK `twitch_login`).
#[derive(Debug, Clone, FromRow)]
pub struct TwitchStreamerRow {
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
    pub created_at: Option<String>,
}

/// Auszug aus `twitch_partners` (PK `id` bigserial).
#[derive(Debug, Clone, FromRow)]
pub struct TwitchPartnerRow {
    pub id: i64,
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub status: String,
    pub raid_bot_enabled: Option<i32>,
    pub live_ping_role_id: Option<i64>,
    pub partnered_at: Option<String>,
}

/// Auszug aus `streamer_plans` (PK `twitch_user_id`).
#[derive(Debug, Clone, FromRow)]
pub struct StreamerPlanRow {
    pub twitch_user_id: String,
    pub twitch_login: Option<String>,
    pub plan_name: String,
    pub promo_disabled: i32,
    pub activated_at: String,
    pub expires_at: Option<String>,
    pub trial_ever_granted: i32,
}
