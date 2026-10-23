use super::model::{EditPlan, KeptInterval, PlanLimits};
use crate::subtitles::{build_branded_ass, segment_subtitles, SubtitleSegment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidLimits,
    PlanTooLarge,
    InvalidSchema,
    InvalidDuration,
    SourceMismatch,
    InvalidInterval,
    IntervalOrder,
    InvalidHook,
    TooManyIntervals,
    OutputTooLong,
    TooMuchRemoved,
    InvalidHighlight,
    HighlightRemoved,
    InvalidDecisionText,
    UncertaintyNeedsReview,
    InvalidSubtitle,
    TooManySubtitles,
    InvalidMedia,
    MediaTooLarge,
    ProcessFailed,
    ProcessDeadline,
}

#[derive(Debug, thiserror::Error)]
#[error("Die Videobearbeitung konnte nicht sicher abgeschlossen werden.")]
pub struct EditorError {
    pub code: ErrorCode,
}

impl EditorError {
    pub(super) fn new(code: ErrorCode) -> Self {
        Self { code }
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedPlan {
    plan: EditPlan,
    timeline: Vec<KeptInterval>,
    output_duration_ms: u64,
}

const MIN_HOOK_MS: u64 = 1_000;
const MAX_HOOK_MS: u64 = 5_000;

impl ValidatedPlan {
    pub fn source_duration_ms(&self) -> u64 {
        self.plan.source_duration_ms
    }

    pub fn output_duration_ms(&self) -> u64 {
        self.output_duration_ms
    }

    pub fn intervals(&self) -> &[KeptInterval] {
        &self.timeline
    }

    pub fn needs_review(&self) -> bool {
        self.plan.significant_uncertainty || !self.plan.review_reasons.is_empty()
    }

    pub fn review_reasons(&self) -> &[String] {
        &self.plan.review_reasons
    }

    pub fn decisions(&self) -> &EditPlan {
        &self.plan
    }

    pub(super) fn preserves_highlight(&self, start_ms: u64, end_ms: u64) -> bool {
        preserves_highlight(&self.plan.kept_intervals, start_ms, end_ms)
    }
}

fn preserves_highlight(intervals: &[KeptInterval], start_ms: u64, end_ms: u64) -> bool {
    intervals
        .iter()
        .any(|interval| interval.start_ms <= start_ms && interval.end_ms >= end_ms)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceSubtitle {
    pub segment: SubtitleSegment,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MappedSubtitle {
    pub segment: SubtitleSegment,
    pub language: Option<String>,
    pub partial: bool,
}

pub(super) fn check_plan_limits(limits: &PlanLimits) -> Result<(), EditorError> {
    if limits.max_plan_bytes == 0
        || limits.max_source_duration_ms == 0
        || limits.max_source_duration_drift_ms == 0
        || limits.max_output_duration_ms == 0
        || limits.max_intervals == 0
        || limits.max_highlights == 0
        || limits.max_review_reasons == 0
        || limits.max_text_bytes == 0
        || limits.max_subtitle_segments == 0
        || limits.max_subtitle_text_bytes == 0
        || limits.min_kept_percent > 100
    {
        return Err(EditorError::new(ErrorCode::InvalidLimits));
    }
    Ok(())
}

pub fn validate_plan_json(
    bytes: &[u8],
    measured_duration_ms: u64,
    limits: &PlanLimits,
) -> Result<ValidatedPlan, EditorError> {
    check_plan_limits(limits)?;
    if bytes.len() > limits.max_plan_bytes {
        return Err(EditorError::new(ErrorCode::PlanTooLarge));
    }
    let plan: EditPlan =
        serde_json::from_slice(bytes).map_err(|_| EditorError::new(ErrorCode::InvalidSchema))?;
    validate_plan(plan, measured_duration_ms, limits)
}

pub fn validate_plan(
    plan: EditPlan,
    measured_duration_ms: u64,
    limits: &PlanLimits,
) -> Result<ValidatedPlan, EditorError> {
    check_plan_limits(limits)?;
    let encoded =
        serde_json::to_vec(&plan).map_err(|_| EditorError::new(ErrorCode::InvalidSchema))?;
    if encoded.len() > limits.max_plan_bytes {
        return Err(EditorError::new(ErrorCode::PlanTooLarge));
    }
    if plan.schema_version != 1 {
        return Err(EditorError::new(ErrorCode::InvalidSchema));
    }
    if plan.source_duration_ms == 0
        || plan.source_duration_ms > limits.max_source_duration_ms
        || measured_duration_ms == 0
        || measured_duration_ms > limits.max_source_duration_ms
    {
        return Err(EditorError::new(ErrorCode::InvalidDuration));
    }
    if plan.source_duration_ms.abs_diff(measured_duration_ms) > limits.max_source_duration_drift_ms
    {
        return Err(EditorError::new(ErrorCode::SourceMismatch));
    }
    if plan.kept_intervals.is_empty() || plan.kept_intervals.len() > limits.max_intervals {
        return Err(EditorError::new(ErrorCode::TooManyIntervals));
    }
    if plan.observed_highlights.len() > limits.max_highlights
        || plan.review_reasons.len() > limits.max_review_reasons
    {
        return Err(EditorError::new(ErrorCode::InvalidLimits));
    }
    if plan.significant_uncertainty && plan.review_reasons.is_empty() {
        return Err(EditorError::new(ErrorCode::UncertaintyNeedsReview));
    }
    let mut end = 0;
    let mut output_duration_ms = 0_u64;
    for interval in &plan.kept_intervals {
        if interval.start_ms >= interval.end_ms || interval.end_ms > measured_duration_ms {
            return Err(EditorError::new(ErrorCode::InvalidInterval));
        }
        if interval.start_ms < end {
            return Err(EditorError::new(ErrorCode::IntervalOrder));
        }
        valid_decision_text(&interval.reason, limits.max_text_bytes)?;
        end = interval.end_ms;
        output_duration_ms = output_duration_ms
            .checked_add(interval.end_ms - interval.start_ms)
            .ok_or_else(|| EditorError::new(ErrorCode::OutputTooLong))?;
    }
    if (output_duration_ms as u128) * 100
        < (measured_duration_ms as u128) * (limits.min_kept_percent as u128)
    {
        return Err(EditorError::new(ErrorCode::TooMuchRemoved));
    }
    if let Some(hook) = &plan.hook {
        validate_hook(hook, &plan.kept_intervals, limits)?;
        output_duration_ms += hook.end_ms - hook.start_ms;
    }
    for highlight in &plan.observed_highlights {
        if highlight.start_ms >= highlight.end_ms || highlight.end_ms > measured_duration_ms {
            return Err(EditorError::new(ErrorCode::InvalidHighlight));
        }
        valid_decision_text(&highlight.observation, limits.max_text_bytes)?;
        if !preserves_highlight(&plan.kept_intervals, highlight.start_ms, highlight.end_ms) {
            return Err(EditorError::new(ErrorCode::HighlightRemoved));
        }
    }
    for reason in &plan.review_reasons {
        valid_decision_text(reason, limits.max_text_bytes)?;
    }
    if output_duration_ms > limits.max_output_duration_ms {
        return Err(EditorError::new(ErrorCode::OutputTooLong));
    }
    Ok(ValidatedPlan {
        timeline: plan.timeline(),
        plan,
        output_duration_ms,
    })
}

fn validate_hook(
    hook: &KeptInterval,
    kept: &[KeptInterval],
    limits: &PlanLimits,
) -> Result<(), EditorError> {
    let length = hook.end_ms.saturating_sub(hook.start_ms);
    if !(MIN_HOOK_MS..=MAX_HOOK_MS).contains(&length)
        || !preserves_highlight(kept, hook.start_ms, hook.end_ms)
    {
        return Err(EditorError::new(ErrorCode::InvalidHook));
    }
    valid_decision_text(&hook.reason, limits.max_text_bytes)
}

fn forbidden_decision_char(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '[' | ']' | '{' | '}' | '<' | '>' | ';' | '`' | '$' | '\\' | '/'
        )
}

pub(crate) fn clean_decision_text(text: &mut String, max_bytes: usize) {
    let replaced: String = text
        .chars()
        .map(|ch| if forbidden_decision_char(ch) { ' ' } else { ch })
        .collect();
    let mut cleaned = replaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches(|ch: char| ch == '-' || ch.is_whitespace())
        .to_string();
    if cleaned.len() > max_bytes {
        let mut end = max_bytes;
        while !cleaned.is_char_boundary(end) {
            end -= 1;
        }
        cleaned.truncate(end);
        cleaned.truncate(cleaned.trim_end().len());
    }
    *text = cleaned;
}

pub(crate) fn clean_plan_texts(plan: &mut EditPlan, max_bytes: usize) {
    for interval in plan.kept_intervals.iter_mut().chain(plan.hook.as_mut()) {
        clean_decision_text(&mut interval.reason, max_bytes);
    }
    for highlight in &mut plan.observed_highlights {
        clean_decision_text(&mut highlight.observation, max_bytes);
    }
    for reason in &mut plan.review_reasons {
        clean_decision_text(reason, max_bytes);
    }
}

fn valid_decision_text(text: &str, max_bytes: usize) -> Result<(), EditorError> {
    let text = text.trim();
    if text.is_empty()
        || text.len() > max_bytes
        || text.chars().any(forbidden_decision_char)
        || text.starts_with('-')
        || text.to_ascii_lowercase().contains("ffmpeg")
    {
        return Err(EditorError::new(ErrorCode::InvalidDecisionText));
    }
    Ok(())
}

pub fn rebase_subtitles(
    plan: &ValidatedPlan,
    source: &[SourceSubtitle],
    limits: &PlanLimits,
) -> Result<Vec<MappedSubtitle>, EditorError> {
    check_plan_limits(limits)?;
    if source.len() > limits.max_subtitle_segments {
        return Err(EditorError::new(ErrorCode::TooManySubtitles));
    }
    let mut previous_start = 0.0;
    for item in source {
        let segment = &item.segment;
        if !segment.start.is_finite()
            || !segment.end.is_finite()
            || segment.start < 0.0
            || segment.start >= segment.end
            || segment.end * 1000.0 > plan.source_duration_ms() as f64
            || segment.start < previous_start
            || segment.text.len() > limits.max_subtitle_text_bytes
            || item.language.as_ref().is_some_and(|language| {
                language.len() > 32
                    || !language
                        .chars()
                        .all(|c| c.is_ascii_alphabetic() || c == '-')
            })
        {
            return Err(EditorError::new(ErrorCode::InvalidSubtitle));
        }
        previous_start = segment.start;
    }
    let mut result = Vec::new();
    let mut offset_ms = 0_u64;
    for interval in plan.intervals() {
        let start = interval.start_ms as f64 / 1000.0;
        let end = interval.end_ms as f64 / 1000.0;
        for item in source {
            let segment = &item.segment;
            if segment.text.trim().is_empty() {
                continue;
            }
            let from = segment.start.max(start);
            let to = segment.end.min(end);
            if from < to {
                result.push(MappedSubtitle {
                    segment: SubtitleSegment {
                        start: offset_ms as f64 / 1000.0 + from - start,
                        end: offset_ms as f64 / 1000.0 + to - start,
                        text: segment.text.clone(),
                    },
                    language: item.language.clone(),
                    partial: segment.start < start || segment.end > end,
                });
                if result.len() > limits.max_subtitle_segments {
                    return Err(EditorError::new(ErrorCode::TooManySubtitles));
                }
            }
        }
        offset_ms += interval.end_ms - interval.start_ms;
    }
    Ok(result)
}

pub fn build_subtitle_ass(
    plan: &ValidatedPlan,
    subtitles: &[MappedSubtitle],
    title: &str,
    login: &str,
    cam_height: i64,
) -> String {
    let cues: Vec<_> = subtitles
        .iter()
        .filter(|item| !item.partial)
        .flat_map(|item| {
            segment_subtitles(std::slice::from_ref(&item.segment))
                .into_iter()
                .filter_map(|mut cue| {
                    cue.end = cue.end.min(item.segment.end);
                    (cue.end > cue.start).then_some(cue)
                })
        })
        .collect();
    build_branded_ass(
        &cues,
        title,
        login,
        cam_height,
        plan.output_duration_ms() as f64 / 1000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_editor::model::{Highlight, LayoutIntent};

    fn limits() -> PlanLimits {
        PlanLimits {
            max_plan_bytes: 4096,
            max_source_duration_ms: 60_000,
            max_source_duration_drift_ms: 100,
            max_output_duration_ms: 60_000,
            max_intervals: 5,
            max_highlights: 5,
            max_review_reasons: 4,
            max_text_bytes: 120,
            max_subtitle_segments: 8,
            max_subtitle_text_bytes: 200,
            min_kept_percent: 30,
        }
    }

    fn plan() -> EditPlan {
        EditPlan {
            schema_version: 1,
            source_duration_ms: 10_000,
            kept_intervals: vec![
                KeptInterval {
                    start_ms: 1000,
                    end_ms: 4000,
                    reason: "Opening fight".into(),
                },
                KeptInterval {
                    start_ms: 6000,
                    end_ms: 9000,
                    reason: "Team reaction".into(),
                },
            ],
            hook: None,
            observed_highlights: vec![Highlight {
                start_ms: 1500,
                end_ms: 2500,
                observation: "Visible elimination".into(),
            }],
            significant_uncertainty: false,
            review_reasons: vec![],
            layout: LayoutIntent::ExistingStable,
        }
    }

    #[test]
    fn hook_plays_first_and_must_repeat_kept_footage() {
        let mut teaser = plan();
        teaser.hook = Some(KeptInterval {
            start_ms: 1500,
            end_ms: 3000,
            reason: "Elimination als Einstieg".into(),
        });
        let validated = validate_plan(teaser.clone(), 10_000, &limits()).expect("valid hook");
        assert_eq!(validated.intervals()[0].start_ms, 1500);
        assert_eq!(validated.intervals().len(), 3);
        assert_eq!(validated.output_duration_ms(), 7_500);
        let mut outside = teaser.clone();
        outside.hook = Some(KeptInterval {
            start_ms: 4500,
            end_ms: 5500,
            reason: "Leerlauf".into(),
        });
        assert_eq!(
            validate_plan(outside, 10_000, &limits())
                .err()
                .map(|e| e.code),
            Some(ErrorCode::InvalidHook)
        );
        let mut long = teaser;
        long.kept_intervals[1].start_ms = 2000;
        long.kept_intervals[0].end_ms = 2000;
        long.hook = Some(KeptInterval {
            start_ms: 2000,
            end_ms: 8000,
            reason: "Zu lang".into(),
        });
        assert_eq!(
            validate_plan(long, 10_000, &limits()).err().map(|e| e.code),
            Some(ErrorCode::InvalidHook)
        );
    }

    #[test]
    fn rejects_injected_and_out_of_bounds_decisions() {
        let mut input = serde_json::to_value(plan()).expect("serializable plan");
        input["filter_complex"] = serde_json::json!("[0:v]null");
        assert_eq!(
            validate_plan_json(
                &serde_json::to_vec(&input).expect("JSON"),
                10_000,
                &limits()
            )
            .err()
            .map(|e| e.code),
            Some(ErrorCode::InvalidSchema)
        );
        let mut invalid = plan();
        invalid.kept_intervals[0].reason = "ffmpeg -filter_complex [0:v]".into();
        assert_eq!(
            validate_plan(invalid, 10_000, &limits())
                .err()
                .map(|e| e.code),
            Some(ErrorCode::InvalidDecisionText)
        );
        let mut invalid = plan();
        invalid.kept_intervals[1].end_ms = 10_001;
        assert_eq!(
            validate_plan(invalid, 10_000, &limits())
                .err()
                .map(|e| e.code),
            Some(ErrorCode::InvalidInterval)
        );
        let mut invalid = plan();
        invalid.observed_highlights[0].start_ms = 4500;
        invalid.observed_highlights[0].end_ms = 5000;
        assert_eq!(
            validate_plan(invalid, 10_000, &limits())
                .err()
                .map(|e| e.code),
            Some(ErrorCode::HighlightRemoved)
        );
    }

    #[test]
    fn cleans_ordinary_model_punctuation_but_still_rejects_commands() {
        let mut cleaned = plan();
        cleaned.kept_intervals[0].reason = "- Kill/Tod; Teamfight [Mitte] über Älä".into();
        cleaned.review_reasons = vec!["Unklar/ob Tod".into()];
        clean_plan_texts(&mut cleaned, limits().max_text_bytes);
        assert_eq!(
            cleaned.kept_intervals[0].reason,
            "Kill Tod Teamfight Mitte über Älä"
        );
        assert_eq!(cleaned.review_reasons, vec!["Unklar ob Tod".to_string()]);
        assert!(validate_plan(cleaned, 10_000, &limits()).is_ok());
        let mut long = "ä".repeat(200);
        clean_decision_text(&mut long, 121);
        assert_eq!(long.len(), 120);
        let mut command = plan();
        command.kept_intervals[0].reason = "ffmpeg -i input".into();
        clean_plan_texts(&mut command, limits().max_text_bytes);
        assert_eq!(
            validate_plan(command, 10_000, &limits())
                .err()
                .map(|e| e.code),
            Some(ErrorCode::InvalidDecisionText)
        );
    }

    #[test]
    fn rebases_segments_without_making_up_speech() {
        let plan = validate_plan(plan(), 10_000, &limits()).expect("valid plan");
        let source = vec![
            SourceSubtitle {
                segment: SubtitleSegment {
                    start: 1.5,
                    end: 2.5,
                    text: "Überraschung".into(),
                },
                language: Some("de".into()),
            },
            SourceSubtitle {
                segment: SubtitleSegment {
                    start: 4.3,
                    end: 5.3,
                    text: "removed".into(),
                },
                language: Some("en".into()),
            },
            SourceSubtitle {
                segment: SubtitleSegment {
                    start: 8.0,
                    end: 9.5,
                    text: "nice".into(),
                },
                language: Some("en".into()),
            },
        ];
        let mapped = rebase_subtitles(&plan, &source, &limits()).expect("rebase");
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].segment.text, "Überraschung");
        assert_eq!(mapped[0].language.as_deref(), Some("de"));
        assert!((mapped[1].segment.start - 5.0).abs() < 0.001);
        assert!((mapped[1].segment.end - 6.0).abs() < 0.001);
        assert!(mapped[1].partial);
        let ass = build_subtitle_ass(&plan, &mapped, "", "", 600);
        assert!(ass.contains("Überraschung"));
        assert!(!ass.contains("nice"));
        assert!(!ass.contains("removed"));
        let empty = rebase_subtitles(&plan, &[], &limits()).expect("silence");
        assert!(build_subtitle_ass(&plan, &empty, "", "", 600).contains("[Events]"));
        assert!(!build_subtitle_ass(&plan, &empty, "", "", 600).contains("Dialogue:"));
    }

    #[test]
    fn subtitle_cues_do_not_extend_across_silence() {
        let plan = validate_plan(plan(), 10_000, &limits()).expect("valid plan");
        let source = vec![
            SourceSubtitle {
                segment: SubtitleSegment {
                    start: 1.0,
                    end: 1.2,
                    text: "Ja".into(),
                },
                language: Some("de".into()),
            },
            SourceSubtitle {
                segment: SubtitleSegment {
                    start: 3.0,
                    end: 3.2,
                    text: "Nein".into(),
                },
                language: Some("de".into()),
            },
        ];
        let mapped = rebase_subtitles(&plan, &source, &limits()).expect("rebase");
        let ass = build_subtitle_ass(&plan, &mapped, "", "", 600);
        assert!(ass.contains("Dialogue: 0,0:00:00.00,0:00:00.20,Default"));
        assert!(ass.contains("Dialogue: 0,0:00:02.00,0:00:02.20,Default"));
    }

    #[test]
    fn significant_uncertainty_cannot_be_auto_approved() {
        let mut uncertain = plan();
        uncertain.significant_uncertainty = true;
        assert_eq!(
            validate_plan(uncertain.clone(), 10_000, &limits())
                .err()
                .map(|e| e.code),
            Some(ErrorCode::UncertaintyNeedsReview)
        );
        uncertain
            .review_reasons
            .push("Ability is not clearly visible".into());
        assert!(validate_plan(uncertain, 10_000, &limits())
            .expect("review")
            .needs_review());
    }
}
