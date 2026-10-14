use super::*;

#[tokio::test]
async fn target_platforms_require_owned_enabled_connections_and_normalize() {
    let pool = pool().await;
    sqlx::raw_sql(
        "INSERT INTO social_media_platform_auth (platform, twitch_user_id, enabled)
        VALUES ('youtube', 'owner', 1), ('instagram', 'owner', 0), ('tiktok', 'other', 1)",
    )
    .execute(&pool)
    .await
    .expect("connection fixtures");
    for platform in ["instagram", "tiktok", "unknown"] {
        assert!(save_schedule(
            &pool,
            "owner",
            &SchedulePatch {
                target_platforms: Some(vec![platform.into()]),
                ..Default::default()
            }
        )
        .await
        .is_err());
    }
    let saved = save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            target_platforms: Some(vec![" YouTube ".into(), "youtube".into()]),
            ..schedule()
        },
    )
    .await
    .expect("connected targets");
    assert_eq!(saved.target_platforms, vec!["youtube"]);
    let state = snapshot(&pool, "owner").await.expect("public targets");
    assert_eq!(state.connected_platforms, vec!["youtube"]);
    assert_eq!(state.settings.target_platforms, vec!["youtube"]);
    cleanup(pool).await;
}

#[tokio::test]
async fn selected_clips_lead_in_the_user_sort_order_without_excluding_unselected_clips() {
    for sort in [ClipSort::OldestFirst, ClipSort::NewestFirst] {
        let pool = pool().await;
        choose_year(&pool, 2025, sort).await;
        save_schedule(&pool, "owner", &schedule())
            .await
            .expect("schedule");
        let unselected = clip(&pool, "owner", "normal", utc("2025-01-01T00:00:00Z")).await;
        let first = clip(
            &pool,
            "owner",
            "selected-first",
            utc("2025-02-01T00:00:00Z"),
        )
        .await;
        let second = clip(
            &pool,
            "owner",
            "selected-second",
            utc("2025-03-01T00:00:00Z"),
        )
        .await;
        let excluded = clip(&pool, "owner", "excluded", utc("2026-01-01T00:00:00Z")).await;
        sqlx::query("UPDATE social_clip_pool SET selected_at = $1 WHERE clip_db_id = ANY($2)")
            .bind(utc("2026-10-01T00:00:00Z"))
            .bind([first, second, excluded].as_slice())
            .execute(&pool)
            .await
            .expect("selected pool entries");
        let expected = if sort == ClipSort::OldestFirst {
            [first, second, unselected]
        } else {
            [second, first, unselected]
        };
        for id in expected {
            let report = plan_owner(&pool, "owner", utc("2026-10-11T18:00:00Z"))
                .await
                .expect("next demand");
            assert_eq!(report.created, 1);
            let queued = production_jobs::list(&pool, "owner", 500)
                .await
                .expect("queue");
            let selected = queued
                .iter()
                .find(|job| job.status == "queued")
                .expect("selected clip");
            assert_eq!(selected.clip_id, id);
            cancel(&pool, "owner", selected.id)
                .await
                .expect("release slot");
        }
        assert_eq!(
            plan_owner(&pool, "owner", utc("2026-10-11T18:00:00Z"))
                .await
                .expect("strict filter")
                .created,
            0
        );
        cleanup(pool).await;
    }
}

#[tokio::test]
async fn unsuitable_decisions_preserve_explanations_and_do_not_block_full_auto() {
    let pool = pool().await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("schedule");
    sqlx::query("UPDATE social_media_autopilot_settings SET publish_mode = 'full_auto' WHERE twitch_user_id = 'owner'")
        .execute(&pool).await.expect("full auto");
    let mut ids = Vec::new();
    for name in ["first", "second", "third"] {
        let clip_id = clip(&pool, "owner", name, utc("2025-01-01T00:00:00Z")).await;
        let queued = job(&pool, "owner", clip_id, None).await;
        let claimed = production_jobs::claim_next(&pool)
            .await
            .expect("claim")
            .expect("reviews do not stop full auto");
        assert_eq!(claimed.id, queued.id);
        production_jobs::flag_unsuitable(
            &pool,
            "owner",
            claimed.id,
            "Die Szene ist unvollständig.",
        )
        .await
        .expect("unsuitable outcome");
        ids.push(claimed.id);
    }
    let state = snapshot(&pool, "owner").await.expect("review explanations");
    assert_ne!(state.production_hold, Some(ProductionHold::ReviewBacklog));
    assert_eq!(
        state.jobs[0].review_explanation.as_deref(),
        Some("Die Szene ist unvollständig.")
    );
    resolve_review(
        &pool,
        "owner",
        ids[0],
        ReviewDecision::Process,
        Some("Szene trotzdem verwenden"),
    )
    .await
    .expect("explicit user decision");
    let job = production_jobs::get(&pool, "owner", ids[0])
        .await
        .expect("stored job")
        .expect("owned job");
    assert_eq!(
        job.metadata["review_decision"]["reasons"],
        json!([production_jobs::UNSUITABLE_REASON])
    );
    assert_eq!(
        job.metadata["review_decision"]["note"],
        "Szene trotzdem verwenden"
    );
    assert_eq!(job.status, "queued");
    assert!(job.started_at.is_none());
    cleanup(pool).await;
}

#[tokio::test]
async fn manual_ai_reviews_do_not_consume_the_autopilot_backlog() {
    let pool = pool().await;
    for index in 0..2 {
        let id = clip(
            &pool,
            "owner",
            &format!("manual-{index}"),
            utc("2025-01-01T00:00:00Z"),
        )
        .await;
        let manual = job(&pool, "owner", id, Some(ProductionMode::Ai)).await;
        sqlx::query("UPDATE social_production_jobs SET status = 'completed' WHERE id = $1")
            .bind(manual.id)
            .execute(&pool)
            .await
            .expect("manual complete");
        sqlx::query("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, expires_at)
            VALUES ('owner', $1, $2, 'ai', 'review', clock_timestamp() + INTERVAL '7 days')")
            .bind(id).bind(manual.id).execute(&pool).await.expect("manual review");
    }
    assert_eq!(
        review_count_on(&mut pool.acquire().await.expect("connection"), "owner")
            .await
            .expect("automatic backlog"),
        0
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn changed_schedule_retimes_queued_and_overdue_videos_without_catch_up() {
    let pool = pool().await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("initial schedule");
    let queued_clip = clip(&pool, "owner", "queued", utc("2025-01-01T00:00:00Z")).await;
    let queued = job(&pool, "owner", queued_clip, None).await;
    let running_clip = clip(&pool, "owner", "running", utc("2025-01-02T00:00:00Z")).await;
    let running = job(&pool, "owner", running_clip, None).await;
    sqlx::query("UPDATE social_production_jobs SET status = 'running', mode = 'static', started_at = $2 WHERE id = $1")
        .bind(running.id).bind(utc("2026-10-01T00:00:00Z")).execute(&pool).await.expect("running work");
    let mut videos = Vec::new();
    for (index, at) in ["2020-01-01T00:00:00Z", "2035-01-01T00:00:00Z"]
        .into_iter()
        .enumerate()
    {
        let id = clip(
            &pool,
            "owner",
            &format!("ready-{index}"),
            utc("2025-01-03T00:00:00Z"),
        )
        .await;
        let completed = job(&pool, "owner", id, None).await;
        sqlx::query("UPDATE social_production_jobs SET status = 'completed', scheduled_at = $2 WHERE id = $1")
            .bind(completed.id).bind(utc(at)).execute(&pool).await.expect("completed reservation");
        let video: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, scheduled_at)
            VALUES ('owner', $1, $2, 'static', 'scheduled', $3) RETURNING id")
            .bind(id).bind(completed.id).bind(utc(at)).fetch_one(&pool).await.expect("ready video");
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, scheduled_at)
            VALUES ($1, $2, 'youtube', $3)")
            .bind(id).bind(video).bind(utc(at)).execute(&pool).await.expect("destination reservation");
        videos.push(video);
    }
    let before: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&pool)
        .await
        .expect("database time");
    let settings = save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            timezone: Some("UTC".into()),
            schedule_slots: Some(vec![ScheduleSlot {
                weekday: 5,
                local_time: "18:00".into(),
            }]),
            ..Default::default()
        },
    )
    .await
    .expect("retime on save");
    assert_eq!(settings.timezone, "UTC");
    let slots: Vec<DateTime<Utc>> =
        sqlx::query_scalar("SELECT scheduled_at FROM social_produced_videos ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("new shared slots");
    assert_eq!(slots.len(), 2);
    assert!(slots.iter().all(|slot| *slot > before));
    assert_eq!(slots[1] - slots[0], Duration::days(7));
    let after = production_jobs::get(&pool, "owner", queued.id)
        .await
        .expect("retimed queue")
        .expect("queued job");
    assert_eq!(after.scheduled_at, Some(slots[1] + Duration::days(7)));
    let running_after = production_jobs::get(&pool, "owner", running.id)
        .await
        .expect("running")
        .expect("running job");
    assert_eq!(running_after.started_at, Some(utc("2026-10-01T00:00:00Z")));
    assert_eq!(running_after.scheduled_at, running.scheduled_at);
    let uploads: Vec<DateTime<Utc>> =
        sqlx::query_scalar("SELECT scheduled_at FROM twitch_clips_upload_queue ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("upload slots");
    assert_eq!(uploads, slots);
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            schedule_slots: Some(vec![]),
            ..Default::default()
        },
    )
    .await
    .expect("clear schedule without stale uploads");
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_produced_videos WHERE scheduled_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("released slots");
    assert_eq!(remaining, 0);
    let approvals: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM social_produced_videos WHERE status = 'approved'")
            .fetch_one(&pool)
            .await
            .expect("preserved approvals");
    assert_eq!(approvals, videos.len() as i64);
    cleanup(pool).await;
}
