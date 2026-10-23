use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    runtime_config::CopyLimits, store::TimelineEntry, AiEditorError, EditPlan, PlanLimits,
};

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
}

fn default_subtitle_scale() -> u16 {
    100
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VariantPlan {
    pub plan: EditPlan,
    pub platforms: BTreeMap<String, PublicationCopy>,
    pub subtitle_scale_percent: u16,
}

impl VariantPlan {
    pub fn planning(&self) -> Planning {
        Planning {
            plan: self.plan.clone(),
            platforms: self.platforms.clone(),
            subtitle_scale_percent: self.subtitle_scale_percent,
            revision_proposal: None,
            variants: Vec::new(),
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
    let variant = object(
        json!({"plan":plan.clone(),"platforms":object(json!({"youtube":copy.clone(),"tiktok":copy.clone(),"instagram":copy.clone()})),"subtitle_scale_percent":{"type":"integer","minimum":60,"maximum":140}}),
    );
    object(
        json!({ "plan": plan, "platforms": object(json!({ "youtube": copy.clone(), "tiktok": copy.clone(), "instagram": copy })), "subtitle_scale_percent":{"type":"integer","minimum":60,"maximum":140}, "variants":array(variant,2) }),
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

    pub fn validate(&self, duration: u64, limits: &PlanLimits) -> Result<(), AiEditorError> {
        let valid_text =
            |text: &String| !text.trim().is_empty() && text.len() <= limits.max_text_bytes;
        if self.highlights.len() > limits.max_highlights {
            return Err(AiEditorError::plan_rejected(
                "analysis: too many highlights",
            ));
        }
        if self.highlights.iter().any(|entry| {
            entry.start_ms < 0 || entry.end_ms <= entry.start_ms || entry.end_ms as u64 > duration
        }) {
            return Err(AiEditorError::plan_rejected(
                "analysis: highlight outside the source",
            ));
        }
        if self
            .highlights
            .iter()
            .any(|entry| !valid_text(&entry.description))
        {
            return Err(AiEditorError::plan_rejected(
                "analysis: highlight text empty or too long",
            ));
        }
        if [&self.uncertainty, &self.analysis_notes]
            .into_iter()
            .any(|values| {
                values.len() > limits.max_review_reasons || !values.iter().all(valid_text)
            })
        {
            return Err(AiEditorError::plan_rejected(
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

    pub fn validate(
        &self,
        duration: u64,
        limits: &PlanLimits,
        copy_limits: &CopyLimits,
        analysis: &Analysis,
    ) -> Result<super::ValidatedPlan, AiEditorError> {
        analysis.validate(duration, limits)?;
        if !(60..=140).contains(&self.subtitle_scale_percent) {
            return Err(AiEditorError::plan_rejected(
                "subtitle_scale_percent outside 60..=140",
            ));
        }
        if self.variants.len() > 2 {
            return Err(AiEditorError::plan_rejected("more than two variants"));
        }
        for variant in &self.variants {
            variant
                .planning()
                .validate(duration, limits, copy_limits, analysis)?;
        }
        if serde_json::to_vec(self)
            .map_err(|_| AiEditorError::Plan)?
            .len()
            > limits.max_plan_bytes
        {
            return Err(AiEditorError::plan_rejected(
                "plan larger than max_plan_bytes",
            ));
        }
        if self.platforms.len() != crate::posting_plan::PLATFORMS.len()
            || self
                .platforms
                .keys()
                .any(|platform| !crate::posting_plan::PLATFORMS.contains(&platform.as_str()))
        {
            return Err(AiEditorError::plan_rejected(
                "platform copy set does not match",
            ));
        }
        for copy in self.platforms.values() {
            if let Some(reason) = copy_rejection(copy, copy_limits) {
                return Err(AiEditorError::plan_rejected(reason));
            }
        }
        let validated =
            super::validate_plan(self.plan.clone(), duration, limits).map_err(|error| {
                AiEditorError::plan_rejected(&format!("edit plan: {:?}", error.code))
            })?;
        if analysis.highlights.iter().any(|highlight| {
            !validated.preserves_highlight(highlight.start_ms as u64, highlight.end_ms as u64)
        }) {
            return Err(AiEditorError::plan_rejected(
                "analysis highlight cut out of the plan",
            ));
        }
        if !analysis.uncertainty.is_empty()
            && (!self.plan.significant_uncertainty
                || analysis
                    .uncertainty
                    .iter()
                    .any(|reason| !self.plan.review_reasons.contains(reason)))
        {
            return Err(AiEditorError::plan_rejected(
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
