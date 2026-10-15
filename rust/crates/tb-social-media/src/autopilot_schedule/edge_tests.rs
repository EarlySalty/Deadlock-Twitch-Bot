use super::*;

#[test]
fn autopilot_review_backpressure_does_not_hold_full_auto() {
    let now = utc("2026-10-12T15:59:30Z");
    let next = Some(now + Duration::seconds(30));
    let mut settings = autopilot_settings::AutopilotSettings {
        enabled: true,
        schedule_slots: schedule().schedule_slots.expect("fixed weekly slot"),
        ..Default::default()
    };
    assert_eq!(
        hold(&settings, true, 2, next, now),
        Some(ProductionHold::ReviewBacklog)
    );
    assert_eq!(hold(&settings, true, 1, next, now), None);
    settings.publish_mode = autopilot_settings::PublishMode::FullAuto;
    assert_eq!(hold(&settings, true, 2, next, now), None);
    settings.paused = true;
    assert_eq!(
        hold(&settings, true, 2, next, now),
        Some(ProductionHold::Paused)
    );
}

#[tokio::test]
async fn snapshot_uses_real_serial_clip_ids_and_hides_internal_checkpoints() {
    let pool = pool().await;
    let clip_id = clip(&pool, "owner", "same-clip", utc("2025-01-01T00:00:00Z")).await;
    let first = job(&pool, "owner", clip_id, Some(ProductionMode::Static)).await;
    production_jobs::claim_next(&pool)
        .await
        .expect("claim")
        .expect("claimed job");
    production_jobs::update_progress(
        &pool,
        "owner",
        first.id,
        "rendering",
        &json!({"internal_analysis":{"private":true}}),
    )
    .await
    .expect("internal checkpoint");
    production_jobs::finish(&pool, "owner", first.id, JobCompletion::Review, &[], None)
        .await
        .expect("review hold");
    let second = job(&pool, "owner", clip_id, Some(ProductionMode::Ai)).await;
    let snapshot = snapshot(&pool, "owner")
        .await
        .expect("real serial ID decoding");
    assert_eq!(snapshot.jobs[0].id, second.id);
    assert_eq!(snapshot.jobs.len(), 2);
    assert!(
        snapshot
            .jobs
            .iter()
            .all(|job| job.clip_title == "same-clip")
    );
    let public = serde_json::to_value(&snapshot).expect("public snapshot");
    assert!(
        public["jobs"]
            .as_array()
            .expect("queue array")
            .iter()
            .all(|job| job.get("metadata").is_none() && job.get("internal_analysis").is_none())
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn newest_selection_is_strict_and_empty_pool_does_not_set_pause() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::NewestFirst).await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("schedule");
    clip(&pool, "owner", "old", utc("2025-01-01T00:00:00Z")).await;
    let newest = clip(&pool, "owner", "newest", utc("2025-12-01T00:00:00Z")).await;
    clip(&pool, "owner", "excluded", utc("2026-01-01T00:00:00Z")).await;
    let now = utc("2026-10-11T18:00:00Z");
    assert_eq!(
        plan_owner(&pool, "owner", now)
            .await
            .expect("newest plan")
            .created,
        1
    );
    let selected = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("queue");
    assert_eq!(selected[0].clip_id, newest);
    cancel(&pool, "owner", selected[0].id)
        .await
        .expect("durable cancellation");
    choose_year(&pool, 1970, ClipSort::NewestFirst).await;
    let report = plan_owner(&pool, "owner", now)
        .await
        .expect("actual empty M1 filter");
    assert_eq!(report.production_hold, Some(ProductionHold::EmptyPool));
    assert!(
        !autopilot_settings::load(&pool, "owner")
            .await
            .expect("settings unchanged")
            .paused
    );
    let uploads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
        .fetch_one(&pool)
        .await
        .expect("no uploader effects");
    assert_eq!(uploads, 0);
    cleanup(pool).await;
}

#[tokio::test]
async fn zero_lead_uses_poll_window_and_changed_filters_preserve_queued_clips() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            lead_time_hours: Some(0),
            ..schedule()
        },
    )
    .await
    .expect("zero lead");
    let old = clip(&pool, "owner", "old", utc("2025-01-01T00:00:00Z")).await;
    let current = clip(&pool, "owner", "current", utc("2026-01-01T00:00:00Z")).await;
    let now = utc("2026-10-12T15:59:30Z");
    assert_eq!(
        plan_owner(&pool, "owner", now - Duration::minutes(2))
            .await
            .expect("before poll window")
            .created,
        0
    );
    assert_eq!(
        plan_owner(&pool, "owner", now)
            .await
            .expect("zero lead activation")
            .created,
        1
    );
    let first = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("first queued clip");
    assert_eq!(first[0].clip_id, old);
    choose_year(&pool, 2026, ClipSort::OldestFirst).await;
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            lead_time_hours: Some(1),
            schedule_slots: Some(vec![
                ScheduleSlot {
                    weekday: 1,
                    local_time: "18:00".into(),
                },
                ScheduleSlot {
                    weekday: 1,
                    local_time: "18:30".into(),
                },
            ]),
            ..Default::default()
        },
    )
    .await
    .expect("next normal demand");
    let report = plan_owner(&pool, "owner", now)
        .await
        .expect("actual changed M1 filter");
    assert_eq!(report.created, 1);
    let after = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("preserved queue");
    assert_eq!(after[0].id, first[0].id);
    assert_eq!(after[0].clip_id, old);
    assert!(
        after[0]
            .scheduled_at
            .is_some_and(|slot| slot > first[0].scheduled_at.expect("old scheduled slot"))
    );
    assert_eq!(after[1].clip_id, current);
    cleanup(pool).await;
}

#[tokio::test]
async fn exhausted_crash_recovery_keeps_checkpoint_and_releases_running_constraint() {
    let pool = pool().await;
    let id = clip(&pool, "owner", "interrupted", utc("2025-01-01T00:00:00Z")).await;
    let queued = job(&pool, "owner", id, Some(ProductionMode::Ai)).await;
    let claimed = production_jobs::claim_next(&pool)
        .await
        .expect("claim")
        .expect("running job");
    production_jobs::update_progress(
        &pool,
        "owner",
        queued.id,
        "rendering",
        &json!({"analysis":{"saved":true}}),
    )
    .await
    .expect("checkpoint");
    sqlx::query("UPDATE social_production_jobs SET attempts = max_attempts WHERE id = $1")
        .bind(queued.id)
        .execute(&pool)
        .await
        .expect("attempt limit reached");
    assert_eq!(
        recover_after_lease(&pool).await.expect("bounded recovery"),
        Recovery {
            resumed: 0,
            failed: 1
        }
    );
    let recovered = production_jobs::get(&pool, "owner", queued.id)
        .await
        .expect("job")
        .expect("kept history");
    assert_eq!(recovered.mode, claimed.mode);
    assert_eq!(recovered.metadata["analysis"]["saved"], true);
    assert_eq!(recovered.status, "failed");
    let running: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM social_production_jobs WHERE status = 'running'")
            .fetch_one(&pool)
            .await
            .expect("running constraint clear");
    assert_eq!(running, 0);
    cleanup(pool).await;
}

#[tokio::test]
async fn one_failed_or_review_job_does_not_block_the_next_clip_in_both_publish_modes() {
    for full_auto in [false, true] {
        for failure in ["resource", "retry_exhausted", "recovery_exhausted"] {
            let pool = pool().await;
            choose_year(&pool, 2025, ClipSort::OldestFirst).await;
            save_schedule(
                &pool,
                "owner",
                &SchedulePatch {
                    lead_time_hours: Some(720),
                    ..schedule()
                },
            )
            .await
            .expect("enabled schedule with an eligible next weekly slot");
            if full_auto {
                let mut settings = autopilot_settings::load(&pool, "owner")
                    .await
                    .expect("settings");
                settings.publish_mode = autopilot_settings::PublishMode::FullAuto;
                autopilot_settings::save(&pool, "owner", &settings)
                    .await
                    .expect("Full Auto");
            }
            let id = clip(&pool, "owner", "held", utc("2025-01-01T00:00:00Z")).await;
            let next = clip(&pool, "owner", "next", utc("2025-01-02T00:00:00Z")).await;
            let queued = job(&pool, "owner", id, None).await;
            let mut running = production_jobs::claim_next(&pool)
                .await
                .expect("claim")
                .expect("automatic job");
            assert_eq!(running.id, queued.id);
            if failure != "resource" {
                sqlx::query(
                    "UPDATE social_production_jobs SET attempts = max_attempts WHERE id = $1",
                )
                .bind(running.id)
                .execute(&pool)
                .await
                .expect("last permitted attempt");
                running.attempts = running.max_attempts;
            }
            if failure == "recovery_exhausted" {
                assert_eq!(
                    recover_after_lease(&pool)
                        .await
                        .expect("actual recovery")
                        .failed,
                    1
                );
            } else {
                retry_or_review(
                    &pool,
                    &running,
                    "resource_unavailable",
                    failure != "resource",
                )
                .await
                .expect("terminal outcome");
            }
            let ended = production_jobs::get(&pool, "owner", queued.id)
                .await
                .expect("job")
                .expect("kept history");
            let expected = if failure == "resource" {
                "review"
            } else {
                "failed"
            };
            assert_eq!(ended.status, expected);
            let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
                .fetch_one(&pool)
                .await
                .expect("claim clock");
            let report = plan_owner(&pool, "owner", now + Duration::minutes(2))
                .await
                .expect("planning continues");
            assert_ne!(report.production_hold, Some(ProductionHold::ReviewBacklog));
            assert_eq!(report.created, 1);
            let claimed = production_jobs::claim_next(&pool)
                .await
                .expect("claim next")
                .expect("next clip produced");
            assert_eq!(claimed.clip_id, next);
            cleanup(pool).await;
        }
    }
}

async fn park_in_review(pool: &sqlx::PgPool, clip_id: i64) -> i64 {
    let queued = job(pool, "owner", clip_id, None).await;
    let running = production_jobs::claim_next(pool)
        .await
        .expect("claim")
        .expect("automatic job");
    assert_eq!(running.id, queued.id);
    retry_or_review(pool, &running, "resource_unavailable", false)
        .await
        .expect("review outcome");
    queued.id
}

#[tokio::test]
async fn two_review_jobs_hold_until_one_is_resolved() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("enabled schedule");
    let first = clip(&pool, "owner", "first", utc("2025-01-01T00:00:00Z")).await;
    let second = clip(&pool, "owner", "second", utc("2025-01-02T00:00:00Z")).await;
    clip(&pool, "owner", "third", utc("2025-01-03T00:00:00Z")).await;
    let skipped = park_in_review(&pool, first).await;
    let processed = park_in_review(&pool, second).await;
    let held = plan_owner(&pool, "owner", utc("2026-10-11T18:00:00Z"))
        .await
        .expect("held planning");
    assert_eq!(held.production_hold, Some(ProductionHold::ReviewBacklog));
    assert_eq!(held.created, 0);
    assert!(matches!(
        resolve_review(&pool, "other", skipped, ReviewDecision::Skip, None).await,
        Err(ScheduleError::NotFound)
    ));
    resolve_review(
        &pool,
        "owner",
        skipped,
        ReviewDecision::Skip,
        Some("  Passt nicht  "),
    )
    .await
    .expect("skip");
    let skipped_job = production_jobs::get(&pool, "owner", skipped)
        .await
        .expect("job")
        .expect("skipped job");
    assert_eq!(skipped_job.status, "cancelled");
    assert_eq!(skipped_job.metadata["review_decision"]["action"], "skip");
    assert_eq!(
        skipped_job.metadata["review_decision"]["note"],
        "Passt nicht"
    );
    assert!(matches!(
        resolve_review(&pool, "owner", skipped, ReviewDecision::Process, None).await,
        Err(ScheduleError::NotInReview)
    ));
    resolve_review(&pool, "owner", processed, ReviewDecision::Process, None)
        .await
        .expect("process anyway");
    let report = plan_owner(&pool, "owner", utc("2026-10-11T18:00:00Z"))
        .await
        .expect("planning resumes");
    assert_ne!(report.production_hold, Some(ProductionHold::ReviewBacklog));
    let state = snapshot(&pool, "owner").await.expect("snapshot");
    let first_queued = state
        .jobs
        .iter()
        .find(|job| job.status == "queued")
        .expect("queued job");
    assert_eq!(first_queued.id, processed);
    assert_eq!(first_queued.attempts, 0);
    assert!(first_queued.review_reasons.is_empty());
    cleanup(pool).await;
}

async fn waiter_behind(pool: &sqlx::PgPool, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE datname = current_database()
                AND $1 = ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(pool)
            .await
            .expect("actual PostgreSQL blocking chain");
            if let Some(pid) = waiting {
                return pid;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("transaction reaches its controlled database lock")
}

async fn queue_change_during_claim(mark: bool, deferred_head: bool) {
    let pool = pool().await;
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            ai_editing_enabled: Some(true),
            ..schedule()
        },
    )
    .await
    .expect("enabled automatic AI queue");
    let first_clip = clip(&pool, "owner", "stale", utc("2025-01-01T00:00:00Z")).await;
    let first = job(&pool, "owner", first_clip, None).await;
    let foreign_clip = clip(&pool, "other", "foreign", utc("2025-01-01T00:00:00Z")).await;
    let foreign = job(&pool, "other", foreign_clip, Some(ProductionMode::Static)).await;
    let next_clip = clip(&pool, "owner", "promoted", utc("2025-01-02T00:00:00Z")).await;
    let next = job(&pool, "owner", next_clip, None).await;
    if deferred_head {
        sqlx::query("UPDATE social_production_jobs SET metadata = '{\"retry_after\":\"infinity\"}'::jsonb WHERE id = $1")
            .bind(next.id).execute(&pool).await.expect("ineligible retry without a clock deadline");
    }
    let mut row_tx = pool.begin().await.expect("controlled queue row blocker");
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *row_tx)
        .await
        .expect("blocker's backend");
    sqlx::query("SELECT id FROM social_production_jobs WHERE twitch_user_id = 'owner' ORDER BY queue_order, id FOR UPDATE")
        .fetch_all(&mut *row_tx).await.expect("block only queue rows, not the owner advisory lock");
    let queue_pool = pool.clone();
    let first_id = first.id;
    let next_id = next.id;
    let mutation = tokio::spawn(async move {
        if mark {
            mark_next(&queue_pool, "owner", next_id).await
        } else {
            reorder(&queue_pool, "owner", &[next_id, first_id]).await
        }
    });
    let queue_pid = waiter_behind(&pool, blocker).await;
    let claim_pool = pool.clone();
    let claim = tokio::spawn(async move { production_jobs::claim_next(&claim_pool).await });
    let claim_pid = waiter_behind(&pool, queue_pid).await;
    let owner_wait: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_locks waiting JOIN pg_locks held
        ON waiting.locktype = held.locktype AND waiting.database = held.database
        AND waiting.classid = held.classid AND waiting.objid = held.objid
        AND waiting.objsubid = held.objsubid
        WHERE waiting.pid = $1 AND held.pid = $2 AND waiting.locktype = 'advisory'
        AND NOT waiting.granted AND held.granted)",
    )
    .bind(claim_pid)
    .bind(queue_pid)
    .fetch_one(&pool)
    .await
    .expect("claim is behind the actual queue owner's advisory lock");
    assert!(owner_wait);
    assert!(!mutation.is_finished());
    assert!(!claim.is_finished());
    row_tx
        .commit()
        .await
        .expect("let actual queue mutation commit");
    mutation
        .await
        .expect("queue task")
        .expect("committed queue change");
    let claimed = claim
        .await
        .expect("claim task")
        .expect("locked head selection");
    let mut expected_stale = first.clone();
    expected_stale.queue_order = next.queue_order;
    let stale = production_jobs::get(&pool, "owner", first.id)
        .await
        .expect("stale job state")
        .expect("stale job preserved");
    assert_eq!(
        serde_json::to_value(&stale).expect("stale post-claim state"),
        serde_json::to_value(&expected_stale).expect("only the requested queue order changed")
    );
    if deferred_head {
        assert!(claimed.is_none());
        let queued_next = production_jobs::get(&pool, "owner", next.id)
            .await
            .expect("deferred head")
            .expect("preserved deferred head");
        assert_eq!(queued_next.status, "queued");
        assert_eq!(queued_next.attempts, 0);
        assert_eq!(queued_next.stage, next.stage);
        assert_eq!(queued_next.mode, None);
        assert_eq!(queued_next.metadata["retry_after"], "infinity");
    } else {
        let claimed = claimed.expect("committed new eligible head is claimed");
        assert_eq!(claimed.id, next.id);
        assert_eq!(claimed.mode, Some(ProductionMode::Ai));
        assert_eq!(claimed.attempts, 1);
        assert_eq!(claimed.stage, "starting");
        production_jobs::finish(&pool, "owner", next.id, JobCompletion::Completed, &[], None)
            .await
            .expect("release global running slot");
    }
    let global_next = production_jobs::claim_next(&pool)
        .await
        .expect("next poll")
        .expect("global order retained");
    assert_eq!(global_next.id, foreign.id);
    assert_eq!(global_next.mode, Some(ProductionMode::Static));
    assert_eq!(global_next.queue_order, foreign.queue_order);
    assert_eq!(global_next.attempts, 1);
    let outputs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM social_produced_videos")
        .fetch_one(&pool)
        .await
        .expect("no immutable version created by claim");
    assert_eq!(outputs, 0);
    cleanup(pool).await;
}

#[tokio::test]
async fn reorder_committing_while_claim_waits_reselects_the_eligible_global_head() {
    for deferred_head in [false, true] {
        queue_change_during_claim(false, deferred_head).await;
    }
}

#[tokio::test]
async fn mark_next_committing_while_claim_waits_reselects_the_eligible_global_head() {
    for deferred_head in [false, true] {
        queue_change_during_claim(true, deferred_head).await;
    }
}

#[tokio::test]
async fn clearing_schedule_while_claim_waits_for_owner_prevents_first_start() {
    let pool = pool().await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("weekly schedule before candidate selection");
    let id = clip(
        &pool,
        "owner",
        "locked-candidate",
        utc("2025-01-01T00:00:00Z"),
    )
    .await;
    let queued = job(&pool, "owner", id, None).await;
    let mut owner_tx = pool.begin().await.expect("schedule edit transaction");
    autopilot_settings::lock_owner(&mut owner_tx, "owner")
        .await
        .expect("actual schedule owner lock");
    let claim_pool = pool.clone();
    let claim = tokio::spawn(async move { production_jobs::claim_next(&claim_pool).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks l
                JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.datname = current_database()
                AND l.locktype = 'advisory' AND NOT l.granted)",
            )
            .fetch_one(&pool)
            .await
            .expect("real claim waiting behind schedule editor");
            if waiting {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("candidate selected before locked recheck");
    sqlx::query("UPDATE social_media_autopilot_settings SET schedule_slots = '[]'::jsonb WHERE twitch_user_id = 'owner'")
        .execute(&mut *owner_tx)
        .await
        .expect("clear weekly slots under the real owner lock");
    owner_tx.commit().await.expect("release changed schedule");
    assert!(
        claim
            .await
            .expect("claim task")
            .expect("locked recheck")
            .is_none()
    );
    let unchanged = production_jobs::get(&pool, "owner", queued.id)
        .await
        .expect("queue after rejected claim")
        .expect("preserved job");
    assert_eq!(
        serde_json::to_value(unchanged).expect("post-claim state"),
        serde_json::to_value(queued).expect("pre-claim state")
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn completion_retry_and_recovery_lock_owner_before_the_job_row() {
    for transition in ["finish", "retry", "recover", "exhausted"] {
        let pool = pool().await;
        let id = clip(&pool, "owner", transition, utc("2025-01-01T00:00:00Z")).await;
        production_jobs::insert(
            &pool,
            &NewProductionJob {
                twitch_user_id: "owner".into(),
                clip_id: id,
                source: JobSource::Manual,
                requested_mode: Some(ProductionMode::Static),
                scheduled_at: Some(utc("2030-10-12T16:00:00Z")),
                max_attempts: 3,
            },
        )
        .await
        .expect("scheduled reservation");
        let running = production_jobs::claim_next(&pool)
            .await
            .expect("claim")
            .expect("running reservation");
        if transition == "exhausted" {
            sqlx::query("UPDATE social_production_jobs SET attempts = max_attempts WHERE id = $1")
                .bind(running.id)
                .execute(&pool)
                .await
                .expect("exhausted interruption");
        }
        let mut owner_tx = pool.begin().await.expect("queue owner's transaction");
        autopilot_settings::lock_owner(&mut owner_tx, "owner")
            .await
            .expect("actual queue owner lock");
        let task_pool = pool.clone();
        let job_id = running.id;
        let mutation = tokio::spawn(async move {
            match transition {
                "finish" => production_jobs::finish(
                    &task_pool,
                    "owner",
                    job_id,
                    JobCompletion::Completed,
                    &[],
                    None,
                )
                .await
                .map_err(ScheduleError::from),
                "retry" => retry_or_review(&task_pool, &running, "transient", true).await,
                _ => recover_after_lease(&task_pool).await.map(|_| ()),
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM pg_locks l
                    JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.datname = current_database()
                    AND l.locktype = 'advisory' AND NOT l.granted)",
                )
                .fetch_one(&pool)
                .await
                .expect("real database lock wait");
                if waiting {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("mutation waits for owner before touching the row");
        sqlx::query("SELECT id FROM social_production_jobs WHERE id = $1 FOR UPDATE NOWAIT")
            .bind(job_id)
            .fetch_one(&mut *owner_tx)
            .await
            .expect("queue can lock the row without an owner/job cycle");
        owner_tx.commit().await.expect("release queue owner");
        mutation
            .await
            .expect("mutation task")
            .expect("transition without deadlock");
        cleanup(pool).await;
    }
}

fn slot(weekday: u8, local_time: &str) -> ScheduleSlot {
    ScheduleSlot {
        weekday,
        local_time: local_time.into(),
    }
}

async fn targets(pool: &PgPool, owner: &str, platforms: &[&str], patch: SchedulePatch) {
    for platform in platforms {
        sqlx::query("INSERT INTO social_media_platform_auth (platform, twitch_user_id, enabled) VALUES ($1, $2, 1)")
            .bind(platform).bind(owner).execute(pool).await.expect("connected destination");
    }
    save_schedule(
        pool,
        owner,
        &SchedulePatch {
            target_platforms: Some(platforms.iter().map(|value| (*value).to_owned()).collect()),
            ..patch
        },
    )
    .await
    .expect("targeted schedule");
}

async fn policy(
    pool: &PgPool,
    owner: &str,
    scope: &str,
    slots: serde_json::Value,
    distribution: &str,
) {
    sqlx::query("UPDATE social_media_autopilot_settings SET pipeline_mode = 'upload', schedule_scope = $2,
        platform_schedule_slots = $3::text::jsonb, distribution_policy = $4 WHERE twitch_user_id = $1")
        .bind(owner).bind(scope).bind(slots.to_string()).bind(distribution)
        .execute(pool).await.expect("canonical policy");
}

async fn reserved(pool: &PgPool, owner: &str) -> Vec<(i64, String, DateTime<Utc>)> {
    sqlx::query_as(
        "SELECT job_id, platform, scheduled_at FROM social_publication_reservations
        WHERE twitch_user_id = $1 AND state = 'reserved' ORDER BY scheduled_at, platform, job_id",
    )
    .bind(owner)
    .fetch_all(pool)
    .await
    .expect("reservations")
}

fn planned(report: Result<PlanReport, ScheduleError>) -> u64 {
    report.expect("planner").created
}

#[tokio::test]
async fn shared_schedule_reserves_every_destination_at_several_times_per_weekday() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let patch = SchedulePatch {
        lead_time_hours: Some(48),
        schedule_slots: Some(vec![slot(1, "08:00"), slot(1, "22:00")]),
        ..schedule()
    };
    targets(&pool, "owner", &["youtube", "tiktok"], patch).await;
    for name in ["a", "b", "c"] {
        clip(&pool, "owner", name, utc("2025-02-01T00:00:00Z")).await;
    }
    let now = utc("2026-10-11T18:00:00Z");
    let (first, second) = tokio::join!(
        plan_owner(&pool, "owner", now),
        plan_owner(&pool, "owner", now)
    );
    assert_eq!(planned(first) + planned(second), 2);
    let jobs = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("queue");
    let morning = utc("2026-10-12T06:00:00Z");
    let evening = utc("2026-10-12T20:00:00Z");
    assert_eq!(
        reserved(&pool, "owner").await,
        vec![
            (jobs[0].id, "tiktok".into(), morning),
            (jobs[0].id, "youtube".into(), morning),
            (jobs[1].id, "tiktok".into(), evening),
            (jobs[1].id, "youtube".into(), evening),
        ]
    );
    let state = snapshot(&pool, "owner").await.expect("snapshot");
    let next = utc("2026-10-19T06:00:00Z");
    assert_eq!(state.next_slot, Some(next));
    assert_eq!(state.next_slots.get("youtube"), Some(&next));
    assert_eq!(state.next_slots.get("tiktok"), Some(&next));
    assert_eq!(state.jobs[0].reservations.len(), 2);
    cleanup(pool).await;
}

#[tokio::test]
async fn per_platform_schedules_take_the_next_free_slot_per_destination() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let patch = SchedulePatch {
        lead_time_hours: Some(48),
        ..schedule()
    };
    targets(&pool, "owner", &["youtube", "tiktok"], patch).await;
    policy(
        &pool,
        "owner",
        "per_platform",
        json!({"youtube":[{"weekday":1,"local_time":"08:00"}],
            "tiktok":[{"weekday":1,"local_time":"12:00"},{"weekday":1,"local_time":"20:00"}]}),
        "same_clip",
    )
    .await;
    let legacy: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (twitch_user_id, clip_id, clip_title, streamer_login, clip_url, source_kind)
        VALUES ('owner','legacy','legacy','owner','https://clips.test/legacy','manual_upload') RETURNING id::bigint")
        .fetch_one(&pool).await.expect("committed legacy clip");
    sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,platform,status,scheduled_at) VALUES ($1,'tiktok','pending',$2)")
        .bind(legacy).bind(utc("2026-10-12T10:00:00Z")).execute(&pool).await.expect("committed legacy upload");
    clip(&pool, "owner", "a", utc("2025-02-01T00:00:00Z")).await;
    clip(&pool, "owner", "b", utc("2025-03-01T00:00:00Z")).await;
    let report = plan_owner(&pool, "owner", utc("2026-10-11T18:00:00Z"))
        .await
        .expect("per destination planner");
    assert_eq!(report.created, 1);
    let jobs = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("queue");
    assert_eq!(jobs[0].scheduled_at, Some(utc("2026-10-12T06:00:00Z")));
    assert_eq!(
        reserved(&pool, "owner").await,
        vec![
            (jobs[0].id, "youtube".into(), utc("2026-10-12T06:00:00Z")),
            (jobs[0].id, "tiktok".into(), utc("2026-10-12T18:00:00Z")),
        ]
    );
    policy(
        &pool,
        "owner",
        "per_platform",
        json!({"tiktok":[]}),
        "same_clip",
    )
    .await;
    let held = plan_owner(&pool, "owner", utc("2026-10-11T18:00:00Z"))
        .await
        .expect("no schedule");
    assert_eq!(held.production_hold, Some(ProductionHold::NoSchedule));
    assert_eq!(held.created, 0);
    cleanup(pool).await;
}

#[tokio::test]
async fn independent_clips_give_each_destination_its_own_clip() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let patch = SchedulePatch {
        lead_time_hours: Some(48),
        ..schedule()
    };
    targets(&pool, "owner", &["youtube", "tiktok"], patch).await;
    policy(&pool, "owner", "shared", json!({}), "independent_clips").await;
    for name in ["a", "b", "c"] {
        clip(&pool, "owner", name, utc("2025-02-01T00:00:00Z")).await;
    }
    let now = utc("2026-10-11T18:00:00Z");
    assert_eq!(planned(plan_owner(&pool, "owner", now).await), 2);
    assert_eq!(planned(plan_owner(&pool, "owner", now).await), 0);
    let jobs = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("queue");
    assert_eq!(jobs.len(), 2);
    assert_ne!(jobs[0].clip_id, jobs[1].clip_id);
    let slots = reserved(&pool, "owner").await;
    let at = utc("2026-10-12T16:00:00Z");
    assert_eq!(slots.len(), 2);
    assert!(slots.iter().all(|slot| slot.2 == at));
    assert_ne!(slots[0].0, slots[1].0);
    assert_ne!(slots[0].1, slots[1].1);
    mark_next(&pool, "owner", jobs[1].id)
        .await
        .expect("reorder");
    let after = reserved(&pool, "owner").await;
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|slot| slot.2 == at));
    cleanup(pool).await;
}

#[tokio::test]
async fn dst_retry_pause_and_missing_targets_keep_reservations_consistent() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let patch = SchedulePatch {
        lead_time_hours: Some(72),
        schedule_slots: Some(vec![slot(1, "08:00")]),
        ..schedule()
    };
    targets(&pool, "owner", &["youtube"], patch).await;
    clip(&pool, "owner", "a", utc("2025-02-01T00:00:00Z")).await;
    let now = utc("2026-10-24T12:00:00Z");
    assert_eq!(planned(plan_owner(&pool, "owner", now).await), 1);
    let winter = utc("2026-10-26T07:00:00Z");
    let jobs = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("queue");
    assert_eq!(jobs[0].scheduled_at, Some(winter));
    sqlx::query("UPDATE social_production_jobs SET status = 'running', mode = 'static', started_at = NOW(), attempts = attempts + 1 WHERE id = $1")
        .bind(jobs[0].id)
        .execute(&pool)
        .await
        .expect("running job");
    let claimed = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("running queue")
        .remove(0);
    assert_eq!(reserved(&pool, "owner").await.len(), 1);
    retry_or_review(&pool, &claimed, "temporary", true)
        .await
        .expect("retry");
    assert_eq!(
        reserved(&pool, "owner").await,
        vec![(jobs[0].id, "youtube".into(), winter)]
    );
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            paused: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("pause");
    let paused = plan_owner(&pool, "owner", now)
        .await
        .expect("paused planner");
    assert_eq!(paused.production_hold, Some(ProductionHold::Paused));
    assert_eq!(reserved(&pool, "owner").await.len(), 1);
    sqlx::query("SELECT social_clip_manage_owner('other')")
        .execute(&pool)
        .await
        .expect("managed other owner");
    save_schedule(&pool, "other", &schedule())
        .await
        .expect("untargeted schedule");
    clip(&pool, "other", "untargeted", utc("2025-02-01T00:00:00Z")).await;
    let report = plan_owner(&pool, "other", utc("2026-10-12T15:59:30Z"))
        .await
        .expect("untargeted planner");
    assert_eq!(report.created, 1);
    let untargeted = production_jobs::list(&pool, "other", 500)
        .await
        .expect("untargeted queue");
    assert_eq!(
        untargeted[0].scheduled_at,
        Some(utc("2026-10-12T16:00:00Z"))
    );
    assert!(reserved(&pool, "other").await.is_empty());
    cleanup(pool).await;
}
