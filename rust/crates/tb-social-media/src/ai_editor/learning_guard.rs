use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgPool, Row};

use super::{
    guidance, rejected_directions, strip_learned, Dimension, FeedbackEvent, GlobalRule,
    LEARNED_HEADING, LEARNING_ACTOR,
};
use crate::ai_editor::{
    skill_registry::{self, SkillRegistryError},
    SkillId,
};

const DIMENSIONS: [Dimension; 10] = [
    Dimension::SubtitleSize,
    Dimension::IntroLength,
    Dimension::VideoLength,
    Dimension::EditingPace,
    Dimension::KeepAction,
    Dimension::KeepReaction,
    Dimension::FacecamOcclusion,
    Dimension::GameUnderstanding,
    Dimension::FacecamPosition,
    Dimension::FacecamSize,
];
const RECENT_EVENT_LIMIT: i64 = 50_000;
const MIN_REGRESSION_SAMPLES: i64 = 6;
const MIN_BASELINE_SAMPLES: i64 = 3;

pub(super) type Rejections = BTreeMap<(Dimension, i8), u32>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Decisions {
    pub total: i64,
    pub rejected: i64,
}

fn known_guidance() -> BTreeSet<&'static str> {
    DIMENSIONS
        .iter()
        .flat_map(|dimension| [guidance(*dimension, -1), guidance(*dimension, 1)])
        .collect()
}

fn learned_entries(text: &str) -> Vec<&str> {
    match text.find(&format!("\n{LEARNED_HEADING}")) {
        Some(index) => text[index..]
            .lines()
            .filter_map(|line| line.strip_prefix("- "))
            .collect(),
        None => Vec::new(),
    }
}

pub(super) fn valid_structure(
    id: SkillId,
    active: &str,
    candidate: &str,
    max_bytes: usize,
) -> bool {
    let known = known_guidance();
    let entries = learned_entries(candidate);
    skill_registry::validate_candidate(id, candidate, max_bytes).is_ok()
        && strip_learned(candidate) == strip_learned(active)
        && entries.iter().collect::<BTreeSet<_>>().len() == entries.len()
        && entries.iter().all(|line| known.contains(line))
}

pub(super) fn regresses(rule: &GlobalRule, rejections: &Rejections) -> bool {
    let rejecting = rejections
        .get(&(rule.dimension, rule.direction))
        .copied()
        .unwrap_or(0);
    rejecting > 0 && rejecting.saturating_mul(2) >= rule.supporting_users
}

pub(super) fn count_rejections(events: &BTreeMap<i64, Vec<FeedbackEvent>>) -> Rejections {
    let mut rejections = Rejections::new();
    for user_events in events.values() {
        for key in rejected_directions(user_events) {
            *rejections.entry(key).or_default() += 1;
        }
    }
    rejections
}

pub(super) async fn recent_rejections(pool: &PgPool) -> Result<Rejections, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT twitch_user_id, root_job_id, job_id, event_kind, payload->>'feedback' AS feedback FROM public.social_ai_revision_events WHERE created_at >= now() - interval '30 days' AND event_kind IN ('feedback', 'version_accepted', 'version_rejected', 'proposal_rejected') ORDER BY id DESC LIMIT $1",
    )
    .bind(RECENT_EVENT_LIMIT)
    .fetch_all(pool)
    .await?;
    let mut events: BTreeMap<i64, Vec<FeedbackEvent>> = BTreeMap::new();
    for row in rows.iter().rev() {
        events
            .entry(row.try_get("twitch_user_id")?)
            .or_default()
            .push(FeedbackEvent {
                root_job_id: row.try_get("root_job_id")?,
                job_id: row.try_get("job_id")?,
                kind: row.try_get("event_kind")?,
                feedback: row.try_get("feedback")?,
            });
    }
    Ok(count_rejections(&events))
}

pub(super) fn clear_regression(current: Decisions, baseline: Decisions) -> bool {
    if current.total < MIN_REGRESSION_SAMPLES || current.rejected * 2 < current.total {
        return false;
    }
    if baseline.total < MIN_BASELINE_SAMPLES {
        return true;
    }
    4 * (current.rejected * baseline.total - baseline.rejected * current.total)
        >= current.total * baseline.total
}

async fn decisions(pool: &PgPool, id: SkillId, sha256: &str) -> Result<Decisions, sqlx::Error> {
    let (total, rejected): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE event_kind = 'version_rejected') FROM (SELECT DISTINCT ON (produced_video_id) event_kind FROM public.social_ai_revision_events WHERE event_kind IN ('version_accepted', 'version_rejected') AND produced_video_id IS NOT NULL AND payload->'frozen_skills' @> jsonb_build_array(jsonb_build_object('id', $1::text, 'sha256', $2::text)) ORDER BY produced_video_id, id DESC) latest",
    )
    .bind(id.as_str())
    .bind(sha256)
    .fetch_one(pool)
    .await?;
    Ok(Decisions { total, rejected })
}

struct LearnedSwitch {
    from: i32,
    to: i32,
    from_sha256: String,
    to_sha256: String,
}

async fn active_learned_switch(
    pool: &PgPool,
    id: SkillId,
) -> Result<Option<LearnedSwitch>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT a.from_version, a.to_version, f.sha256 AS from_sha256, t.sha256 AS to_sha256 FROM public.social_ai_skill_activations a JOIN public.social_ai_skill_active s ON s.skill_id = a.skill_id AND s.version = a.to_version JOIN public.social_ai_skill_versions f ON f.skill_id = a.skill_id AND f.version = a.from_version JOIN public.social_ai_skill_versions t ON t.skill_id = a.skill_id AND t.version = a.to_version WHERE a.skill_id = $1 AND a.kind = 'learned' AND a.id = (SELECT MAX(id) FROM public.social_ai_skill_activations WHERE skill_id = $1)",
    )
    .bind(id.as_str())
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(LearnedSwitch {
            from: row.try_get("from_version")?,
            to: row.try_get("to_version")?,
            from_sha256: row.try_get("from_sha256")?,
            to_sha256: row.try_get("to_sha256")?,
        })
    })
    .transpose()
}

fn revert_reason(current: Decisions, baseline: Decisions) -> String {
    format!(
        "Automatisch zurückgesetzt: {} von {} Videos mit der gelernten Version wurden abgelehnt, mit der Vorversion {} von {}.",
        current.rejected, current.total, baseline.rejected, baseline.total
    )
}

async fn revert_skill(pool: &PgPool, id: SkillId) -> Result<Option<i32>, SkillRegistryError> {
    let Some(switch) = active_learned_switch(pool, id).await? else {
        return Ok(None);
    };
    let current = decisions(pool, id, &switch.to_sha256).await?;
    let baseline = decisions(pool, id, &switch.from_sha256).await?;
    if !clear_regression(current, baseline) {
        return Ok(None);
    }
    let reason = revert_reason(current, baseline);
    match skill_registry::auto_rollback(pool, id, switch.to, switch.from, &reason, LEARNING_ACTOR)
        .await
    {
        Ok(version) => Ok(Some(version)),
        Err(SkillRegistryError::Conflict) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) async fn revert_regressions(
    pool: &PgPool,
) -> Result<Vec<(SkillId, i32)>, SkillRegistryError> {
    let mut reverted = Vec::new();
    for id in SkillId::ALL {
        if let Some(version) = revert_skill(pool, id).await? {
            reverted.push((id, version));
        }
    }
    Ok(reverted)
}
