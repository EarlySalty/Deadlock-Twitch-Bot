use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::{autopilot_settings, produced_video::{self, ProducedVideo}};

use super::{pipeline, stage_contract::Planning, store::{self, AiJobView, AiStage, AttemptLimit}, AiEditorError, EditorRuntime};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionProposal {
    pub summary: String,
    pub impact: String,
    pub reanalyze: bool,
    pub requires_confirmation: bool,
    pub estimated_provider_cost_usd: Option<f64>,
}

pub(super) struct RevisionRequest {
    pub feedback: String,
    pub parent_version_id: i64,
    pub confirmed: bool,
}

pub(super) async fn request(pool: &PgPool, owner: &str, job_id: i64) -> Result<Option<RevisionRequest>, AiEditorError> {
    let row = sqlx::query("SELECT revision_feedback, parent_version_id, revision_confirmed FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2")
        .bind(job_id).bind(owner).fetch_optional(pool).await.map_err(|_| AiEditorError::Storage)?.ok_or(AiEditorError::NotFound)?;
    let feedback: Option<String> = row.try_get("revision_feedback").map_err(|_| AiEditorError::Storage)?;
    feedback.map(|feedback| Ok(RevisionRequest {
        feedback,
        parent_version_id: row.try_get("parent_version_id").map_err(|_| AiEditorError::Storage)?,
        confirmed: row.try_get("revision_confirmed").map_err(|_| AiEditorError::Storage)?,
    })).transpose()
}

pub async fn owned_version(pool: &PgPool, owner: &str, job_id: i64, version_id: i64) -> Result<ProducedVideo, AiEditorError> {
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_ai_jobs j JOIN social_ai_jobs maker ON COALESCE(maker.root_job_id,maker.id)=COALESCE(j.root_job_id,j.id) AND maker.twitch_user_id=j.twitch_user_id AND maker.clip_db_id=j.clip_db_id JOIN social_produced_videos p ON p.metadata->>'manual_ai_job_id'=maker.id::text AND p.clip_id=j.clip_db_id AND p.twitch_user_id=j.twitch_user_id::text AND p.mode='ai' AND p.job_id IS NULL WHERE j.id=$1 AND j.twitch_user_id::text=$2 AND p.id=$3)")
        .bind(job_id).bind(owner).bind(version_id).fetch_one(pool).await.map_err(|_| AiEditorError::Storage)?;
    if !valid { return Err(AiEditorError::NotFound); }
    produced_video::get(pool, owner, version_id).await.map_err(|_| AiEditorError::Storage)?.ok_or(AiEditorError::NotFound)
}

pub(super) fn usable(video: &ProducedVideo) -> bool {
    video.file_path.is_some() && matches!(video.status.as_str(), "review"|"approved"|"scheduled")
        && (video.status != "review" || video.expires_at.is_some_and(|expiry| expiry > chrono::Utc::now()))
}

pub(super) async fn parent_planning(pool: &PgPool, owner: &str, version_id: i64) -> Result<Planning, AiEditorError> {
    let video = produced_video::get(pool, owner, version_id).await.map_err(|_| AiEditorError::Storage)?.ok_or(AiEditorError::NotFound)?;
    if let Some(value) = video.metadata.get("planning") {
        return serde_json::from_value(value.clone()).map_err(|_| AiEditorError::Plan);
    }
    let maker = video.metadata.get("manual_ai_job_id").and_then(Value::as_i64).ok_or(AiEditorError::NotFound)?;
    let checkpoints = store::checkpoints(pool, owner, maker).await?;
    match checkpoints.get(&AiStage::Planning) {
        Some(store::StageCheckpoint::Planning { edit_plan, .. }) => serde_json::from_value(Value::Object(edit_plan.clone())).map_err(|_| AiEditorError::Plan),
        _ => Err(AiEditorError::Plan),
    }
}

pub async fn create_revision(pool: &PgPool, runtime: &EditorRuntime, owner: &str, job_id: i64, version_id: i64, feedback: &str) -> Result<AiJobView, AiEditorError> {
    if feedback.trim().is_empty() || feedback.len() > runtime.limits.revisions.max_feedback_bytes { return Err(AiEditorError::Input); }
    tb_llm::social_editor::consent_allowed(pool, owner).await?;
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if !usable(&video) { return Err(AiEditorError::Conflict); }
    super::authorized_path(&runtime.limits.output_root, video.file_path.as_deref().ok_or(AiEditorError::NotFound)?.as_ref(), runtime.limits.resources.max_output_bytes)?;
    let maker = video.metadata.get("manual_ai_job_id").and_then(Value::as_i64).ok_or(AiEditorError::NotFound)?;
    let mut checkpoints = store::checkpoints(pool, owner, maker).await?;
    let frozen = pipeline::checkpoint_runtime(&checkpoints, runtime)?;
    checkpoints.retain(|stage,_| matches!(stage, AiStage::Preparing|AiStage::Transcribing|AiStage::Analyzing));
    if checkpoints.len() != 3 { return Err(AiEditorError::Plan); }
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner).await.map_err(|_| AiEditorError::Storage)?;
    let root: i64 = sqlx::query_scalar("SELECT COALESCE(root_job_id,id) FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2").bind(job_id).bind(owner).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM social_ai_jobs WHERE root_job_id=$1 AND twitch_user_id::text=$2").bind(root).bind(owner).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    if count >= frozen.limits.revisions.max_revisions { return Err(AiEditorError::Attempts); }
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_ai_jobs WHERE twitch_user_id::text=$1 AND clip_db_id=$2 AND status IN ('queued','preparing','transcribing','analyzing','planning','rendering','validating','awaiting_confirmation'))").bind(owner).bind(video.clip_id).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    if busy { return Err(AiEditorError::Conflict); }
    let id: i64 = sqlx::query_scalar("INSERT INTO social_ai_jobs(twitch_user_id,clip_db_id,source_clip_id,source_twitch_clip_id,root_job_id,parent_version_id,revision_feedback,stage,checkpoints,original_duration_ms,highlights,uncertainty) SELECT twitch_user_id,clip_db_id,source_clip_id,source_twitch_clip_id,$3,$4,$5,'planning',$6::text::jsonb,original_duration_ms,highlights,uncertainty FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2 RETURNING id")
        .bind(maker).bind(owner).bind(root).bind(version_id).bind(feedback.trim()).bind(serde_json::to_string(&checkpoints).map_err(|_| AiEditorError::Storage)?).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    sqlx::query("INSERT INTO social_ai_revision_events(twitch_user_id,root_job_id,job_id,version_id,event,feedback) VALUES($1::text::bigint,$2,$3,$4,'feedback',$5)")
        .bind(owner).bind(root).bind(id).bind(version_id).bind(feedback.trim()).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    super::service::get_manual_job(pool, runtime, owner, id).await
}

pub async fn decide_proposal(pool: &PgPool, runtime: &EditorRuntime, owner: &str, job_id: i64, confirm: bool) -> Result<AiJobView, AiEditorError> {
    if confirm { tb_llm::social_editor::consent_allowed(pool, owner).await?; }
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner).await.map_err(|_| AiEditorError::Storage)?;
    let row = sqlx::query("SELECT root_job_id,parent_version_id,revision_proposal::text AS proposal FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2 AND status='awaiting_confirmation' FOR UPDATE")
        .bind(job_id).bind(owner).fetch_optional(&mut *tx).await.map_err(|_| AiEditorError::Storage)?.ok_or(AiEditorError::Conflict)?;
    let proposal: RevisionProposal = serde_json::from_str(row.try_get("proposal").map_err(|_| AiEditorError::Storage)?).map_err(|_| AiEditorError::Plan)?;
    let root: i64 = row.try_get("root_job_id").map_err(|_| AiEditorError::Storage)?;
    let parent: i64 = row.try_get("parent_version_id").map_err(|_| AiEditorError::Storage)?;
    if confirm {
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_produced_videos WHERE id=$1 AND twitch_user_id=$2 AND status IN ('review','approved','scheduled') AND (status<>'review' OR expires_at>NOW()) AND file_path IS NOT NULL)").bind(parent).bind(owner).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
        if !valid { return Err(AiEditorError::Conflict); }
    }
    sqlx::query("UPDATE social_ai_jobs SET status=$3, revision_confirmed=$4,stage=CASE WHEN $5 THEN 'analyzing' ELSE stage END,checkpoints=CASE WHEN $5 THEN checkpoints-'analyzing'-'planning' ELSE checkpoints END,updated_at=NOW() WHERE id=$1 AND twitch_user_id::text=$2")
        .bind(job_id).bind(owner).bind(if confirm { "queued" } else { "rejected" }).bind(confirm).bind(confirm && proposal.reanalyze).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    sqlx::query("INSERT INTO social_ai_revision_events(twitch_user_id,root_job_id,job_id,version_id,event) VALUES($1::text::bigint,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(owner).bind(root).bind(job_id).bind(parent).bind(if confirm {"confirmed"} else {"proposal_rejected"}).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    super::service::get_manual_job(pool, runtime, owner, job_id).await
}

pub async fn select_version(pool: &PgPool, runtime: &EditorRuntime, owner: &str, job_id: i64, version_id: i64) -> Result<AiJobView, AiEditorError> {
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if !usable(&video) { return Err(AiEditorError::Conflict); }
    sqlx::query("UPDATE social_ai_jobs SET selected_version_id=$3 WHERE id=(SELECT COALESCE(root_job_id,id) FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2) AND twitch_user_id::text=$2")
        .bind(job_id).bind(owner).bind(version_id).execute(pool).await.map_err(|_| AiEditorError::Storage)?;
    super::service::get_manual_job(pool, runtime, owner, job_id).await
}

pub async fn record_version_decision(pool: &PgPool, owner: &str, job_id: i64, version_id: i64, accept: bool) -> Result<(), AiEditorError> {
    let video = owned_version(pool, owner, job_id, version_id).await?;
    if !usable(&video) { return Err(AiEditorError::Conflict); }
    sqlx::query("INSERT INTO social_ai_revision_events(twitch_user_id,root_job_id,job_id,version_id,event) SELECT twitch_user_id,COALESCE(root_job_id,id),id,$3,$4 FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2 ON CONFLICT DO NOTHING")
        .bind(job_id).bind(owner).bind(version_id).bind(if accept {"accepted"} else {"rejected"}).execute(pool).await.map_err(|_| AiEditorError::Storage)?;
    Ok(())
}

pub async fn approve_version(pool: &PgPool, owner: &str, job_id: i64, version_id: i64, platforms: &[String]) -> Result<(), AiEditorError> {
    owned_version(pool, owner, job_id, version_id).await?;
    produced_video::approve(pool, owner, version_id, platforms).await.map_err(AiEditorError::from)?;
    record_version_decision(pool, owner, job_id, version_id, true).await
}

pub async fn publish_version(pool: &PgPool, owner: &str, job_id: i64, version_id: i64, platforms: &[String]) -> Result<produced_video::PublishingReservation, AiEditorError> {
    owned_version(pool, owner, job_id, version_id).await?;
    produced_video::enqueue_for_publishing_for_user(pool, owner, version_id, platforms).await.map_err(AiEditorError::from)
}

pub(super) async fn enrich(pool: &PgPool, owner: &str, job: &mut AiJobView) -> Result<(), AiEditorError> {
    let row = sqlx::query("SELECT COALESCE(j.root_job_id,j.id) AS root_job_id,r.selected_version_id,j.revision_proposal::text AS proposal FROM social_ai_jobs j JOIN social_ai_jobs r ON r.id=COALESCE(j.root_job_id,j.id) AND r.twitch_user_id=j.twitch_user_id WHERE j.id=$1 AND j.twitch_user_id::text=$2")
        .bind(job.id).bind(owner).fetch_one(pool).await.map_err(|_| AiEditorError::Storage)?;
    job.root_job_id = Some(row.try_get("root_job_id").map_err(|_| AiEditorError::Storage)?);
    job.selected_version_id = row.try_get("selected_version_id").map_err(|_| AiEditorError::Storage)?;
    let proposal: Option<String> = row.try_get("proposal").map_err(|_| AiEditorError::Storage)?;
    job.revision_proposal = proposal.map(|value| serde_json::from_str(&value).map_err(|_| AiEditorError::Plan)).transpose()?;
    Ok(())
}

pub(super) fn changed_percent(base: &Planning, new: &Planning) -> u64 {
    let duration = |plan: &Planning| plan.plan.kept_intervals.iter().map(|i| i.end_ms-i.start_ms).sum::<u64>();
    let overlap = base.plan.kept_intervals.iter().flat_map(|a| new.plan.kept_intervals.iter().map(move |b| a.end_ms.min(b.end_ms).saturating_sub(a.start_ms.max(b.start_ms)))).sum::<u64>();
    (duration(base)+duration(new)-2*overlap).saturating_mul(100)/duration(base).max(1)
}

pub(super) fn validate_proposal(proposal: &mut RevisionProposal, base: &Planning, new: &Planning, runtime: &EditorRuntime) -> Result<(), AiEditorError> {
    if [&proposal.summary,&proposal.impact].iter().any(|text| text.trim().is_empty() || text.len()>runtime.limits.plan.max_text_bytes) { return Err(AiEditorError::Plan); }
    proposal.requires_confirmation |= proposal.reanalyze || changed_percent(base,new)>u64::from(runtime.limits.revisions.max_auto_changed_percent) || !new.variants.is_empty();
    proposal.estimated_provider_cost_usd = None;
    Ok(())
}

pub(super) fn request_context(request: &RevisionRequest, base: &Planning) -> Value {
    json!({"feedback":request.feedback,"previous":base,"reanalysis_confirmed":request.confirmed,"instruction":"Interpret the feedback as a bounded edit of the saved analysis. Set reanalyze=true if understanding needs new visual analysis. Mark substantial changes for confirmation. Never fabricate subtitles, cost estimates or executable commands. Explain unsupported creative/layout changes honestly; preserve the existing stable layout and original audio."})
}
