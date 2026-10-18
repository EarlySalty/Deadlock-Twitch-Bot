mod placement;
mod planner;
mod queue;
mod recovery;
mod settings;
mod test_run;

pub(crate) use placement::{place_video_on, queued_platforms_on};
pub use planner::{plan_owner, PlanReport};

pub use queue::{cancel, mark_next, reorder, resolve_review, ReviewDecision};
pub use recovery::{recover_after_lease, retry_or_review, Recovery};
pub(crate) use settings::retime_on;
pub use settings::{save_schedule, SchedulePatch, ScheduleSettings};
pub use test_run::{discard_test_run, start_test_run, TestRun};

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgConnection, PgPool};

use super::{from_row, JobError, JobSource, ProductionJob, ProductionMode, SELECT};
use crate::autopilot_settings::{
    self, AutopilotSettings, DistributionPolicy, PipelinePolicy, PolicyScope, SettingsError,
};

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("Der Zeitplan ist ungültig: {0}")]
    Invalid(String),
    #[error("Der Produktionsauftrag wurde für deinen Kanal nicht gefunden")]
    NotFound,
    #[error("Die Warteschlange wurde inzwischen geändert. Bitte aktualisiere sie")]
    QueueChanged,
    #[error("Der Auftrag wurde bereits gestartet und kann nicht mehr geändert werden")]
    Started,
    #[error("Der Auftrag wartet nicht mehr auf eine Entscheidung")]
    NotInReview,
    #[error("Es läuft bereits ein Testlauf. Prüfe oder verwirf ihn zuerst")]
    TestRunActive,
    #[error("Im Clip-Vorrat ist gerade kein passender Clip für einen Testlauf")]
    EmptyPool,
    #[error("Der Zeitplan konnte nicht gespeichert oder geladen werden")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Job(#[from] JobError),
    #[error("Die Zeitplandaten sind ungültig")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionHold {
    Disabled,
    Paused,
    ReviewBacklog,
    EmptyPool,
    NoSchedule,
    OutsideLead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reservation {
    pub platform: String,
    pub scheduled_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct QueueJob {
    pub id: i64,
    pub clip_id: i64,
    pub clip_title: String,
    pub source: JobSource,
    pub mode: Option<ProductionMode>,
    pub requested_mode: Option<ProductionMode>,
    pub status: String,
    pub stage: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub reservations: Vec<Reservation>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub review_reasons: Vec<String>,
    pub review_explanation: Option<String>,
}

impl QueueJob {
    fn from_job(
        job: ProductionJob,
        clip_title: String,
        reservations: Vec<Reservation>,
    ) -> Result<Self, ScheduleError> {
        Ok(Self {
            id: job.id,
            clip_id: job.clip_id,
            clip_title,
            source: job.source,
            mode: job.mode,
            requested_mode: job.requested_mode,
            status: job.status,
            stage: job.stage,
            attempts: job.attempts,
            max_attempts: job.max_attempts,
            scheduled_at: job.scheduled_at,
            reservations,
            started_at: job.started_at,
            last_error: job.last_error,
            review_reasons: serde_json::from_value(job.review_reasons)?,
            review_explanation: job
                .metadata
                .get("review_explanation")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct PolicyPreview {
    pub next_slot: Option<DateTime<Utc>>,
    pub next_slots: BTreeMap<String, DateTime<Utc>>,
    pub production_hold: Option<ProductionHold>,
}

#[derive(Debug, Serialize)]
pub struct PolicyPreviews {
    pub same_clip: PolicyPreview,
    pub independent_clips: PolicyPreview,
}

#[derive(Debug, Serialize)]
pub struct ScheduleQueueSnapshot {
    pub settings: ScheduleSettings,
    pub persisted: bool,
    pub schedule_scope: PolicyScope,
    pub distribution_policy: DistributionPolicy,
    pub next_slot: Option<DateTime<Utc>>,
    pub next_slots: BTreeMap<String, DateTime<Utc>>,
    pub jobs: Vec<QueueJob>,
    pub review_count: i64,
    pub production_hold: Option<ProductionHold>,
    pub policy_previews: PolicyPreviews,
    pub connected_platforms: Vec<String>,
    pub test_run: Option<TestRun>,
}

pub(crate) fn review_backlog_sql(owner: &str) -> String {
    format!(
        "((SELECT COUNT(*) FROM social_produced_videos v JOIN social_production_jobs vj ON vj.id = v.job_id
            WHERE v.twitch_user_id = {owner} AND vj.source = 'autopilot' AND v.status = 'review'
            AND (v.expires_at IS NULL OR v.expires_at > clock_timestamp()))
        + (SELECT COUNT(*) FROM social_production_jobs rj WHERE rj.twitch_user_id = {owner}
            AND rj.source = 'autopilot' AND rj.status = 'review' AND NOT EXISTS (SELECT 1 FROM social_produced_videos v
                WHERE v.job_id = rj.id AND v.status = 'review'
                AND (v.expires_at IS NULL OR v.expires_at > clock_timestamp()))))"
    )
}

pub(crate) async fn review_count_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {}",
        review_backlog_sql("$1")
    )))
    .bind(owner)
    .fetch_one(connection)
    .await
}

pub(crate) fn has_schedule_sql(settings: &str) -> String {
    format!(
        "CASE WHEN {settings}.pipeline_mode IS NOT NULL AND {settings}.schedule_scope = 'per_platform'
            AND jsonb_array_length({settings}.target_platforms) > 0
        THEN EXISTS (SELECT 1 FROM jsonb_array_elements_text({settings}.target_platforms) target(platform)
            WHERE jsonb_typeof({settings}.platform_schedule_slots->target.platform) = 'array'
            AND jsonb_array_length({settings}.platform_schedule_slots->target.platform) > 0)
        ELSE jsonb_array_length({settings}.schedule_slots) > 0 END"
    )
}

fn schedule_exists(settings: &AutopilotSettings, policy: &PipelinePolicy) -> bool {
    if settings.target_platforms.is_empty() {
        return !settings.schedule_slots.is_empty();
    }
    settings
        .target_platforms
        .iter()
        .any(|platform| !policy.slots_for(settings, platform).is_empty())
}

pub(crate) async fn has_schedule_on(
    connection: &mut PgConnection,
    owner: &str,
    settings: &AutopilotSettings,
) -> Result<bool, SettingsError> {
    let policy = autopilot_settings::load_policy_on(connection, owner).await?;
    Ok(schedule_exists(settings, &policy))
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Subject {
    pub job: Option<i64>,
    pub video: Option<i64>,
}

#[derive(Debug, Default, Clone)]
struct Occupancy {
    channel: Vec<DateTime<Utc>>,
    platforms: BTreeMap<String, Vec<DateTime<Utc>>>,
}

impl Occupancy {
    async fn load_on(
        connection: &mut PgConnection,
        owner: &str,
        subject: Subject,
    ) -> Result<Self, sqlx::Error> {
        let channel = sqlx::query_scalar("SELECT j.scheduled_at FROM social_production_jobs j
            WHERE j.twitch_user_id = $1 AND j.scheduled_at IS NOT NULL AND j.status IN ('queued', 'running', 'completed')
            AND j.id IS DISTINCT FROM $2
            AND NOT EXISTS (SELECT 1 FROM social_publication_reservations r WHERE r.job_id = j.id AND r.state = 'reserved')
            UNION SELECT v.scheduled_at FROM social_produced_videos v
            WHERE v.twitch_user_id = $1 AND v.scheduled_at IS NOT NULL
            AND v.id IS DISTINCT FROM $3 AND (v.job_id IS NULL OR v.job_id IS DISTINCT FROM $2)
            AND NOT EXISTS (SELECT 1 FROM social_publication_reservations r WHERE r.job_id = v.job_id
                AND r.state = 'reserved' AND r.scheduled_at = v.scheduled_at)")
            .bind(owner).bind(subject.job).bind(subject.video).fetch_all(&mut *connection).await?;
        let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT platform, scheduled_at FROM social_publication_reservations
            WHERE twitch_user_id = $1 AND state = 'reserved' AND job_id IS DISTINCT FROM $2
            UNION SELECT q.platform, q.scheduled_at FROM twitch_clips_upload_queue q
            JOIN twitch_clips_social_media c ON c.id = q.clip_id
            WHERE c.twitch_user_id = $1 AND q.scheduled_at IS NOT NULL AND q.platform IS NOT NULL
            AND COALESCE(q.status, '') NOT IN ('failed', 'cancelled', 'autopilot_discarded', 'autopilot_failed')
            AND NOT EXISTS (SELECT 1 FROM social_produced_videos v WHERE v.id = q.produced_video_id
                AND (v.id = $3 OR v.job_id = $2))",
        )
        .bind(owner)
        .bind(subject.job)
        .bind(subject.video)
        .fetch_all(connection)
        .await?;
        let mut platforms: BTreeMap<String, Vec<DateTime<Utc>>> = BTreeMap::new();
        for (platform, at) in rows {
            platforms.entry(platform).or_default().extend([at]);
        }
        Ok(Self { channel, platforms })
    }

    fn everything(&self) -> Vec<DateTime<Utc>> {
        self.channel
            .iter()
            .chain(self.platforms.values().flatten())
            .copied()
            .collect()
    }

    fn for_platforms(&self, platforms: &[&str]) -> Vec<DateTime<Utc>> {
        let mut taken = self.channel.clone();
        for platform in platforms {
            taken.extend(self.platforms.get(*platform).into_iter().flatten());
        }
        taken
    }

    fn is_free(&self, slot: &Reservation) -> bool {
        !self
            .for_platforms(&[slot.platform.as_str()])
            .contains(&slot.scheduled_at)
    }

    fn add(&mut self, allocation: &Allocation) {
        if allocation.slots.is_empty() {
            self.channel.extend([allocation.reference]);
        }
        for slot in &allocation.slots {
            self.platforms
                .entry(slot.platform.clone())
                .or_default()
                .extend([slot.scheduled_at]);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Allocation {
    pub reference: DateTime<Utc>,
    pub slots: Vec<Reservation>,
}

impl Allocation {
    pub(crate) fn at(&self, platform: &str) -> DateTime<Utc> {
        self.slots
            .iter()
            .find(|slot| slot.platform == platform)
            .map_or(self.reference, |slot| slot.scheduled_at)
    }

    fn from_reservations(reservations: Vec<Reservation>) -> Option<Self> {
        let reference = reservations.iter().map(|slot| slot.scheduled_at).min()?;
        Some(Self {
            reference,
            slots: reservations,
        })
    }
}

#[derive(Clone)]
pub(crate) struct Allocator {
    settings: AutopilotSettings,
    policy: PipelinePolicy,
    occupancy: Occupancy,
}

impl Allocator {
    pub(crate) async fn load_on(
        connection: &mut PgConnection,
        owner: &str,
        settings: &AutopilotSettings,
    ) -> Result<Self, ScheduleError> {
        Self::load_for(connection, owner, settings, Subject::default()).await
    }

    pub(crate) async fn load_for(
        connection: &mut PgConnection,
        owner: &str,
        settings: &AutopilotSettings,
        subject: Subject,
    ) -> Result<Self, ScheduleError> {
        let policy = autopilot_settings::load_policy_on(connection, owner).await?;
        let occupancy = Occupancy::load_on(connection, owner, subject).await?;
        Ok(Self {
            settings: settings.clone(),
            policy,
            occupancy,
        })
    }

    fn scheduled<'a>(&self, platforms: &'a [String]) -> Vec<&'a str> {
        platforms
            .iter()
            .map(String::as_str)
            .filter(|platform| !self.policy.slots_for(&self.settings, platform).is_empty())
            .collect()
    }

    pub(crate) fn has_schedule(&self) -> bool {
        schedule_exists(&self.settings, &self.policy)
    }

    fn next_in(
        &self,
        now: DateTime<Utc>,
        taken: &[DateTime<Utc>],
        slots: &[autopilot_settings::ScheduleSlot],
    ) -> Result<Option<DateTime<Utc>>, ScheduleError> {
        Ok(autopilot_settings::next_slot_in(
            now,
            taken,
            slots,
            &self.settings.timezone,
        )?)
    }

    fn next_for(
        &self,
        now: DateTime<Utc>,
        platform: &str,
    ) -> Result<Option<Reservation>, ScheduleError> {
        let taken = self.occupancy.for_platforms(&[platform]);
        let slots = self.policy.slots_for(&self.settings, platform);
        Ok(self
            .next_in(now, &taken, slots)?
            .map(|scheduled_at| Reservation {
                platform: platform.to_owned(),
                scheduled_at,
            }))
    }

    fn channel_wide(&self, now: DateTime<Utc>) -> Result<Option<Allocation>, ScheduleError> {
        Ok(self
            .next_in(
                now,
                &self.occupancy.everything(),
                &self.settings.schedule_slots,
            )?
            .map(|reference| Allocation {
                reference,
                slots: Vec::new(),
            }))
    }

    fn shared(
        &self,
        now: DateTime<Utc>,
        targets: &[&str],
    ) -> Result<Option<Allocation>, ScheduleError> {
        let taken = self.occupancy.for_platforms(targets);
        Ok(self
            .next_in(now, &taken, &self.settings.schedule_slots)?
            .map(|reference| Allocation {
                reference,
                slots: targets
                    .iter()
                    .map(|platform| Reservation {
                        platform: (*platform).to_owned(),
                        scheduled_at: reference,
                    })
                    .collect(),
            }))
    }

    fn per_platform(
        &self,
        now: DateTime<Utc>,
        targets: &[&str],
    ) -> Result<Vec<Reservation>, ScheduleError> {
        let mut slots = Vec::new();
        for platform in targets {
            slots.extend(self.next_for(now, platform)?);
        }
        Ok(slots)
    }

    fn with_distribution(&self, distribution: DistributionPolicy) -> Self {
        let mut allocator = self.clone();
        allocator.policy.distribution_policy = distribution;
        allocator
    }

    pub(crate) fn independent(&self) -> bool {
        self.policy.distribution_policy == DistributionPolicy::IndependentClips
    }

    pub(crate) fn plan_for(
        &self,
        now: DateTime<Utc>,
        platforms: &[String],
    ) -> Result<Option<Allocation>, ScheduleError> {
        self.settings.validate()?;
        if platforms.is_empty() {
            return self.channel_wide(now);
        }
        let targets = self.scheduled(platforms);
        if targets.is_empty() {
            return Ok(None);
        }
        if self.independent() {
            let earliest = self
                .per_platform(now, &targets)?
                .into_iter()
                .min_by_key(|slot| slot.scheduled_at);
            return Ok(earliest.and_then(|slot| Allocation::from_reservations(vec![slot])));
        }
        match self.policy.schedule_scope {
            PolicyScope::Shared => self.shared(now, &targets),
            PolicyScope::PerPlatform => Ok(Allocation::from_reservations(
                self.per_platform(now, &targets)?,
            )),
        }
    }

    fn place_missing(
        &self,
        now: DateTime<Utc>,
        missing: &[String],
    ) -> Result<Vec<Reservation>, ScheduleError> {
        if !self.independent() {
            return Ok(self
                .plan_for(now, missing)?
                .map(|plan| plan.slots)
                .unwrap_or_default());
        }
        let mut slots = Vec::new();
        for platform in missing {
            if let Some(plan) = self.plan_for(now, std::slice::from_ref(platform))? {
                slots.extend(plan.slots);
            }
        }
        Ok(slots)
    }

    fn keeps(&self, now: DateTime<Utc>, slot: &Reservation) -> Result<bool, ScheduleError> {
        let slots = self.policy.slots_for(&self.settings, &slot.platform);
        Ok(slot.scheduled_at > now
            && self.occupancy.is_free(slot)
            && self.is_slot(slot.scheduled_at, slots)?)
    }

    fn arrange(
        &self,
        now: DateTime<Utc>,
        kept: &[Reservation],
        selected: &[String],
        validate: bool,
    ) -> Result<Option<Allocation>, ScheduleError> {
        if selected.is_empty() {
            return self.channel_wide(now);
        }
        let targets = self.scheduled(selected);
        let mut placed = Vec::new();
        for slot in kept {
            let wanted = if validate {
                targets.contains(&slot.platform.as_str()) && self.keeps(now, slot)?
            } else {
                selected.contains(&slot.platform)
            };
            if wanted
                && !placed
                    .iter()
                    .any(|done: &Reservation| done.platform == slot.platform)
            {
                placed.extend([slot.clone()]);
            }
        }
        let missing: Vec<String> = targets
            .iter()
            .filter(|platform| !placed.iter().any(|slot| slot.platform == **platform))
            .map(|platform| (*platform).to_owned())
            .collect();
        if !missing.is_empty() {
            placed.extend(self.place_missing(now, &missing)?);
        }
        Ok(Allocation::from_reservations(placed))
    }

    fn settle(&mut self, allocation: Option<Allocation>) -> Option<Allocation> {
        if let Some(plan) = &allocation {
            self.commit(plan);
        }
        allocation
    }

    pub(crate) fn place(
        &mut self,
        now: DateTime<Utc>,
        kept: &[Reservation],
        selected: &[String],
    ) -> Result<Option<Allocation>, ScheduleError> {
        let allocation = self.arrange(now, kept, selected, true)?;
        Ok(self.settle(allocation))
    }

    pub(crate) fn place_committed(
        &mut self,
        now: DateTime<Utc>,
        kept: &[Reservation],
        selected: &[String],
    ) -> Result<Option<Allocation>, ScheduleError> {
        let allocation = self.arrange(now, kept, selected, false)?;
        Ok(self.settle(allocation))
    }

    pub(crate) fn place_video(
        &mut self,
        now: DateTime<Utc>,
        previous: &[Reservation],
        queued: &[String],
    ) -> Result<Option<Allocation>, ScheduleError> {
        let targets = &self.settings.target_platforms;
        let mut selected: Vec<String> = previous
            .iter()
            .map(|slot| slot.platform.clone())
            .filter(|platform| targets.is_empty() || targets.contains(platform))
            .collect();
        if !self.independent() {
            selected.extend(targets.iter().cloned());
        }
        selected.extend(queued.iter().cloned());
        selected.sort();
        selected.dedup();
        if selected.is_empty() && !targets.is_empty() {
            let allocation = self.plan(now)?;
            return Ok(self.settle(allocation));
        }
        self.place(now, previous, &selected)
    }

    pub(crate) fn plan(&self, now: DateTime<Utc>) -> Result<Option<Allocation>, ScheduleError> {
        self.plan_for(now, &self.settings.target_platforms)
    }

    pub(crate) fn plan_job(
        &self,
        now: DateTime<Utc>,
        previous: &[Reservation],
    ) -> Result<Option<Allocation>, ScheduleError> {
        let targets = &self.settings.target_platforms;
        if targets.is_empty() {
            return self.plan(now);
        }
        let selected = if self.independent() {
            match previous
                .iter()
                .find(|slot| targets.contains(&slot.platform))
            {
                Some(slot) => vec![slot.platform.clone()],
                None => return self.plan(now),
            }
        } else {
            targets.clone()
        };
        match self.arrange(now, previous, &selected, true)? {
            Some(allocation) => Ok(Some(allocation)),
            None => self.plan(now),
        }
    }

    pub(crate) fn next_slots(
        &self,
        now: DateTime<Utc>,
    ) -> Result<BTreeMap<String, DateTime<Utc>>, ScheduleError> {
        let targets = self.scheduled(&self.settings.target_platforms);
        let slots = if self.independent() {
            self.per_platform(now, &targets)?
        } else {
            self.plan(now)?.map(|plan| plan.slots).unwrap_or_default()
        };
        Ok(slots
            .into_iter()
            .map(|slot| (slot.platform, slot.scheduled_at))
            .collect())
    }

    fn is_slot(
        &self,
        at: DateTime<Utc>,
        slots: &[autopilot_settings::ScheduleSlot],
    ) -> Result<bool, ScheduleError> {
        let before = at - chrono::Duration::seconds(1);
        Ok(self.next_in(before, &[], slots)? == Some(at))
    }

    fn fits(&self, reservations: &[Reservation]) -> bool {
        let targets = self.scheduled(&self.settings.target_platforms);
        let covered = reservations
            .iter()
            .all(|slot| targets.contains(&slot.platform.as_str()));
        if self.independent() {
            return covered && reservations.len() == 1;
        }
        covered && reservations.len() == targets.len()
    }

    pub(crate) fn holds(
        &self,
        at: DateTime<Utc>,
        reservations: &[Reservation],
    ) -> Result<bool, ScheduleError> {
        if reservations.is_empty() {
            return self.is_slot(at, &self.settings.schedule_slots);
        }
        if !self.fits(reservations) {
            return Ok(false);
        }
        for slot in reservations {
            let slots = self.policy.slots_for(&self.settings, &slot.platform);
            if !self.is_slot(slot.scheduled_at, slots)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn commit(&mut self, allocation: &Allocation) {
        self.occupancy.add(allocation);
    }
}

pub(crate) async fn reservations_on(
    connection: &mut PgConnection,
    job: i64,
) -> Result<Vec<Reservation>, sqlx::Error> {
    let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT platform, scheduled_at FROM social_publication_reservations
        WHERE job_id = $1 AND state = 'reserved' ORDER BY scheduled_at, platform",
    )
    .bind(job)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(platform, scheduled_at)| Reservation {
            platform,
            scheduled_at,
        })
        .collect())
}

pub(crate) async fn video_destinations_on(
    connection: &mut PgConnection,
    video: i64,
) -> Result<Vec<Reservation>, sqlx::Error> {
    let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT r.platform, r.scheduled_at FROM social_publication_reservations r
        JOIN social_produced_videos v ON v.job_id = r.job_id
        WHERE v.id = $1 AND r.state = 'reserved'
        AND (r.produced_video_id IS NULL OR r.produced_video_id = v.id)
        ORDER BY r.scheduled_at, r.platform",
    )
    .bind(video)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(platform, scheduled_at)| Reservation {
            platform,
            scheduled_at,
        })
        .collect())
}

pub(crate) async fn release_on(connection: &mut PgConnection, job: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE social_publication_reservations SET state = 'released', released_at = clock_timestamp()
        WHERE job_id = $1 AND state = 'reserved'",
    )
    .bind(job)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn hold_slots_on(
    connection: &mut PgConnection,
    owner: &str,
    job: i64,
    video: Option<i64>,
    slots: &[Reservation],
) -> Result<(), sqlx::Error> {
    release_on(connection, job).await?;
    for slot in slots {
        sqlx::query(
            "INSERT INTO social_publication_reservations
            (twitch_user_id, platform, scheduled_at, job_id, produced_video_id) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(owner)
        .bind(&slot.platform)
        .bind(slot.scheduled_at)
        .bind(job)
        .bind(video)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub(crate) async fn reserve_on(
    connection: &mut PgConnection,
    owner: &str,
    job: i64,
    allocation: Option<&Allocation>,
) -> Result<(), sqlx::Error> {
    let active: bool = sqlx::query_scalar(
        "UPDATE social_production_jobs SET scheduled_at = $3 WHERE id = $1 AND twitch_user_id = $2
        RETURNING status IN ('queued', 'running', 'completed')",
    )
    .bind(job)
    .bind(owner)
    .bind(allocation.map(|value| value.reference))
    .fetch_optional(&mut *connection)
    .await?
    .unwrap_or(false);
    let slots = match allocation {
        Some(value) if active => value.slots.as_slice(),
        _ => &[],
    };
    hold_slots_on(connection, owner, job, None, slots).await
}

pub(crate) const REVIEW_LIMIT: i64 = 2;

pub(crate) fn production_horizon(
    settings: &AutopilotSettings,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    now + chrono::Duration::hours(i64::from(settings.lead_time_hours))
        .max(chrono::Duration::minutes(1))
}

pub(crate) async fn review_required_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<bool, sqlx::Error> {
    Ok(matches!(
        crate::autopilot_publish::review_mode_on(connection, owner).await?,
        crate::autopilot_publish::ReviewMode::Review
    ))
}

pub(crate) fn hold(
    settings: &AutopilotSettings,
    review_required: bool,
    has_schedule: bool,
    reviews: i64,
    next: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<ProductionHold> {
    if !settings.enabled {
        return Some(ProductionHold::Disabled);
    }
    if settings.paused {
        return Some(ProductionHold::Paused);
    }
    if review_required && reviews >= REVIEW_LIMIT {
        return Some(ProductionHold::ReviewBacklog);
    }
    if !has_schedule {
        return Some(ProductionHold::NoSchedule);
    }
    if next.is_none_or(|slot| slot > production_horizon(settings, now)) {
        return Some(ProductionHold::OutsideLead);
    }
    None
}

async fn queue_jobs_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Vec<QueueJob>, ScheduleError> {
    let jobs = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE twitch_user_id = $1
        AND status NOT IN ('completed', 'cancelled')
        ORDER BY CASE WHEN status = 'running' THEN 0 WHEN status = 'queued' THEN 1 ELSE 2 END,
            queue_order, id LIMIT 500"
    )))
    .bind(owner)
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(from_row)
    .collect::<Result<Vec<_>, _>>()?;
    let clip_ids: Vec<i64> = jobs.iter().map(|job| job.clip_id).collect();
    let titles: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id::bigint, COALESCE(clip_title, '') FROM twitch_clips_social_media
        WHERE twitch_user_id = $1 AND id = ANY($2)",
    )
    .bind(owner)
    .bind(&clip_ids)
    .fetch_all(&mut *connection)
    .await?;
    let titles: std::collections::HashMap<i64, String> = titles.into_iter().collect();
    let ids: Vec<i64> = jobs.iter().map(|job| job.id).collect();
    let rows: Vec<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT job_id, platform, scheduled_at FROM social_publication_reservations
        WHERE job_id = ANY($1) AND state = 'reserved' ORDER BY scheduled_at, platform",
    )
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await?;
    let mut reserved: std::collections::HashMap<i64, Vec<Reservation>> =
        std::collections::HashMap::new();
    for (job, platform, scheduled_at) in rows {
        reserved.entry(job).or_default().extend([Reservation {
            platform,
            scheduled_at,
        }]);
    }
    let mut queued = Vec::with_capacity(jobs.len());
    for job in jobs {
        let title = titles.get(&job.clip_id).cloned().unwrap_or_default();
        let reservations = reserved.remove(&job.id).unwrap_or_default();
        queued.extend([QueueJob::from_job(job, title, reservations)?]);
    }
    Ok(queued)
}

struct PreviewContext<'a> {
    settings: &'a AutopilotSettings,
    review_required: bool,
    reviews: i64,
    empty_pool: bool,
    now: DateTime<Utc>,
}

fn preview(
    allocator: &Allocator,
    context: &PreviewContext<'_>,
) -> Result<PolicyPreview, ScheduleError> {
    let next_slots = allocator.next_slots(context.now)?;
    let next_slot = allocator.plan(context.now)?.map(|plan| plan.reference);
    let mut production_hold = hold(
        context.settings,
        context.review_required,
        allocator.has_schedule(),
        context.reviews,
        next_slot,
        context.now,
    );
    if production_hold.is_none() && context.empty_pool {
        production_hold = Some(ProductionHold::EmptyPool);
    }
    Ok(PolicyPreview {
        next_slot,
        next_slots,
        production_hold,
    })
}

pub async fn snapshot(pool: &PgPool, owner: &str) -> Result<ScheduleQueueSnapshot, ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let persisted = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_media_autopilot_settings WHERE twitch_user_id = $1)",
    )
    .bind(owner)
    .fetch_one(&mut *tx)
    .await?;
    let now = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let allocator = Allocator::load_on(&mut tx, owner, &settings).await?;
    let review_count = review_count_on(&mut tx, owner).await?;
    let review_required = review_required_on(&mut tx, owner).await?;
    let jobs = queue_jobs_on(&mut tx, owner).await?;
    let empty_pool: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_autopilot_schedule_state
        WHERE twitch_user_id = $1 AND production_hold = 'empty_pool' AND planned_at > $2 - INTERVAL '2 minutes')")
        .bind(owner).bind(now).fetch_one(&mut *tx).await?;
    let context = PreviewContext {
        settings: &settings,
        review_required,
        reviews: review_count,
        empty_pool,
        now,
    };
    let current = preview(&allocator, &context)?;
    let policy_previews = PolicyPreviews {
        same_clip: preview(
            &allocator.with_distribution(DistributionPolicy::SameClip),
            &context,
        )?,
        independent_clips: preview(
            &allocator.with_distribution(DistributionPolicy::IndependentClips),
            &context,
        )?,
    };
    let mut connected_platforms: Vec<String> =
        crate::autopilot_publish::connections_on(&mut tx, owner)
            .await?
            .into_keys()
            .filter(|platform| crate::posting_plan::PLATFORMS.contains(&platform.as_str()))
            .collect();
    connected_platforms.sort_unstable();
    let test_run = test_run::active_on(&mut tx, owner).await?;
    tx.commit().await?;
    Ok(ScheduleQueueSnapshot {
        settings: ScheduleSettings::from(&settings),
        persisted,
        schedule_scope: allocator.policy.schedule_scope,
        distribution_policy: allocator.policy.distribution_policy,
        next_slot: current.next_slot,
        next_slots: current.next_slots,
        jobs,
        review_count,
        production_hold: current.production_hold,
        policy_previews,
        connected_platforms,
        test_run,
    })
}

#[cfg(test)]
mod tests;
