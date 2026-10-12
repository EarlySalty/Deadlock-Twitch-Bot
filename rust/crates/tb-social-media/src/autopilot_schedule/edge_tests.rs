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
        hold(&settings, 2, false, next, now),
        Some(ProductionHold::ReviewBacklog)
    );
    settings.publish_mode = autopilot_settings::PublishMode::FullAuto;
    assert_eq!(hold(&settings, 2, false, next, now), None);
    assert_eq!(
        hold(&settings, 1, true, next, now),
        Some(ProductionHold::ReviewBacklog)
    );
    settings.paused = true;
    assert_eq!(
        hold(&settings, 2, false, next, now),
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
    assert!(snapshot
        .jobs
        .iter()
        .all(|job| job.clip_title == "same-clip"));
    let public = serde_json::to_value(&snapshot).expect("public snapshot");
    assert!(public["jobs"]
        .as_array()
        .expect("queue array")
        .iter()
        .all(|job| job.get("metadata").is_none() && job.get("internal_analysis").is_none()));
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
    assert_eq!(after[0].scheduled_at, first[0].scheduled_at);
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
            held: 1
        }
    );
    let recovered = production_jobs::get(&pool, "owner", queued.id)
        .await
        .expect("job")
        .expect("kept history");
    assert_eq!(recovered.mode, claimed.mode);
    assert_eq!(recovered.metadata["analysis"]["saved"], true);
    assert_eq!(recovered.status, "review");
    let running: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM social_production_jobs WHERE status = 'running'")
            .fetch_one(&pool)
            .await
            .expect("running constraint clear");
    assert_eq!(running, 0);
    cleanup(pool).await;
}

#[tokio::test]
async fn automatic_retry_and_recovery_reviews_stop_reselection_in_both_publish_modes() {
    for full_auto in [false, true] {
        for failure in ["resource", "retry_exhausted", "recovery_exhausted"] {
            let pool = pool().await;
            choose_year(&pool, 2025, ClipSort::OldestFirst).await;
            save_schedule(&pool, "owner", &schedule())
                .await
                .expect("enabled schedule");
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
            let untouched = clip(&pool, "owner", "untouched", utc("2025-01-02T00:00:00Z")).await;
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
                        .held,
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
                .expect("actual terminal retry hold");
            }
            for week in 0..4 {
                let report = plan_owner(
                    &pool,
                    "owner",
                    utc("2026-10-11T18:00:00Z") + Duration::weeks(week),
                )
                .await
                .expect("repeated planning");
                assert_eq!(report.production_hold, Some(ProductionHold::ReviewBacklog));
                assert_eq!(report.created, 0);
                assert!(production_jobs::claim_next(&pool)
                    .await
                    .expect("automatic backpressure")
                    .is_none());
            }
            let state = snapshot(&pool, "owner")
                .await
                .expect("truthful hold snapshot");
            assert_eq!(state.review_count, 1);
            assert_eq!(state.production_hold, Some(ProductionHold::ReviewBacklog));
            let selected: bool = sqlx::query_scalar(
                "SELECT selected_at IS NOT NULL FROM social_clip_pool WHERE clip_db_id = $1",
            )
            .bind(untouched)
            .fetch_one(&pool)
            .await
            .expect("pool remains eligible");
            assert!(!selected);
            let manual = job(&pool, "owner", untouched, Some(ProductionMode::Ai)).await;
            assert_eq!(
                production_jobs::claim_next(&pool)
                    .await
                    .expect("manual claim")
                    .expect("independent manual AI")
                    .id,
                manual.id
            );
            cleanup(pool).await;
        }
    }
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
