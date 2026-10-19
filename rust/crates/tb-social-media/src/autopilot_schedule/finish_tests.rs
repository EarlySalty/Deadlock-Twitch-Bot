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
        sqlx::query("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, expires_at)
            VALUES ('owner', $1, $2, 'ai', 'review', '/fixture/manual.mp4', 1000, clock_timestamp() + INTERVAL '7 days')")
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
        let video: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, scheduled_at)
            VALUES ('owner', $1, $2, 'static', 'scheduled', '/fixture/scheduled.mp4', 1000, $3) RETURNING id")
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

#[tokio::test]
async fn retiming_preserves_artifacts_of_started_unfinished_jobs() {
    for status in ["running", "queued"] {
        let pool = pool().await;
        save_schedule(&pool, "owner", &schedule())
            .await
            .expect("schedule");
        let clip_id = clip(&pool, "owner", "interrupted", utc("2025-01-01T00:00:00Z")).await;
        let job = job(&pool, "owner", clip_id, None).await;
        let original = utc("2020-01-01T00:00:00Z");
        sqlx::query("UPDATE social_production_jobs SET status = $2, mode = 'static', started_at = $3, scheduled_at = $3 WHERE id = $1")
            .bind(job.id).bind(status).bind(original).execute(&pool).await.expect("unfinished job");
        let video: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, scheduled_at)
            VALUES ('owner', $1, $2, 'static', 'review', '/fixture/interrupted.mp4', 1000, $3) RETURNING id")
            .bind(clip_id).bind(job.id).bind(original).fetch_one(&pool).await.expect("finished rendering before job completion");
        save_schedule(
            &pool,
            "owner",
            &SchedulePatch {
                timezone: Some("UTC".into()),
                ..Default::default()
            },
        )
        .await
        .expect("changed schedule");
        let slots: (Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT j.scheduled_at, v.scheduled_at FROM social_production_jobs j JOIN social_produced_videos v ON v.job_id = j.id WHERE v.id = $1")
            .bind(video).fetch_one(&pool).await.expect("consistent recovery reservations");
        assert_eq!(slots, (Some(original), Some(original)));
        cleanup(pool).await;
    }
}

#[tokio::test]
async fn skipping_releases_unpublished_review_outputs_and_preserves_other_owners() {
    let pool = pool().await;
    let mut fixtures = Vec::new();
    for owner in ["owner", "other"] {
        let clip_id = clip(
            &pool,
            owner,
            "partial-variants",
            utc("2025-01-01T00:00:00Z"),
        )
        .await;
        let job = job(&pool, owner, clip_id, None).await;
        sqlx::query("UPDATE social_production_jobs SET status = 'review', finished_at = NOW() WHERE id = $1")
            .bind(job.id).execute(&pool).await.expect("partial production review");
        let video: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, scheduled_at, metadata)
            VALUES ($1, $2, $3, 'ai', 'review', '/fixture/partial.mp4', 1000, $4, jsonb_build_object('production_ai_job_id', $3::bigint)) RETURNING id")
            .bind(owner).bind(clip_id).bind(job.id).bind(utc("2035-01-01T00:00:00Z"))
            .fetch_one(&pool).await.expect("partial review output");
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at) VALUES ($1, $2, 'youtube', $3, $4)")
            .bind(clip_id).bind(video).bind(crate::autopilot_publish::queue_status::PENDING)
            .bind(utc("2035-01-01T00:00:00Z")).execute(&pool).await.expect("pending exact-file reservation");
        fixtures.push((job.id, video));
    }
    resolve_review(
        &pool,
        "owner",
        fixtures[0].0,
        ReviewDecision::Skip,
        Some("Nicht verwenden"),
    )
    .await
    .expect("atomic skip");
    let output: (String, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT status, scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(fixtures[0].1)
            .fetch_one(&pool)
            .await
            .expect("discarded output");
    assert_eq!(output, ("rejected".into(), None));
    let uploads: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE produced_video_id = $1",
    )
    .bind(fixtures[0].1)
    .fetch_one(&pool)
    .await
    .expect("released reservation");
    assert_eq!(uploads, 0);
    let other: (String, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT status, scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(fixtures[1].1)
            .fetch_one(&pool)
            .await
            .expect("other owner unchanged");
    assert_eq!(other, ("review".into(), Some(utc("2035-01-01T00:00:00Z"))));
    let backlog = review_count_on(&mut pool.acquire().await.expect("connection"), "owner")
        .await
        .expect("review backlog");
    assert_eq!(backlog, 0);
    let expiry = crate::produced_video::claim_expired_ai_artifacts(&pool, "owner", fixtures[0].1)
        .await
        .expect("existing AI artifact expiry consumer");
    assert_eq!(expiry.as_deref(), Some("rejected"));
    let skipped = production_jobs::get(&pool, "owner", fixtures[0].0)
        .await
        .expect("audit")
        .expect("job");
    assert_eq!(
        skipped.metadata["review_decision"]["note"],
        "Nicht verwenden"
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn skipping_static_reviews_preserves_the_existing_expiry_cleanup_path() {
    let pool = pool().await;
    let clip_id = clip(
        &pool,
        "owner",
        "skipped-static",
        utc("2025-01-01T00:00:00Z"),
    )
    .await;
    let job = job(&pool, "owner", clip_id, None).await;
    sqlx::query(
        "UPDATE social_production_jobs SET status = 'review', finished_at = NOW() WHERE id = $1",
    )
    .bind(job.id)
    .execute(&pool)
    .await
    .expect("static review job");
    let video: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, expires_at)
        VALUES ('owner', $1, $2, 'static', 'review', '/fixture/skipped.mp4', 1000, clock_timestamp() + INTERVAL '7 days') RETURNING id")
        .bind(clip_id).bind(job.id).fetch_one(&pool).await.expect("static artifact");
    resolve_review(&pool, "owner", job.id, ReviewDecision::Skip, None)
        .await
        .expect("skip static output");
    let cleanup_due: bool = sqlx::query_scalar(
        "SELECT status = 'review' AND expires_at <= clock_timestamp()
        AND publication_executed_at IS NULL AND scheduled_at IS NULL
        AND NOT (metadata ? 'manual_ai_job_id') AND NOT (metadata ? 'production_ai_job_id')
        FROM social_produced_videos WHERE id = $1",
    )
    .bind(video)
    .fetch_one(&pool)
    .await
    .expect("existing publisher expiry predicate");
    assert!(cleanup_due);
    assert_eq!(
        review_count_on(&mut pool.acquire().await.expect("connection"), "owner")
            .await
            .expect("backlog released"),
        0
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn skipped_partial_variants_cannot_be_promoted_or_published() {
    use crate::autopilot_publish::{
        DestinationPublisher, PublishExecutor, PublishOutcome, PublishRequest,
    };
    use crate::uploaders::UploadError;
    use std::sync::Arc;

    struct NoPublication;

    #[async_trait::async_trait]
    impl DestinationPublisher for NoPublication {
        async fn publish(
            &self,
            _: &str,
            _: &str,
            _: &PublishRequest,
        ) -> Result<PublishOutcome, UploadError> {
            panic!("skipped variants must not reach the publication boundary")
        }
    }

    for mode in ["ai", "static"] {
        for status in ["review", "approved", "scheduled"] {
            let pool = pool().await;
            save_schedule(&pool, "owner", &schedule())
                .await
                .expect("schedule");
            sqlx::raw_sql("INSERT INTO social_media_platform_auth (platform, twitch_user_id, enabled) VALUES ('youtube', 'owner', 1);
                UPDATE social_media_autopilot_settings SET publish_mode = 'full_auto', target_platforms = '[\"youtube\"]'::jsonb WHERE twitch_user_id = 'owner'")
                .execute(&pool).await.expect("full-auto fixture with an owned connection");
            let clip_id = clip(
                &pool,
                "owner",
                "partial-variant",
                utc("2025-01-01T00:00:00Z"),
            )
            .await;
            let job = job(&pool, "owner", clip_id, None).await;
            sqlx::query("UPDATE social_production_jobs SET status = 'review', finished_at = NOW() WHERE id = $1")
                .bind(job.id).execute(&pool).await.expect("one variant failed after another passed validation");
            let metadata = if mode == "ai" {
                json!({"production_ai_job_id":job.id})
            } else {
                json!({})
            };
            let video: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos
                (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, scheduled_at, needs_review, expires_at, metadata)
                VALUES ('owner', $1, $2, $3, $4, '/fixture/skipped-variant.mp4', 1000, $5, FALSE, clock_timestamp() + INTERVAL '7 days', $6) RETURNING id")
                .bind(clip_id).bind(job.id).bind(mode).bind(status).bind(utc("2035-01-01T00:00:00Z"))
                .bind(metadata).fetch_one(&pool).await.expect("validated partial output");
            sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at)
                VALUES ($1, $2, 'youtube', $3, $4)")
                .bind(clip_id).bind(video).bind(crate::autopilot_publish::queue_status::PENDING)
                .bind(utc("2035-01-01T00:00:00Z")).execute(&pool).await.expect("pending reservation");
            resolve_review(&pool, "owner", job.id, ReviewDecision::Skip, None)
                .await
                .expect("skip every unpublished variant");
            let state: (String, bool, Option<DateTime<Utc>>, bool) = sqlx::query_as(
                "SELECT status, needs_review, scheduled_at, expires_at <= clock_timestamp()
                FROM social_produced_videos WHERE id = $1",
            )
            .bind(video)
            .fetch_one(&pool)
            .await
            .expect("skip boundary");
            assert_eq!(
                state,
                (
                    if mode == "ai" { "rejected" } else { "review" }.into(),
                    true,
                    None,
                    true
                )
            );
            let report = PublishExecutor::new(pool.clone(), Arc::new(NoPublication))
                .tick()
                .await
                .expect("real publication executor after skip");
            assert_eq!(report.promoted, 0);
            assert_eq!(report.executed, 0);
            assert_eq!(report.uploads, 0);
            let reservations: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE produced_video_id = $1",
            )
            .bind(video)
            .fetch_one(&pool)
            .await
            .expect("no recreated reservations");
            assert_eq!(reservations, 0);
            if mode == "ai" {
                assert_eq!(
                    crate::produced_video::claim_expired_ai_artifacts(&pool, "owner", video)
                        .await
                        .expect("existing rejected-output cleanup")
                        .as_deref(),
                    Some("rejected")
                );
            }
            cleanup(pool).await;
        }
    }
}

#[tokio::test]
async fn explicit_retry_resets_unfinished_ai_attempts_without_losing_checkpoints() {
    let pool = pool().await;
    let clip_id = clip(
        &pool,
        "owner",
        "exhausted-stage",
        utc("2025-01-01T00:00:00Z"),
    )
    .await;
    let job = job(&pool, "owner", clip_id, None).await;
    let checkpoints = json!({"preparing":{"stage":"preparing","fixture":"preserved"}});
    let metadata = json!({"ai_editor":{"checkpoints":checkpoints,"attempts":{"preparing":1,"transcribing":3,"rendering":3}},
        "retry_after":"2035-01-01T00:00:00Z","static":{"stale":true},"retained":"audit"});
    sqlx::query("UPDATE social_production_jobs SET status = 'review', attempts = 3, finished_at = NOW(), metadata = $2 WHERE id = $1")
        .bind(job.id).bind(metadata).execute(&pool).await.expect("exhausted stage");
    resolve_review(&pool, "owner", job.id, ReviewDecision::Process, None)
        .await
        .expect("explicit retry");
    let resumed = production_jobs::get(&pool, "owner", job.id)
        .await
        .expect("persisted retry")
        .expect("owned job");
    assert_eq!(resumed.attempts, 0);
    assert_eq!(resumed.metadata["ai_editor"]["checkpoints"], checkpoints);
    assert_eq!(
        resumed.metadata["ai_editor"]["attempts"],
        json!({"preparing":1})
    );
    assert_eq!(resumed.metadata["retained"], "audit");
    assert!(resumed.metadata.get("retry_after").is_none());
    assert!(resumed.metadata.get("static").is_none());
    assert_eq!(resumed.status, "queued");
    cleanup(pool).await;
}

#[tokio::test]
async fn discarding_a_reviewed_test_run_returns_the_clip_to_the_pool() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let id = clip(&pool, "owner", "test-run", utc("2025-01-01T00:00:00Z")).await;
    let run = start_test_run(&pool, "owner").await.expect("test run");
    assert_eq!(run.clip_id, id);
    sqlx::query("UPDATE social_production_jobs SET status = 'completed' WHERE id = $1")
        .bind(run.job_id)
        .execute(&pool)
        .await
        .expect("test job complete");
    let video: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, expires_at)
        VALUES ('owner', $1, $2, 'static', 'review', '/fixture/test.mp4', 1000, clock_timestamp() + INTERVAL '7 days') RETURNING id")
        .bind(id).bind(run.job_id).fetch_one(&pool).await.expect("test review");
    sqlx::query("INSERT INTO social_media_clip_approval (produced_video_id, platform, state) VALUES ($1, 'youtube', 'awaiting_approval')")
        .bind(video)
        .execute(&pool)
        .await
        .expect("pending approval");
    let processed: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT processed_at FROM social_clip_pool WHERE clip_db_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("processed before discard");
    assert!(processed.is_some());
    discard_test_run(&pool, "owner", run.job_id)
        .await
        .expect("discard");
    let (processed, selected): (Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT processed_at, selected_at FROM social_clip_pool WHERE clip_db_id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("pool row");
    assert_eq!((processed, selected), (None, None));
    let approvals: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_media_clip_approval WHERE produced_video_id = $1",
    )
    .bind(video)
    .fetch_one(&pool)
    .await
    .expect("approval count");
    assert_eq!(approvals, 0);
    let holds: bool = sqlx::query_scalar("SELECT social_clip_video_holds($1)")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("clip hold");
    assert!(!holds);
    let managed: bool = sqlx::query_scalar(
        "UPDATE twitch_clips_social_media SET game_id = '2132205352', clip_title = 'touched'
        WHERE id = $1 RETURNING pool_managed_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("clip row write after discard");
    assert!(managed);
    let processed: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT processed_at FROM social_clip_pool WHERE clip_db_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("processed after clip write");
    assert_eq!(processed, None);
    let again = start_test_run(&pool, "owner").await.expect("second run");
    assert_eq!(again.clip_id, id);
    cleanup(pool).await;
}

#[tokio::test]
async fn discarding_a_revised_test_run_rejects_every_version_and_frees_the_clip() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let id = clip(&pool, "owner", "test-revision", utc("2025-01-01T00:00:00Z")).await;
    let run = start_test_run(&pool, "owner").await.expect("test run");
    sqlx::query("UPDATE social_production_jobs SET status = 'completed' WHERE id = $1")
        .bind(run.job_id)
        .execute(&pool)
        .await
        .expect("test job complete");
    let parent: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, expires_at, metadata)
        VALUES ('owner', $1, $2, 'ai', 'rejected', '/fixture/parent.mp4', 1000, clock_timestamp() + INTERVAL '7 days',
            jsonb_build_object('production_ai_job_id', $2::bigint)) RETURNING id")
        .bind(id).bind(run.job_id).fetch_one(&pool).await.expect("superseded parent");
    let revision: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, expires_at, parent_version_id, version_no, metadata)
        VALUES ('owner', $1, NULL, 'ai', 'review', '/fixture/revision.mp4', 1000, clock_timestamp() + INTERVAL '7 days', $3, 2,
            jsonb_build_object('production_ai_job_id', $2::bigint, 'manual_ai_job_id', 1)) RETURNING id")
        .bind(id).bind(run.job_id).bind(parent).fetch_one(&pool).await.expect("revision version");
    sqlx::query("UPDATE social_produced_videos SET metadata = metadata || jsonb_build_object('superseded_by', $2::bigint) WHERE id = $1")
        .bind(parent)
        .bind(revision)
        .execute(&pool)
        .await
        .expect("supersede parent");
    let active = snapshot(&pool, "owner").await.expect("snapshot").test_run;
    assert_eq!(active.and_then(|run| run.video_id), Some(revision));
    discard_test_run(&pool, "owner", run.job_id)
        .await
        .expect("discard revised test run");
    let revived: String = sqlx::query_scalar(
        "UPDATE social_produced_videos SET status = 'review' WHERE id = $1 RETURNING status",
    )
    .bind(revision)
    .fetch_one(&pool)
    .await
    .expect("late revision write");
    assert_eq!(revived, "rejected");
    let (processed, selected): (Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT processed_at, selected_at FROM social_clip_pool WHERE clip_db_id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("pool row");
    assert_eq!((processed, selected), (None, None));
    assert!(snapshot(&pool, "owner")
        .await
        .expect("snapshot")
        .test_run
        .is_none());
    let again = start_test_run(&pool, "owner").await.expect("second run");
    assert_eq!(again.clip_id, id);
    cleanup(pool).await;
}

#[tokio::test]
async fn retrying_a_failed_test_run_requeues_the_same_clip() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let id = clip(&pool, "owner", "retry-failed", utc("2025-01-01T00:00:00Z")).await;
    let run = start_test_run(&pool, "owner").await.expect("test run");
    sqlx::query(
        "UPDATE social_production_jobs SET status = 'failed', stage = 'failed',
        last_error = 'Der Clip konnte nicht von Twitch geladen werden.', finished_at = NOW()
        WHERE id = $1",
    )
    .bind(run.job_id)
    .execute(&pool)
    .await
    .expect("failed test");
    sqlx::query("UPDATE social_clip_pool SET selected_at = NULL WHERE clip_db_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("unpin");
    let again = retry_test_run(&pool, "owner", run.job_id)
        .await
        .expect("retry");
    assert_eq!(again.job_id, run.job_id);
    assert_eq!(again.clip_id, id);
    assert_eq!(again.status, "queued");
    assert_eq!(again.stage, "queued");
    assert_eq!(again.last_error, None);
    let selected: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT selected_at FROM social_clip_pool WHERE clip_db_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("pin");
    assert!(selected.is_some());
    cleanup(pool).await;
}

#[tokio::test]
async fn retrying_a_review_without_video_requeues_the_clip() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let id = clip(&pool, "owner", "retry-review", utc("2025-01-01T00:00:00Z")).await;
    let run = start_test_run(&pool, "owner").await.expect("test run");
    sqlx::query(
        "UPDATE social_production_jobs SET status = 'review', stage = 'review',
        last_error = 'Die erlaubten Versuche sind aufgebraucht.', finished_at = NOW()
        WHERE id = $1",
    )
    .bind(run.job_id)
    .execute(&pool)
    .await
    .expect("review without video");
    let again = retry_test_run(&pool, "owner", run.job_id)
        .await
        .expect("retry empty review");
    assert_eq!(again.job_id, run.job_id);
    assert_eq!(again.clip_id, id);
    assert_eq!(again.status, "queued");
    cleanup(pool).await;
}

#[tokio::test]
async fn retrying_a_test_with_a_review_video_is_rejected() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    let id = clip(&pool, "owner", "retry-ready", utc("2025-01-01T00:00:00Z")).await;
    let run = start_test_run(&pool, "owner").await.expect("test run");
    sqlx::query("UPDATE social_production_jobs SET status = 'completed' WHERE id = $1")
        .bind(run.job_id)
        .execute(&pool)
        .await
        .expect("complete");
    sqlx::query(
        "INSERT INTO social_produced_videos (twitch_user_id, clip_id, job_id, mode, status, file_path, duration_ms, expires_at)
        VALUES ('owner', $1, $2, 'static', 'review', '/fixture/ready.mp4', 1000, clock_timestamp() + INTERVAL '7 days')",
    )
    .bind(id)
    .bind(run.job_id)
    .execute(&pool)
    .await
    .expect("review video");
    let error = retry_test_run(&pool, "owner", run.job_id)
        .await
        .expect_err("video stays in review");
    assert!(matches!(error, ScheduleError::Invalid(_)));
    cleanup(pool).await;
}
