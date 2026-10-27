use std::collections::BTreeMap;

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
    next_slot_in(now, taken, &settings.schedule_slots, &settings.timezone)
}

pub fn next_slot_in(
    now: DateTime<Utc>,
    taken: &[DateTime<Utc>],
    slots: &[ScheduleSlot],
    timezone: &str,
) -> Result<Option<DateTime<Utc>>, SettingsError> {
    let timezone = timezone
        .parse::<Tz>()
        .map_err(|_| SettingsError::Invalid("Unbekannte IANA-Zeitzone".into()))?;
    let mut times = slots
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMode {
    #[default]
    Upload,
    UploadText,
    AiEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPolicy {
    Off,
    #[default]
    Required,
    VetoUntilDeadline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PolicyScope {
    #[default]
    Shared,
    PerPlatform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DistributionPolicy {
    #[default]
    SameClip,
    IndependentClips,
}

impl DistributionPolicy {
    pub fn for_scope(scope: PolicyScope) -> Self {
        match scope {
            PolicyScope::Shared => Self::SameClip,
            PolicyScope::PerPlatform => Self::IndependentClips,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TextDefaults {
    pub title: String,
    pub description: String,
    pub hashtags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ScopedTextDefaults {
    pub shared: TextDefaults,
    pub per_platform: BTreeMap<String, TextDefaults>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PipelinePolicy {
    pub pipeline_mode: PipelineMode,
    pub review_policy: ReviewPolicy,
    pub metadata_scope: PolicyScope,
    pub text_defaults: ScopedTextDefaults,
    pub schedule_scope: PolicyScope,
    pub platform_schedule_slots: BTreeMap<String, Vec<ScheduleSlot>>,
    pub distribution_policy: DistributionPolicy,
    pub policy_revision: i64,
}

impl PipelinePolicy {
    pub fn slots_for<'a>(
        &'a self,
        settings: &'a AutopilotSettings,
        platform: &str,
    ) -> &'a [ScheduleSlot] {
        match self.schedule_scope {
            PolicyScope::Shared => &settings.schedule_slots,
            PolicyScope::PerPlatform => self
                .platform_schedule_slots
                .get(platform)
                .map_or(&[], Vec::as_slice),
        }
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.policy_revision < 0 {
            return Err(SettingsError::Invalid(
                "Ungültiger Einstellungsstand".into(),
            ));
        }
        for platform in self
            .text_defaults
            .per_platform
            .keys()
            .chain(self.platform_schedule_slots.keys())
        {
            if !crate::posting_plan::PLATFORMS.contains(&platform.as_str()) {
                return Err(SettingsError::Invalid("Unbekannte Zielplattform".into()));
            }
        }
        for slots in self.platform_schedule_slots.values() {
            for slot in slots {
                slot_time(slot)?;
            }
        }
        Ok(())
    }

    pub fn from_legacy(settings: &AutopilotSettings) -> Self {
        Self {
            pipeline_mode: if settings.ai_editing_enabled {
                PipelineMode::AiEdit
            } else {
                PipelineMode::Upload
            },
            review_policy: if settings.publish_mode == PublishMode::FullAuto {
                ReviewPolicy::Off
            } else {
                ReviewPolicy::Required
            },
            ..Self::default()
        }
    }
}

pub async fn load_policy(pool: &PgPool, owner: &str) -> Result<PipelinePolicy, SettingsError> {
    let mut connection = pool.acquire().await?;
    load_policy_on(&mut connection, owner).await
}

pub(crate) async fn load_policy_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<PipelinePolicy, SettingsError> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT to_jsonb(s)::text FROM social_media_autopilot_settings s WHERE twitch_user_id = $1",
    )
    .bind(owner)
    .fetch_optional(connection)
    .await?;
    let Some(raw) = raw else {
        return Ok(PipelinePolicy::default());
    };
    let stored: serde_json::Value = serde_json::from_str(&raw)?;
    if stored
        .get("pipeline_mode")
        .is_none_or(serde_json::Value::is_null)
    {
        return Ok(PipelinePolicy::from_legacy(&serde_json::from_value(
            stored,
        )?));
    }
    let mut policy = serde_json::to_value(PipelinePolicy::default())?;
    if let Some(fields) = policy.as_object_mut() {
        for (key, value) in fields {
            if let Some(stored) = stored.get(key) {
                *value = stored.clone();
            }
        }
    }
    let policy: PipelinePolicy = serde_json::from_value(policy)?;
    policy.validate()?;
    Ok(policy)
}

pub async fn save_policy(
    pool: &PgPool,
    owner: &str,
    policy: &PipelinePolicy,
) -> Result<PipelinePolicy, SettingsError> {
    policy.validate()?;
    let mut policy = policy.clone();
    policy.distribution_policy = DistributionPolicy::for_scope(policy.schedule_scope);
    for slots in policy.platform_schedule_slots.values_mut() {
        slots.sort_by(|a, b| (a.weekday, &a.local_time).cmp(&(b.weekday, &b.local_time)));
        slots.dedup();
    }
    let raw = serde_json::to_string(&policy)?;
    let mut tx = pool.begin().await?;
    lock_owner(&mut tx, owner).await?;
    let previous = load_policy_on(&mut tx, owner).await?;
    let revision: Option<i64> = sqlx::query_scalar(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, pipeline_mode, review_policy,
            metadata_scope, text_defaults, schedule_scope, platform_schedule_slots,
            distribution_policy, policy_revision)
         SELECT $1, s.pipeline_mode, s.review_policy, s.metadata_scope,
            s.text_defaults, s.schedule_scope, s.platform_schedule_slots, s.distribution_policy, 1
         FROM jsonb_populate_record(NULL::social_media_autopilot_settings, $2::text::jsonb) s
         WHERE s.policy_revision = COALESCE((SELECT policy_revision FROM social_media_autopilot_settings WHERE twitch_user_id = $1), 0)
         ON CONFLICT (twitch_user_id) DO UPDATE SET pipeline_mode = EXCLUDED.pipeline_mode,
            review_policy = EXCLUDED.review_policy,
            metadata_scope = EXCLUDED.metadata_scope, text_defaults = EXCLUDED.text_defaults,
            schedule_scope = EXCLUDED.schedule_scope, platform_schedule_slots = EXCLUDED.platform_schedule_slots,
            distribution_policy = EXCLUDED.distribution_policy,
            policy_revision = social_media_autopilot_settings.policy_revision + 1, updated_at = NOW()
         RETURNING policy_revision"
    ).bind(owner).bind(raw).fetch_optional(&mut *tx).await?;
    policy.policy_revision = revision.ok_or_else(|| {
        SettingsError::Invalid(
            "Die Einstellungen wurden zwischenzeitlich geändert. Bitte neu laden.".into(),
        )
    })?;
    if schedule_changed(&previous, &policy) {
        let settings = load_on(&mut tx, owner).await?;
        crate::production_jobs::autopilot_schedule::retime_on(&mut tx, owner, &settings)
            .await
            .map_err(retime_error)?;
    }
    tx.commit().await?;
    Ok(policy)
}

fn schedule_changed(previous: &PipelinePolicy, next: &PipelinePolicy) -> bool {
    previous.schedule_scope != next.schedule_scope
        || previous.platform_schedule_slots != next.platform_schedule_slots
        || previous.distribution_policy != next.distribution_policy
}

fn retime_error(error: crate::production_jobs::autopilot_schedule::ScheduleError) -> SettingsError {
    use crate::production_jobs::autopilot_schedule::ScheduleError;
    match error {
        ScheduleError::Database(error) => SettingsError::Database(error),
        ScheduleError::Settings(error) => error,
        ScheduleError::Json(error) => SettingsError::Json(error),
        other => SettingsError::Invalid(other.to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CutoverState {
    #[default]
    Legacy,
    Shadow,
    Paused,
    Unified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineMigrationMapping {
    pub twitch_user_id: String,
    pub legacy_table: String,
    pub legacy_key: String,
    pub migration_version: i32,
    pub input_digest: String,
    pub canonical_subject: Option<serde_json::Value>,
    pub disposition: MigrationDisposition,
    pub unresolved_reason: Option<String>,
    pub original_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationDisposition {
    Mapped,
    History,
    Unresolved,
}
