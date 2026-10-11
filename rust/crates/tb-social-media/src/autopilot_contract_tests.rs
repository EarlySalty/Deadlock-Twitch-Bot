use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::str::FromStr;

use crate::{
    autopilot_settings::{self, AutopilotSettings, ScheduleSlot},
    produced_video::{self, NewProducedVideo, ProducedVideoError, RenderedVideo},
    production_jobs::{self, JobCompletion, JobSource, NewProductionJob, ProductionMode},
};

async fn pool() -> PgPool {
    let dsn = crate::test_support::test_dsn().expect("isolated PostgreSQL test database");
    let schema = "t_social_m0_wf_e5e711";
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("test database");
    sqlx::query(crate::test_sql::drop_schema(schema, true))
        .execute(&admin)
        .await
        .expect("drop test schema");
    sqlx::query(crate::test_sql::create_schema(schema, false))
        .execute(&admin)
        .await
        .expect("create test schema");
    admin.close().await;
    let options = PgConnectOptions::from_str(&dsn)
        .expect("test DSN")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(options)
        .await
        .expect("test pool");
    sqlx::raw_sql("CREATE TABLE twitch_streamers (twitch_user_id TEXT PRIMARY KEY);
        INSERT INTO twitch_streamers VALUES ('owner'), ('other');
        CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, clip_id TEXT,
            clip_url TEXT, clip_title TEXT, streamer_login TEXT, local_file_path TEXT, converted_file_path TEXT,
            status TEXT DEFAULT 'pending');
        CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval',
            approved_platforms JSONB NOT NULL DEFAULT '[]', approver_user_id TEXT, decided_at TIMESTAMPTZ,
            dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ);
        CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL,
            status TEXT DEFAULT 'pending', scheduled_at TIMESTAMPTZ, title TEXT, description TEXT, hashtags TEXT,
            priority INTEGER DEFAULT 0, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER DEFAULT 0,
            last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT NOW(), completed_at TIMESTAMPTZ)")
        .execute(&pool).await.expect("legacy schema");
    sqlx::raw_sql("INSERT INTO twitch_clips_social_media (twitch_user_id, clip_id, streamer_login) VALUES ('owner', 'legacy', 'nani');
        INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, approver_user_id) VALUES (1, 'approved', '[\"youtube\"]', 'legacy_reviewer')")
        .execute(&pool).await.expect("existing legacy approval");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011100000_social_autopilot_contracts.sql"
    ))
    .execute(&pool)
    .await
    .expect("actual additive migration");
    pool
}

async fn ready(pool: &PgPool, file: &str, clip: i64) -> i64 {
    let video = produced_video::insert(
        pool,
        &NewProducedVideo {
            twitch_user_id: "owner".into(),
            clip_id: clip,
            job_id: None,
            mode: ProductionMode::Ai,
            version_no: 1,
            parent_version_id: None,
            variant_index: 0,
        },
    )
    .await
    .expect("insert rendering video");
    produced_video::complete_render(pool, "owner", video.id, &RenderedVideo {
        file_path: file.into(), duration_ms: 15000, needs_review: true,
        review_reasons: vec!["uncertain_gameplay".into()],
        metadata: json!({"platforms":{"youtube":{"title":"Highlight","hashtags":["Deadlock"]}}}),
    }).await.expect("complete render");
    video.id
}

#[tokio::test]
async fn postgres_contracts_preserve_legacy_and_exact_file_reservations() {
    let pool = pool().await;
    let legacy = crate::approval::get_approval_record(&pool, 1)
        .await
        .expect("preserved legacy approval");
    assert_eq!(legacy.approved_platforms, vec!["youtube"]);
    assert_eq!(legacy.approver_user_id.as_deref(), Some("legacy_reviewer"));
    let defaults = autopilot_settings::load(&pool, "owner")
        .await
        .expect("defaults");
    assert!(!defaults.enabled && !defaults.ai_editing_enabled && !defaults.multi_output_enabled);
    assert_eq!(
        defaults.publish_mode,
        autopilot_settings::PublishMode::Review
    );
    assert_eq!(defaults.lead_time_hours, 24);
    let settings = AutopilotSettings {
        timezone: "Europe/Berlin".into(),
        schedule_slots: (1..=7)
            .map(|weekday| ScheduleSlot {
                weekday,
                local_time: "18:00".into(),
            })
            .collect(),
        target_platforms: vec!["youtube".into(), "tiktok".into()],
        ..defaults
    };
    autopilot_settings::save(&pool, "owner", &settings)
        .await
        .expect("save settings");
    assert_eq!(
        autopilot_settings::load(&pool, "owner")
            .await
            .expect("load settings"),
        settings
    );
    let clips: Vec<i64> = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (twitch_user_id, clip_id, streamer_login) VALUES ('owner', 'first', 'nani'), ('owner', 'second', 'nani'), ('other', 'third', 'other') RETURNING id")
        .fetch_all(&pool).await.expect("owned clips");
    sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state) VALUES ($1, 'editing') ON CONFLICT (clip_db_id) DO NOTHING")
        .bind(clips[0] as i32).execute(&pool).await.expect("legacy conflict target");
    assert_eq!(
        crate::approval::get_approval_record(&pool, clips[0] as i32)
            .await
            .expect("legacy approval")
            .state,
        "editing"
    );
    let directory = tempfile::tempdir().expect("render directory");
    let file = directory.path().join("render.mp4");
    tokio::fs::write(&file, b"finished render contract fixture")
        .await
        .expect("file fixture");
    let file = file.to_str().expect("file path");
    let first = ready(&pool, file, clips[0]).await;
    let second = ready(&pool, file, clips[1]).await;
    let selected = vec!["youtube".into(), "tiktok".into()];
    assert!(matches!(
        produced_video::enqueue_for_publishing(&pool, first, &selected).await,
        Err(ProducedVideoError::ApprovalRequired)
    ));
    assert!(matches!(
        produced_video::approve(&pool, "other", first, &selected).await,
        Err(ProducedVideoError::Ownership)
    ));
    produced_video::approve(&pool, "owner", first, &selected)
        .await
        .expect("approve exact version");
    produced_video::approve(&pool, "owner", second, &selected)
        .await
        .expect("approve second version");
    let (one, two) = tokio::join!(
        produced_video::enqueue_for_publishing(&pool, first, &selected),
        produced_video::enqueue_for_publishing(&pool, second, &selected)
    );
    let one = one.expect("first reservation");
    let two = two.expect("second reservation");
    assert_ne!(one.scheduled_at, two.scheduled_at);
    let repeated = produced_video::enqueue_for_publishing(&pool, first, &selected)
        .await
        .expect("idempotent reservation");
    assert_eq!(one.scheduled_at, repeated.scheduled_at);
    assert_eq!(one.queue_ids, repeated.queue_ids);
    assert_eq!(one.queue_ids.len(), 2);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT scheduled_at) FROM twitch_clips_upload_queue WHERE produced_video_id = $1")
        .bind(first).fetch_one(&pool).await.expect("shared slot count");
    assert_eq!(count, 1);
    let held: bool = sqlx::query_scalar("SELECT BOOL_AND(status = 'waiting_exact_file') FROM twitch_clips_upload_queue WHERE produced_video_id IS NOT NULL")
        .fetch_one(&pool).await.expect("exact-file rollout guard");
    assert!(held);
    assert_eq!(
        crate::approval::iter_approved_clips_pending_queue(&pool, 20).await,
        vec![1]
    );
    assert!(
        produced_video::enqueue_for_publishing_for_user(&pool, "other", first, &selected)
            .await
            .is_err()
    );
    let record = produced_video::get(&pool, "owner", first)
        .await
        .expect("video")
        .expect("existing video");
    assert_eq!(record.file_path.as_deref(), Some(file));
    assert_eq!(record.status, "scheduled");
    assert!(
        !crate::approval::is_clip_approved_for(&pool, clips[0], "youtube")
            .await
            .expect("legacy independence")
    );
    let legacy_id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, scheduled_at) VALUES ($1, 'youtube', 'pending', NOW() - INTERVAL '1 hour') RETURNING id")
        .bind(clips[2]).fetch_one(&pool).await.expect("legacy queue");
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = 'pending', scheduled_at = NOW() - INTERVAL '1 hour' WHERE produced_video_id IS NOT NULL")
        .execute(&pool).await.expect("defensive legacy retrieval test");
    let queue = crate::clip_queue::get_upload_queue(&pool, None, "pending", 20, None).await;
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].id, legacy_id);
    assert!(matches!(
        produced_video::insert(
            &pool,
            &NewProducedVideo {
                twitch_user_id: "owner".into(),
                clip_id: clips[2],
                job_id: None,
                mode: ProductionMode::Ai,
                version_no: 1,
                parent_version_id: None,
                variant_index: 0,
            }
        )
        .await,
        Err(ProducedVideoError::Ownership)
    ));
    let job = production_jobs::insert(
        &pool,
        &NewProductionJob {
            twitch_user_id: "owner".into(),
            clip_id: clips[0],
            source: JobSource::Manual,
            requested_mode: Some(ProductionMode::Ai),
            scheduled_at: None,
            max_attempts: 3,
        },
    )
    .await
    .expect("manual job with autopilot disabled");
    let claimed = production_jobs::claim_next(&pool)
        .await
        .expect("claim manual")
        .expect("claimed");
    assert_eq!(claimed.id, job.id);
    assert_eq!(claimed.mode, Some(ProductionMode::Ai));
    assert!(production_jobs::claim_next(&pool)
        .await
        .expect("sequential guard")
        .is_none());
    autopilot_settings::save(
        &pool,
        "owner",
        &AutopilotSettings {
            ai_editing_enabled: false,
            ..settings
        },
    )
    .await
    .expect("toggle during running job");
    assert_eq!(
        production_jobs::get(&pool, "owner", job.id)
            .await
            .expect("running job")
            .expect("job")
            .mode,
        Some(ProductionMode::Ai)
    );
    production_jobs::finish(
        &pool,
        "owner",
        job.id,
        JobCompletion::Review,
        &["ai_editor_unavailable".into()],
        None,
    )
    .await
    .expect("no static fallback");
    assert!(production_jobs::claim_next(&pool)
        .await
        .expect("no queued jobs")
        .is_none());
    tokio::fs::remove_file(file)
        .await
        .expect("file unavailable");
    assert!(matches!(
        produced_video::enqueue_for_publishing(&pool, first, &selected).await,
        Err(ProducedVideoError::FileNotReady)
    ));
    pool.close().await;
}

fn utc(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .expect("fixed timestamp")
        .with_timezone(&Utc)
}

#[test]
fn shared_schedule_handles_weekdays_dst_and_no_implicit_slots() {
    let settings = AutopilotSettings {
        schedule_slots: vec![ScheduleSlot {
            weekday: 7,
            local_time: "02:30".into(),
        }],
        ..AutopilotSettings::default()
    };
    assert_eq!(
        autopilot_settings::next_shared_slot(utc("2026-03-28T12:00:00Z"), &[], &settings)
            .expect("DST gap"),
        Some(utc("2026-04-05T00:30:00Z"))
    );
    let first = utc("2026-10-25T00:30:00Z");
    assert_eq!(
        autopilot_settings::next_shared_slot(utc("2026-10-24T12:00:00Z"), &[], &settings)
            .expect("DST overlap"),
        Some(first)
    );
    assert_eq!(
        autopilot_settings::next_shared_slot(first + Duration::minutes(1), &[first], &settings)
            .expect("no double post"),
        Some(utc("2026-11-01T01:30:00Z"))
    );
    assert_eq!(
        autopilot_settings::next_shared_slot(first, &[], &AutopilotSettings::default())
            .expect("empty schedule"),
        None
    );
    assert!(AutopilotSettings {
        timezone: "Invalid/Zone".into(),
        ..settings
    }
    .validate()
    .is_err());
}
