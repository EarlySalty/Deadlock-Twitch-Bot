use serde::{Deserialize, Serialize};

use crate::autopilot_settings::{FrameFormat, PublishLanguage};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditPlan {
    pub schema_version: u16,
    pub source_duration_ms: u64,
    pub kept_intervals: Vec<KeptInterval>,
    #[serde(default)]
    pub hook: Option<KeptInterval>,
    pub observed_highlights: Vec<Highlight>,
    pub significant_uncertainty: bool,
    pub review_reasons: Vec<String>,
    pub layout: LayoutIntent,
}

impl EditPlan {
    pub fn timeline(&self) -> Vec<KeptInterval> {
        self.hook
            .iter()
            .chain(&self.kept_intervals)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptInterval {
    pub start_ms: u64,
    pub end_ms: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Highlight {
    pub start_ms: u64,
    pub end_ms: u64,
    pub observation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutIntent {
    ExistingStable,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorOptions {
    pub multi_output_enabled: bool,
    pub experimental_ai_layout: bool,
    pub publish_language: PublishLanguage,
    pub frame_format: FrameFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanLimits {
    pub max_plan_bytes: usize,
    pub max_source_duration_ms: u64,
    pub max_source_duration_drift_ms: u64,
    pub max_output_duration_ms: u64,
    pub max_intervals: usize,
    pub max_highlights: usize,
    pub max_review_reasons: usize,
    pub max_text_bytes: usize,
    pub max_subtitle_segments: usize,
    pub max_subtitle_text_bytes: usize,
    pub min_kept_percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub max_source_bytes: u64,
    pub max_output_bytes: u64,
    pub max_probe_bytes: usize,
    pub max_media_duration_ms: u64,
    pub max_media_width: u32,
    pub max_media_height: u32,
    pub max_frames: usize,
    pub frame_width: u32,
    pub max_frame_bytes: u64,
    pub max_ass_bytes: usize,
    pub process_deadline_ms: u64,
    pub total_frames_deadline_ms: u64,
    pub max_duration_drift_ms: u64,
    pub encoder_threads: u16,
}
