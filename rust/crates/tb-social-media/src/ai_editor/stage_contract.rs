use std::collections::BTreeMap;

use crate::autopilot_settings::FrameFormat;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    runtime_config::CopyLimits, store::TimelineEntry, AiEditorError, EditPlan, ErrorCode,
    PlanLimits,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlanOrigin {
    FirstPlan,
    Revision,
}

const REJECTED_PLAN: &str = "Der neue Plan hat die technische Prüfung nicht bestanden.";

#[derive(Debug)]
pub(super) struct PlanRejection {
    reason: String,
    answer: &'static str,
}

impl PlanRejection {
    fn new(reason: impl Into<String>, answer: &'static str) -> Self {
        Self {
            reason: reason.into(),
            answer,
        }
    }

    fn technical(reason: &str) -> Self {
        Self::new(reason, REJECTED_PLAN)
    }

    fn edit_plan(code: ErrorCode) -> Self {
        let answer = match code {
            ErrorCode::TooMuchRemoved => {
                "So viel lässt sich nicht wegschneiden, ein Mindestanteil des Clips muss erhalten bleiben."
            }
            ErrorCode::OutputTooLong => "Das Video wäre länger geworden als erlaubt.",
            ErrorCode::InvalidHook => {
                "Die Vorschau am Anfang muss 1 bis 5 Sekunden lang sein und aus dem behaltenen Teil stammen."
            }
            ErrorCode::HighlightRemoved => "Der Schnitt hätte einen markierten Höhepunkt entfernt.",
            ErrorCode::InvalidInterval | ErrorCode::IntervalOrder | ErrorCode::TooManyIntervals => {
                "Der neue Schnitt passt nicht zur Länge des Clips."
            }
            _ => REJECTED_PLAN,
        };
        Self::new(format!("edit plan: {code:?}"), answer)
    }

    pub fn answer(&self) -> &'static str {
        self.answer
    }
}

impl From<PlanRejection> for AiEditorError {
    fn from(rejection: PlanRejection) -> Self {
        AiEditorError::plan_rejected(&rejection.reason)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Analysis {
    pub highlights: Vec<TimelineEntry>,
    pub uncertainty: Vec<String>,
    pub analysis_notes: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Planning {
    pub plan: EditPlan,
    pub platforms: BTreeMap<String, PublicationCopy>,
    #[serde(default = "default_subtitle_scale")]
    pub subtitle_scale_percent: u16,
    #[serde(default)]
    pub revision_proposal: Option<super::revision::RevisionProposal>,
    #[serde(default)]
    pub variants: Vec<VariantPlan>,
    #[serde(default)]
    pub overlay_title: OverlayTitle,
    #[serde(default = "default_subtitles_enabled")]
    pub subtitles_enabled: bool,
    #[serde(default)]
    pub frame: FrameFormat,
}

const FRAME_DESCRIPTION: &str = "Picture of the vertical 9:16 video. layout: the streamer's saved layout. fill: the game picture zoomed to fill the whole frame, without facecam. full: the complete 16:9 picture centered on a blurred background. For a revision keep previous.frame unless the feedback asks for another format: 9:16, Hochformat, zoom in or no blurred bars mean fill; 16:9, ganzes Bild or zoom out mean full; mit Facecam or mein Layout mean layout.";

pub(super) const OVERLAY_TITLE_MAX_CHARS: usize = 54;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OverlayTitleMode {
    #[default]
    ClipTitle,
    Custom,
    None,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OverlayTitle {
    pub mode: OverlayTitleMode,
    #[serde(default)]
    pub text: Option<String>,
}

impl OverlayTitle {
    pub fn resolve(&self, clip_title: &str) -> String {
        match self.mode {
            OverlayTitleMode::ClipTitle => clip_title.trim().to_owned(),
            OverlayTitleMode::Custom => self.text.as_deref().unwrap_or("").trim().to_owned(),
            OverlayTitleMode::None => String::new(),
        }
    }

    fn rejection(&self) -> Option<&'static str> {
        if self.mode != OverlayTitleMode::Custom {
            return None;
        }
        let text = self.text.as_deref().unwrap_or("").trim();
        if text.is_empty() || text.chars().count() > OVERLAY_TITLE_MAX_CHARS {
            return Some("overlay title empty or too long");
        }
        text.chars()
            .any(char::is_control)
            .then_some("overlay title contains control characters")
    }
}

fn default_subtitle_scale() -> u16 {
    100
}

fn default_subtitles_enabled() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VariantPlan {
    pub plan: EditPlan,
    pub platforms: BTreeMap<String, PublicationCopy>,
    pub subtitle_scale_percent: u16,
    #[serde(default)]
    pub overlay_title: OverlayTitle,
    #[serde(default = "default_subtitles_enabled")]
    pub subtitles_enabled: bool,
    #[serde(default)]
    pub frame: FrameFormat,
}

impl VariantPlan {
    pub fn planning(&self) -> Planning {
        Planning {
            plan: self.plan.clone(),
            platforms: self.platforms.clone(),
            subtitle_scale_percent: self.subtitle_scale_percent,
            revision_proposal: None,
            variants: Vec::new(),
            overlay_title: self.overlay_title.clone(),
            subtitles_enabled: self.subtitles_enabled,
            frame: self.frame,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PublicationCopy {
    pub title: String,
    pub description: String,
    pub hashtags: Vec<String>,
}

fn object(properties: Value) -> Value {
    let required = properties
        .as_object()
        .map(|fields| fields.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    json!({ "type": "object", "additionalProperties": false, "properties": properties, "required": required })
}

fn nullable(mut schema: Value) -> Value {
    schema["type"] = json!(["object", "null"]);
    schema
}

fn text(max: usize) -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": max })
}
fn array(items: Value, max: usize) -> Value {
    json!({ "type": "array", "items": items, "maxItems": max })
}
fn timestamp(duration: u64) -> Value {
    json!({ "type": "integer", "minimum": 0, "maximum": duration })
}

pub(super) fn analysis_schema(limits: &PlanLimits, duration: u64) -> Value {
    object(json!({
        "highlights": array(object(json!({ "start_ms": timestamp(duration), "end_ms": timestamp(duration), "description": text(limits.max_text_bytes) })), limits.max_highlights),
        "uncertainty": array(text(limits.max_text_bytes), limits.max_review_reasons),
        "analysis_notes": array(text(limits.max_text_bytes), limits.max_review_reasons),
    }))
}

pub(super) fn planning_schema(
    limits: &PlanLimits,
    copy_limits: &CopyLimits,
    duration: u64,
) -> Value {
    let copy = object(
        json!({ "title": text(copy_limits.max_title_chars), "description": text(copy_limits.max_description_chars), "hashtags": array(text(copy_limits.max_hashtag_chars), copy_limits.max_hashtags) }),
    );
    let interval = object(
        json!({ "start_ms": timestamp(duration), "end_ms": timestamp(duration), "reason": text(limits.max_text_bytes) }),
    );
    let plan = object(json!({
        "schema_version": { "type": "integer", "const": 1 },
        "source_duration_ms": { "type": "integer", "const": duration },
        "kept_intervals": { "type": "array", "minItems": 1, "maxItems": limits.max_intervals, "items": interval.clone() },
        "hook": nullable(interval),
        "observed_highlights": array(object(json!({ "start_ms": timestamp(duration), "end_ms": timestamp(duration), "observation": text(limits.max_text_bytes) })), limits.max_highlights),
        "significant_uncertainty": { "type": "boolean" },
        "review_reasons": array(text(limits.max_text_bytes), limits.max_review_reasons),
        "layout": { "type": "string", "enum": ["existing_stable"] },
    }));
    let overlay = object(json!({
        "mode": { "type": "string", "enum": ["clip_title", "custom", "none"] },
        "text": { "type": ["string", "null"], "maxLength": OVERLAY_TITLE_MAX_CHARS },
    }));
    let frame = json!({ "type": "string", "enum": ["layout", "fill", "full"], "description": FRAME_DESCRIPTION });
    let variant = object(
        json!({"plan":plan.clone(),"platforms":object(json!({"youtube":copy.clone(),"tiktok":copy.clone(),"instagram":copy.clone()})),"subtitle_scale_percent":{"type":"integer","minimum":60,"maximum":140},"subtitles_enabled":{"type":"boolean"},"frame":frame.clone(),"overlay_title":overlay.clone()}),
    );
    object(
        json!({ "plan": plan, "platforms": object(json!({ "youtube": copy.clone(), "tiktok": copy.clone(), "instagram": copy })), "subtitle_scale_percent":{"type":"integer","minimum":60,"maximum":140}, "subtitles_enabled":{"type":"boolean"}, "frame": frame, "overlay_title": overlay, "variants":array(variant,2) }),
    )
}

pub(super) fn drop_variants(schema: &mut Value) {
    if let Some(properties) = schema["properties"].as_object_mut() {
        properties.remove("variants");
    }
    if let Some(required) = schema["required"].as_array_mut() {
        required.retain(|name| name != "variants");
    }
}

pub(super) fn revision_schema(limits: &PlanLimits, copy: &CopyLimits, duration: u64) -> Value {
    let mut schema = planning_schema(limits, copy, duration);
    schema["properties"]["revision_proposal"] = object(json!({
        "summary":text(limits.max_text_bytes),"impact":text(limits.max_text_bytes),
        "reanalyze":{"type":"boolean"},"requires_confirmation":{"type":"boolean"},
        "estimated_provider_cost_usd":{"type":"null"}
    }));
    if let Some(required) = schema["required"].as_array_mut() {
        required.push(json!("revision_proposal"));
    }
    schema
}

impl Analysis {
    pub fn clean_texts(&mut self, max_bytes: usize) {
        for entry in &mut self.highlights {
            super::validation::clean_decision_text(&mut entry.description, max_bytes);
        }
        for text in self.uncertainty.iter_mut().chain(&mut self.analysis_notes) {
            super::validation::clean_decision_text(text, max_bytes);
        }
    }

    pub fn validate(&self, duration: u64, limits: &PlanLimits) -> Result<(), PlanRejection> {
        let valid_text =
            |text: &String| !text.trim().is_empty() && text.len() <= limits.max_text_bytes;
        if self.highlights.len() > limits.max_highlights {
            return Err(PlanRejection::technical("analysis: too many highlights"));
        }
        if self.highlights.iter().any(|entry| {
            entry.start_ms < 0 || entry.end_ms <= entry.start_ms || entry.end_ms as u64 > duration
        }) {
            return Err(PlanRejection::technical(
                "analysis: highlight outside the source",
            ));
        }
        if self
            .highlights
            .iter()
            .any(|entry| !valid_text(&entry.description))
        {
            return Err(PlanRejection::technical(
                "analysis: highlight text empty or too long",
            ));
        }
        if [&self.uncertainty, &self.analysis_notes]
            .into_iter()
            .any(|values| {
                values.len() > limits.max_review_reasons || !values.iter().all(valid_text)
            })
        {
            return Err(PlanRejection::technical(
                "analysis: notes or uncertainty over limits",
            ));
        }
        Ok(())
    }
}

impl Planning {
    pub fn clean_texts(&mut self, max_bytes: usize) {
        super::validation::clean_plan_texts(&mut self.plan, max_bytes);
        for variant in &mut self.variants {
            super::validation::clean_plan_texts(&mut variant.plan, max_bytes);
        }
    }

    pub fn fit_revision_cut(&mut self) {
        super::validation::fit_to_kept(&mut self.plan);
        for variant in &mut self.variants {
            super::validation::fit_to_kept(&mut variant.plan);
        }
    }

    pub fn origin(&self) -> PlanOrigin {
        if self.revision_proposal.is_some() {
            PlanOrigin::Revision
        } else {
            PlanOrigin::FirstPlan
        }
    }

    pub fn validate(
        &self,
        duration: u64,
        limits: &PlanLimits,
        copy_limits: &CopyLimits,
        analysis: &Analysis,
        origin: PlanOrigin,
    ) -> Result<super::ValidatedPlan, AiEditorError> {
        self.check(duration, limits, copy_limits, analysis, origin)
            .map_err(Into::into)
    }

    pub fn check(
        &self,
        duration: u64,
        limits: &PlanLimits,
        copy_limits: &CopyLimits,
        analysis: &Analysis,
        origin: PlanOrigin,
    ) -> Result<super::ValidatedPlan, PlanRejection> {
        analysis.validate(duration, limits)?;
        if !(60..=140).contains(&self.subtitle_scale_percent) {
            return Err(PlanRejection::new(
                "subtitle_scale_percent outside 60..=140",
                "Die Untertitelgröße muss zwischen 60 und 140 % liegen.",
            ));
        }
        if self.variants.len() > 2 {
            return Err(PlanRejection::technical("more than two variants"));
        }
        if let Some(reason) = self.overlay_title.rejection() {
            return Err(PlanRejection::new(
                reason,
                "Der neue Titel ist leer, zu lang oder enthält ungültige Zeichen.",
            ));
        }
        for variant in &self.variants {
            variant
                .planning()
                .check(duration, limits, copy_limits, analysis, origin)?;
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|_| PlanRejection::technical("plan not serializable"))?;
        if encoded.len() > limits.max_plan_bytes {
            return Err(PlanRejection::technical("plan larger than max_plan_bytes"));
        }
        if self.platforms.len() != crate::posting_plan::PLATFORMS.len()
            || self
                .platforms
                .keys()
                .any(|platform| !crate::posting_plan::PLATFORMS.contains(&platform.as_str()))
        {
            return Err(PlanRejection::technical("platform copy set does not match"));
        }
        for copy in self.platforms.values() {
            if let Some(reason) = copy_rejection(copy, copy_limits) {
                return Err(PlanRejection::new(
                    reason,
                    "Die Plattformtexte sind leer, zu lang oder enthalten ungültige Hashtags.",
                ));
            }
        }
        let validated = super::validate_plan(self.plan.clone(), duration, limits)
            .map_err(|error| PlanRejection::edit_plan(error.code))?;
        if origin == PlanOrigin::FirstPlan
            && analysis.highlights.iter().any(|highlight| {
                !validated.preserves_highlight(highlight.start_ms as u64, highlight.end_ms as u64)
            })
        {
            return Err(PlanRejection::new(
                "analysis highlight cut out of the plan",
                "Der Schnitt hätte den erkannten Höhepunkt entfernt.",
            ));
        }
        if !analysis.uncertainty.is_empty()
            && (!self.plan.significant_uncertainty
                || analysis
                    .uncertainty
                    .iter()
                    .any(|reason| !self.plan.review_reasons.contains(reason)))
        {
            return Err(PlanRejection::technical(
                "analysis uncertainty missing in review reasons",
            ));
        }
        Ok(validated)
    }
}

fn copy_rejection(copy: &PublicationCopy, limits: &CopyLimits) -> Option<&'static str> {
    if copy.title.trim().is_empty() || copy.title.chars().count() > limits.max_title_chars {
        return Some("copy title empty or too long");
    }
    if copy.description.trim().is_empty()
        || copy.description.chars().count() > limits.max_description_chars
    {
        return Some("copy description empty or too long");
    }
    if copy.hashtags.len() > limits.max_hashtags {
        return Some("too many hashtags");
    }
    if copy
        .hashtags
        .iter()
        .any(|tag| !hashtag_valid(tag, limits.max_hashtag_chars))
    {
        return Some("hashtag format invalid");
    }
    None
}

fn hashtag_valid(tag: &str, max_chars: usize) -> bool {
    let body = super::metadata::normalize_hashtag(tag);
    let length = body.chars().count() + 1;
    !body.is_empty()
        && length <= max_chars
        && !body
            .chars()
            .any(|c| c.is_whitespace() || c == '#' || c == ',')
}

#[cfg(test)]
mod hashtag_tests {
    use super::hashtag_valid;

    #[test]
    fn accepts_tags_with_and_without_prefix() {
        assert!(hashtag_valid("#Deadlock", 64));
        assert!(hashtag_valid("Deadlock", 64));
        assert!(!hashtag_valid("#", 64));
        assert!(!hashtag_valid("Dead lock", 64));
        assert!(!hashtag_valid("Dead#lock", 64));
        assert!(!hashtag_valid(&"a".repeat(64), 64));
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::{OverlayTitle, OverlayTitleMode, OVERLAY_TITLE_MAX_CHARS};

    fn custom(text: &str) -> OverlayTitle {
        OverlayTitle {
            mode: OverlayTitleMode::Custom,
            text: Some(text.into()),
        }
    }

    #[test]
    fn overlay_title_modes_resolve_and_bound_custom_text() {
        assert_eq!(
            OverlayTitle::default().resolve(" EZ OUTPLAYED "),
            "EZ OUTPLAYED"
        );
        assert_eq!(
            custom(" Parry-Kette ").resolve("EZ OUTPLAYED"),
            "Parry-Kette"
        );
        let none = OverlayTitle {
            mode: OverlayTitleMode::None,
            text: Some("ignored".into()),
        };
        assert_eq!(none.resolve("EZ OUTPLAYED"), "");
        assert!(none.rejection().is_none());
        assert!(custom("Parry-Kette").rejection().is_none());
        assert!(custom(" ").rejection().is_some());
        assert!(custom(&"a".repeat(OVERLAY_TITLE_MAX_CHARS + 1))
            .rejection()
            .is_some());
        assert!(custom("Zeile\neins").rejection().is_some());
    }
}
