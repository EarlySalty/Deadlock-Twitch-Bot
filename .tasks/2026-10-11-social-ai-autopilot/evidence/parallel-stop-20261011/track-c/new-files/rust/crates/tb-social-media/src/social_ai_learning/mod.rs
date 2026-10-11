mod config;
mod personal;
mod skills;
mod types;

pub use config::LearningConfig;
pub use personal::{load_personal_preferences, observe_feedback, observe_feedback_in_transaction};
pub use skills::{
    activate_recurring_pattern, active_skill_snapshots, get_skill_change, list_critical_proposals,
    list_skill_history, propose_critical_change, review_critical_proposal, rollback_skill,
    validate_owned_snapshot,
};
pub use types::*;

use serde::{de::DeserializeOwned, Serialize};

fn owner_id(owner: &str) -> Result<i64, LearningError> {
    if owner.starts_with('0') || !owner.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LearningError::InvalidInput);
    }
    owner.parse::<i64>().ok().filter(|id| *id > 0).ok_or(LearningError::InvalidInput)
}

fn encoded<T: Serialize>(value: &T) -> Result<String, LearningError> {
    serde_json::to_string(value).map_err(|_| LearningError::InvalidInput)
}

fn decoded<T: DeserializeOwned>(value: &str) -> Result<T, LearningError> {
    serde_json::from_str(value).map_err(|_| LearningError::InvalidStoredData)
}

fn enum_key<T: Serialize>(value: T) -> Result<String, LearningError> {
    serde_json::to_value(value).map_err(|_| LearningError::InvalidInput)?
        .as_str().map(str::to_owned).ok_or(LearningError::InvalidInput)
}

fn enum_value<T: DeserializeOwned>(value: &str) -> Result<T, LearningError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| LearningError::InvalidStoredData)
}

fn page(limit: i64, before: Option<i64>) -> Result<i64, LearningError> {
    if !(1..=100).contains(&limit) || before.is_some_and(|value| value <= 0) {
        return Err(LearningError::InvalidInput);
    }
    Ok(limit)
}
