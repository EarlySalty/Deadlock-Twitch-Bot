//! Gemeinsame Datenverträge des persistenten Analyse-Folgechats.
//! Der separate Dashboard-Assistent verwendet diesen Speicher nicht.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LLM_HOURLY_FOLLOW_UP_LIMIT: i64 = 10;
pub const OPUS_SESSION_FOLLOW_UP_LIMIT: i64 = 3;
pub const CHAT_SESSION_RETENTION_HOURS: i64 = 24;
pub const AI_MODEL_OPUS: &str = "opus";
pub const AI_MODEL_LLM: &str = "llm";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatSession {
    pub model: String,
    pub streamer: String,
    pub analysis_id: i64,
    pub days: i64,
    pub game_filter: String,
    pub user_context: String,
    pub ctx: Value,
    pub points: Value,
    pub history: Vec<Value>,
    pub follow_up_count: i64,
    pub created_at: DateTime<Utc>,
}

pub fn chat_session_key(streamer: &str, analysis_id: i64) -> String {
    format!("{streamer}_{analysis_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_key_format() {
        assert_eq!(chat_session_key("nani", 42), "nani_42");
    }
}
