use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use crate::ai_editor::SkillId;

#[derive(Debug, thiserror::Error)]
pub enum LearningError {
    #[error("Die Lerndaten konnten nicht gespeichert oder geladen werden.")]
    Database(#[from] sqlx::Error),
    #[error("Die Lernanfrage ist ungültig.")]
    InvalidInput,
    #[error("Die Lerndaten wurden nicht gefunden.")]
    NotFound,
    #[error("Die Lernversion hat sich bereits geändert.")]
    Conflict,
    #[error("Die gespeicherten Lerndaten konnten nicht sicher geprüft werden.")]
    InvalidStoredData,
    #[error("Für eine globale Änderung gibt es noch nicht genügend verlässliche Hinweise.")]
    InsufficientEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningPattern {
    PreserveContext,
    ShorterOpening,
    SmallerSubtitles,
    LargerSubtitles,
    PreserveReactions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AggregateEvidence {
    pub pattern: LearningPattern,
    pub distinct_owners: u32,
    pub supporting_decisions: u32,
    pub opposing_decisions: u32,
    pub window_days: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OwnedSkillSnapshot {
    pub id: SkillId,
    pub version: String,
    pub sha256: String,
    pub base_sha256: String,
    pub text: String,
    pub patterns: Vec<LearningPattern>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillChangeKind {
    Automatic,
    Rollback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillChange {
    pub id: i64,
    pub skill_id: SkillId,
    pub previous_version: String,
    pub new_version: String,
    pub created_at: DateTime<Utc>,
    pub kind: SkillChangeKind,
    pub summary: String,
    pub reason: String,
    pub evidence: Vec<AggregateEvidence>,
    pub previous_sha256: String,
    pub new_sha256: String,
    pub previous_text: String,
    pub new_text: String,
    pub textual_diff: String,
    pub restored_from_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackRequest {
    pub skill_id: SkillId,
    pub expected_current_version: String,
    pub restore_version: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Code,
    Architecture,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriticalProposal {
    pub id: i64,
    pub kind: ProposalKind,
    pub created_at: DateTime<Utc>,
    pub summary: String,
    pub reason: String,
    pub proposed_change: String,
    pub evidence: Vec<AggregateEvidence>,
    pub status: ProposalStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Proposed,
    Reviewed,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "parameter", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreferenceValue {
    SubtitleFontSize { value: u16 },
    SubtitleMargin { value: u16 },
    TargetDurationMs { value: u32 },
    EditingIntensity { value: EditingIntensity },
    PreserveReactions { value: bool },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditingIntensity {
    Conservative,
    Balanced,
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreferenceAdjustment {
    pub previous: PreferenceValue,
    pub requested: PreferenceValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LearnedPreference {
    pub value: PreferenceValue,
    pub supporting_versions: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PersonalPreferences {
    pub through_sequence: i64,
    pub values: Vec<LearnedPreference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationKind {
    Feedback { adjustments: Vec<PreferenceAdjustment> },
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FeedbackObservation {
    pub event_key: String,
    pub video_version_id: i64,
    pub kind: ObservationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObservationReceipt {
    pub sequence: i64,
    pub inserted: bool,
    pub preferences: PersonalPreferences,
}
