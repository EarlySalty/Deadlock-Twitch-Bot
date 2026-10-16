use serde_json::Value;
use sqlx::{PgConnection, PgPool};

use super::executor::{Claim, DestinationPublisher};
use super::{queue_status, stored_tiktok_options, tiktok_subject, PublishError};
use crate::tiktok_recovery::ClaimCheckpoint;
use crate::uploaders::tiktok::TikTokPostOptions;
use crate::uploaders::UploadError;

pub(super) const BOUNDARY_PENDING: &str = "tiktok_effect_boundary_pending";
const PROCESSING: &str = "tiktok_processing";
const INIT_UNCERTAIN: &str = "tiktok_init_uncertain";
const TRANSFER_UNCERTAIN: &str = "tiktok_transfer_uncertain";
const OPTIONS_INVALID: &str = "tiktok_options_invalid";

pub(super) async fn verified_options(
    pool: &PgPool,
    claim: &Claim,
) -> Result<Result<TikTokPostOptions, &'static str>, PublishError> {
    match tiktok_subject(pool, &claim.owner, claim.video_id).await {
        Ok(subject) => Ok(subject.binding().cloned().map_err(|issue| issue.reason())),
        Err(PublishError::NotFound | PublishError::State | PublishError::Io(_)) => {
            Ok(Err("exact_file_changed"))
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn binding_issue_on(
    connection: &mut PgConnection,
    video_id: i64,
    options: &TikTokPostOptions,
) -> Result<Option<&'static str>, PublishError> {
    let metadata: Option<String> = sqlx::query_scalar(
        "SELECT metadata::text FROM social_produced_videos WHERE id = $1 FOR UPDATE",
    )
    .bind(video_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(metadata) = metadata else {
        return Ok(Some("exact_file_changed"));
    };
    let metadata: Value = serde_json::from_str(&metadata)?;
    if stored_tiktok_options(&metadata).as_ref() != Some(options) {
        return Ok(Some("tiktok_consent_missing"));
    }
    if crate::ai_editor::metadata::tiktok_caption(&metadata) != options.caption {
        return Ok(Some("tiktok_caption_changed"));
    }
    Ok(None)
}

pub(super) async fn quarantine(pool: &PgPool, claim: &Claim) -> Result<(), PublishError> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2, outcome_reason = $3, last_error = $3,
         next_retry_at = NULL WHERE id = $1 AND claim_generation = $4 AND status = $5",
    )
    .bind(claim.queue_id)
    .bind(queue_status::UNCERTAIN)
    .bind(TRANSFER_UNCERTAIN)
    .bind(claim.generation)
    .bind(queue_status::PREPARING)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn dispatch(
    pool: &PgPool,
    publisher: &dyn DestinationPublisher,
    claim: &Claim,
    options: &TikTokPostOptions,
    max_attempts: i32,
) -> Result<(), PublishError> {
    let checkpoint = ClaimCheckpoint {
        pool,
        queue_id: claim.queue_id,
        generation: claim.generation,
    };
    let result = publisher
        .publish_tiktok(&claim.owner, &claim.request, options, &checkpoint)
        .await;
    match result {
        Ok(publish_id) => processing(pool, claim, &publish_id).await,
        Err(UploadError::NotStarted(error)) => not_started(pool, claim, *error, max_attempts).await,
        Err(error) => {
            tracing::warn!(%error, queue = claim.queue_id, "TikTok transfer outcome is uncertain");
            uncertain(pool, claim).await
        }
    }
}

async fn processing(pool: &PgPool, claim: &Claim, publish_id: &str) -> Result<(), PublishError> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2,
         outcome_reason = CASE WHEN tiktok_publish_id = $3 THEN $4 ELSE $5 END,
         last_error = CASE WHEN tiktok_publish_id = $3 THEN NULL ELSE $5 END,
         next_retry_at = NULL WHERE id = $1 AND claim_generation = $6 AND status = $7",
    )
    .bind(claim.queue_id)
    .bind(queue_status::UNCERTAIN)
    .bind(publish_id)
    .bind(PROCESSING)
    .bind(TRANSFER_UNCERTAIN)
    .bind(claim.generation)
    .bind(queue_status::EFFECT_STARTED)
    .execute(pool)
    .await?;
    Ok(())
}

async fn uncertain(pool: &PgPool, claim: &Claim) -> Result<(), PublishError> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2,
         outcome_reason = CASE WHEN tiktok_publish_id IS NULL THEN $3 ELSE $4 END,
         last_error = CASE WHEN tiktok_publish_id IS NULL THEN $3 ELSE $4 END,
         next_retry_at = NULL WHERE id = $1 AND claim_generation = $5 AND status = $6",
    )
    .bind(claim.queue_id)
    .bind(queue_status::UNCERTAIN)
    .bind(INIT_UNCERTAIN)
    .bind(TRANSFER_UNCERTAIN)
    .bind(claim.generation)
    .bind(queue_status::EFFECT_STARTED)
    .execute(pool)
    .await?;
    Ok(())
}

async fn not_started(
    pool: &PgPool,
    claim: &Claim,
    error: UploadError,
    max_attempts: i32,
) -> Result<(), PublishError> {
    let attempts = claim.attempts.saturating_add(1);
    let (status, reason, delay) = match &error {
        UploadError::Validation(_) => (queue_status::HELD, OPTIONS_INVALID, None),
        UploadError::NotImplemented(_) => (queue_status::FAILED, "validation_failed", None),
        _ if attempts >= max_attempts => (queue_status::FAILED, retry_reason(&error), None),
        _ => (
            queue_status::RETRY,
            retry_reason(&error),
            Some(15 * 60 * (1_i64 << (attempts - 1).clamp(0, 6))),
        ),
    };
    let detail = match &error {
        UploadError::Validation(message) => message.clone(),
        _ => reason.to_string(),
    };
    let changed = sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2, outcome_reason = $3, last_error = $4,
         effect_started_at = NULL,
         next_retry_at = CASE WHEN $5::bigint IS NULL THEN NULL ELSE clock_timestamp() + make_interval(secs => $5::bigint::double precision) END
         WHERE id = $1 AND claim_generation = $6 AND status = $7 AND tiktok_publish_id IS NULL",
    )
    .bind(claim.queue_id)
    .bind(status)
    .bind(reason)
    .bind(detail)
    .bind(delay)
    .bind(claim.generation)
    .bind(queue_status::EFFECT_STARTED)
    .execute(pool)
    .await?
    .rows_affected();
    if changed == 0 {
        return uncertain(pool, claim).await;
    }
    Ok(())
}

fn retry_reason(error: &UploadError) -> &'static str {
    match error {
        UploadError::QuotaExceeded(_) => "quota_exceeded",
        UploadError::NotAuthenticated => "not_authenticated",
        _ => "platform_error",
    }
}
