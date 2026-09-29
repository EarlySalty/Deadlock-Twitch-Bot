use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct QuestResponse {
    pub key: &'static str,
    pub text: String,
    pub progress: i64,
    pub goal: i64,
    pub completed: bool,
}

#[derive(Debug, Serialize)]
pub struct StreakResponse {
    pub freeze_used_this_month: bool,
    pub week_qualified: bool,
    pub current: i32,
    pub longest: i32,
}

#[derive(Debug, Serialize)]
pub struct LevelResponse {
    pub level: usize,
    pub total_points: i64,
    pub current_threshold: i64,
    pub next_threshold: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct NextGoalResponse {
    pub missing_points: i64,
    pub fastest_route: String,
}

#[derive(Debug, Serialize)]
pub struct AchievementTier {
    pub earned_at: Option<DateTime<Utc>>,
    pub target: i64,
    pub unlocked: bool,
}

#[derive(Debug, Serialize)]
pub struct AchievementResponse {
    pub key: &'static str,
    pub name: &'static str,
    pub progress: i64,
    pub tiers: Vec<AchievementTier>,
}

#[derive(Debug, Serialize)]
pub struct WithUsResponse {
    pub people_brought_in_who_stayed: i64,
    pub community_hours: f64,
    pub received_raids: i64,
}

#[derive(Debug, Serialize)]
pub struct SeasonResponse {
    pub next_reset_at: DateTime<Utc>,
    pub month: String,
    pub points: i64,
    pub rank: i64,
    pub active_partners: i64,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub twitch_user_id: String,
    pub week_start: chrono::NaiveDate,
    pub next_reset_at: DateTime<Utc>,
    pub generated_at: DateTime<Utc>,
    pub timezone: &'static str,
    pub streamer: String,
    pub quests: Vec<QuestResponse>,
    pub streak: StreakResponse,
    pub level: LevelResponse,
    pub next_goal: NextGoalResponse,
    pub achievements: Vec<AchievementResponse>,
    pub with_us: WithUsResponse,
    pub season: SeasonResponse,
}

#[derive(Debug, Serialize)]
pub struct ViewerRecruiter {
    pub twitch_user_id: String,
    pub display_name: Option<String>,
    pub qualified_invites: i64,
}

#[derive(Debug, Serialize)]
pub struct ViewersResponse {
    pub generated_at: DateTime<Utc>,
    pub streamer: String,
    pub recruiters: Vec<ViewerRecruiter>,
}
