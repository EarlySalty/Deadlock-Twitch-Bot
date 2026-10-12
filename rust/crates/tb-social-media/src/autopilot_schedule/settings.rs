use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::ScheduleError;
use crate::autopilot_settings::{self, AutopilotSettings, ScheduleSlot};

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleSettings {
    pub enabled: bool,
    pub paused: bool,
    pub ai_editing_enabled: bool,
    pub lead_time_hours: i32,
    pub timezone: String,
    pub schedule_slots: Vec<ScheduleSlot>,
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
        settings.validate()?;
        Ok(())
    }
}

pub async fn save_schedule(
    pool: &PgPool,
    owner: &str,
    patch: &SchedulePatch,
) -> Result<ScheduleSettings, ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let mut settings = autopilot_settings::load_on(&mut tx, owner).await?;
    patch.apply(&mut settings)?;
    sqlx::query("INSERT INTO social_media_autopilot_settings
        (twitch_user_id, enabled, paused, ai_editing_enabled, lead_time_hours, timezone, schedule_slots)
        VALUES ($1, $2, $3, $4, $5, $6, $7::text::jsonb)
        ON CONFLICT (twitch_user_id) DO UPDATE SET enabled = EXCLUDED.enabled, paused = EXCLUDED.paused,
        ai_editing_enabled = EXCLUDED.ai_editing_enabled, lead_time_hours = EXCLUDED.lead_time_hours,
        timezone = EXCLUDED.timezone, schedule_slots = EXCLUDED.schedule_slots, updated_at = NOW()")
        .bind(owner).bind(settings.enabled).bind(settings.paused).bind(settings.ai_editing_enabled)
        .bind(settings.lead_time_hours).bind(&settings.timezone).bind(serde_json::to_string(&settings.schedule_slots)?)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ScheduleSettings::from(&settings))
}
