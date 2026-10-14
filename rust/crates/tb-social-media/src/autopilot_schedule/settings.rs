use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};

use super::{taken_on, ScheduleError};
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

fn is_slot(settings: &AutopilotSettings, at: DateTime<Utc>) -> Result<bool, ScheduleError> {
    let before = at - chrono::Duration::seconds(1);
    Ok(autopilot_settings::next_shared_slot(before, &[], settings)? == Some(at))
}

type ScheduledVideo = (i64, Option<i64>, DateTime<Utc>);

async fn move_video_on(
    connection: &mut PgConnection,
    video: &ScheduledVideo,
    slot: Option<DateTime<Utc>>,
) -> Result<(), ScheduleError> {
    sqlx::query(
        "UPDATE social_produced_videos SET scheduled_at = $2,
        status = CASE WHEN $2 IS NULL AND status = 'scheduled' THEN 'approved' ELSE status END
        WHERE id = $1",
    )
    .bind(video.0)
    .bind(slot)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE social_production_jobs SET scheduled_at = $2
        WHERE id = $1 AND status = 'completed' AND scheduled_at = $3",
    )
    .bind(video.1)
    .bind(slot)
    .bind(video.2)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET scheduled_at = $2
        WHERE produced_video_id = $1 AND scheduled_at IS NOT NULL",
    )
    .bind(video.0)
    .bind(slot)
    .execute(connection)
    .await?;
    Ok(())
}

async fn release_queued_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Vec<i64>, ScheduleError> {
    let queued: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM social_production_jobs WHERE twitch_user_id = $1 AND source = 'autopilot'
        AND status = 'queued' AND started_at IS NULL ORDER BY queue_order, id FOR UPDATE",
    )
    .bind(owner)
    .fetch_all(&mut *connection)
    .await?;
    sqlx::query("UPDATE social_production_jobs SET scheduled_at = NULL WHERE id = ANY($1)")
        .bind(&queued)
        .execute(connection)
        .await?;
    Ok(queued)
}

async fn stale_videos_on(
    connection: &mut PgConnection,
    owner: &str,
    settings: &AutopilotSettings,
    now: DateTime<Utc>,
) -> Result<Vec<ScheduledVideo>, ScheduleError> {
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
    .fetch_all(connection)
    .await?;
    let mut stale = Vec::new();
    for video in videos {
        if video.2 <= now || !is_slot(settings, video.2)? {
            stale.extend([video]);
        }
    }
    Ok(stale)
}

async fn retime_on(
    connection: &mut PgConnection,
    owner: &str,
    settings: &AutopilotSettings,
) -> Result<(), ScheduleError> {
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *connection)
        .await?;
    let queued = release_queued_on(connection, owner).await?;
    let stale = stale_videos_on(connection, owner, settings, now).await?;
    let mut taken = taken_on(connection, owner).await?;
    for video in stale {
        let slot = autopilot_settings::next_shared_slot(now, &taken, settings)?;
        move_video_on(connection, &video, slot).await?;
        taken.extend(slot);
    }
    for id in queued {
        let slot = autopilot_settings::next_shared_slot(now, &taken, settings)?;
        sqlx::query("UPDATE social_production_jobs SET scheduled_at = $2 WHERE id = $1")
            .bind(id)
            .bind(slot)
            .execute(&mut *connection)
            .await?;
        taken.extend(slot);
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
    if previous.schedule_slots != settings.schedule_slots || previous.timezone != settings.timezone
    {
        retime_on(&mut tx, owner, &settings).await?;
    }
    tx.commit().await?;
    Ok(ScheduleSettings::from(&settings))
}
