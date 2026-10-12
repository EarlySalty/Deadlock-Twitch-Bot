use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillId {
    DeadlockUnderstanding,
    HighlightAnalysis,
    EditingStrategy,
    VideoLayout,
    Subtitles,
    Copywriting,
    QualityValidation,
    FeedbackLearning,
}

#[derive(Debug, Clone)]
pub struct SkillSnapshot {
    pub id: SkillId,
    pub version: &'static str,
    pub sha256: String,
    pub text: &'static str,
}

pub fn skill_snapshot(id: SkillId) -> SkillSnapshot {
    let text = match id {
        SkillId::DeadlockUnderstanding => {
            include_str!("../../../../../ops/social-ai/skills/deadlock-understanding.md")
        }
        SkillId::HighlightAnalysis => {
            include_str!("../../../../../ops/social-ai/skills/highlight-analysis.md")
        }
        SkillId::EditingStrategy => {
            include_str!("../../../../../ops/social-ai/skills/editing-strategy.md")
        }
        SkillId::VideoLayout => include_str!("../../../../../ops/social-ai/skills/video-layout.md"),
        SkillId::Subtitles => include_str!("../../../../../ops/social-ai/skills/subtitles.md"),
        SkillId::Copywriting => include_str!("../../../../../ops/social-ai/skills/copywriting.md"),
        SkillId::QualityValidation => {
            include_str!("../../../../../ops/social-ai/skills/quality-validation.md")
        }
        SkillId::FeedbackLearning => {
            include_str!("../../../../../ops/social-ai/skills/feedback-learning.md")
        }
    };
    let sha256 = Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    SkillSnapshot {
        id,
        version: "1",
        sha256,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_snapshots_are_versioned_and_bounded() {
        let stages = [
            SkillId::DeadlockUnderstanding,
            SkillId::HighlightAnalysis,
            SkillId::EditingStrategy,
            SkillId::VideoLayout,
            SkillId::Subtitles,
            SkillId::Copywriting,
            SkillId::QualityValidation,
            SkillId::FeedbackLearning,
        ];
        for id in stages {
            let snapshot = skill_snapshot(id);
            assert_eq!(snapshot.version, "1");
            assert!(!snapshot.text.is_empty());
            assert!(snapshot.text.len() <= 4096);
            assert_eq!(snapshot.sha256.len(), 64);
        }
        assert_ne!(
            skill_snapshot(stages[0]).sha256,
            skill_snapshot(stages[1]).sha256
        );
    }
}
