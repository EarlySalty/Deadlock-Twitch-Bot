use serde::{Deserialize, Serialize};

use super::LearningError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningConfig {
    pub personal_min_versions: u32,
    pub global_min_owners: u32,
    pub global_min_decisions: u32,
    pub global_min_support_percent: u8,
    pub evidence_window_days: u16,
    pub max_skill_bytes: usize,
}

impl LearningConfig {
    pub fn from_yaml(yaml: &str) -> Result<Self, LearningError> {
        let config: Self = serde_yaml::from_str(yaml).map_err(|_| LearningError::InvalidInput)?;
        config.validate()?;
        Ok(config)
    }

    pub fn compiled() -> Result<Self, LearningError> {
        Self::from_yaml(include_str!("../../../../../ops/social-ai/learning.yaml"))
    }

    pub fn validate(&self) -> Result<(), LearningError> {
        if !(2..=20).contains(&self.personal_min_versions)
            || !(5..=1000).contains(&self.global_min_owners)
            || !(10..=100_000).contains(&self.global_min_decisions)
            || self.global_min_decisions < self.global_min_owners
            || !(80..=100).contains(&self.global_min_support_percent)
            || !(7..=365).contains(&self.evidence_window_days)
            || !(2048..=4096).contains(&self.max_skill_bytes)
        {
            return Err(LearningError::InvalidInput);
        }
        Ok(())
    }
}
