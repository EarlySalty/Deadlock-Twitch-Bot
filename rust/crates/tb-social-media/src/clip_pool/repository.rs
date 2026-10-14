use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgConnection, PgPool, Row};

use super::{bounds, eligible, ImportWindow, PoolError};
use crate::autopilot_settings::{self, AutopilotSettings, ClipDateFilter, ClipSort};
use crate::clip::model::ClipRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolPreferences {
    pub clip_date_filter: ClipDateFilter,
    pub clip_sort: ClipSort,
    pub timezone: String,
}

impl From<&AutopilotSettings> for PoolPreferences {
    fn from(settings: &AutopilotSettings) -> Self {
        Self {
            clip_date_filter: settings.clip_date_filter.clone(),
            clip_sort: settings.clip_sort,
            timezone: settings.timezone.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PoolClip {
    pub clip_id: String,
    pub clip_db_id: Option<i64>,
    pub source_created_at: DateTime<Utc>,
    pub game_id: String,
    pub source_metadata: Value,
    pub view_count: Option<i32>,
    pub selected_at: Option<DateTime<Utc>>,
    pub first_selected_at: Option<DateTime<Utc>>,
    pub processed_at: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
    pub legacy_reserved: bool,
    pub eligible: bool,
    pub selectable: bool,
}

#[derive(Debug, Serialize)]
pub struct PoolPage {
    pub clips: Vec<PoolClip>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub preferences: PoolPreferences,
}

pub async fn legacy_automatic_allowed(pool: &PgPool, clip_id: i64) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, bool>("SELECT to_jsonb(c)->>'pool_managed_at' IS NULL FROM twitch_clips_social_media c WHERE id = $1")
        .bind(clip_id).fetch_optional(pool).await?.unwrap_or(false))
}

pub async fn is_managed(pool: &PgPool, owner: &str) -> Result<bool, PoolError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_clip_pool_owners WHERE twitch_user_id = $1)",
    )
    .bind(owner)
    .fetch_one(pool)
    .await?)
}

pub(crate) async fn manage_on(connection: &mut PgConnection, owner: &str) -> Result<(), PoolError> {
    sqlx::query("SELECT social_clip_manage_owner($1)")
        .bind(owner)
        .execute(connection)
        .await?;
    Ok(())
}

pub async fn save_preferences(
    pool: &PgPool,
    owner: &str,
    preferences: &PoolPreferences,
) -> Result<PoolPreferences, PoolError> {
    let settings = AutopilotSettings {
        clip_date_filter: preferences.clip_date_filter.clone(),
        clip_sort: preferences.clip_sort,
        timezone: preferences.timezone.clone(),
        ..Default::default()
    };
    bounds(&settings, Utc::now())?;
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    sqlx::query("INSERT INTO social_media_autopilot_settings (twitch_user_id, clip_date_filter, clip_sort, timezone)
        VALUES ($1, $2, $3, $4) ON CONFLICT (twitch_user_id) DO UPDATE SET clip_date_filter = EXCLUDED.clip_date_filter,
        clip_sort = EXCLUDED.clip_sort, timezone = EXCLUDED.timezone, updated_at = NOW()")
        .bind(owner).bind(serde_json::to_value(&preferences.clip_date_filter)?)
        .bind(match preferences.clip_sort { ClipSort::OldestFirst => "oldest_first", ClipSort::NewestFirst => "newest_first" })
        .bind(&preferences.timezone).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(PoolPreferences::from(&settings))
}

pub(crate) async fn register_on(
    connection: &mut PgConnection,
    clip: &ClipRecord,
    window: &ImportWindow,
) -> Result<Option<(i64, bool)>, PoolError> {
    if !eligible(&clip.twitch_user_id, clip, window)? {
        return Ok(None);
    }
    let created_at = DateTime::parse_from_rfc3339(&clip.created_at)
        .map_err(|_| PoolError::InvalidSource)?
        .with_timezone(&Utc);
    let view_count = i32::try_from(clip.view_count).map_err(|_| PoolError::InvalidSource)?;
    let (id, _) = crate::clip::repository::register_on(
        connection, clip, created_at, view_count, "deadlock", true,
    )
    .await?;
    crate::clip::repository::initialize_layout_on(connection, id, &clip.twitch_user_id).await?;
    let inserted: bool = sqlx::query_scalar("INSERT INTO social_clip_pool AS p
        (twitch_user_id, clip_id, clip_db_id, source_created_at, game_id, source_metadata)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (twitch_user_id, clip_id) DO UPDATE SET clip_db_id = EXCLUDED.clip_db_id
        RETURNING (xmax = 0)")
        .bind(&clip.twitch_user_id).bind(&clip.clip_id).bind(id).bind(created_at).bind(super::DEADLOCK_GAME_ID)
        .bind(serde_json::json!({"clip_url": clip.clip_url, "clip_title": clip.clip_title,
            "thumbnail_url": clip.thumbnail_url, "duration_seconds": clip.duration_seconds, "streamer_login": clip.streamer_login}))
        .fetch_one(&mut *connection).await?;
    sqlx::query(
        "SELECT social_clip_capture_history(c) FROM twitch_clips_social_media c WHERE c.id = $1",
    )
    .bind(id)
    .execute(connection)
    .await?;
    Ok(Some((id, inserted)))
}

pub async fn register_recent(
    pool: &PgPool,
    owner: &str,
    clips: &[ClipRecord],
) -> Result<(i32, i32), PoolError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let window = bounds(&settings, Utc::now())?;
    let mut eligible_count = 0;
    let mut inserted_count = 0;
    for clip in clips {
        if eligible(owner, clip, &window)? {
            if let Some((_, inserted)) = register_on(&mut tx, clip, &window).await? {
                eligible_count += 1;
                inserted_count += i32::from(inserted);
            }
        }
    }
    tx.commit().await?;
    Ok((eligible_count, inserted_count))
}

pub async fn list(
    pool: &PgPool,
    owner: &str,
    limit: i64,
    offset: i64,
) -> Result<PoolPage, PoolError> {
    if !(1..=200).contains(&limit) || offset < 0 {
        return Err(PoolError::InvalidFilter);
    }
    let settings = autopilot_settings::load(pool, owner).await?;
    let window = bounds(&settings, Utc::now())?;
    let total =
        sqlx::query_scalar("SELECT COUNT(*) FROM social_clip_pool WHERE twitch_user_id = $1")
            .bind(owner)
            .fetch_one(pool)
            .await?;
    let newest = settings.clip_sort == ClipSort::NewestFirst;
    let rows = sqlx::query("SELECT p.*, p.source_created_at >= $4 AND p.source_created_at < $5 AS eligible,
        EXISTS(SELECT 1 FROM social_production_jobs j WHERE j.clip_id = p.clip_db_id AND j.status <> 'cancelled')
        OR EXISTS(SELECT 1 FROM social_produced_videos v WHERE v.clip_id = p.clip_db_id)
        OR EXISTS(SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = p.clip_db_id) AS reserved,
        (SELECT c.view_count FROM twitch_clips_social_media c WHERE c.id = p.clip_db_id) AS view_count
        FROM social_clip_pool p WHERE p.twitch_user_id = $1
        ORDER BY CASE WHEN $6 THEN p.source_created_at END DESC, CASE WHEN NOT $6 THEN p.source_created_at END ASC, p.clip_id ASC
        LIMIT $2 OFFSET $3")
        .bind(owner).bind(limit).bind(offset).bind(window.start).bind(window.end).bind(newest).fetch_all(pool).await?;
    let clips = rows
        .into_iter()
        .map(|row| {
            let processed_at: Option<DateTime<Utc>> = row.try_get("processed_at")?;
            let published_at: Option<DateTime<Utc>> = row.try_get("published_at")?;
            let eligible: bool = row.try_get("eligible")?;
            let reserved: bool = row.try_get("reserved")?;
            let legacy_reserved: bool = row.try_get("legacy_reserved")?;
            Ok(PoolClip {
                clip_id: row.try_get("clip_id")?,
                clip_db_id: row.try_get("clip_db_id")?,
                source_created_at: row.try_get("source_created_at")?,
                game_id: row.try_get("game_id")?,
                source_metadata: row.try_get("source_metadata")?,
                view_count: row.try_get("view_count")?,
                selected_at: row.try_get("selected_at")?,
                first_selected_at: row.try_get("first_selected_at")?,
                processed_at,
                published_at,
                legacy_reserved,
                eligible,
                selectable: eligible
                    && processed_at.is_none()
                    && published_at.is_none()
                    && !reserved
                    && !legacy_reserved,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(PoolPage {
        clips,
        total,
        limit,
        offset,
        preferences: PoolPreferences::from(&settings),
    })
}

pub async fn set_selected(
    pool: &PgPool,
    owner: &str,
    clip_id: &str,
    selected: bool,
) -> Result<(), PoolError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let window = bounds(&settings, Utc::now())?;
    let row = sqlx::query(
        "SELECT * FROM social_clip_pool WHERE twitch_user_id = $1 AND clip_id = $2 FOR UPDATE",
    )
    .bind(owner)
    .bind(clip_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(PoolError::NotFound)?;
    let source_id: Option<i64> = row.try_get("clip_db_id")?;
    let protected: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_production_jobs WHERE clip_id = $1 AND status <> 'cancelled')
        OR EXISTS(SELECT 1 FROM social_produced_videos WHERE clip_id = $1)
        OR EXISTS(SELECT 1 FROM twitch_clips_upload_queue WHERE clip_id = $1)")
        .bind(source_id).fetch_one(&mut *tx).await?;
    let processed_at: Option<DateTime<Utc>> = row.try_get("processed_at")?;
    let published_at: Option<DateTime<Utc>> = row.try_get("published_at")?;
    let legacy_reserved: bool = row.try_get("legacy_reserved")?;
    if protected || processed_at.is_some() || published_at.is_some() || legacy_reserved {
        return Err(if selected {
            PoolError::NotSelectable
        } else {
            PoolError::Reserved
        });
    }
    let created_at: DateTime<Utc> = row.try_get("source_created_at")?;
    if selected && (created_at < window.start || created_at >= window.end) {
        return Err(PoolError::NotSelectable);
    }
    sqlx::query("UPDATE social_clip_pool SET selected_at = CASE WHEN $3 THEN COALESCE(selected_at, NOW()) END,
        first_selected_at = CASE WHEN $3 THEN COALESCE(first_selected_at, NOW()) ELSE first_selected_at END
        WHERE twitch_user_id = $1 AND clip_id = $2")
        .bind(owner).bind(clip_id).bind(selected).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
