use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool, Row};

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("Ungültige Auto-Pilot-Einstellung: {0}")]
    Invalid(String),
    #[error("Die Auto-Pilot-Einstellungen konnten nicht gespeichert oder geladen werden")]
    Database(#[from] sqlx::Error),
    #[error("Die gespeicherten Auto-Pilot-Einstellungen sind ungültig")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PublishMode {
    #[default]
    Review,
    FullAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClipSort {
    #[default]
    OldestFirst,
    NewestFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PublishLanguage {
    #[default]
    De,
    En,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipDateFilter {
    #[default]
    All,
    Year {
        year: i32,
    },
    Quarter {
        year: i32,
        quarter: u8,
    },
    Range {
        start: NaiveDate,
        end: NaiveDate,
    },
    Before {
        date: NaiveDate,
    },
    After {
        date: NaiveDate,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSlot {
    pub weekday: u8,
    pub local_time: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationPreferences {
    pub dashboard: bool,
    pub discord: bool,
    pub email: bool,
    pub ready_for_review: bool,
    pub publication_deadline: bool,
    pub processing_failed: bool,
    pub publication_failed: bool,
    pub empty_pool: bool,
    pub review_expiring: bool,
    pub reminder_hours: u32,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            dashboard: true,
            discord: false,
            email: false,
            ready_for_review: true,
            publication_deadline: true,
            processing_failed: true,
            publication_failed: true,
            empty_pool: true,
            review_expiring: true,
            reminder_hours: 24,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutopilotSettings {
    pub enabled: bool,
    pub paused: bool,
    pub ai_editing_enabled: bool,
    pub publish_mode: PublishMode,
    pub lead_time_hours: i32,
    pub timezone: String,
    pub schedule_slots: Vec<ScheduleSlot>,
    pub target_platforms: Vec<String>,
    pub clip_date_filter: ClipDateFilter,
    pub clip_sort: ClipSort,
    pub multi_output_enabled: bool,
    pub experimental_ai_layout: bool,
    pub publish_language: PublishLanguage,
    pub notifications: NotificationPreferences,
}

impl Default for AutopilotSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            ai_editing_enabled: false,
            publish_mode: PublishMode::Review,
            lead_time_hours: 24,
            timezone: "Europe/Berlin".into(),
            schedule_slots: Vec::new(),
            target_platforms: Vec::new(),
            clip_date_filter: ClipDateFilter::All,
            clip_sort: ClipSort::OldestFirst,
            multi_output_enabled: false,
            experimental_ai_layout: false,
            publish_language: PublishLanguage::De,
            notifications: NotificationPreferences::default(),
        }
    }
}

pub fn normalize_platforms(platforms: &[String]) -> Result<Vec<String>, SettingsError> {
    let mut result = Vec::new();
    for platform in platforms {
        let platform = platform.trim().to_lowercase();
        if !crate::posting_plan::PLATFORMS.contains(&platform.as_str()) {
            return Err(SettingsError::Invalid("Unbekannte Zielplattform".into()));
        }
        if !result.contains(&platform) {
            result.push(platform);
        }
    }
    Ok(result)
}

fn slot_time(slot: &ScheduleSlot) -> Result<NaiveTime, SettingsError> {
    let time = NaiveTime::parse_from_str(&slot.local_time, "%H:%M")
        .map_err(|_| SettingsError::Invalid("Uhrzeit muss HH:MM entsprechen".into()))?;
    if !(1..=7).contains(&slot.weekday) || time.format("%H:%M").to_string() != slot.local_time {
        return Err(SettingsError::Invalid(
            "Ungültiger Wochentag oder Uhrzeit".into(),
        ));
    }
    Ok(time)
}

impl AutopilotSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        self.timezone
            .parse::<Tz>()
            .map_err(|_| SettingsError::Invalid("Unbekannte IANA-Zeitzone".into()))?;
        if self.lead_time_hours < 0 {
            return Err(SettingsError::Invalid(
                "Vorlauf darf nicht negativ sein".into(),
            ));
        }
        for slot in &self.schedule_slots {
            slot_time(slot)?;
        }
        normalize_platforms(&self.target_platforms)?;
        match self.clip_date_filter {
            ClipDateFilter::Year { year } | ClipDateFilter::Quarter { year, .. }
                if NaiveDate::from_ymd_opt(year, 1, 1).is_none() =>
            {
                return Err(SettingsError::Invalid("Ungültiges Jahr".into()));
            }
            ClipDateFilter::Quarter { quarter, .. } if !(1..=4).contains(&quarter) => {
                return Err(SettingsError::Invalid("Ungültiges Quartal".into()));
            }
            ClipDateFilter::Range { start, end } if start > end => {
                return Err(SettingsError::Invalid(
                    "Der Beginn liegt nach dem Ende".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

pub async fn load(pool: &PgPool, twitch_user_id: &str) -> Result<AutopilotSettings, SettingsError> {
    let mut connection = pool.acquire().await?;
    load_on(&mut connection, twitch_user_id).await
}

pub(crate) async fn load_on(
    connection: &mut PgConnection,
    twitch_user_id: &str,
) -> Result<AutopilotSettings, SettingsError> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT (to_jsonb(s) - 'twitch_user_id' - 'updated_at')::text FROM social_media_autopilot_settings s WHERE twitch_user_id = $1",
    ).bind(twitch_user_id).fetch_optional(connection).await?;
    let settings = match raw {
        Some(raw) => serde_json::from_str(&raw)?,
        None => AutopilotSettings::default(),
    };
    settings.validate()?;
    Ok(settings)
}

pub async fn save(
    pool: &PgPool,
    twitch_user_id: &str,
    settings: &AutopilotSettings,
) -> Result<AutopilotSettings, SettingsError> {
    settings.validate()?;
    let mut settings = settings.clone();
    settings.target_platforms = normalize_platforms(&settings.target_platforms)?;
    settings
        .schedule_slots
        .sort_by(|a, b| (a.weekday, &a.local_time).cmp(&(b.weekday, &b.local_time)));
    settings.schedule_slots.dedup();
    let raw = serde_json::to_string(&settings)?;
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (
             twitch_user_id, enabled, paused, ai_editing_enabled, publish_mode, lead_time_hours,
             timezone, schedule_slots, target_platforms, clip_date_filter, clip_sort,
             multi_output_enabled, experimental_ai_layout, publish_language, notifications)
         SELECT $1, s.enabled, s.paused, s.ai_editing_enabled, s.publish_mode, s.lead_time_hours,
             s.timezone, s.schedule_slots, s.target_platforms, s.clip_date_filter, s.clip_sort,
             s.multi_output_enabled, s.experimental_ai_layout, s.publish_language, s.notifications
         FROM jsonb_populate_record(NULL::social_media_autopilot_settings, $2::text::jsonb) s
         ON CONFLICT (twitch_user_id) DO UPDATE SET enabled = EXCLUDED.enabled,
             paused = EXCLUDED.paused, ai_editing_enabled = EXCLUDED.ai_editing_enabled,
             publish_mode = EXCLUDED.publish_mode, lead_time_hours = EXCLUDED.lead_time_hours,
             timezone = EXCLUDED.timezone, schedule_slots = EXCLUDED.schedule_slots,
             target_platforms = EXCLUDED.target_platforms, clip_date_filter = EXCLUDED.clip_date_filter,
             clip_sort = EXCLUDED.clip_sort, multi_output_enabled = EXCLUDED.multi_output_enabled,
             experimental_ai_layout = EXCLUDED.experimental_ai_layout,
             publish_language = EXCLUDED.publish_language, notifications = EXCLUDED.notifications,
             updated_at = NOW()",
    ).bind(twitch_user_id).bind(raw).execute(pool).await?;
    Ok(settings)
}

pub fn next_shared_slot(
    now: DateTime<Utc>,
    taken: &[DateTime<Utc>],
    settings: &AutopilotSettings,
) -> Result<Option<DateTime<Utc>>, SettingsError> {
    settings.validate()?;
    let timezone = settings
        .timezone
        .parse::<Tz>()
        .map_err(|_| SettingsError::Invalid("Unbekannte IANA-Zeitzone".into()))?;
    let mut times = settings
        .schedule_slots
        .iter()
        .map(|slot| Ok((slot.weekday, slot_time(slot)?)))
        .collect::<Result<Vec<_>, SettingsError>>()?;
    times.sort_unstable();
    times.dedup();
    let mut date = now.with_timezone(&timezone).date_naive();
    let Some(last) = date.checked_add_signed(Duration::days(180)) else {
        return Ok(None);
    };
    while date <= last {
        for (_, time) in times
            .iter()
            .filter(|(weekday, _)| u32::from(*weekday) == date.weekday().number_from_monday())
        {
            let candidate = match timezone.from_local_datetime(&date.and_time(*time)) {
                LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
                    value.with_timezone(&Utc)
                }
                LocalResult::None => continue,
            };
            if candidate > now && !taken.contains(&candidate) {
                return Ok(Some(candidate));
            }
        }
        let Some(next) = date.succ_opt() else {
            break;
        };
        date = next;
    }
    Ok(None)
}

pub(crate) async fn lock_owner(
    connection: &mut PgConnection,
    twitch_user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('social_autopilot:' || $1, 0))")
        .bind(twitch_user_id)
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) fn json_column(
    row: &sqlx::postgres::PgRow,
    name: &str,
) -> Result<serde_json::Value, sqlx::Error> {
    let raw: String = row.try_get(name)?;
    serde_json::from_str(&raw).map_err(|error| sqlx::Error::Decode(Box::new(error)))
}
