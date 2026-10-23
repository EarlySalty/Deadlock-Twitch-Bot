use serde_json::{json, Value};
use sqlx::{Connection, PgConnection, PgPool};

use crate::{autopilot_settings, produced_video, production_jobs};

use super::{pipeline, revision, store, AiEditorError, EditorRuntime};

pub async fn open_editor(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    video_id: i64,
) -> Result<store::AiJobView, AiEditorError> {
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    let video = produced_video::get(pool, owner, video_id)
        .await
        .map_err(|_| AiEditorError::Storage)?
        .ok_or(AiEditorError::NotFound)?;
    if video.mode != production_jobs::ProductionMode::Ai || !revision::usable(&video) {
        return Err(AiEditorError::Conflict);
    }
    let maker = match revision::maker_id(&video) {
        Ok(id) => id,
        Err(_) => {
            let production_id = video.job_id.ok_or(AiEditorError::NotFound)?;
            let production = production_jobs::get(pool, owner, production_id)
                .await
                .map_err(|_| AiEditorError::Storage)?
                .filter(|job| job.clip_id == video.clip_id && job.mode == Some(video.mode))
                .ok_or(AiEditorError::NotFound)?;
            let checkpoints = pipeline::production_checkpoints(&production.metadata)?;
            pipeline::checkpoint_runtime(&checkpoints, runtime)?;
            let original_duration = match checkpoints.get(&store::AiStage::Preparing) {
                Some(store::StageCheckpoint::Preparing {
                    original_duration_ms,
                    ..
                }) => *original_duration_ms,
                _ => return Err(AiEditorError::Plan),
            };
            let (highlights, uncertainty) = match checkpoints.get(&store::AiStage::Analyzing) {
                Some(store::StageCheckpoint::Analyzing {
                    highlights,
                    uncertainty,
                    ..
                }) => (json!(highlights), json!(uncertainty)),
                _ => return Err(AiEditorError::Plan),
            };
            let decisions = revision::parent_planning(pool, owner, video.id)
                .await?
                .plan
                .timeline()
                .into_iter()
                .map(|interval| store::TimelineEntry {
                    start_ms: interval.start_ms as i64,
                    end_ms: interval.end_ms as i64,
                    description: interval.reason,
                })
                .collect::<Vec<_>>();
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO social_ai_jobs(twitch_user_id,clip_db_id,source_clip_id,source_twitch_clip_id,status,stage,checkpoints,produced_video_id,selected_version_id,original_duration_ms,output_duration_ms,highlights,uncertainty,decisions,review_reasons,completed_at)
                 SELECT $1::text::bigint,c.id,c.id,c.clip_id,'review','validating',$3::text::jsonb,$4,$4,$5,$6,$7::text::jsonb,$8::text::jsonb,$9::text::jsonb,$10::text::jsonb,NOW()
                 FROM twitch_clips_social_media c WHERE c.id=$2 AND c.twitch_user_id=$1 AND c.source_kind='twitch' AND c.category_key='deadlock' AND c.discarded_at IS NULL RETURNING id"
            ).bind(owner).bind(video.clip_id).bind(serde_json::to_string(&checkpoints).map_err(|_| AiEditorError::Storage)?)
                .bind(video.id).bind(original_duration).bind(video.duration_ms).bind(highlights.to_string()).bind(uncertainty.to_string())
                .bind(serde_json::to_string(&decisions).map_err(|_| AiEditorError::Storage)?).bind(video.review_reasons.to_string())
                .fetch_optional(&mut *tx).await.map_err(|_| AiEditorError::Storage)?.ok_or(AiEditorError::NotFound)?;
            sqlx::query("UPDATE social_produced_videos SET metadata=metadata || jsonb_build_object('manual_ai_job_id',$3::bigint,'root_ai_job_id',$3::bigint) WHERE id=$1 AND twitch_user_id=$2")
                .bind(video.id).bind(owner).bind(id).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
            id
        }
    };
    let latest: i64 = sqlx::query_scalar(
        "SELECT sibling.id FROM social_ai_jobs maker JOIN social_ai_jobs sibling ON COALESCE(sibling.root_job_id,sibling.id)=COALESCE(maker.root_job_id,maker.id) AND sibling.twitch_user_id=maker.twitch_user_id WHERE maker.id=$1 AND maker.twitch_user_id::text=$2 ORDER BY sibling.id DESC LIMIT 1"
    ).bind(maker).bind(owner).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    super::service::get_manual_job(pool, runtime, owner, latest).await
}

pub(super) fn production_id(video: &produced_video::ProducedVideo) -> Option<i64> {
    video
        .metadata
        .get("production_ai_job_id")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
}

pub(super) async fn hold_parent(
    tx: &mut PgConnection,
    owner: &str,
    parent: &produced_video::ProducedVideo,
    child_job: i64,
) -> Result<(), AiEditorError> {
    if production_id(parent).is_none()
        || parent
            .metadata
            .get("revision_pending")
            .and_then(Value::as_i64)
            == Some(child_job)
    {
        return Ok(());
    }
    let can_hold: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_produced_videos v WHERE v.id=$1 AND v.twitch_user_id=$2 AND v.status IN ('review','approved','scheduled') AND v.publication_executed_at IS NULL AND v.artifact_deleted_at IS NULL AND NOT EXISTS(SELECT 1 FROM twitch_clips_upload_queue q WHERE q.produced_video_id=v.id AND (q.status NOT IN ('waiting_exact_file','autopilot_held') OR q.effect_started_at IS NOT NULL OR q.external_id IS NOT NULL)))"
    ).bind(parent.id).bind(owner).fetch_one(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    if !can_hold {
        return Err(AiEditorError::Conflict);
    }
    sqlx::query(
        "UPDATE social_produced_videos SET expires_at=CASE WHEN status='review' THEN expires_at ELSE NOW()+INTERVAL '7 days' END,status='rejected',needs_review=TRUE,scheduled_at=NULL,metadata=(metadata-'publication') || jsonb_build_object('revision_pending',$3::bigint,'manual_review',true) WHERE id=$1 AND twitch_user_id=$2"
    ).bind(parent.id).bind(owner).bind(child_job).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    invalidate_approvals(tx, parent.id)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    Ok(())
}

async fn invalidate_approvals(tx: &mut PgConnection, video: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE social_media_clip_approval SET state='awaiting_approval',approver_user_id=NULL,decided_at=NULL,approved_platforms='[]'::jsonb WHERE produced_video_id=$1")
        .bind(video).execute(&mut *tx).await?;
    sqlx::query("UPDATE twitch_clips_upload_queue SET status='autopilot_discarded',outcome_reason='revision_replaced',next_retry_at=NULL WHERE produced_video_id=$1 AND status IN ('waiting_exact_file','autopilot_held')")
        .bind(video).execute(tx).await?;
    Ok(())
}

pub(super) async fn replace_parent_on(
    tx: &mut PgConnection,
    owner: &str,
    job_id: i64,
    version_id: i64,
) -> Result<(), store::AiStoreError> {
    let parent: Option<i64> = sqlx::query_scalar(
        "UPDATE social_produced_videos p SET metadata=(p.metadata-'revision_pending'-'publication') || jsonb_build_object('superseded_by',$3::bigint),status='rejected',needs_review=TRUE
         FROM social_ai_jobs j WHERE j.id=$1 AND j.twitch_user_id::text=$2 AND p.id=j.parent_version_id AND p.twitch_user_id=$2 AND p.metadata->>'revision_pending'=j.id::text RETURNING p.id"
    ).bind(job_id).bind(owner).bind(version_id).fetch_optional(&mut *tx).await?;
    if let Some(parent) = parent {
        invalidate_approvals(tx, parent).await?;
        sqlx::query("UPDATE social_ai_jobs SET selected_version_id=$3,updated_at=NOW() WHERE id=(SELECT COALESCE(root_job_id,id) FROM social_ai_jobs WHERE id=$1 AND twitch_user_id::text=$2)")
            .bind(job_id).bind(owner).bind(version_id).execute(tx).await?;
    }
    Ok(())
}

pub(super) async fn restore_failed_parents(
    lease: &mut store::ProductionLease,
) -> Result<(), store::AiStoreError> {
    let jobs: Vec<(String, i64)> = sqlx::query_as(
        "SELECT j.twitch_user_id::text,j.id FROM social_ai_jobs j JOIN social_produced_videos p ON p.id=j.parent_version_id AND p.twitch_user_id=j.twitch_user_id::text AND p.metadata->>'revision_pending'=j.id::text WHERE j.status IN ('failed','rejected') ORDER BY j.id LIMIT 100"
    ).fetch_all(&mut *lease.connection).await?;
    for (owner, id) in jobs {
        let mut tx = lease.connection.begin().await?;
        autopilot_settings::lock_owner(&mut tx, &owner).await?;
        match restore_parent_on(&mut tx, &owner, id).await {
            Ok(()) => tx.commit().await?,
            Err(store::AiStoreError::PipelineBusy) => {
                tx.rollback().await?;
                tracing::info!(
                    job_id = id,
                    "AI revision parent stays parked while a legacy publication is open"
                );
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) async fn restore_parent_on(
    tx: &mut PgConnection,
    owner: &str,
    job_id: i64,
) -> Result<(), store::AiStoreError> {
    let parent: Option<i64> = sqlx::query_scalar(
        "UPDATE social_produced_videos p SET status='review',needs_review=TRUE,metadata=p.metadata-'revision_pending'
         FROM social_ai_jobs j WHERE j.id=$1 AND j.twitch_user_id::text=$2 AND j.status IN ('failed','rejected') AND p.id=j.parent_version_id AND p.twitch_user_id=$2 AND p.metadata->>'revision_pending'=j.id::text RETURNING p.id"
    ).bind(job_id).bind(owner).fetch_optional(&mut *tx).await?;
    if let Some(parent) = parent {
        sqlx::query("UPDATE twitch_clips_upload_queue SET status='waiting_exact_file',outcome_reason=NULL,next_retry_at=NULL WHERE produced_video_id=$1 AND status='autopilot_discarded' AND outcome_reason='revision_replaced' AND effect_started_at IS NULL AND external_id IS NULL")
            .bind(parent).execute(tx).await.map_err(busy_publication)?;
    }
    Ok(())
}

fn busy_publication(error: sqlx::Error) -> store::AiStoreError {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
            store::AiStoreError::PipelineBusy
        }
        _ => store::AiStoreError::Database(error),
    }
}
