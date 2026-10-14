use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool, Row};

use crate::{
    autopilot_settings,
    produced_video::{self, ProducedVideo},
};

use super::{
    pipeline,
    stage_contract::Planning,
    store::{self, AiJobView, AiStage},
    AiEditorError, EditorRuntime,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionProposal {
    pub summary: String,
    pub impact: String,
    pub reanalyze: bool,
    pub requires_confirmation: bool,
    pub estimated_provider_cost_usd: Option<f64>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionEventKind {
    Feedback,
    ProposalAccepted,
    ProposalRejected,
    VersionSelected,
    VersionAccepted,
    VersionRejected,
}

impl RevisionEventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Feedback => "feedback",
            Self::ProposalAccepted => "proposal_accepted",
            Self::ProposalRejected => "proposal_rejected",
            Self::VersionSelected => "version_selected",
            Self::VersionAccepted => "version_accepted",
            Self::VersionRejected => "version_rejected",
        }
    }
}

pub(super) struct RevisionRequest {
    pub feedback: String,
    pub parent_version_id: i64,
    pub confirmed: bool,
    pub reanalysis_confirmed: bool,
}

pub(super) async fn request(
    pool: &PgPool,
    owner: &str,
    job_id: i64,
) -> Result<Option<RevisionRequest>, AiEditorError> {
    let row = sqlx::query("SELECT revision_feedback, parent_version_id, revision_confirmed, revision_reanalysis_confirmed FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2")
        .bind(job_id).bind(owner).fetch_optional(pool).await.map_err(|_| AiEditorError::Storage)?.ok_or(AiEditorError::NotFound)?;
    let feedback: Option<String> = row
        .try_get("revision_feedback")
        .map_err(|_| AiEditorError::Storage)?;
    feedback
        .map(|feedback| {
            Ok(RevisionRequest {
                feedback,
                parent_version_id: row
                    .try_get("parent_version_id")
                    .map_err(|_| AiEditorError::Storage)?,
                confirmed: row
                    .try_get("revision_confirmed")
                    .map_err(|_| AiEditorError::Storage)?,
                reanalysis_confirmed: row
                    .try_get("revision_reanalysis_confirmed")
                    .map_err(|_| AiEditorError::Storage)?,
            })
        })
        .transpose()
}

pub async fn owned_version(
    pool: &PgPool,
    owner: &str,
    job_id: i64,
    version_id: i64,
) -> Result<ProducedVideo, AiEditorError> {
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_ai_jobs j JOIN social_ai_jobs maker ON COALESCE(maker.root_job_id,maker.id)=COALESCE(j.root_job_id,j.id) AND maker.twitch_user_id=j.twitch_user_id AND maker.clip_db_id=j.clip_db_id JOIN social_produced_videos p ON p.metadata->>'manual_ai_job_id'=maker.id::text AND p.clip_id=j.clip_db_id AND p.twitch_user_id=j.twitch_user_id::text AND p.mode='ai' WHERE j.id=$1 AND j.twitch_user_id::text=$2 AND p.id=$3)")
        .bind(job_id).bind(owner).bind(version_id).fetch_one(pool).await.map_err(|_| AiEditorError::Storage)?;
    if !valid {
        return Err(AiEditorError::NotFound);
    }
    produced_video::get(pool, owner, version_id)
        .await
        .map_err(|_| AiEditorError::Storage)?
        .ok_or(AiEditorError::NotFound)
}

pub(super) fn maker_id(video: &ProducedVideo) -> Result<i64, AiEditorError> {
    video
        .metadata
        .get("manual_ai_job_id")
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|id| *id > 0)
        .ok_or(AiEditorError::NotFound)
}

pub fn usable(video: &ProducedVideo) -> bool {
    video.file_path.is_some()
        && (matches!(
            video.status.as_str(),
            "review" | "approved" | "scheduled" | "published"
        ) || (video.status == "rejected"
            && (video.metadata.get("revision_pending").is_some()
                || video.metadata.get("superseded_by").is_some())))
        && (!matches!(video.status.as_str(), "review" | "rejected")
            || video
                .expires_at
                .is_some_and(|expiry| expiry > chrono::Utc::now()))
}

async fn ensure_ready(
    tx: &mut PgConnection,
    owner: &str,
    version_id: i64,
) -> Result<(), AiEditorError> {
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_produced_videos WHERE id=$1 AND twitch_user_id=$2 AND status IN ('review','approved','scheduled','published') AND (status<>'review' OR expires_at>NOW()) AND file_path IS NOT NULL)")
        .bind(version_id).bind(owner).fetch_one(tx).await.map_err(|_| AiEditorError::Storage)?;
    if !valid {
        return Err(AiEditorError::Conflict);
    }
    Ok(())
}

pub(super) async fn parent_planning(
    pool: &PgPool,
    owner: &str,
    version_id: i64,
) -> Result<Planning, AiEditorError> {
    let video = produced_video::get(pool, owner, version_id)
        .await
        .map_err(|_| AiEditorError::Storage)?
        .ok_or(AiEditorError::NotFound)?;
    if let Some(value) = video.metadata.get("planning") {
        return serde_json::from_value(value.clone()).map_err(|_| AiEditorError::Plan);
    }
    let checkpoints = store::checkpoints(pool, owner, maker_id(&video)?).await?;
    match checkpoints.get(&AiStage::Planning) {
        Some(store::StageCheckpoint::Planning { edit_plan, .. }) => {
            serde_json::from_value(Value::Object(edit_plan.clone()))
                .map_err(|_| AiEditorError::Plan)
        }
        _ => Err(AiEditorError::Plan),
    }
}

async fn append_event(
    tx: &mut PgConnection,
    owner: &str,
    job_id: i64,
    version_id: i64,
    kind: RevisionEventKind,
    payload: Value,
) -> Result<(), AiEditorError> {
    let payload = serde_json::to_string(&payload).map_err(|_| AiEditorError::Storage)?;
    if payload.len() > 60_000 {
        return Err(AiEditorError::Plan);
    }
    sqlx::query("INSERT INTO social_ai_revision_events(twitch_user_id,root_job_id,job_id,produced_video_id,event_kind,payload) SELECT twitch_user_id,COALESCE(root_job_id,id),id,$3,$4,$5::text::jsonb FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2")
        .bind(job_id).bind(owner).bind(version_id).bind(kind.as_str()).bind(payload).execute(tx).await.map_err(|_| AiEditorError::Storage)?;
    Ok(())
}

pub async fn create_revision(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    job_id: i64,
    version_id: i64,
    feedback: &str,
) -> Result<AiJobView, AiEditorError> {
    if feedback.trim().is_empty()
        || feedback.len() > runtime.limits.revisions.max_feedback_bytes
        || feedback.contains('\0')
    {
        return Err(AiEditorError::Input);
    }
    tb_llm::social_editor::consent_allowed(pool, owner).await?;
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if !usable(&video) {
        return Err(AiEditorError::Conflict);
    }
    let maker = maker_id(&video)?;
    let mut checkpoints = store::checkpoints(pool, owner, maker).await?;
    let frozen = pipeline::checkpoint_runtime(&checkpoints, runtime)?;
    if feedback.len() > frozen.limits.revisions.max_feedback_bytes {
        return Err(AiEditorError::Input);
    }
    super::authorized_path(
        &frozen.limits.output_root,
        video
            .file_path
            .as_deref()
            .ok_or(AiEditorError::NotFound)?
            .as_ref(),
        frozen.limits.resources.max_output_bytes,
    )?;
    pipeline::prepare_revision(&mut checkpoints, &frozen)?;
    let skills = pipeline::frozen_skills(&checkpoints, &frozen)?;
    let previous = parent_planning(pool, owner, version_id).await?;
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    ensure_ready(&mut tx, owner, version_id).await?;
    let root: i64 = sqlx::query_scalar("SELECT COALESCE(root_job_id,id) FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2").bind(maker).bind(owner).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_ai_jobs WHERE root_job_id=$1 AND twitch_user_id::text=$2",
    )
    .bind(root)
    .bind(owner)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| AiEditorError::Storage)?;
    if count >= frozen.limits.revisions.max_revisions {
        return Err(AiEditorError::Attempts);
    }
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_ai_jobs WHERE twitch_user_id::text=$1 AND clip_db_id=$2 AND status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating','awaiting_confirmation'))").bind(owner).bind(video.clip_id).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    if busy {
        return Err(AiEditorError::Conflict);
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO social_ai_jobs(twitch_user_id,clip_db_id,source_clip_id,source_twitch_clip_id,root_job_id,parent_version_id,revision_feedback,stage,checkpoints,original_duration_ms,highlights,uncertainty) SELECT twitch_user_id,clip_db_id,source_clip_id,source_twitch_clip_id,$3,$4,$5,'planning',$6::text::jsonb,original_duration_ms,highlights,uncertainty FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2 RETURNING id")
        .bind(maker).bind(owner).bind(root).bind(version_id).bind(feedback.trim()).bind(serde_json::to_string(&checkpoints).map_err(|_| AiEditorError::Storage)?).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    super::autopilot_revision::hold_parent(&mut tx, owner, &video, id).await?;
    append_event(&mut tx, owner, id, version_id, RevisionEventKind::Feedback, json!({"feedback":feedback.trim(),"parent_version_id":version_id,"previous":previous,"frozen_skills":skills})).await?;
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    super::service::get_manual_job(pool, runtime, owner, id).await
}

pub async fn decide_proposal(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    job_id: i64,
    confirm: bool,
) -> Result<AiJobView, AiEditorError> {
    if confirm {
        tb_llm::social_editor::consent_allowed(pool, owner).await?;
    }
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    let row = sqlx::query("SELECT parent_version_id,revision_proposal::text AS proposal, revision_reanalysis_confirmed FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2 AND status='awaiting_confirmation' FOR UPDATE")
        .bind(job_id).bind(owner).fetch_optional(&mut *tx).await.map_err(|_| AiEditorError::Storage)?.ok_or(AiEditorError::Conflict)?;
    let proposal: RevisionProposal = serde_json::from_str(
        row.try_get("proposal")
            .map_err(|_| AiEditorError::Storage)?,
    )
    .map_err(|_| AiEditorError::Plan)?;
    let parent: i64 = row
        .try_get("parent_version_id")
        .map_err(|_| AiEditorError::Storage)?;
    if confirm {
        let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_produced_videos WHERE id=$1 AND twitch_user_id=$2 AND status='rejected' AND metadata->>'revision_pending'=$3::text AND expires_at>NOW() AND file_path IS NOT NULL)")
            .bind(parent).bind(owner).bind(job_id).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
        if !pending {
            ensure_ready(&mut tx, owner, parent).await?;
        }
    }
    let reanalyze = confirm
        && proposal.reanalyze
        && !row
            .try_get::<bool, _>("revision_reanalysis_confirmed")
            .map_err(|_| AiEditorError::Storage)?;
    sqlx::query("UPDATE social_ai_jobs SET status=$3, revision_confirmed=$4,revision_reanalysis_confirmed=revision_reanalysis_confirmed OR $5,stage=CASE WHEN $5 THEN 'analyzing' ELSE stage END,checkpoints=CASE WHEN $5 THEN checkpoints-'analyzing'-'planning' ELSE checkpoints END,updated_at=NOW() WHERE id=$1 AND twitch_user_id::text=$2")
        .bind(job_id).bind(owner).bind(if confirm { "queued" } else { "rejected" }).bind(confirm && !reanalyze).bind(reanalyze).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    if !confirm {
        super::autopilot_revision::restore_parent_on(&mut tx, owner, job_id).await?;
    }
    append_event(
        &mut tx,
        owner,
        job_id,
        parent,
        if confirm {
            RevisionEventKind::ProposalAccepted
        } else {
            RevisionEventKind::ProposalRejected
        },
        json!({"proposal":proposal,"reanalysis_authorized":reanalyze}),
    )
    .await?;
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    super::service::get_manual_job(pool, runtime, owner, job_id).await
}

pub async fn select_version(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    job_id: i64,
    version_id: i64,
) -> Result<AiJobView, AiEditorError> {
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if !usable(&video) {
        return Err(AiEditorError::Conflict);
    }
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    ensure_ready(&mut tx, owner, version_id).await?;
    let changed = sqlx::query("UPDATE social_ai_jobs SET selected_version_id=$3,updated_at=NOW() WHERE id=(SELECT COALESCE(root_job_id,id) FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2) AND twitch_user_id::text=$2 AND selected_version_id IS DISTINCT FROM $3")
        .bind(job_id).bind(owner).bind(version_id).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?.rows_affected();
    if changed == 1 {
        append_event(&mut tx, owner, maker_id(&video)?, version_id, RevisionEventKind::VersionSelected, json!({"parent_version_id":video.parent_version_id,"variant_index":video.variant_index})).await?;
    }
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    super::service::get_manual_job(pool, runtime, owner, job_id).await
}

pub async fn record_version_decision(
    pool: &PgPool,
    owner: &str,
    job_id: i64,
    version_id: i64,
    accept: bool,
) -> Result<(), AiEditorError> {
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if !usable(&video) || (!accept && video.status != "review") {
        return Err(AiEditorError::Conflict);
    }
    let maker = maker_id(&video)?;
    let checkpoints = store::checkpoints(pool, owner, maker).await?;
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    ensure_ready(&mut tx, owner, version_id).await?;
    if !accept {
        let status: String = sqlx::query_scalar(
            "SELECT status FROM social_produced_videos WHERE id=$1 AND twitch_user_id=$2",
        )
        .bind(version_id)
        .bind(owner)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| AiEditorError::Storage)?;
        if status != "review" {
            return Err(AiEditorError::Conflict);
        }
    }
    let kind = if accept {
        RevisionEventKind::VersionAccepted
    } else {
        RevisionEventKind::VersionRejected
    };
    let latest: Option<String> = sqlx::query_scalar("SELECT event_kind FROM social_ai_revision_events WHERE twitch_user_id::text=$1 AND produced_video_id=$2 AND event_kind IN ('version_accepted','version_rejected') ORDER BY id DESC LIMIT 1")
        .bind(owner).bind(version_id).fetch_optional(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    if latest.as_deref() != Some(kind.as_str()) {
        let planning =
            checkpoints
                .get(&AiStage::Planning)
                .and_then(|checkpoint| match checkpoint {
                    store::StageCheckpoint::Planning { edit_plan, .. } => Some(edit_plan),
                    _ => None,
                });
        append_event(&mut tx, owner, maker, version_id, kind, json!({"parent_version_id":video.parent_version_id,"variant_index":video.variant_index,"planning":video.metadata.get("planning").cloned().or_else(|| planning.map(|plan| Value::Object(plan.clone()))),"frozen_skills":checkpoints.get(&AiStage::Preparing).and_then(|checkpoint| match checkpoint { store::StageCheckpoint::Preparing {snapshot,..} => snapshot.get("skills"),_ => None })})).await?;
    }
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    Ok(())
}

pub async fn approve_version(
    pool: &PgPool,
    owner: &str,
    job_id: i64,
    version_id: i64,
    platforms: &[String],
) -> Result<(), AiEditorError> {
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if super::autopilot_revision::production_id(&video).is_some() {
        return Err(AiEditorError::Conflict);
    }
    produced_video::approve(pool, owner, version_id, platforms).await?;
    record_version_decision(pool, owner, job_id, version_id, true).await
}

pub async fn publish_version(
    pool: &PgPool,
    owner: &str,
    job_id: i64,
    version_id: i64,
    platforms: &[String],
) -> Result<produced_video::PublishingReservation, AiEditorError> {
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if super::autopilot_revision::production_id(&video).is_some() {
        return Err(AiEditorError::Conflict);
    }
    produced_video::enqueue_for_publishing_for_user(pool, owner, version_id, platforms)
        .await
        .map_err(Into::into)
}

pub(super) fn changed_percent(base: &Planning, new: &Planning) -> u64 {
    let duration = |plan: &Planning| {
        plan.plan
            .kept_intervals
            .iter()
            .map(|i| i.end_ms.saturating_sub(i.start_ms))
            .sum::<u64>()
    };
    let overlap = base
        .plan
        .kept_intervals
        .iter()
        .flat_map(|a| {
            new.plan.kept_intervals.iter().map(move |b| {
                a.end_ms
                    .min(b.end_ms)
                    .saturating_sub(a.start_ms.max(b.start_ms))
            })
        })
        .sum::<u64>();
    (duration(base) + duration(new))
        .saturating_sub(2 * overlap)
        .saturating_mul(100)
        .div_ceil(duration(base).max(1))
}

pub(super) fn validate_proposal(
    proposal: &mut RevisionProposal,
    base: &Planning,
    new: &Planning,
    runtime: &EditorRuntime,
    reanalysis_estimate: Option<f64>,
) -> Result<(), AiEditorError> {
    if [&proposal.summary, &proposal.impact].iter().any(|text| {
        text.trim().is_empty()
            || text.len() > runtime.limits.plan.max_text_bytes
            || text.chars().any(char::is_control)
    }) {
        return Err(AiEditorError::Plan);
    }
    proposal.requires_confirmation |= proposal.reanalyze
        || changed_percent(base, new)
            > u64::from(runtime.limits.revisions.max_auto_changed_percent)
        || !new.variants.is_empty();
    proposal.estimated_provider_cost_usd = if proposal.reanalyze {
        reanalysis_estimate.filter(|cost| cost.is_finite() && *cost >= 0.0)
    } else {
        Some(0.0)
    };
    Ok(())
}

pub(super) async fn reanalysis_estimate(
    pool: &PgPool,
    owner: &str,
    job_id: i64,
) -> Result<Option<f64>, AiEditorError> {
    sqlx::query_scalar("SELECT root.estimated_provider_cost_usd FROM social_ai_jobs j JOIN social_ai_jobs root ON root.id=COALESCE(j.root_job_id,j.id) AND root.twitch_user_id=j.twitch_user_id WHERE j.id=$1 AND j.twitch_user_id::text=$2")
        .bind(job_id)
        .bind(owner)
        .fetch_optional(pool)
        .await
        .map_err(|_| AiEditorError::Storage)
        .map(Option::flatten)
}

pub(super) fn request_context(request: &RevisionRequest, base: &Planning) -> Value {
    json!({"feedback":request.feedback,"previous":base,"reanalysis_confirmed":request.reanalysis_confirmed,"instruction":"Interpret feedback as a bounded edit of saved analysis. Set reanalyze=true only if new visual understanding is necessary and not yet authorized. Mark substantial changes for confirmation. Subtitle size may change only through subtitle_scale_percent. Preserve real speech and its language, stable layout and original audio. Explain unsupported layout, creative or speech-rewriting requests honestly in the summary; never fabricate commands or cost estimates. Return a complete main plan and no extra variants for ordinary revisions."})
}
