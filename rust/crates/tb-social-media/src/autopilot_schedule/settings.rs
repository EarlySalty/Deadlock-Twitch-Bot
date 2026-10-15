use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};

use super::{
    place_video_on, queued_platforms_on, reservations_on, reserve_on, Allocator, Reservation,
    ScheduleError, Subject,
};
use crate::autopilot_settings::{self, AutopilotSettings, ScheduleSlot};

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleSettings {
    pub enabled: bool,
    pub paused: bool,
    pub ai_editing_enabled: bool,
    pub lead_time_hours: i32,
    pub timezone: String,
    pub schedule_slots: Vec<ScheduleSlot>,
    pub target_platforms: Vec<String>,
}

impl From<&AutopilotSettings> for ScheduleSettings {
    fn from(settings: &AutopilotSettings) -> Self {
        Self {
            enabled: settings.enabled,
            paused: settings.paused,
            ai_editing_enabled: settings.ai_editing_enabled,
            lead_time_hours: settings.lead_time_hours,
            timezone: settings.timezone.clone(),
            schedule_slots: settings.schedule_slots.clone(),
            target_platforms: settings.target_platforms.clone(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulePatch {
    pub enabled: Option<bool>,
    pub paused: Option<bool>,
    pub ai_editing_enabled: Option<bool>,
    pub lead_time_hours: Option<i32>,
    pub timezone: Option<String>,
    pub schedule_slots: Option<Vec<ScheduleSlot>>,
    pub target_platforms: Option<Vec<String>>,
}

impl SchedulePatch {
    fn apply(&self, settings: &mut AutopilotSettings) -> Result<(), ScheduleError> {
        if let Some(value) = self.enabled {
            settings.enabled = value;
        }
        if let Some(value) = self.paused {
            settings.paused = value;
        }
        if let Some(value) = self.ai_editing_enabled {
            settings.ai_editing_enabled = value;
        }
        if let Some(value) = self.lead_time_hours {
            if !(0..=720).contains(&value) {
                return Err(ScheduleError::Invalid(
                    "Vorlauf muss zwischen 0 und 720 Stunden liegen".into(),
                ));
            }
            settings.lead_time_hours = value;
        }
        if let Some(value) = &self.timezone {
            settings.timezone = value.clone();
        }
        if let Some(value) = &self.schedule_slots {
            if value.len() > 28 {
                return Err(ScheduleError::Invalid(
                    "Höchstens 28 wöchentliche Termine sind möglich".into(),
                ));
            }
            settings.schedule_slots = value.clone();
            settings
                .schedule_slots
                .sort_by(|a, b| (a.weekday, &a.local_time).cmp(&(b.weekday, &b.local_time)));
            settings.schedule_slots.dedup();
        }
        if let Some(value) = &self.target_platforms {
            settings.target_platforms = autopilot_settings::normalize_platforms(value)?;
        }
        settings.validate()?;
        Ok(())
    }
}

async fn require_connected(
    connection: &mut PgConnection,
    owner: &str,
    platforms: &[String],
) -> Result<(), ScheduleError> {
    let connected = crate::autopilot_publish::connections_on(connection, owner).await?;
    if let Some(missing) = platforms
        .iter()
        .find(|platform| !connected.contains_key(platform.as_str()))
    {
        return Err(ScheduleError::Invalid(format!(
            "{missing} ist nicht verbunden. Verbinde die Plattform zuerst"
        )));
    }
    Ok(())
}

type ScheduledVideo = (i64, Option<i64>, DateTime<Utc>);

async fn unschedule_video_on(
    connection: &mut PgConnection,
    video: &ScheduledVideo,
) -> Result<(), ScheduleError> {
    sqlx::query(
        "UPDATE social_produced_videos SET scheduled_at = NULL,
        status = CASE WHEN status = 'scheduled' THEN 'approved' ELSE status END
        WHERE id = $1",
    )
    .bind(video.0)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE social_production_jobs SET scheduled_at = NULL
        WHERE id = $1 AND status = 'completed' AND scheduled_at = $2",
    )
    .bind(video.1)
    .bind(video.2)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET scheduled_at = NULL
        WHERE produced_video_id = $1 AND scheduled_at IS NOT NULL",
    )
    .bind(video.0)
    .execute(connection)
    .await?;
    Ok(())
}

async fn release_queued_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Vec<(i64, Vec<Reservation>)>, ScheduleError> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM social_production_jobs WHERE twitch_user_id = $1 AND source = 'autopilot'
        AND status = 'queued' AND started_at IS NULL ORDER BY queue_order, id FOR UPDATE",
    )
    .bind(owner)
    .fetch_all(&mut *connection)
    .await?;
    let mut queued = Vec::with_capacity(ids.len());
    for id in ids {
        let previous = reservations_on(connection, id).await?;
        queued.extend([(id, previous)]);
    }
    sqlx::query("UPDATE social_production_jobs SET scheduled_at = NULL WHERE id = ANY($1)")
        .bind(queued.iter().map(|job| job.0).collect::<Vec<_>>())
        .execute(connection)
        .await?;
    Ok(queued)
}

async fn stale_videos_on(
    connection: &mut PgConnection,
    owner: &str,
    allocator: &Allocator,
    now: DateTime<Utc>,
) -> Result<Vec<(ScheduledVideo, Vec<Reservation>)>, ScheduleError> {
    let videos: Vec<ScheduledVideo> = sqlx::query_as(
        "SELECT v.id, v.job_id, v.scheduled_at FROM social_produced_videos v
        WHERE v.twitch_user_id = $1 AND v.scheduled_at IS NOT NULL AND v.publication_executed_at IS NULL
        AND v.status IN ('review', 'approved', 'scheduled')
        AND NOT EXISTS (SELECT 1 FROM social_production_jobs j WHERE j.id = v.job_id
            AND j.twitch_user_id = v.twitch_user_id AND j.status IN ('queued', 'running')
            AND j.started_at IS NOT NULL AND j.finished_at IS NULL)
        ORDER BY v.scheduled_at, v.id FOR UPDATE OF v",
    )
    .bind(owner)
    .fetch_all(&mut *connection)
    .await?;
    let mut stale = Vec::new();
    for video in videos {
        let reservations = match video.1 {
            Some(job) => reservations_on(connection, job).await?,
            None => Vec::new(),
        };
        if video.2 <= now || !allocator.holds(video.2, &reservations)? {
            stale.extend([(video, reservations)]);
        }
    }
    Ok(stale)
}

pub(crate) async fn retime_on(
    connection: &mut PgConnection,
    owner: &str,
    settings: &AutopilotSettings,
) -> Result<(), ScheduleError> {
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *connection)
        .await?;
    let queued = release_queued_on(connection, owner).await?;
    let probe = Allocator::load_on(connection, owner, settings).await?;
    let stale = stale_videos_on(connection, owner, &probe, now).await?;
    for (video, previous) in stale {
        let subject = Subject {
            job: video.1,
            video: Some(video.0),
        };
        let mut allocator = Allocator::load_for(connection, owner, settings, subject).await?;
        let queued_platforms = queued_platforms_on(connection, video.0).await?;
        match allocator.place_video(now, &previous, &queued_platforms)? {
            Some(allocation) => place_video_on(connection, owner, video.0, &allocation).await?,
            None => unschedule_video_on(connection, &video).await?,
        }
    }
    let mut allocator = Allocator::load_on(connection, owner, settings).await?;
    for (id, previous) in queued {
        let allocation = allocator.plan_job(now, &previous)?;
        reserve_on(connection, owner, id, allocation.as_ref()).await?;
        if let Some(allocation) = allocation {
            allocator.commit(&allocation);
        }
    }
    Ok(())
}

pub async fn save_schedule(
    pool: &PgPool,
    owner: &str,
    patch: &SchedulePatch,
) -> Result<ScheduleSettings, ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let mut settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let previous = settings.clone();
    patch.apply(&mut settings)?;
    if patch.target_platforms.is_some() {
        require_connected(&mut tx, owner, &settings.target_platforms).await?;
    }
    sqlx::query("INSERT INTO social_media_autopilot_settings
        (twitch_user_id, enabled, paused, ai_editing_enabled, lead_time_hours, timezone, schedule_slots, target_platforms)
        VALUES ($1, $2, $3, $4, $5, $6, $7::text::jsonb, $8::text::jsonb)
        ON CONFLICT (twitch_user_id) DO UPDATE SET enabled = EXCLUDED.enabled, paused = EXCLUDED.paused,
        ai_editing_enabled = EXCLUDED.ai_editing_enabled, lead_time_hours = EXCLUDED.lead_time_hours,
        timezone = EXCLUDED.timezone, schedule_slots = EXCLUDED.schedule_slots,
        target_platforms = EXCLUDED.target_platforms, updated_at = NOW()")
        .bind(owner).bind(settings.enabled).bind(settings.paused).bind(settings.ai_editing_enabled)
        .bind(settings.lead_time_hours).bind(&settings.timezone).bind(serde_json::to_string(&settings.schedule_slots)?)
        .bind(serde_json::to_string(&settings.target_platforms)?)
        .execute(&mut *tx).await?;
    if previous.schedule_slots != settings.schedule_slots
        || previous.timezone != settings.timezone
        || previous.target_platforms != settings.target_platforms
    {
        retime_on(&mut tx, owner, &settings).await?;
    }
    tx.commit().await?;
    Ok(ScheduleSettings::from(&settings))
}
