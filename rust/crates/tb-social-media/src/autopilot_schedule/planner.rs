use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgConnection, PgPool};

use super::{
    failure_count_on, hold, production_horizon, reservations_on, reserve_on, review_count_on,
    review_required_on, Allocation, Allocator, ProductionHold, ScheduleError, Subject,
};
use crate::autopilot_settings::{self, AutopilotSettings, ClipSort};

pub(super) type ClipBounds = (DateTime<Utc>, DateTime<Utc>);

#[derive(Debug, Default, Serialize)]
pub struct PlanReport {
    pub created: u64,
    pub retimed: u64,
    pub next_slot: Option<DateTime<Utc>>,
    pub production_hold: Option<ProductionHold>,
}

pub(super) async fn select_on(
    connection: &mut PgConnection,
    owner: &str,
    bounds: ClipBounds,
    sort: ClipSort,
) -> Result<Option<i64>, ScheduleError> {
    Ok(sqlx::query_scalar("SELECT p.clip_db_id FROM social_clip_pool p
        JOIN twitch_clips_social_media c ON c.id = p.clip_db_id AND c.twitch_user_id = p.twitch_user_id
        WHERE p.twitch_user_id = $1 AND p.game_id = '2132205352'
        AND p.source_created_at >= $2 AND p.source_created_at < $3
        AND p.processed_at IS NULL AND p.published_at IS NULL AND NOT p.legacy_reserved
        AND NOT EXISTS(SELECT 1 FROM social_production_jobs j WHERE j.clip_id = p.clip_db_id
            AND NOT social_video_discarded(j.id))
        AND NOT social_clip_video_holds(p.clip_db_id)
        AND NOT EXISTS(SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = p.clip_db_id)
        ORDER BY p.selected_at IS NULL, CASE WHEN $4 THEN p.source_created_at END DESC,
            CASE WHEN NOT $4 THEN p.source_created_at END ASC, p.clip_id ASC
        LIMIT 1 FOR UPDATE OF p")
        .bind(owner).bind(bounds.0).bind(bounds.1).bind(sort == ClipSort::NewestFirst)
        .fetch_optional(connection).await?)
}

async fn retime_on(
    connection: &mut PgConnection,
    owner: &str,
    settings: &AutopilotSettings,
    now: DateTime<Utc>,
) -> Result<u64, ScheduleError> {
    let missed: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM social_production_jobs
        WHERE twitch_user_id = $1 AND source = 'autopilot' AND status = 'queued'
        AND started_at IS NULL AND (scheduled_at IS NULL OR scheduled_at <= $2)
        ORDER BY queue_order, id FOR UPDATE",
    )
    .bind(owner)
    .bind(now)
    .fetch_all(&mut *connection)
    .await?;
    let mut changed = 0;
    for id in missed {
        let previous = reservations_on(connection, id).await?;
        let subject = Subject {
            job: Some(id),
            video: None,
        };
        let allocator = Allocator::load_for(connection, owner, settings, subject).await?;
        let allocation = allocator.plan_job(now, &previous)?;
        reserve_on(connection, owner, id, allocation.as_ref()).await?;
        if allocation.is_some() {
            changed += 1;
        }
    }
    Ok(changed)
}

async fn create_on(
    connection: &mut PgConnection,
    owner: &str,
    clip: i64,
    allocation: &Allocation,
    now: DateTime<Utc>,
) -> Result<(), ScheduleError> {
    let job: i64 = sqlx::query_scalar("INSERT INTO social_production_jobs (twitch_user_id, clip_id, source, queue_order, scheduled_at)
        VALUES ($1, $2, 'autopilot', COALESCE((SELECT MAX(queue_order) + 1 FROM social_production_jobs), 1), $3) RETURNING id")
        .bind(owner).bind(clip).bind(allocation.reference).fetch_one(&mut *connection).await?;
    reserve_on(connection, owner, job, Some(allocation)).await?;
    sqlx::query("UPDATE social_clip_pool SET selected_at = COALESCE(selected_at, $3),
        first_selected_at = COALESCE(first_selected_at, $3) WHERE twitch_user_id = $1 AND clip_db_id = $2")
        .bind(owner).bind(clip).bind(now).execute(connection).await?;
    Ok(())
}

async fn save_report(
    connection: &mut PgConnection,
    owner: &str,
    now: DateTime<Utc>,
    report: &PlanReport,
) -> Result<(), ScheduleError> {
    let reason = report.production_hold.map(|hold| match hold {
        ProductionHold::Disabled => "disabled",
        ProductionHold::Paused => "paused",
        ProductionHold::ReviewBacklog => "review_backlog",
        ProductionHold::FailureBacklog => "failure_backlog",
        ProductionHold::EmptyPool => "empty_pool",
        ProductionHold::NoSchedule => "no_schedule",
        ProductionHold::OutsideLead => "outside_lead",
    });
    sqlx::query("INSERT INTO social_autopilot_schedule_state (twitch_user_id, planned_at, production_hold)
        VALUES ($1, $2, $3) ON CONFLICT (twitch_user_id) DO UPDATE SET planned_at = EXCLUDED.planned_at,
        production_hold = EXCLUDED.production_hold")
        .bind(owner).bind(now).bind(reason).execute(connection).await?;
    Ok(())
}

pub async fn plan_owner(
    pool: &PgPool,
    owner: &str,
    now: DateTime<Utc>,
) -> Result<PlanReport, ScheduleError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    let reviews = review_count_on(&mut tx, owner).await?;
    let failures = failure_count_on(&mut tx, owner).await?;
    let allocator = Allocator::load_on(&mut tx, owner, &settings).await?;
    let mut report = PlanReport {
        next_slot: allocator.plan(now)?.map(|plan| plan.reference),
        ..Default::default()
    };
    let review_required = review_required_on(&mut tx, owner).await?;
    report.production_hold = hold(
        &settings,
        review_required,
        allocator.has_schedule(),
        reviews,
        failures,
        report.next_slot,
        now,
    );
    if matches!(
        report.production_hold,
        Some(
            ProductionHold::Disabled
                | ProductionHold::Paused
                | ProductionHold::ReviewBacklog
                | ProductionHold::FailureBacklog
                | ProductionHold::NoSchedule
        )
    ) {
        save_report(&mut tx, owner, now, &report).await?;
        tx.commit().await?;
        return Ok(report);
    }
    report.retimed = retime_on(&mut tx, owner, &settings, now).await?;
    let mut allocator = Allocator::load_on(&mut tx, owner, &settings).await?;
    let window = crate::clip_pool::bounds(&settings, now)
        .map_err(|error| ScheduleError::Invalid(error.to_string()))?;
    let bounds = (window.start, window.end);
    let horizon = production_horizon(&settings, now);
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM social_production_jobs WHERE twitch_user_id = $1 AND status IN ('queued', 'running')")
        .bind(owner).fetch_one(&mut *tx).await?;
    for _ in pending..500 {
        let next = allocator.plan(now)?;
        report.next_slot = next.as_ref().map(|plan| plan.reference);
        let Some(allocation) = next.filter(|plan| plan.reference <= horizon) else {
            report.production_hold = if report.created == 0 && report.retimed == 0 {
                Some(ProductionHold::OutsideLead)
            } else {
                None
            };
            break;
        };
        let Some(clip) = select_on(&mut tx, owner, bounds, settings.clip_sort).await? else {
            report.production_hold = Some(ProductionHold::EmptyPool);
            break;
        };
        create_on(&mut tx, owner, clip, &allocation, now).await?;
        allocator.commit(&allocation);
        report.created += 1;
    }
    save_report(&mut tx, owner, now, &report).await?;
    tx.commit().await?;
    Ok(report)
}
