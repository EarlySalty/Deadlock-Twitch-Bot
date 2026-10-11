use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, Row};

use crate::autopilot_settings::{self, json_column, SettingsError};
pub use crate::production_jobs::ProductionMode;

#[derive(Debug, thiserror::Error)]
pub enum ProducedVideoError {
    #[error("Das produzierte Video wurde nicht gefunden")]
    NotFound,
    #[error("Das Video gehört nicht zu deinem Kanal")]
    Ownership,
    #[error("Das Video ist in diesem Zustand nicht verfügbar")]
    State,
    #[error("Die gerenderte Videodatei ist nicht verfügbar")]
    FileNotReady,
    #[error("Mindestens eine Zielplattform muss ausgewählt werden")]
    NoPlatforms,
    #[error("Für eine ausgewählte Plattform fehlt die Videofreigabe")]
    ApprovalRequired,
    #[error("Im gemeinsamen Zeitplan ist kein freier Termin verfügbar")]
    NoSlot,
    #[error("Die Videodaten sind ungültig")]
    Invalid,
    #[error("Die Videodaten konnten nicht gespeichert oder geladen werden")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error("Die Videodaten sind ungültig")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct NewProducedVideo {
    pub twitch_user_id: String,
    pub clip_id: i64,
    pub job_id: Option<i64>,
    pub mode: ProductionMode,
    pub version_no: i32,
    pub parent_version_id: Option<i64>,
    pub variant_index: i32,
}

#[derive(Debug, Clone)]
pub struct RenderedVideo {
    pub file_path: String,
    pub duration_ms: i64,
    pub needs_review: bool,
    pub review_reasons: Vec<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProducedVideo {
    pub id: i64,
    pub twitch_user_id: String,
    pub clip_id: i64,
    pub job_id: Option<i64>,
    pub mode: ProductionMode,
    pub version_no: i32,
    pub parent_version_id: Option<i64>,
    pub variant_index: i32,
    pub file_path: Option<String>,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub needs_review: bool,
    pub review_reasons: Value,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scheduled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformApproval {
    pub platform: String,
    pub state: String,
    pub approver_user_id: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishingReservation {
    pub produced_video_id: i64,
    pub scheduled_at: DateTime<Utc>,
    pub queue_ids: Vec<(String, i64)>,
}

const SELECT: &str = "SELECT id, twitch_user_id, clip_id, job_id, mode, version_no, parent_version_id,
    variant_index, file_path, duration_ms, status, needs_review, review_reasons::text, metadata::text,
    created_at, expires_at, scheduled_at FROM social_produced_videos";

fn from_row(row: sqlx::postgres::PgRow) -> Result<ProducedVideo, ProducedVideoError> {
    let mode: String = row.try_get("mode")?;
    Ok(ProducedVideo {
        id: row.try_get("id")?,
        twitch_user_id: row.try_get("twitch_user_id")?,
        clip_id: row.try_get("clip_id")?,
        job_id: row.try_get("job_id")?,
        mode: ProductionMode::parse(&mode).map_err(|_| ProducedVideoError::Invalid)?,
        version_no: row.try_get("version_no")?,
        parent_version_id: row.try_get("parent_version_id")?,
        variant_index: row.try_get("variant_index")?,
        file_path: row.try_get("file_path")?,
        duration_ms: row.try_get("duration_ms")?,
        status: row.try_get("status")?,
        needs_review: row.try_get("needs_review")?,
        review_reasons: json_column(&row, "review_reasons")?,
        metadata: json_column(&row, "metadata")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        scheduled_at: row.try_get("scheduled_at")?,
    })
}

pub async fn get(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
) -> Result<Option<ProducedVideo>, ProducedVideoError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE id = $1 AND twitch_user_id = $2"
    )))
    .bind(id)
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await?
    .map(from_row)
    .transpose()
}

pub async fn list(
    pool: &PgPool,
    twitch_user_id: &str,
    limit: i64,
) -> Result<Vec<ProducedVideo>, ProducedVideoError> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE twitch_user_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2"
    )))
    .bind(twitch_user_id)
    .bind(limit.clamp(0, 500))
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(from_row)
    .collect()
}

pub async fn insert(
    pool: &PgPool,
    video: &NewProducedVideo,
) -> Result<ProducedVideo, ProducedVideoError> {
    if video.version_no <= 0 || video.variant_index < 0 {
        return Err(ProducedVideoError::Invalid);
    }
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, &video.twitch_user_id).await?;
    let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_clips_social_media WHERE id = $1 AND twitch_user_id = $2)")
        .bind(video.clip_id).bind(&video.twitch_user_id).fetch_one(&mut *tx).await?;
    if !owned {
        return Err(ProducedVideoError::Ownership);
    }
    if let Some(job_id) = video.job_id {
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_production_jobs WHERE id = $1 AND twitch_user_id = $2 AND clip_id = $3 AND mode = $4)")
            .bind(job_id).bind(&video.twitch_user_id).bind(video.clip_id).bind(video.mode.as_str()).fetch_one(&mut *tx).await?;
        if !owned {
            return Err(ProducedVideoError::Ownership);
        }
    }
    if let Some(parent) = video.parent_version_id {
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_produced_videos WHERE id = $1 AND twitch_user_id = $2 AND clip_id = $3 AND version_no < $4)")
            .bind(parent).bind(&video.twitch_user_id).bind(video.clip_id).bind(video.version_no).fetch_one(&mut *tx).await?;
        if !owned {
            return Err(ProducedVideoError::Ownership);
        }
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, version_no, parent_version_id, variant_index) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id")
        .bind(&video.twitch_user_id).bind(video.clip_id).bind(video.job_id).bind(video.mode.as_str())
        .bind(video.version_no).bind(video.parent_version_id).bind(video.variant_index).fetch_one(&mut *tx).await?;
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("{SELECT} WHERE id = $1")))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let video = from_row(row)?;
    tx.commit().await?;
    Ok(video)
}

async fn ensure_file(path: &str) -> Result<(), ProducedVideoError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| ProducedVideoError::FileNotReady)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(ProducedVideoError::FileNotReady);
    }
    Ok(())
}

pub async fn complete_render(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
    rendered: &RenderedVideo,
) -> Result<ProducedVideo, ProducedVideoError> {
    if rendered.duration_ms <= 0 || !rendered.metadata.is_object() {
        return Err(ProducedVideoError::Invalid);
    }
    ensure_file(&rendered.file_path).await?;
    let changed = sqlx::query("UPDATE social_produced_videos SET file_path = $3, duration_ms = $4, needs_review = $5,
        review_reasons = $6::text::jsonb, metadata = $7::text::jsonb, status = 'review', expires_at = NOW() + INTERVAL '7 days'
        WHERE id = $1 AND twitch_user_id = $2 AND status = 'rendering'")
        .bind(id).bind(twitch_user_id).bind(&rendered.file_path).bind(rendered.duration_ms).bind(rendered.needs_review)
        .bind(serde_json::to_string(&rendered.review_reasons)?).bind(serde_json::to_string(&rendered.metadata)?)
        .execute(pool).await?.rows_affected();
    if changed != 1 {
        return Err(ProducedVideoError::State);
    }
    get(pool, twitch_user_id, id)
        .await?
        .ok_or(ProducedVideoError::NotFound)
}

pub async fn fail_render(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
    reasons: &[String],
) -> Result<(), ProducedVideoError> {
    let changed = sqlx::query(
        "UPDATE social_produced_videos SET status = 'failed', needs_review = TRUE,
         review_reasons = $3::text::jsonb WHERE id = $1 AND twitch_user_id = $2 AND status = 'rendering'",
    )
    .bind(id)
    .bind(twitch_user_id)
    .bind(serde_json::to_string(reasons)?)
    .execute(pool)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(ProducedVideoError::State);
    }
    Ok(())
}

async fn lock_video(
    tx: &mut sqlx::PgConnection,
    id: i64,
    owner: Option<&str>,
) -> Result<ProducedVideo, ProducedVideoError> {
    let twitch_user_id: String =
        sqlx::query_scalar("SELECT twitch_user_id FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ProducedVideoError::NotFound)?;
    if owner.is_some_and(|owner| owner != twitch_user_id) {
        return Err(ProducedVideoError::Ownership);
    }
    autopilot_settings::lock_owner(tx, &twitch_user_id).await?;
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE id = $1 FOR UPDATE"
    )))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let video = from_row(row)?;
    let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_clips_social_media WHERE id = $1 AND twitch_user_id = $2)")
        .bind(video.clip_id).bind(&video.twitch_user_id).fetch_one(tx).await?;
    if !owned {
        return Err(ProducedVideoError::Ownership);
    }
    Ok(video)
}

async fn ensure_ready(video: &ProducedVideo, now: DateTime<Utc>) -> Result<(), ProducedVideoError> {
    if !matches!(video.status.as_str(), "review" | "approved" | "scheduled") {
        return Err(ProducedVideoError::State);
    }
    if video.status == "review" && video.expires_at.is_none_or(|expiry| expiry <= now) {
        return Err(ProducedVideoError::State);
    }
    if video.duration_ms.is_none_or(|duration| duration <= 0) {
        return Err(ProducedVideoError::FileNotReady);
    }
    ensure_file(
        video
            .file_path
            .as_deref()
            .ok_or(ProducedVideoError::FileNotReady)?,
    )
    .await
}

fn platforms(values: &[String]) -> Result<Vec<String>, ProducedVideoError> {
    let values = autopilot_settings::normalize_platforms(values)?;
    if values.is_empty() {
        return Err(ProducedVideoError::NoPlatforms);
    }
    Ok(values)
}

pub async fn approve(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
    selected: &[String],
) -> Result<(), ProducedVideoError> {
    let selected = platforms(selected)?;
    let mut tx = pool.begin().await?;
    let video = lock_video(&mut tx, id, Some(twitch_user_id)).await?;
    let now = sqlx::query_scalar("SELECT NOW()")
        .fetch_one(&mut *tx)
        .await?;
    ensure_ready(&video, now).await?;
    for platform in selected {
        sqlx::query("INSERT INTO social_media_clip_approval (produced_video_id, platform, state, approved_platforms, approver_user_id, decided_at)
            VALUES ($1, $2, 'approved', '[]'::jsonb, $3, NOW())
            ON CONFLICT (produced_video_id, platform) DO UPDATE SET state = 'approved', approved_platforms = EXCLUDED.approved_platforms,
                approver_user_id = EXCLUDED.approver_user_id, decided_at = EXCLUDED.decided_at")
            .bind(id).bind(platform).bind(twitch_user_id).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE social_produced_videos SET status = CASE WHEN status = 'scheduled' THEN status ELSE 'approved' END, needs_review = FALSE WHERE id = $1")
        .bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn approvals(
    pool: &PgPool,
    twitch_user_id: &str,
    id: i64,
) -> Result<Vec<PlatformApproval>, ProducedVideoError> {
    if get(pool, twitch_user_id, id).await?.is_none() {
        return Err(ProducedVideoError::NotFound);
    }
    sqlx::query("SELECT platform, state, approver_user_id, decided_at FROM social_media_clip_approval WHERE produced_video_id = $1 ORDER BY platform")
        .bind(id).fetch_all(pool).await?.into_iter().map(|row| Ok(PlatformApproval {
            platform: row.try_get("platform")?, state: row.try_get("state")?,
            approver_user_id: row.try_get("approver_user_id")?, decided_at: row.try_get("decided_at")?,
        })).collect()
}

pub async fn enqueue_for_publishing(
    pool: &PgPool,
    produced_video_id: i64,
    selected: &[String],
) -> Result<PublishingReservation, ProducedVideoError> {
    enqueue(pool, None, produced_video_id, selected).await
}

pub async fn enqueue_for_publishing_for_user(
    pool: &PgPool,
    twitch_user_id: &str,
    produced_video_id: i64,
    selected: &[String],
) -> Result<PublishingReservation, ProducedVideoError> {
    enqueue(pool, Some(twitch_user_id), produced_video_id, selected).await
}

async fn enqueue(
    pool: &PgPool,
    owner: Option<&str>,
    id: i64,
    selected: &[String],
) -> Result<PublishingReservation, ProducedVideoError> {
    let selected = platforms(selected)?;
    let mut tx = pool.begin().await?;
    let video = lock_video(&mut tx, id, owner).await?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT NOW()")
        .fetch_one(&mut *tx)
        .await?;
    ensure_ready(&video, now).await?;
    for platform in &selected {
        let approved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_media_clip_approval WHERE produced_video_id = $1 AND platform = $2 AND state = 'approved' AND approver_user_id = $3)")
            .bind(id).bind(platform).bind(&video.twitch_user_id).fetch_one(&mut *tx).await?;
        if !approved {
            return Err(ProducedVideoError::ApprovalRequired);
        }
    }
    let settings = autopilot_settings::load_on(&mut tx, &video.twitch_user_id).await?;
    let scheduled_at = match video.scheduled_at {
        Some(slot) => slot,
        None => {
            let taken: Vec<DateTime<Utc>> = sqlx::query_scalar(
                "SELECT scheduled_at FROM social_produced_videos WHERE twitch_user_id = $1 AND scheduled_at IS NOT NULL
                 UNION SELECT q.scheduled_at FROM twitch_clips_upload_queue q JOIN twitch_clips_social_media c ON c.id = q.clip_id
                 WHERE c.twitch_user_id = $1 AND q.scheduled_at IS NOT NULL AND COALESCE(q.status, '') NOT IN ('failed', 'cancelled')",
            ).bind(&video.twitch_user_id).fetch_all(&mut *tx).await?;
            autopilot_settings::next_shared_slot(now, &taken, &settings)?
                .ok_or(ProducedVideoError::NoSlot)?
        }
    };
    let mut queue_ids = Vec::new();
    for platform in selected {
        let existing: Option<i64> = sqlx::query_scalar("SELECT id::bigint FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND platform = $2 FOR UPDATE")
            .bind(id).bind(&platform).fetch_optional(&mut *tx).await?;
        let queue_id = match existing {
            Some(queue_id) => queue_id,
            None => {
                if scheduled_at <= now {
                    return Err(ProducedVideoError::State);
                }
                sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at, title, description, hashtags)
                    VALUES ($1, $2, $3, 'waiting_exact_file', $4,
                        $5::text::jsonb->'platforms'->$3->>'title', $5::text::jsonb->'platforms'->$3->>'description',
                        ($5::text::jsonb->'platforms'->$3->'hashtags')::text) RETURNING id::bigint")
                    .bind(video.clip_id).bind(id).bind(&platform).bind(scheduled_at).bind(serde_json::to_string(&video.metadata)?)
                    .fetch_one(&mut *tx).await?
            }
        };
        queue_ids.push((platform, queue_id));
    }
    sqlx::query(
        "UPDATE social_produced_videos SET status = 'scheduled', scheduled_at = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(scheduled_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PublishingReservation {
        produced_video_id: id,
        scheduled_at,
        queue_ids,
    })
}
