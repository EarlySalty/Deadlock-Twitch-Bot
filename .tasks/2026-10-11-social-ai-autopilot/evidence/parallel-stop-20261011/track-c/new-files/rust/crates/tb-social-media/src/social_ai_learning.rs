mod evidence;
mod personal;
mod skills;
mod types;

pub use evidence::recurring_patterns;
pub use personal::{observe, personal_preferences};
pub use skills::{
    activate_pattern, active_skill_snapshots, apply_recurring_patterns, create_critical_proposal,
    list_critical_proposals, review_critical_proposal, rollback_skill, skill_history,
    validate_skill_snapshot,
};
pub use types::*;

use sha2::{Digest, Sha256};

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn owner_valid(owner: &str) -> Result<(), LearningError> {
    if owner.is_empty()
        || owner.len() > 20
        || !owner.bytes().all(|byte| byte.is_ascii_digit())
        || owner.parse::<u64>().ok().is_none_or(|id| id == 0)
    {
        return Err(LearningError::Invalid);
    }
    Ok(())
}

fn json<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, LearningError> {
    serde_json::to_value(value).map_err(|_| LearningError::Invalid)
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, LearningError> {
    serde_json::from_value(value).map_err(|_| LearningError::Invalid)
}
