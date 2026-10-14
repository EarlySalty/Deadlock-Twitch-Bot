use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::{
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;
use crate::{
    autopilot_settings::{ClipDateFilter, ClipSort, ScheduleSlot},
    production_jobs::{self, JobCompletion, JobSource, NewProductionJob, ProductionMode},
};

#[path = "edge_tests.rs"]
mod edges;
#[path = "finish_tests.rs"]
mod finish;
#[path = "runner_tests.rs"]
mod runner;

static NEXT_SCHEMA: AtomicU64 = AtomicU64::new(0);

async fn pool() -> PgPool {
    let pool = pool_before_slot_guards().await;
    apply_slot_guards(&pool).await;
    pool
}

async fn apply_slot_guards(pool: &PgPool) {
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011102000_social_production_slot_guards.sql"
    ))
    .execute(pool)
    .await
    .expect("actual M2 migration");
}

async fn pool_before_slot_guards() -> PgPool {
    let dsn = crate::test_support::test_dsn().expect("isolated test database is required");
    let schema = format!(
        "t_sm_b_{}_{}",
        std::process::id(),
        NEXT_SCHEMA.fetch_add(1, Ordering::Relaxed)
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("test database");
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {schema}")))
        .execute(&admin)
        .await
        .expect("owned advisory-lock namespace");
    admin.close().await;
    let options = PgConnectOptions::from_str(&dsn)
        .expect("test DSN")
        .database(&schema);
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .expect("owned database");
    sqlx::query(crate::test_sql::create_schema(&schema, false))
        .execute(&admin)
        .await
        .expect("owned schema");
    admin.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options.options([("search_path", schema.as_str())]))
        .await
        .expect("test pool");
    sqlx::raw_sql("CREATE TABLE twitch_streamers (twitch_user_id TEXT PRIMARY KEY);
        INSERT INTO twitch_streamers VALUES ('owner'), ('other');
        CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT NOT NULL, twitch_user_id TEXT,
            platform_user_id TEXT, enabled INTEGER DEFAULT 1, authorized_at TEXT DEFAULT CURRENT_TIMESTAMP);
        CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, twitch_user_id TEXT, clip_id TEXT,
            clip_url TEXT, clip_title TEXT, streamer_login TEXT, local_file_path TEXT, converted_file_path TEXT,
            status TEXT DEFAULT 'pending', game_id TEXT DEFAULT '2132205352', source_kind TEXT DEFAULT 'twitch',
            clip_thumbnail_url TEXT, duration_seconds DOUBLE PRECISION DEFAULT 15, created_at TIMESTAMPTZ DEFAULT NOW(),
            uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE);
        CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]', approver_user_id TEXT, decided_at TIMESTAMPTZ);
        CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL,
            status TEXT DEFAULT 'pending', scheduled_at TIMESTAMPTZ, title TEXT, description TEXT, hashtags TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())")
        .execute(&pool).await.expect("legacy prerequisite schema");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011100000_social_autopilot_contracts.sql"
    ))
    .execute(&pool)
    .await
    .expect("actual M0 migration");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011101000_social_clip_pool.sql"
    ))
    .execute(&pool)
    .await
    .expect("actual merged M1 migration");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261013140000_social_autopilot_publishing.sql"
    ))
    .execute(&pool)
    .await
    .expect("actual publishing migration");
    pool
}

async fn cleanup(pool: PgPool) {
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .expect("owned database name");
    assert!(database.starts_with(&format!("t_sm_b_{}_", std::process::id())));
    pool.close().await;
    let dsn = crate::test_support::test_dsn().expect("isolated test database");
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("isolated admin");
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {database}")))
        .execute(&admin)
        .await
        .expect("drop owned database");
    admin.close().await;
}

fn utc(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .expect("fixed time")
        .with_timezone(&Utc)
}

async fn clip(pool: &PgPool, owner: &str, name: &str, created: DateTime<Utc>) -> i64 {
    sqlx::query("SELECT social_clip_manage_owner($1)")
        .bind(owner)
        .execute(pool)
        .await
        .expect("actual managed pool owner");
    let id = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (twitch_user_id, clip_id, clip_title, created_at, streamer_login, clip_url)
        VALUES ($1,$2,$2,$3,$1,'https://clips.test/fixture') RETURNING id::bigint")
        .bind(owner).bind(name).bind(created).fetch_one(pool).await.expect("owned clip");
    sqlx::query("INSERT INTO social_clip_pool (twitch_user_id, clip_id, clip_db_id, source_created_at, game_id, source_metadata) VALUES ($1,$2,$3,$4,'2132205352','{}')")
        .bind(owner).bind(name).bind(id).bind(created).execute(pool).await.expect("pool fixture");
    id
}

async fn job(pool: &PgPool, owner: &str, clip: i64, mode: Option<ProductionMode>) -> ProductionJob {
    production_jobs::insert(
        pool,
        &NewProductionJob {
            twitch_user_id: owner.into(),
            clip_id: clip,
            source: if mode.is_some() {
                JobSource::Manual
            } else {
                JobSource::Autopilot
            },
            requested_mode: mode,
            scheduled_at: None,
            max_attempts: 3,
        },
    )
    .await
    .expect("production job")
}

fn schedule() -> SchedulePatch {
    SchedulePatch {
        enabled: Some(true),
        timezone: Some("Europe/Berlin".into()),
        schedule_slots: Some(vec![ScheduleSlot {
            weekday: 1,
            local_time: "18:00".into(),
        }]),
        ..Default::default()
    }
}

async fn choose_year(pool: &PgPool, year: i32, clip_sort: ClipSort) {
    crate::clip_pool::save_preferences(
        pool,
        "owner",
        &crate::clip_pool::PoolPreferences {
            clip_date_filter: ClipDateFilter::Year { year },
            clip_sort,
            timezone: "Europe/Berlin".into(),
        },
    )
    .await
    .expect("actual M1 preferences");
}

#[tokio::test]
async fn schedule_narrow_writes_preserve_other_settings_and_validate() {
    let pool = pool().await;
    sqlx::query("INSERT INTO social_media_autopilot_settings (twitch_user_id, publish_mode, clip_sort, target_platforms, notifications) VALUES ('owner','full_auto','newest_first','[\"youtube\"]','{\"dashboard\":false}')")
        .execute(&pool).await.expect("independent settings");
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("schedule write");
    let stored = autopilot_settings::load(&pool, "owner")
        .await
        .expect("settings");
    assert_eq!(
        stored.publish_mode,
        autopilot_settings::PublishMode::FullAuto
    );
    assert_eq!(stored.clip_sort, ClipSort::NewestFirst);
    assert_eq!(stored.target_platforms, vec!["youtube"]);
    assert!(!stored.notifications.dashboard);
    assert!(save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            lead_time_hours: Some(721),
            ..Default::default()
        }
    )
    .await
    .is_err());
    assert!(save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            timezone: Some("Unknown/Zone".into()),
            ..Default::default()
        }
    )
    .await
    .is_err());
    assert!(serde_json::from_value::<SchedulePatch>(json!({"publish_mode":"full_auto"})).is_err());
    let loaded = snapshot(&pool, "owner").await.expect("snapshot");
    assert!(loaded.persisted);
    assert!(loaded.settings.enabled);
    cleanup(pool).await;
}

#[tokio::test]
async fn planner_inside_lead_is_chronological_and_cancel_is_durable() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("schedule");
    let old = clip(&pool, "owner", "old", utc("2025-02-01T00:00:00Z")).await;
    let next = clip(&pool, "owner", "next", utc("2025-03-01T00:00:00Z")).await;
    clip(&pool, "owner", "out-of-window", utc("2026-01-01T00:00:00Z")).await;
    clip(&pool, "other", "foreign", utc("2025-01-01T00:00:00Z")).await;
    let now = utc("2026-10-11T18:00:00Z");
    let (first, second) = tokio::join!(
        plan_owner(&pool, "owner", now),
        plan_owner(&pool, "owner", now)
    );
    assert_eq!(
        first.expect("first planner").created + second.expect("second planner").created,
        1
    );
    let jobs = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("queue");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].clip_id, old);
    assert_eq!(jobs[0].scheduled_at, Some(utc("2026-10-12T16:00:00Z")));
    cancel(&pool, "owner", jobs[0].id).await.expect("cancel");
    assert_eq!(
        plan_owner(&pool, "owner", now)
            .await
            .expect("replacement")
            .created,
        1
    );
    let active: i64 =
        sqlx::query_scalar("SELECT clip_id FROM social_production_jobs WHERE status='queued'")
            .fetch_one(&pool)
            .await
            .expect("selected next");
    assert_eq!(active, next);
    assert_eq!(
        plan_owner(&pool, "owner", now)
            .await
            .expect("idempotent planner")
            .created,
        0
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn queue_reorders_owned_unstarted_jobs_and_rejects_partial_lists() {
    let pool = pool().await;
    let one = clip(&pool, "owner", "one", utc("2025-01-01T00:00:00Z")).await;
    let two = clip(&pool, "owner", "two", utc("2025-01-02T00:00:00Z")).await;
    let first = job(&pool, "owner", one, Some(ProductionMode::Static)).await;
    let second = job(&pool, "owner", two, Some(ProductionMode::Ai)).await;
    assert!(matches!(
        reorder(&pool, "owner", &[first.id]).await,
        Err(ScheduleError::QueueChanged)
    ));
    assert!(matches!(
        mark_next(&pool, "other", second.id).await,
        Err(ScheduleError::NotFound)
    ));
    mark_next(&pool, "owner", second.id)
        .await
        .expect("mark next");
    let queued = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("queue");
    assert_eq!(queued[0].id, second.id);
    let claimed = production_jobs::claim_next(&pool)
        .await
        .expect("claim")
        .expect("job");
    assert_eq!(claimed.id, second.id);
    assert!(matches!(
        cancel(&pool, "owner", second.id).await,
        Err(ScheduleError::Started)
    ));
    assert!(matches!(
        reorder(&pool, "owner", &[second.id, first.id]).await,
        Err(ScheduleError::QueueChanged)
    ));
    cleanup(pool).await;
}

#[tokio::test]
async fn claims_are_global_recovery_keeps_mode_and_checkpoint() {
    let pool = pool().await;
    let one = clip(&pool, "owner", "one", utc("2025-01-01T00:00:00Z")).await;
    let two = clip(&pool, "other", "two", utc("2025-01-01T00:00:00Z")).await;
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            ai_editing_enabled: Some(true),
            ..schedule()
        },
    )
    .await
    .expect("AI mode");
    let first = job(&pool, "owner", one, None).await;
    job(&pool, "other", two, Some(ProductionMode::Static)).await;
    let (a, b) = tokio::join!(
        production_jobs::claim_next(&pool),
        production_jobs::claim_next(&pool)
    );
    let claims: Vec<_> = [a.expect("claim a"), b.expect("claim b")]
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(claims.len(), 1);
    let running = &claims[0];
    assert_eq!(running.id, first.id);
    assert_eq!(running.mode, Some(ProductionMode::Ai));
    production_jobs::update_progress(
        &pool,
        "owner",
        first.id,
        "rendering",
        &json!({"analysis":{"saved":true}}),
    )
    .await
    .expect("checkpoint");
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            paused: Some(true),
            ai_editing_enabled: Some(false),
            schedule_slots: Some(vec![]),
            ..Default::default()
        },
    )
    .await
    .expect("toggle during work");
    let recovered = recover_after_lease(&pool)
        .await
        .expect("crash recovery under simulated exclusive ownership");
    assert_eq!(
        recovered,
        Recovery {
            resumed: 1,
            failed: 0
        }
    );
    let resumed = production_jobs::claim_next(&pool)
        .await
        .expect("resume")
        .expect("started work continues while paused");
    assert_eq!(resumed.mode, Some(ProductionMode::Ai));
    assert_eq!(resumed.stage, "rendering");
    assert_eq!(resumed.metadata["analysis"]["saved"], true);
    assert_eq!(resumed.started_at, running.started_at);
    assert_eq!(resumed.attempts, 2);
    production_jobs::finish(
        &pool,
        "owner",
        first.id,
        JobCompletion::Review,
        &["ai_editor_unavailable".into()],
        None,
    )
    .await
    .expect("AI hold without fallback");
    cleanup(pool).await;
}

#[tokio::test]
async fn two_reviews_block_automatic_claims_but_not_manual_work() {
    let pool = pool().await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("enabled weekly schedule");
    let clip_id = clip(&pool, "owner", "auto", utc("2025-01-01T00:00:00Z")).await;
    job(&pool, "owner", clip_id, None).await;
    for index in 0..2 {
        let id = clip(
            &pool,
            "owner",
            &format!("review-{index}"),
            utc("2025-01-01T00:00:00Z"),
        )
        .await;
        let review_job = job(&pool, "owner", id, None).await;
        sqlx::query("UPDATE social_production_jobs SET status = 'completed' WHERE id = $1")
            .bind(review_job.id)
            .execute(&pool)
            .await
            .expect("completed automatic production");
        sqlx::query("INSERT INTO social_produced_videos (twitch_user_id,clip_id,job_id,mode,status,file_path,duration_ms,expires_at) VALUES ('owner',$1,$2,'static','review','test.mp4',1000,clock_timestamp()+INTERVAL '7 days')")
            .bind(id).bind(review_job.id).execute(&pool).await.expect("review backlog");
    }
    assert!(production_jobs::claim_next(&pool)
        .await
        .expect("backpressure")
        .is_none());
    let id = clip(&pool, "owner", "manual", utc("2025-01-01T00:00:00Z")).await;
    let manual = job(&pool, "owner", id, Some(ProductionMode::Ai)).await;
    let running_manual = production_jobs::claim_next(&pool)
        .await
        .expect("manual claim")
        .expect("manual independent");
    assert_eq!(running_manual.id, manual.id);
    production_jobs::finish(&pool, "owner", manual.id, JobCompletion::Review, &[], None)
        .await
        .expect("manual review without publication");
    let mut settings = autopilot_settings::load(&pool, "owner")
        .await
        .expect("existing settings");
    settings.publish_mode = autopilot_settings::PublishMode::FullAuto;
    autopilot_settings::save(&pool, "owner", &settings)
        .await
        .expect("existing Full Auto setting");
    let automatic = production_jobs::claim_next(&pool)
        .await
        .expect("Full Auto claim")
        .expect("Review-only backpressure does not stop Full Auto");
    assert_eq!(automatic.clip_id, clip_id);
    cleanup(pool).await;
}

#[tokio::test]
async fn pause_resume_retimes_preserved_queue_without_missed_slot_bursts() {
    let pool = pool().await;
    choose_year(&pool, 2025, ClipSort::OldestFirst).await;
    save_schedule(&pool, "owner", &schedule())
        .await
        .expect("schedule");
    clip(&pool, "owner", "one", utc("2025-01-01T00:00:00Z")).await;
    clip(&pool, "owner", "two", utc("2025-01-02T00:00:00Z")).await;
    let before = utc("2026-10-11T18:00:00Z");
    assert_eq!(
        plan_owner(&pool, "owner", before)
            .await
            .expect("initial plan")
            .created,
        1
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
    let resume = before + Duration::days(21);
    let paused = plan_owner(&pool, "owner", resume)
        .await
        .expect("paused plan");
    assert_eq!(paused.production_hold, Some(ProductionHold::Paused));
    assert_eq!(paused.retimed, 0);
    save_schedule(
        &pool,
        "owner",
        &SchedulePatch {
            paused: Some(false),
            ..Default::default()
        },
    )
    .await
    .expect("resume");
    let report = plan_owner(&pool, "owner", resume)
        .await
        .expect("normal resume");
    assert_eq!(report.retimed, 1);
    assert_eq!(report.created, 0);
    let queued = production_jobs::list(&pool, "owner", 500)
        .await
        .expect("preserved queue");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].scheduled_at, Some(utc("2026-11-02T17:00:00Z")));
    cleanup(pool).await;
}

#[tokio::test]
async fn shared_slot_guard_covers_produced_and_upload_races() {
    let pool = pool().await;
    let one = clip(&pool, "owner", "one", utc("2025-01-01T00:00:00Z")).await;
    let two = clip(&pool, "owner", "two", utc("2025-01-02T00:00:00Z")).await;
    let scheduled = utc("2026-10-12T16:00:00Z");
    let first = production_jobs::insert(
        &pool,
        &NewProductionJob {
            twitch_user_id: "owner".into(),
            clip_id: one,
            source: JobSource::Manual,
            requested_mode: Some(ProductionMode::Static),
            scheduled_at: Some(scheduled),
            max_attempts: 3,
        },
    )
    .await
    .expect("production reservation");
    assert!(sqlx::query("INSERT INTO social_produced_videos (twitch_user_id,clip_id,mode,scheduled_at) VALUES ('owner',$1,'static',$2)").bind(two).bind(scheduled).execute(&pool).await.is_err());
    assert!(sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,platform,scheduled_at) VALUES ($1,'youtube',$2)").bind(two).bind(scheduled).execute(&pool).await.is_err());
    let produced: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id,clip_id,job_id,mode,scheduled_at) VALUES ('owner',$1,$2,'static',$3) RETURNING id")
        .bind(one).bind(first.id).bind(scheduled).fetch_one(&pool).await.expect("same job transfers one reservation");
    for platform in ["youtube", "tiktok"] {
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,produced_video_id,platform,status,scheduled_at) VALUES ($1,$2,$3,'waiting_exact_file',$4)")
            .bind(one).bind(produced).bind(platform).bind(scheduled).execute(&pool).await.expect("same immutable video shared across platforms");
    }
    cancel(&pool, "owner", first.id)
        .await
        .expect("unstarted cancellation");
    assert!(production_jobs::insert(
        &pool,
        &NewProductionJob {
            twitch_user_id: "owner".into(),
            clip_id: two,
            source: JobSource::Manual,
            requested_mode: Some(ProductionMode::Static),
            scheduled_at: Some(scheduled),
            max_attempts: 3
        }
    )
    .await
    .is_err());
    let third = clip(&pool, "owner", "race-job", utc("2025-01-03T00:00:00Z")).await;
    let fourth = clip(&pool, "owner", "race-video", utc("2025-01-04T00:00:00Z")).await;
    let next_slot = scheduled + Duration::days(7);
    let reservation = NewProductionJob {
        twitch_user_id: "owner".into(),
        clip_id: third,
        source: JobSource::Manual,
        requested_mode: Some(ProductionMode::Static),
        scheduled_at: Some(next_slot),
        max_attempts: 3,
    };
    let (job_reservation, video_reservation) = tokio::join!(
        production_jobs::insert(&pool, &reservation),
        sqlx::query("INSERT INTO social_produced_videos (twitch_user_id,clip_id,mode,scheduled_at) VALUES ('owner',$1,'static',$2)")
            .bind(fourth).bind(next_slot).execute(&pool)
    );
    assert_ne!(job_reservation.is_ok(), video_reservation.is_ok());
    let video_slot = scheduled + Duration::days(14);
    let (video_one, video_two) = tokio::join!(
        sqlx::query("INSERT INTO social_produced_videos (twitch_user_id,clip_id,mode,version_no,scheduled_at) VALUES ('owner',$1,'static',2,$2)")
            .bind(third).bind(video_slot).execute(&pool),
        sqlx::query("INSERT INTO social_produced_videos (twitch_user_id,clip_id,mode,version_no,scheduled_at) VALUES ('owner',$1,'static',2,$2)")
            .bind(fourth).bind(video_slot).execute(&pool)
    );
    assert_ne!(video_one.is_ok(), video_two.is_ok());
    let rejected = video_one
        .err()
        .or_else(|| video_two.err())
        .expect("one sibling video rejected");
    assert_eq!(
        rejected
            .as_database_error()
            .expect("actual unique constraint")
            .code()
            .as_deref(),
        Some("23505")
    );
    let upload_slot = scheduled + Duration::days(21);
    let upload_video_one: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id,clip_id,mode,version_no) VALUES ('owner',$1,'static',3) RETURNING id")
        .bind(third).fetch_one(&pool).await.expect("unscheduled immutable version one");
    let upload_video_two: i64 = sqlx::query_scalar("INSERT INTO social_produced_videos (twitch_user_id,clip_id,mode,version_no) VALUES ('owner',$1,'static',3) RETURNING id")
        .bind(fourth).fetch_one(&pool).await.expect("unscheduled immutable version two");
    let (upload_one, upload_two) = tokio::join!(
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,produced_video_id,platform,scheduled_at) VALUES ($1,$2,'youtube',$3)")
            .bind(third).bind(upload_video_one).bind(upload_slot).execute(&pool),
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,produced_video_id,platform,scheduled_at) VALUES ($1,$2,'tiktok',$3)")
            .bind(fourth).bind(upload_video_two).bind(upload_slot).execute(&pool)
    );
    assert_ne!(upload_one.is_ok(), upload_two.is_ok());
    let (same_clip, same_video) = if upload_one.is_ok() {
        (third, upload_video_one)
    } else {
        (fourth, upload_video_two)
    };
    let rejected = upload_one
        .err()
        .or_else(|| upload_two.err())
        .expect("one sibling upload rejected");
    assert_eq!(
        rejected
            .as_database_error()
            .expect("actual shared slot guard")
            .code()
            .as_deref(),
        Some("23505")
    );
    sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,produced_video_id,platform,scheduled_at) VALUES ($1,$2,'instagram',$3)")
        .bind(same_clip).bind(same_video).bind(upload_slot).execute(&pool).await.expect("same immutable video reuses one slot across platforms");
    cleanup(pool).await;
}

#[tokio::test]
async fn legacy_uploads_sharing_a_slot_keep_their_status_lifecycle() {
    let pool = pool_before_slot_guards().await;
    let slot = utc("2026-10-14T16:00:00Z");
    let mut rows = Vec::new();
    for (name, platform) in [("legacy-a", "tiktok"), ("legacy-b", "youtube")] {
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (twitch_user_id, clip_id, clip_title, streamer_login, clip_url)
            VALUES ('other',$1,$1,'other','https://clips.test/legacy') RETURNING id::bigint")
            .bind(name).fetch_one(&pool).await.expect("legacy clip");
        let row: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id,platform,status,scheduled_at) VALUES ($1,$2,'pending',$3) RETURNING id")
            .bind(clip).bind(platform).bind(slot).fetch_one(&pool).await.expect("legacy pending upload");
        rows.push((clip, row));
    }
    apply_slot_guards(&pool).await;
    for status in ["processing", "completed"] {
        for (_, row) in &rows {
            sqlx::query("UPDATE twitch_clips_upload_queue SET status = $2 WHERE id = $1")
                .bind(row)
                .bind(status)
                .execute(&pool)
                .await
                .expect("legacy status update in a shared slot");
        }
    }
    let rejected = sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,platform,status,scheduled_at) VALUES ($1,'instagram','pending',$2)")
        .bind(rows[0].0).bind(slot).execute(&pool).await.expect_err("legacy insert into a foreign occupied slot");
    assert_eq!(
        rejected
            .as_database_error()
            .expect("actual shared slot guard")
            .code()
            .as_deref(),
        Some("23505")
    );
    sqlx::query("UPDATE twitch_clips_upload_queue SET scheduled_at = $2 WHERE id = $1")
        .bind(rows[1].1)
        .bind(slot + Duration::hours(1))
        .execute(&pool)
        .await
        .expect("legacy reschedule");
    sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,platform,status,scheduled_at) VALUES ($1,'instagram','pending',$2)")
        .bind(rows[0].0).bind(slot).execute(&pool).await.expect("legacy insert next to its own clip");
    cleanup(pool).await;
}

#[tokio::test]
async fn legacy_upload_cannot_take_a_reserved_production_slot() {
    let pool = pool().await;
    let reserved = clip(&pool, "owner", "reserved", utc("2025-01-01T00:00:00Z")).await;
    let legacy: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (twitch_user_id, clip_id, clip_title, streamer_login, clip_url, source_kind)
        VALUES ('owner','legacy','legacy','owner','https://clips.test/legacy','manual_upload') RETURNING id::bigint")
        .fetch_one(&pool).await.expect("unmanaged legacy clip");
    let slot = utc("2026-10-15T16:00:00Z");
    production_jobs::insert(
        &pool,
        &NewProductionJob {
            twitch_user_id: "owner".into(),
            clip_id: reserved,
            source: JobSource::Manual,
            requested_mode: Some(ProductionMode::Static),
            scheduled_at: Some(slot),
            max_attempts: 3,
        },
    )
    .await
    .expect("production reservation");
    let rejected = sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id,platform,status,scheduled_at) VALUES ($1,'youtube','pending',$2)")
        .bind(legacy).bind(slot).execute(&pool).await.expect_err("legacy upload in a reserved slot");
    assert_eq!(
        rejected
            .as_database_error()
            .expect("actual shared slot guard")
            .code()
            .as_deref(),
        Some("23505")
    );
    let moved: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id,platform,status,scheduled_at) VALUES ($1,'youtube','pending',$2) RETURNING id")
        .bind(legacy).bind(slot + Duration::hours(1)).fetch_one(&pool).await.expect("legacy upload in a free slot");
    let rejected =
        sqlx::query("UPDATE twitch_clips_upload_queue SET scheduled_at = $2 WHERE id = $1")
            .bind(moved)
            .bind(slot)
            .execute(&pool)
            .await
            .expect_err("legacy reschedule into a reserved slot");
    assert_eq!(
        rejected
            .as_database_error()
            .expect("actual shared slot guard")
            .code()
            .as_deref(),
        Some("23505")
    );
    cleanup(pool).await;
}
