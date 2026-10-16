use serde_json::Value;
use sqlx::{PgConnection, PgPool};

use super::executor::{Claim, DestinationPublisher};
use super::{
    queue_status, stored_tiktok_options, tiktok_account_on, tiktok_subject_on, Binding,
    PublishError,
};
use crate::tiktok_recovery::{self, ClaimCheckpoint, PublishState};
use crate::uploaders::tiktok::TikTokPostOptions;
use crate::uploaders::UploadError;

pub(super) const BOUNDARY_PENDING: &str = "tiktok_effect_boundary_pending";
pub(super) const INIT_UNCERTAIN: &str = "tiktok_init_uncertain";
pub(super) const TRANSFER_UNCERTAIN: &str = "tiktok_transfer_uncertain";
const PROCESSING: &str = "tiktok_processing";
const OPTIONS_INVALID: &str = "tiktok_options_invalid";
const FILE_INVALID: &str = "tiktok_file_invalid";
const ALREADY_PUBLISHED: &str = "tiktok_already_published";
const TRANSFER_ACTIVE: &str = "tiktok_transfer_active";
const INIT_UNCONFIRMED: &str = "tiktok_init_unconfirmed";
const STATUS_UNCONFIRMED: &str = "tiktok_status_unconfirmed";
const INBOX_ONLY: &str = "tiktok_inbox_only";
const REJECTED: &str = "tiktok_rejected";
const WITHOUT_POST_ID: &str = "tiktok_published_without_post_id";
const INBOX_ONLY_MESSAGE: &str =
    "TikTok hat das Video nur in dein TikTok-Postfach gelegt. Es wurde nicht veröffentlicht.";
const PROCESSING_POLL_SECONDS: i32 = 60;
const UNCERTAIN_POLL_SECONDS: i32 = 600;
const PROCESSING_LIMIT_HOURS: i32 = 24;
const RECONCILE_WINDOW_DAYS: i32 = 7;
const RECONCILE_BATCH: i64 = 10;

pub(super) async fn verified_options(
    pool: &PgPool,
    claim: &Claim,
    digest: &str,
) -> Result<Result<TikTokPostOptions, &'static str>, PublishError> {
    let binding = Binding {
        path: claim.request.file_path.clone(),
        digest: digest.to_string(),
    };
    let mut connection = pool.acquire().await?;
    match tiktok_subject_on(&mut connection, &claim.owner, claim.video_id, &binding).await {
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

pub(super) async fn effect_issue_on(
    connection: &mut PgConnection,
    claim: &Claim,
    options: &TikTokPostOptions,
) -> Result<Option<&'static str>, PublishError> {
    let unified: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_pipeline_cutover
         WHERE twitch_user_id = $1 AND state = 'unified')",
    )
    .bind(&claim.owner)
    .fetch_one(&mut *connection)
    .await?;
    if !unified {
        return Ok(Some(BOUNDARY_PENDING));
    }
    let account = tiktok_account_on(connection, &claim.owner).await?;
    if account.credential_id.is_none() || account.platform_user_id.is_none() {
        return Ok(Some("account_missing"));
    }
    if account.credential_id != Some(options.credential_id)
        || account.platform_user_id.as_deref() != Some(options.platform_user_id.as_str())
    {
        return Ok(Some("account_changed"));
    }
    if !account.music_consent {
        return Ok(Some("tiktok_music_consent_missing"));
    }
    clip_issue_on(connection, claim.queue_id).await
}

async fn clip_issue_on(
    connection: &mut PgConnection,
    queue_id: i64,
) -> Result<Option<&'static str>, PublishError> {
    let clip: Option<(i64, bool)> = sqlx::query_as(
        "SELECT q.clip_id::bigint, COALESCE(c.uploaded_tiktok, FALSE) FROM twitch_clips_social_media c
         JOIN twitch_clips_upload_queue q ON q.clip_id = c.id WHERE q.id = $1 FOR UPDATE OF c",
    )
    .bind(queue_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((clip_id, published)) = clip else {
        return Ok(Some("exact_file_changed"));
    };
    if published {
        return Ok(Some(ALREADY_PUBLISHED));
    }
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM twitch_clips_upload_queue WHERE clip_id = $1
         AND platform = 'tiktok' AND id <> $2 AND (status IN ('inbox', 'inbox_pending')
             OR (produced_video_id IS NOT NULL AND status = ANY($3))))",
    )
    .bind(clip_id)
    .bind(queue_id)
    .bind(tiktok_recovery::autopilot_transfer_states())
    .fetch_one(&mut *connection)
    .await?;
    Ok(active.then_some(TRANSFER_ACTIVE))
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
        Err(UploadError::NotStarted(error)) => {
            not_started(pool, claim, options, *error, max_attempts).await
        }
        Err(error) => {
            tracing::warn!(%error, queue = claim.queue_id, "TikTok transfer outcome is uncertain");
            uncertain(pool, claim).await
        }
    }
}

async fn processing(pool: &PgPool, claim: &Claim, publish_id: &str) -> Result<(), PublishError> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET
         status = CASE WHEN tiktok_publish_id = $3 THEN $2 ELSE $8 END,
         outcome_reason = CASE WHEN tiktok_publish_id = $3 THEN $4 ELSE $5 END,
         last_error = CASE WHEN tiktok_publish_id = $3 THEN NULL ELSE $5 END,
         next_retry_at = NULL WHERE id = $1 AND claim_generation = $6 AND status = $7",
    )
    .bind(claim.queue_id)
    .bind(queue_status::PROCESSING)
    .bind(publish_id)
    .bind(PROCESSING)
    .bind(TRANSFER_UNCERTAIN)
    .bind(claim.generation)
    .bind(queue_status::EFFECT_STARTED)
    .bind(queue_status::UNCERTAIN)
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
    options: &TikTokPostOptions,
    error: UploadError,
    max_attempts: i32,
) -> Result<(), PublishError> {
    let attempts = claim.attempts.saturating_add(1);
    let file_invalid = match &error {
        UploadError::Io(_) => true,
        UploadError::Validation(_) => !file_intact(claim, options).await,
        _ => false,
    };
    let (status, reason, delay) = match &error {
        _ if file_invalid => (queue_status::HELD, FILE_INVALID, None),
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

async fn file_intact(claim: &Claim, options: &TikTokPostOptions) -> bool {
    let path = &claim.request.file_path;
    let non_empty = tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
    non_empty
        && crate::uploaders::tiktok::video_digest(path)
            .await
            .is_ok_and(|digest| digest == options.approved_video_sha256)
}

pub(super) async fn reconcile(
    pool: &PgPool,
    publisher: &dyn DestinationPublisher,
    max_attempts: i32,
) -> Result<usize, PublishError> {
    retry_unconfirmed_init(pool, max_attempts).await?;
    escalate_stalled(pool).await?;
    let rows: Vec<(i64, String, String, String, String)> = sqlx::query_as(
        "SELECT q.id::bigint, q.status, v.twitch_user_id, q.tiktok_publish_id,
             q.tiktok_post_options::text
         FROM twitch_clips_upload_queue q JOIN social_produced_videos v ON v.id = q.produced_video_id
         WHERE q.platform = 'tiktok' AND q.tiktok_publish_id IS NOT NULL
         AND q.tiktok_post_options IS NOT NULL
         AND q.effect_started_at > clock_timestamp() - make_interval(days => $3)
         AND ((q.status = $1 AND COALESCE(q.last_attempt_at, '-infinity'::timestamptz)
                 < clock_timestamp() - make_interval(secs => $4::double precision))
             OR (q.status = $2 AND COALESCE(q.last_attempt_at, '-infinity'::timestamptz)
                 < clock_timestamp() - make_interval(secs => $5::double precision)))
         ORDER BY q.last_attempt_at NULLS FIRST, q.id LIMIT $6",
    )
    .bind(queue_status::PROCESSING)
    .bind(queue_status::UNCERTAIN)
    .bind(RECONCILE_WINDOW_DAYS)
    .bind(PROCESSING_POLL_SECONDS)
    .bind(UNCERTAIN_POLL_SECONDS)
    .bind(RECONCILE_BATCH)
    .fetch_all(pool)
    .await?;
    let mut resolved = 0;
    for (queue_id, status, owner, publish_id, options) in rows {
        if poll(
            pool,
            publisher,
            queue_id,
            &status,
            &owner,
            &publish_id,
            &options,
        )
        .await?
        {
            resolved += 1;
        }
    }
    Ok(resolved)
}

async fn poll(
    pool: &PgPool,
    publisher: &dyn DestinationPublisher,
    queue_id: i64,
    status: &str,
    owner: &str,
    publish_id: &str,
    options: &str,
) -> Result<bool, PublishError> {
    let touched = sqlx::query(
        "UPDATE twitch_clips_upload_queue SET last_attempt_at = clock_timestamp()
         WHERE id = $1 AND status = $2 AND tiktok_publish_id = $3",
    )
    .bind(queue_id)
    .bind(status)
    .bind(publish_id)
    .execute(pool)
    .await?
    .rows_affected();
    if touched != 1 {
        return Ok(false);
    }
    let options: TikTokPostOptions = serde_json::from_str(options)?;
    let response = match publisher.tiktok_status(owner, &options, publish_id).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, queue = queue_id, "TikTok publication status unavailable");
            return Ok(false);
        }
    };
    tiktok_recovery::record_status(pool, queue_id, &response).await?;
    match tiktok_recovery::classify(&response) {
        PublishState::Published(post_id) => {
            published(pool, queue_id, status, publish_id, post_id.as_deref()).await
        }
        PublishState::Inbox => {
            finish(
                pool,
                queue_id,
                status,
                publish_id,
                INBOX_ONLY,
                Some(INBOX_ONLY_MESSAGE),
            )
            .await
        }
        PublishState::Rejected => finish(pool, queue_id, status, publish_id, REJECTED, None).await,
        PublishState::Processing => {
            resume_processing(pool, queue_id, status, publish_id).await?;
            Ok(false)
        }
        PublishState::Unknown => Ok(false),
    }
}

async fn published(
    pool: &PgPool,
    queue_id: i64,
    status: &str,
    publish_id: &str,
    post_id: Option<&str>,
) -> Result<bool, PublishError> {
    let external_id = post_id.unwrap_or(publish_id);
    let reason = post_id.is_none().then_some(WITHOUT_POST_ID);
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $4, external_id = $5,
         completed_at = clock_timestamp(), outcome_reason = $6, last_error = NULL,
         next_retry_at = NULL WHERE id = $1 AND status = $2 AND tiktok_publish_id = $3",
    )
    .bind(queue_id)
    .bind(status)
    .bind(publish_id)
    .bind(queue_status::PUBLISHED)
    .bind(external_id)
    .bind(reason)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 1 {
        sqlx::query(
            "UPDATE twitch_clips_social_media c SET uploaded_tiktok = TRUE, tiktok_video_id = $1,
             tiktok_uploaded_at = clock_timestamp() FROM twitch_clips_upload_queue q
             WHERE q.id = $2 AND c.id = q.clip_id",
        )
        .bind(external_id)
        .bind(queue_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(changed == 1)
}

async fn finish(
    pool: &PgPool,
    queue_id: i64,
    status: &str,
    publish_id: &str,
    reason: &str,
    message: Option<&str>,
) -> Result<bool, PublishError> {
    let changed = sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $4, outcome_reason = $5,
         last_error = COALESCE($6, last_error, $5), next_retry_at = NULL
         WHERE id = $1 AND status = $2 AND tiktok_publish_id = $3",
    )
    .bind(queue_id)
    .bind(status)
    .bind(publish_id)
    .bind(queue_status::FAILED)
    .bind(reason)
    .bind(message)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

async fn resume_processing(
    pool: &PgPool,
    queue_id: i64,
    status: &str,
    publish_id: &str,
) -> Result<(), PublishError> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $4, outcome_reason = $5, last_error = NULL
         WHERE id = $1 AND status = $2 AND tiktok_publish_id = $3 AND status <> $4
         AND effect_started_at > clock_timestamp() - make_interval(hours => $6)",
    )
    .bind(queue_id)
    .bind(status)
    .bind(publish_id)
    .bind(queue_status::PROCESSING)
    .bind(PROCESSING)
    .bind(PROCESSING_LIMIT_HOURS)
    .execute(pool)
    .await?;
    Ok(())
}

async fn escalate_stalled(pool: &PgPool) -> Result<(), PublishError> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2, outcome_reason = $3, last_error = $3
         WHERE produced_video_id IS NOT NULL AND platform = 'tiktok' AND status = $1
         AND effect_started_at < clock_timestamp() - make_interval(hours => $4)",
    )
    .bind(queue_status::PROCESSING)
    .bind(queue_status::UNCERTAIN)
    .bind(STATUS_UNCONFIRMED)
    .bind(PROCESSING_LIMIT_HOURS)
    .execute(pool)
    .await?;
    Ok(())
}

async fn retry_unconfirmed_init(pool: &PgPool, max_attempts: i32) -> Result<(), PublishError> {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET
         status = CASE WHEN COALESCE(attempts, 0) >= $4 THEN $3 ELSE $2 END,
         outcome_reason = $5, last_error = $5, effect_started_at = NULL,
         next_retry_at = CASE WHEN COALESCE(attempts, 0) >= $4 THEN NULL
             ELSE clock_timestamp() + make_interval(mins => 15) END
         WHERE produced_video_id IS NOT NULL AND platform = 'tiktok' AND status = $1
         AND outcome_reason = $6 AND tiktok_publish_id IS NULL",
    )
    .bind(queue_status::UNCERTAIN)
    .bind(queue_status::RETRY)
    .bind(queue_status::FAILED)
    .bind(max_attempts)
    .bind(INIT_UNCONFIRMED)
    .bind(INIT_UNCERTAIN)
    .execute(pool)
    .await?;
    Ok(())
}
