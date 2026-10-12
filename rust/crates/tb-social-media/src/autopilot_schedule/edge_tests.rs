use super::*;

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
    sqlx::query("INSERT INTO social_media_autopilot_settings (twitch_user_id, clip_sort) VALUES ('owner','newest_first')").execute(&pool).await.expect("chosen sorting");
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("schedule");
    clip(&pool, "owner", "old", utc("2025-01-01T00:00:00Z")).await;
    let newest = clip(&pool, "owner", "newest", utc("2025-12-01T00:00:00Z")).await;
    clip(&pool, "owner", "excluded", utc("2026-01-01T00:00:00Z")).await;
    let now = utc("2026-10-11T18:00:00Z");
    assert_eq!(
        plan_owner(&pool, "owner", now, bounds)
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
    let report = plan_owner(&pool, "owner", now, |_, _| {
        Ok((utc("1970-01-01T00:00:00Z"), utc("1971-01-01T00:00:00Z")))
    })
    .await
    .expect("empty envelope");
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
        plan_owner(&pool, "owner", now - Duration::minutes(2), bounds)
            .await
            .expect("before poll window")
            .created,
        0
    );
    assert_eq!(
        plan_owner(&pool, "owner", now, bounds)
            .await
            .expect("zero lead activation")
            .created,
        1
    );
    let first = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("first queued clip");
    assert_eq!(first[0].clip_id, old);
    sqlx::query("UPDATE social_media_autopilot_settings SET clip_date_filter = '{\"kind\":\"year\",\"year\":2026}' WHERE twitch_user_id = 'owner'").execute(&pool).await.expect("changed pool filter");
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
    let report = plan_owner(&pool, "owner", now, |_, _| {
        Ok((utc("2026-01-01T00:00:00Z"), now))
    })
    .await
    .expect("new selection envelope");
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
