use chrono::{DateTime, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use super::PoolError;
use crate::autopilot_settings::{AutopilotSettings, ClipDateFilter};

pub const DEADLOCK_GAME_ID: &str = "2132205352";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    #[serde(default)]
    pub fetched: i64,
    #[serde(default)]
    pub cursor_restarts: u32,
    #[serde(default)]
    pub pages: u32,
}

impl ImportWindow {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            start,
            end,
            fetched: 0,
            cursor_restarts: 0,
            pages: 0,
        }
    }

    pub fn split(&self) -> Option<[Self; 2]> {
        let seconds = (self.end - self.start).num_seconds();
        if seconds <= 1 {
            return None;
        }
        let middle = self.start + Duration::seconds(seconds / 2);
        Some([Self::new(self.start, middle), Self::new(middle, self.end)])
    }
}

fn midnight(date: NaiveDate, timezone: Tz) -> Result<DateTime<Utc>, PoolError> {
    let start = date.and_hms_opt(0, 0, 0).ok_or(PoolError::InvalidFilter)?;
    for minute in 0..=1440 {
        let local = start
            .checked_add_signed(Duration::minutes(minute))
            .ok_or(PoolError::InvalidFilter)?;
        match timezone.from_local_datetime(&local) {
            LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
                return Ok(value.with_timezone(&Utc))
            }
            LocalResult::None => {}
        }
    }
    Err(PoolError::InvalidFilter)
}

fn day(year: i32, month: u32) -> Result<NaiveDate, PoolError> {
    NaiveDate::from_ymd_opt(year, month, 1).ok_or(PoolError::InvalidFilter)
}

pub fn bounds(settings: &AutopilotSettings, now: DateTime<Utc>) -> Result<ImportWindow, PoolError> {
    settings.validate()?;
    let timezone: Tz = settings
        .timezone
        .parse()
        .map_err(|_| PoolError::InvalidFilter)?;
    let epoch = DateTime::from_timestamp(0, 0).ok_or(PoolError::InvalidFilter)?;
    let (start, end) = match settings.clip_date_filter {
        ClipDateFilter::All => (epoch, now),
        ClipDateFilter::Year { year } => (
            midnight(day(year, 1)?, timezone)?,
            midnight(
                day(year.checked_add(1).ok_or(PoolError::InvalidFilter)?, 1)?,
                timezone,
            )?,
        ),
        ClipDateFilter::Quarter { year, quarter } => {
            let month = (u32::from(quarter) - 1) * 3 + 1;
            let next = if month == 10 {
                day(year.checked_add(1).ok_or(PoolError::InvalidFilter)?, 1)?
            } else {
                day(year, month + 3)?
            };
            (
                midnight(day(year, month)?, timezone)?,
                midnight(next, timezone)?,
            )
        }
        ClipDateFilter::Range { start, end } => (
            midnight(start, timezone)?,
            midnight(end.succ_opt().ok_or(PoolError::InvalidFilter)?, timezone)?,
        ),
        ClipDateFilter::Before { date } => (epoch, midnight(date, timezone)?),
        ClipDateFilter::After { date } => (
            midnight(date.succ_opt().ok_or(PoolError::InvalidFilter)?, timezone)?,
            now,
        ),
    };
    let end = end.min(now);
    Ok(ImportWindow::new(start.min(end), end))
}

pub fn eligible(
    owner: &str,
    clip: &crate::clip::model::ClipRecord,
    window: &ImportWindow,
) -> Result<bool, PoolError> {
    if clip.twitch_user_id.is_empty() || clip.game_id.as_deref().is_none_or(str::is_empty) {
        return Err(PoolError::InvalidSource);
    }
    let created_at = DateTime::parse_from_rfc3339(&clip.created_at)
        .map_err(|_| PoolError::InvalidSource)?
        .with_timezone(&Utc);
    Ok(clip.twitch_user_id == owner
        && clip.game_id.as_deref() == Some(DEADLOCK_GAME_ID)
        && created_at >= window.start
        && created_at < window.end)
}
