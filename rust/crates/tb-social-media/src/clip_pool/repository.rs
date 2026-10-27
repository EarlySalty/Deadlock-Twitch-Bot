use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{postgres::PgRow, PgConnection, PgPool, Row};

use super::{bounds, eligible, ImportWindow, PoolError};
use crate::autopilot_settings::{self, AutopilotSettings, ClipDateFilter, ClipSort};
use crate::clip::model::ClipRecord;
use crate::production_jobs::autopilot_schedule::Reservation;
use crate::production_jobs::{JobSource, ProductionMode};

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
        OR social_clip_video_holds(p.clip_db_id)
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
        OR social_clip_video_holds($1)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEligibility {
    Automatic,
    Manual,
    LegacyNonDeadlockVisible,
}

impl SourceEligibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Manual => "manual",
            Self::LegacyNonDeadlockVisible => "legacy_non_deadlock_visible",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, PoolError> {
        match raw {
            "automatic" => Ok(Self::Automatic),
            "manual" => Ok(Self::Manual),
            "legacy_non_deadlock_visible" => Ok(Self::LegacyNonDeadlockVisible),
            _ => Err(PoolError::InvalidFilter),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SourceJob {
    pub id: i64,
    pub source: JobSource,
    pub requested_mode: Option<ProductionMode>,
    pub mode: Option<ProductionMode>,
    pub status: String,
    pub stage: String,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub reservations: Vec<Reservation>,
}

#[derive(Debug, Serialize)]
pub struct SourceVersion {
    pub id: i64,
    pub job_id: Option<i64>,
    pub mode: ProductionMode,
    pub version_no: i32,
    pub parent_version_id: Option<i64>,
    pub variant_index: i32,
    pub status: String,
    pub needs_review: bool,
    pub duration_ms: Option<i64>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SourceDestination {
    pub queue_id: i64,
    pub platform: String,
    pub status: Option<String>,
    pub produced_video_id: Option<i64>,
    pub scheduled_at: Option<String>,
    pub attempts: Option<i32>,
    pub last_error: Option<String>,
    pub external_id: Option<String>,
    pub outcome_reason: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LegacyPublication {
    pub platform: &'static str,
    pub external_id: String,
}

#[derive(Debug, Serialize)]
pub struct PipelineSource {
    pub clip_id: String,
    pub clip_db_id: Option<i64>,
    pub source_kind: String,
    pub eligibility: SourceEligibility,
    pub pooled: bool,
    pub auto_post: bool,
    pub source_created_at: DateTime<Utc>,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub source_metadata: Value,
    pub view_count: Option<i32>,
    pub selected_at: Option<DateTime<Utc>>,
    pub processed_at: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
    pub legacy_reserved: bool,
    pub discarded_at: Option<DateTime<Utc>>,
    pub jobs: Vec<SourceJob>,
    pub versions: Vec<SourceVersion>,
    pub destinations: Vec<SourceDestination>,
    pub legacy_publications: Vec<LegacyPublication>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct EligibilityCounts {
    pub automatic: i64,
    pub manual: i64,
    pub legacy_non_deadlock_visible: i64,
}

impl EligibilityCounts {
    fn total(&self, filter: Option<SourceEligibility>) -> i64 {
        match filter {
            None => self.automatic + self.manual + self.legacy_non_deadlock_visible,
            Some(SourceEligibility::Automatic) => self.automatic,
            Some(SourceEligibility::Manual) => self.manual,
            Some(SourceEligibility::LegacyNonDeadlockVisible) => self.legacy_non_deadlock_visible,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PipelinePage {
    pub sources: Vec<PipelineSource>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub eligibility: Option<SourceEligibility>,
    pub counts: EligibilityCounts,
    pub preferences: PoolPreferences,
    pub latest_import: Option<super::ImportJob>,
}

const PIPELINE_SOURCES: &str = "WITH sources AS (
    SELECT p.clip_id, p.clip_db_id, COALESCE(c.source_kind, 'twitch') AS source_kind, 'automatic'::text AS eligibility,
        TRUE AS pooled, p.source_created_at, p.game_id,
        COALESCE(NULLIF(c.game_name, ''), 'Deadlock') AS game_name, p.source_metadata, c.view_count, p.selected_at,
        p.processed_at, p.published_at, p.legacy_reserved, c.discarded_at,
        c.youtube_video_id, c.tiktok_video_id, c.instagram_media_id
    FROM social_clip_pool p LEFT JOIN twitch_clips_social_media c ON c.id = p.clip_db_id
    WHERE p.twitch_user_id = $1
    UNION ALL
    SELECT c.clip_id, c.id, c.source_kind,
        CASE WHEN c.source_kind = 'manual_upload' THEN 'manual' WHEN c.category_key = 'deadlock' THEN 'automatic'
            ELSE 'legacy_non_deadlock_visible' END,
        FALSE, c.created_at, c.game_id,
        COALESCE(NULLIF(c.game_name, ''), CASE WHEN c.category_key = 'deadlock' THEN 'Deadlock' END),
        jsonb_build_object('clip_url', c.clip_url, 'clip_title', c.clip_title, 'thumbnail_url', c.clip_thumbnail_url,
            'duration_seconds', c.duration_seconds, 'streamer_login', c.streamer_login),
        c.view_count, NULL::timestamptz, NULL::timestamptz, NULL::timestamptz, c.pool_legacy_reserved, c.discarded_at,
        c.youtube_video_id, c.tiktok_video_id, c.instagram_media_id
    FROM twitch_clips_social_media c
    WHERE c.twitch_user_id = $1 AND NOT EXISTS (SELECT 1 FROM social_clip_pool p
        WHERE p.twitch_user_id = $1 AND (p.clip_id = c.clip_id OR p.clip_db_id = c.id)))";

type Grouped<T> = HashMap<i64, Vec<T>>;

pub async fn pipeline_sources(
    pool: &PgPool,
    owner: &str,
    eligibility: Option<SourceEligibility>,
    limit: i64,
    offset: i64,
) -> Result<PipelinePage, PoolError> {
    if !(1..=200).contains(&limit) || offset < 0 {
        return Err(PoolError::InvalidFilter);
    }
    let settings = autopilot_settings::load(pool, owner).await?;
    let latest_import = super::latest_import(pool, owner).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let counts = eligibility_counts(&mut tx, owner).await?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!("{PIPELINE_SOURCES}
        SELECT * FROM sources WHERE $2::text IS NULL OR eligibility = $2
        ORDER BY CASE WHEN $5 THEN source_created_at END DESC, CASE WHEN NOT $5 THEN source_created_at END ASC, clip_id ASC
        LIMIT $3 OFFSET $4")))
        .bind(owner).bind(eligibility.map(SourceEligibility::as_str)).bind(limit).bind(offset)
        .bind(settings.clip_sort == ClipSort::NewestFirst)
        .fetch_all(&mut *tx).await?;
    let mut sources = rows
        .iter()
        .map(source_from_row)
        .collect::<Result<Vec<_>, PoolError>>()?;
    attach_children(&mut tx, owner, &mut sources).await?;
    tx.commit().await?;
    Ok(PipelinePage {
        sources,
        total: counts.total(eligibility),
        limit,
        offset,
        eligibility,
        counts,
        preferences: PoolPreferences::from(&settings),
        latest_import,
    })
}

async fn eligibility_counts(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<EligibilityCounts, PoolError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{PIPELINE_SOURCES} SELECT eligibility, COUNT(*) FROM sources GROUP BY eligibility"
    )))
    .bind(owner)
    .fetch_all(connection)
    .await?;
    let mut counts = EligibilityCounts::default();
    for (eligibility, count) in rows {
        match SourceEligibility::parse(&eligibility)? {
            SourceEligibility::Automatic => counts.automatic = count,
            SourceEligibility::Manual => counts.manual = count,
            SourceEligibility::LegacyNonDeadlockVisible => {
                counts.legacy_non_deadlock_visible = count
            }
        }
    }
    Ok(counts)
}

fn legacy_publications(row: &PgRow) -> Result<Vec<LegacyPublication>, sqlx::Error> {
    let mut publications = Vec::new();
    for (platform, column) in [
        ("youtube", "youtube_video_id"),
        ("tiktok", "tiktok_video_id"),
        ("instagram", "instagram_media_id"),
    ] {
        if let Some(external_id) = row.try_get::<Option<String>, _>(column)? {
            if is_unpublished_draft(platform, &external_id) {
                continue;
            }
            publications.push(LegacyPublication {
                platform,
                external_id,
            });
        }
    }
    Ok(publications)
}

fn is_unpublished_draft(platform: &str, external_id: &str) -> bool {
    platform == "tiktok" && external_id.starts_with("v_inbox_")
}

fn source_from_row(row: &PgRow) -> Result<PipelineSource, PoolError> {
    let eligibility = SourceEligibility::parse(&row.try_get::<String, _>("eligibility")?)?;
    let pooled: bool = row.try_get("pooled")?;
    Ok(PipelineSource {
        clip_id: row.try_get("clip_id")?,
        clip_db_id: row.try_get("clip_db_id")?,
        source_kind: row.try_get("source_kind")?,
        eligibility,
        pooled,
        auto_post: pooled && eligibility == SourceEligibility::Automatic,
        source_created_at: row.try_get("source_created_at")?,
        game_id: row.try_get("game_id")?,
        game_name: row.try_get("game_name")?,
        source_metadata: row.try_get("source_metadata")?,
        view_count: row.try_get("view_count")?,
        selected_at: row.try_get("selected_at")?,
        processed_at: row.try_get("processed_at")?,
        published_at: row.try_get("published_at")?,
        legacy_reserved: row.try_get("legacy_reserved")?,
        discarded_at: row.try_get("discarded_at")?,
        jobs: Vec::new(),
        versions: Vec::new(),
        destinations: Vec::new(),
        legacy_publications: legacy_publications(row)?,
    })
}

async fn attach_children(
    connection: &mut PgConnection,
    owner: &str,
    sources: &mut [PipelineSource],
) -> Result<(), PoolError> {
    let ids: Vec<i64> = sources
        .iter()
        .filter_map(|source| source.clip_db_id)
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let mut jobs = load_jobs(connection, owner, &ids).await?;
    let mut versions = load_versions(connection, owner, &ids).await?;
    let mut destinations = load_destinations(connection, &ids).await?;
    for source in sources.iter_mut() {
        let Some(id) = source.clip_db_id else {
            continue;
        };
        source.jobs = jobs.remove(&id).unwrap_or_default();
        source.versions = versions.remove(&id).unwrap_or_default();
        source.destinations = destinations.remove(&id).unwrap_or_default();
    }
    Ok(())
}

fn optional_mode(raw: Option<String>) -> Result<Option<ProductionMode>, PoolError> {
    raw.as_deref()
        .map(ProductionMode::parse)
        .transpose()
        .map_err(|_| PoolError::InvalidSource)
}

fn job_source(raw: &str) -> Result<JobSource, PoolError> {
    match raw {
        "autopilot" => Ok(JobSource::Autopilot),
        "manual" => Ok(JobSource::Manual),
        "test" => Ok(JobSource::Test),
        _ => Err(PoolError::InvalidSource),
    }
}

async fn load_jobs(
    connection: &mut PgConnection,
    owner: &str,
    ids: &[i64],
) -> Result<Grouped<SourceJob>, PoolError> {
    let rows = sqlx::query("SELECT id, clip_id, source, requested_mode, mode, status, stage, scheduled_at, created_at
        FROM social_production_jobs WHERE twitch_user_id = $1 AND clip_id = ANY($2) ORDER BY created_at, id")
        .bind(owner).bind(ids).fetch_all(&mut *connection).await?;
    let job_ids = rows
        .iter()
        .map(|row| row.try_get::<i64, _>("id"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut reservations = load_reservations(connection, owner, &job_ids).await?;
    let mut grouped = Grouped::<SourceJob>::new();
    for row in rows {
        let id: i64 = row.try_get("id")?;
        grouped
            .entry(row.try_get("clip_id")?)
            .or_default()
            .push(SourceJob {
                id,
                source: job_source(&row.try_get::<String, _>("source")?)?,
                requested_mode: optional_mode(row.try_get("requested_mode")?)?,
                mode: optional_mode(row.try_get("mode")?)?,
                status: row.try_get("status")?,
                stage: row.try_get("stage")?,
                scheduled_at: row.try_get("scheduled_at")?,
                created_at: row.try_get("created_at")?,
                reservations: reservations.remove(&id).unwrap_or_default(),
            });
    }
    Ok(grouped)
}

async fn load_reservations(
    connection: &mut PgConnection,
    owner: &str,
    job_ids: &[i64],
) -> Result<Grouped<Reservation>, PoolError> {
    let rows: Vec<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT job_id, platform, scheduled_at FROM social_publication_reservations
        WHERE twitch_user_id = $1 AND job_id = ANY($2) AND state = 'reserved' ORDER BY scheduled_at, platform",
    )
    .bind(owner)
    .bind(job_ids)
    .fetch_all(connection)
    .await?;
    let mut grouped = Grouped::<Reservation>::new();
    for (job_id, platform, scheduled_at) in rows {
        grouped.entry(job_id).or_default().push(Reservation {
            platform,
            scheduled_at,
        });
    }
    Ok(grouped)
}

async fn load_versions(
    connection: &mut PgConnection,
    owner: &str,
    ids: &[i64],
) -> Result<Grouped<SourceVersion>, PoolError> {
    let rows = sqlx::query(
        "SELECT id, clip_id, job_id, mode, version_no, parent_version_id, variant_index, status,
        needs_review, duration_ms, scheduled_at, created_at FROM social_produced_videos
        WHERE twitch_user_id = $1 AND clip_id = ANY($2) ORDER BY version_no, variant_index, id",
    )
    .bind(owner)
    .bind(ids)
    .fetch_all(connection)
    .await?;
    let mut grouped = Grouped::<SourceVersion>::new();
    for row in rows {
        let mode: String = row.try_get("mode")?;
        grouped
            .entry(row.try_get("clip_id")?)
            .or_default()
            .push(SourceVersion {
                id: row.try_get("id")?,
                job_id: row.try_get("job_id")?,
                mode: ProductionMode::parse(&mode).map_err(|_| PoolError::InvalidSource)?,
                version_no: row.try_get("version_no")?,
                parent_version_id: row.try_get("parent_version_id")?,
                variant_index: row.try_get("variant_index")?,
                status: row.try_get("status")?,
                needs_review: row.try_get("needs_review")?,
                duration_ms: row.try_get("duration_ms")?,
                scheduled_at: row.try_get("scheduled_at")?,
                created_at: row.try_get("created_at")?,
            });
    }
    Ok(grouped)
}

async fn load_destinations(
    connection: &mut PgConnection,
    ids: &[i64],
) -> Result<Grouped<SourceDestination>, PoolError> {
    let rows = sqlx::query(
        "SELECT q.id, q.clip_id, q.platform, q.status, q.scheduled_at::text AS scheduled_at,
        q.attempts, q.last_error, q.completed_at::text AS completed_at, to_jsonb(q) AS raw
        FROM twitch_clips_upload_queue q WHERE q.clip_id = ANY($1) ORDER BY q.id",
    )
    .bind(ids)
    .fetch_all(connection)
    .await?;
    let mut grouped = Grouped::<SourceDestination>::new();
    for row in rows {
        let raw: Value = row.try_get("raw")?;
        grouped
            .entry(row.try_get("clip_id")?)
            .or_default()
            .push(SourceDestination {
                queue_id: row.try_get("id")?,
                platform: row.try_get("platform")?,
                status: row.try_get("status")?,
                produced_video_id: raw.get("produced_video_id").and_then(Value::as_i64),
                scheduled_at: row.try_get("scheduled_at")?,
                attempts: row.try_get("attempts")?,
                last_error: row.try_get("last_error")?,
                external_id: raw_text(&raw, "external_id"),
                outcome_reason: raw_text(&raw, "outcome_reason"),
                completed_at: row.try_get("completed_at")?,
            });
    }
    Ok(grouped)
}

fn raw_text(raw: &Value, key: &str) -> Option<String> {
    raw.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod draft_tests {
    use super::is_unpublished_draft;

    #[test]
    fn tiktok_inbox_ids_are_not_publications() {
        assert!(is_unpublished_draft(
            "tiktok",
            "v_inbox_file~v2.7699546714403031062"
        ));
        assert!(!is_unpublished_draft("tiktok", "7699546714403031062"));
        assert!(!is_unpublished_draft("youtube", "v_inbox_file~x"));
    }
}
