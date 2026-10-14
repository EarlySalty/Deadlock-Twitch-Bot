use super::{cleanup, clip, job, pool, utc};
use crate::{
    ai_editor::MediaRuntime,
    autopilot_runner::{AutopilotWorker, WorkerConfig},
    clip_prep_worker::ClipDownloader,
    produced_video, production_jobs,
    production_jobs::ProductionMode,
    production_lease::acquire_production_lease,
    video_processor::VideoProcessor,
};
use serde_json::json;
use sqlx::PgPool;
use std::{path::Path, sync::Arc};

struct NoDownload;

#[async_trait::async_trait]
impl ClipDownloader for NoDownload {
    async fn download(&self, _url: &str, _destination: &Path) -> Result<(), String> {
        panic!("local production must not download media")
    }
}

fn configuration(root: &Path) -> WorkerConfig {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("owned source package");
    let mut media = MediaRuntime::load(package_root).expect("existing packaged media runtime");
    media.package_root = root.canonicalize().expect("owned isolated package root");
    media.source_root = media.package_root.join("clips");
    media.output_root = media.source_root.clone();
    media.work_root = media.source_root.join(".ai-editor-work");
    WorkerConfig::from_media_runtime(&media, None, None)
}

fn worker(pool: &PgPool, root: &Path, processor: VideoProcessor) -> AutopilotWorker {
    AutopilotWorker::new(
        pool.clone(),
        configuration(root),
        Arc::new(NoDownload),
        processor,
    )
    .expect("concrete runner configuration")
}

#[tokio::test]
async fn autopilot_runner_lease_precedes_claim_and_unavailable_ai_has_no_static_fallback() {
    let pool = pool().await;
    let root = tempfile::tempdir().expect("owned media root");
    let clip_id = clip(
        &pool,
        "owner",
        "unavailable-ai",
        utc("2025-01-01T00:00:00Z"),
    )
    .await;
    let queued = job(&pool, "owner", clip_id, Some(ProductionMode::Ai)).await;
    let worker = worker(
        &pool,
        root.path(),
        VideoProcessor::new("unavailable-ffmpeg", "unavailable-ffprobe"),
    );
    let lease = acquire_production_lease(&pool)
        .await
        .expect("lease acquisition")
        .expect("owned lease");
    assert!(!worker
        .run_once()
        .await
        .expect("busy lease is a truthful hold"));
    let current = production_jobs::get(&pool, "owner", queued.id)
        .await
        .expect("queued job")
        .expect("owned job");
    assert_eq!(current.status, "queued");
    assert_eq!(current.attempts, 0);
    lease.release().await.expect("release before dispatch");
    assert!(worker.run_once().await.expect("unavailable AI is handled"));
    let current = production_jobs::get(&pool, "owner", queued.id)
        .await
        .expect("held job")
        .expect("owned job");
    assert_eq!(current.mode, Some(ProductionMode::Ai));
    assert_eq!(current.status, "review");
    assert_eq!(current.review_reasons, json!(["ai_editor_unavailable"]));
    assert!(produced_video::list(&pool, "owner", 50)
        .await
        .expect("produced versions")
        .is_empty());
    let lease = acquire_production_lease(&pool)
        .await
        .expect("released lease")
        .expect("no leaked lock");
    lease.release().await.expect("test lease release");
    cleanup(pool).await;
}

async fn real_static_fixture(
    pool: &PgPool,
) -> (tempfile::TempDir, WorkerConfig, std::path::PathBuf, i64) {
    sqlx::raw_sql("INSERT INTO twitch_streamers VALUES ('42');
        ALTER TABLE twitch_clips_social_media ADD COLUMN custom_title TEXT;
        ALTER TABLE twitch_clips_social_media ADD COLUMN discarded_at TIMESTAMPTZ;
        ALTER TABLE twitch_clips_social_media ADD COLUMN custom_description TEXT;
        ALTER TABLE twitch_clips_social_media ADD COLUMN hashtags TEXT DEFAULT '[]';
        ALTER TABLE twitch_clips_social_media ADD COLUMN layout_override_json JSONB;
        CREATE TABLE social_media_streamer_layout (twitch_user_id TEXT PRIMARY KEY, layout_json JSONB, cam_enabled BOOLEAN, mode TEXT)")
        .execute(pool).await.expect("static rendering prerequisites");
    let root = tempfile::tempdir().expect("owned media root");
    let config = configuration(root.path());
    tokio::fs::create_dir_all(&config.source_root)
        .await
        .expect("owned clips directory");
    let source = config.source_root.join("fixture-original.mp4");
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=640x360:r=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "1.6",
            "-c:v",
            "libx264",
            "-threads",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
        ])
        .arg(&source)
        .status()
        .await
        .expect("existing real-media test pattern");
    assert!(status.success());
    let clip_id = clip(pool, "42", "static-local", utc("2025-01-01T00:00:00Z")).await;
    sqlx::query("UPDATE twitch_clips_social_media SET local_file_path=$2, custom_description='Lokaler Test' WHERE id=$1")
        .bind(clip_id).bind(source.to_string_lossy().as_ref()).execute(pool).await.expect("local source only");
    (root, config, source, clip_id)
}

#[tokio::test]
async fn autopilot_runner_static_local_media_and_restart_preserve_immutable_version() {
    let pool = pool().await;
    let (root, config, source, clip_id) = real_static_fixture(&pool).await;
    let original = tokio::fs::read(&source)
        .await
        .expect("synthetic original bytes");
    sqlx::raw_sql("CREATE TABLE llm_usage (id BIGSERIAL PRIMARY KEY, ts TEXT, purpose TEXT,
        provider TEXT, model TEXT, attempt_state TEXT, tokens_in BIGINT, tokens_out BIGINT, latency_ms BIGINT)")
        .execute(&pool).await.expect("usage ledger fixture");
    let queued = job(&pool, "42", clip_id, Some(ProductionMode::Static)).await;
    let runner = worker(&pool, root.path(), VideoProcessor::default());
    assert!(runner.run_once().await.expect("concrete static dispatch"));
    let usage: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM llm_usage WHERE purpose = 'social_editor'")
            .fetch_one(&pool)
            .await
            .expect("no static gameplay analysis");
    assert_eq!(usage, 0);
    let videos = produced_video::list(&pool, "42", 50)
        .await
        .expect("immutable versions");
    assert_eq!(videos.len(), 1);
    let video = &videos[0];
    let expected = config
        .output_root
        .canonicalize()
        .expect("output root")
        .join("42")
        .join(format!("production-{}", queued.id))
        .join("version-1.mp4");
    assert_eq!(video.file_path.as_deref(), expected.to_str());
    assert_eq!(video.mode, ProductionMode::Static);
    assert_eq!(video.status, "review");
    assert!(video.needs_review);
    assert!(video.metadata["rendering_ms"]
        .as_i64()
        .is_some_and(|ms| ms > 0));
    let rendered = tokio::fs::read(&expected)
        .await
        .expect("persisted local output");
    assert!(!rendered.is_empty());
    let done = production_jobs::get(&pool, "42", queued.id)
        .await
        .expect("completed job")
        .expect("owned job");
    assert_eq!(done.status, "completed");
    assert_eq!(done.mode, Some(ProductionMode::Static));
    assert_eq!(
        tokio::fs::read(&source).await.expect("unchanged original"),
        original
    );
    let upload_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
        .fetch_one(&pool)
        .await
        .expect("no publication");
    assert_eq!(upload_count, 0);
    sqlx::query("UPDATE social_produced_videos SET status='rendering', file_path=NULL,
        metadata=jsonb_set(metadata - 'rendering_ms', '{static,rendering_ms}', 'null'::jsonb) WHERE id=$1")
        .bind(video.id).execute(&pool).await.expect("interrupted completion with unknown timing");
    sqlx::query(
        "UPDATE social_production_jobs SET status='running', stage='rendered',
        finished_at=NULL, metadata=$2::text::jsonb WHERE id=$1",
    )
    .bind(queued.id)
    .bind(json!({"static":{"produced_video_id":video.id}}).to_string())
    .execute(&pool)
    .await
    .expect("persisted interruption checkpoint");
    let restart = worker(
        &pool,
        root.path(),
        VideoProcessor::new("unavailable-ffmpeg", "unavailable-ffprobe"),
    );
    assert!(restart
        .run_once()
        .await
        .expect("actual recovery and dispatch"));
    let recovered = produced_video::list(&pool, "42", 50)
        .await
        .expect("recovered version");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, video.id);
    assert_eq!(recovered[0].file_path.as_deref(), expected.to_str());
    assert_eq!(recovered[0].metadata["rendering_ms"], json!(null));
    assert_eq!(
        tokio::fs::read(&expected)
            .await
            .expect("immutable recovered bytes"),
        rendered
    );
    let preview = expected
        .parent()
        .expect("owned version directory")
        .join("preview.mp4");
    tokio::fs::copy(&expected, &preview)
        .await
        .expect("different file with identical valid media");
    sqlx::query(
        "UPDATE social_produced_videos SET status='rendering', file_path=NULL,
        metadata=jsonb_set(metadata, '{static,output_path}', to_jsonb($2::text)) WHERE id=$1",
    )
    .bind(video.id)
    .bind(preview.to_string_lossy().as_ref())
    .execute(&pool)
    .await
    .expect("wrong immutable file checkpoint");
    sqlx::query("UPDATE social_production_jobs SET status='running', stage='rendered', finished_at=NULL WHERE id=$1")
        .bind(queued.id).execute(&pool).await.expect("interrupted wrong-file checkpoint");
    assert!(restart
        .run_once()
        .await
        .expect("wrong file is held without rendering a replacement"));
    let held = production_jobs::get(&pool, "42", queued.id)
        .await
        .expect("held job")
        .expect("owned job");
    assert_eq!(held.status, "failed");
    assert_eq!(held.review_reasons, json!(["attempts_exhausted"]));
    let held = produced_video::list(&pool, "42", 50)
        .await
        .expect("held version");
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].status, "failed");
    assert!(held[0].file_path.is_none());
    assert_eq!(
        tokio::fs::read(&expected)
            .await
            .expect("original version remains immutable"),
        rendered
    );
    cleanup(pool).await;
}

async fn automatic_schedule(
    pool: &PgPool,
    owner: &str,
    ai: bool,
    full_auto: bool,
) -> chrono::DateTime<chrono::Utc> {
    use chrono::Datelike;
    let slot: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT date_trunc('minute', clock_timestamp()) + INTERVAL '5 minutes'")
            .fetch_one(pool)
            .await
            .expect("actual future database slot");
    super::save_schedule(
        pool,
        owner,
        &super::SchedulePatch {
            enabled: Some(true),
            ai_editing_enabled: Some(ai),
            timezone: Some("UTC".into()),
            schedule_slots: Some(vec![crate::autopilot_settings::ScheduleSlot {
                weekday: slot.weekday().number_from_monday() as u8,
                local_time: slot.format("%H:%M").to_string(),
            }]),
            ..Default::default()
        },
    )
    .await
    .expect("automatic schedule");
    let mut settings = crate::autopilot_settings::load(pool, owner)
        .await
        .expect("actual settings");
    settings.publish_mode = if full_auto {
        crate::autopilot_settings::PublishMode::FullAuto
    } else {
        crate::autopilot_settings::PublishMode::Review
    };
    crate::autopilot_settings::save(pool, owner, &settings)
        .await
        .expect("publication policy");
    slot
}

async fn inject_job_database_failure(pool: &PgPool, stage: &str, skip: i64, failures: i64) {
    sqlx::raw_sql("CREATE SEQUENCE job_database_fault;
        CREATE FUNCTION fail_job_write() RETURNS trigger LANGUAGE plpgsql AS $$
        DECLARE invocation BIGINT;
        BEGIN
            IF NEW.stage = TG_ARGV[0] AND NEW.status IN ('running', 'completed') THEN
                invocation := nextval('job_database_fault');
                IF invocation > TG_ARGV[1]::bigint AND invocation <= TG_ARGV[1]::bigint + TG_ARGV[2]::bigint THEN
                    RAISE EXCEPTION 'isolated transient production write failure' USING ERRCODE = '40001';
                END IF;
            END IF;
            RETURN NEW;
        END;
        $$;")
        .execute(pool).await.expect("real PostgreSQL transient fault");
    assert!(matches!(stage, "rendering" | "rendered" | "completed"));
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE TRIGGER isolated_job_database_fault BEFORE UPDATE ON social_production_jobs
        FOR EACH ROW EXECUTE FUNCTION fail_job_write('{stage}', '{skip}', '{failures}')"
    )))
    .execute(pool)
    .await
    .expect("owned database fault trigger");
}

async fn retry_now(pool: &PgPool, id: i64) {
    sqlx::query("UPDATE social_production_jobs SET metadata = metadata - 'retry_after' WHERE id = $1 AND status = 'queued'")
        .bind(id).execute(pool).await.expect("advance synthetic retry eligibility without sleeping");
}

#[tokio::test]
async fn autopilot_runner_cleared_schedule_preserves_queued_work_and_manual_independence() {
    for full_auto in [false, true] {
        let pool = pool().await;
        let (root, _, _, clip_id) = real_static_fixture(&pool).await;
        let slot = automatic_schedule(&pool, "42", false, full_auto).await;
        crate::clip_pool::save_preferences(
            &pool,
            "42",
            &crate::clip_pool::PoolPreferences {
                clip_date_filter: crate::autopilot_settings::ClipDateFilter::Year { year: 2025 },
                clip_sort: crate::autopilot_settings::ClipSort::OldestFirst,
                timezone: "UTC".into(),
            },
        )
        .await
        .expect("eligible synthetic clip");
        assert_eq!(
            super::plan_owner(&pool, "42", slot - chrono::Duration::minutes(5))
                .await
                .expect("actual queued schedule")
                .created,
            1
        );
        let queued = production_jobs::list(&pool, "42", 50)
            .await
            .expect("original queue")
            .remove(0);
        assert_eq!(queued.scheduled_at, Some(slot));
        assert_eq!(queued.clip_id, clip_id);
        super::save_schedule(
            &pool,
            "42",
            &super::SchedulePatch {
                schedule_slots: Some(vec![]),
                ..Default::default()
            },
        )
        .await
        .expect("clear all weekly slots after queueing");
        let runner = worker(&pool, root.path(), VideoProcessor::default());
        for _ in 0..3 {
            let report = super::plan_owner(&pool, "42", slot - chrono::Duration::minutes(4))
                .await
                .expect("empty schedule planning");
            assert_eq!(
                report.production_hold,
                Some(super::ProductionHold::NoSchedule)
            );
            assert_eq!(report.created, 0);
            assert_eq!(report.retimed, 0);
            assert!(!runner.run_once().await.expect("no automatic dispatch"));
        }
        let unchanged = production_jobs::get(&pool, "42", queued.id)
            .await
            .expect("preserved automatic job")
            .expect("job still exists");
        let mut expected = queued.clone();
        expected.scheduled_at = None;
        assert_eq!(
            serde_json::to_value(&unchanged).expect("preserved job state"),
            serde_json::to_value(&expected).expect("only the removed schedule reservation changes")
        );
        assert!(produced_video::list(&pool, "42", 50)
            .await
            .expect("no automatic versions or progress")
            .is_empty());
        let state = super::snapshot(&pool, "42")
            .await
            .expect("truthful snapshot");
        assert_eq!(
            state.production_hold,
            Some(super::ProductionHold::NoSchedule)
        );
        assert!(state.settings.schedule_slots.is_empty());
        for mode in [ProductionMode::Ai, ProductionMode::Static] {
            let manual_clip = clip(
                &pool,
                "42",
                &format!("manual-{}", mode.as_str()),
                utc("2025-01-02T00:00:00Z"),
            )
            .await;
            sqlx::query("UPDATE twitch_clips_social_media SET local_file_path = (SELECT local_file_path FROM twitch_clips_social_media WHERE id = $2) WHERE id = $1")
                .bind(manual_clip)
                .bind(clip_id)
                .execute(&pool)
                .await
                .expect("separate manual clip with real local synthetic media");
            let manual = job(&pool, "42", manual_clip, Some(mode)).await;
            assert!(runner
                .run_once()
                .await
                .expect("manual dispatch without slots"));
            let result = production_jobs::get(&pool, "42", manual.id)
                .await
                .expect("manual result")
                .expect("owned manual result");
            assert_eq!(result.attempts, 1);
            assert_eq!(result.mode, Some(mode));
            if mode == ProductionMode::Ai {
                assert_eq!(result.status, "review");
                assert_eq!(result.review_reasons, json!(["ai_editor_unavailable"]));
            } else {
                assert_eq!(result.status, "completed");
            }
        }
        let uploads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
            .fetch_one(&pool)
            .await
            .expect("no publication side effects");
        assert_eq!(uploads, 0);
        assert_eq!(
            production_jobs::get(&pool, "42", queued.id)
                .await
                .expect("automatic attempt count")
                .expect("automatic job")
                .attempts,
            0
        );
        cleanup(pool).await;
    }
}

#[tokio::test]
async fn autopilot_runner_repeated_resource_holds_do_not_drain_the_pool() {
    for full_auto in [false, true] {
        for ai in [false, true] {
            let pool = pool().await;
            sqlx::raw_sql("INSERT INTO twitch_streamers VALUES ('42'); ALTER TABLE twitch_clips_social_media ADD COLUMN discarded_at TIMESTAMPTZ")
                .execute(&pool).await.expect("source prerequisites");
            let root = tempfile::tempdir().expect("owned media root");
            let config = configuration(root.path());
            tokio::fs::create_dir_all(&config.source_root)
                .await
                .expect("owned sources");
            let invalid = config.source_root.join("invalid-local.mp4");
            tokio::fs::write(&invalid, b"synthetic invalid media")
                .await
                .expect("invalid local media, not a downloader failure");
            automatic_schedule(&pool, "42", ai, full_auto).await;
            crate::clip_pool::save_preferences(
                &pool,
                "42",
                &crate::clip_pool::PoolPreferences {
                    clip_date_filter: crate::autopilot_settings::ClipDateFilter::Year {
                        year: 2025,
                    },
                    clip_sort: crate::autopilot_settings::ClipSort::OldestFirst,
                    timezone: "UTC".into(),
                },
            )
            .await
            .expect("actual pool window");
            let mut clips = Vec::new();
            for index in 0..8 {
                let id = clip(
                    &pool,
                    "42",
                    &format!("resource-{index}"),
                    utc("2025-01-01T00:00:00Z") + chrono::Duration::days(index),
                )
                .await;
                sqlx::query(
                    "UPDATE twitch_clips_social_media SET local_file_path = $2 WHERE id = $1",
                )
                .bind(id)
                .bind(invalid.to_string_lossy().as_ref())
                .execute(&pool)
                .await
                .expect("authorized local invalid source");
                clips.push(id);
            }
            let runner = worker(&pool, root.path(), VideoProcessor::default());
            let expected_reviews = if full_auto { clips.len() } else { 2 };
            for _ in 0..expected_reviews {
                assert!(runner
                    .run_once()
                    .await
                    .expect("one review job does not block the next clip"));
            }
            for _ in 0..8 {
                assert!(!runner
                    .run_once()
                    .await
                    .expect("backpressure, not repeated clip consumption"));
            }
            let state = super::snapshot(&pool, "42")
                .await
                .expect("actual queue snapshot");
            assert_eq!(state.jobs.len(), expected_reviews);
            assert!(state.jobs.iter().all(|job| job.status == "review"));
            assert_eq!(state.jobs[0].attempts, 1);
            assert_eq!(state.review_count, expected_reviews as i64);
            assert_eq!(
                state.production_hold,
                Some(if full_auto {
                    super::ProductionHold::EmptyPool
                } else {
                    super::ProductionHold::ReviewBacklog
                })
            );
            assert_eq!(
                state.jobs[0].review_reasons,
                vec![if ai {
                    "ai_editor_unavailable"
                } else {
                    "static_production_failed"
                }]
            );
            let selected: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM social_clip_pool WHERE selected_at IS NOT NULL",
            )
            .fetch_one(&pool)
            .await
            .expect("bounded pool consumption");
            assert_eq!(selected, expected_reviews as i64);
            assert!(produced_video::list(&pool, "42", 50)
                .await
                .expect("no fallback versions")
                .is_empty());
            super::save_schedule(
                &pool,
                "42",
                &super::SchedulePatch {
                    enabled: Some(false),
                    paused: Some(true),
                    ..Default::default()
                },
            )
            .await
            .expect("automatic controls independent of manual AI");
            let manual = job(&pool, "42", clips[2], Some(ProductionMode::Ai)).await;
            assert!(runner
                .run_once()
                .await
                .expect("manual AI remains independently dispatchable"));
            let manual = production_jobs::get(&pool, "42", manual.id)
                .await
                .expect("manual result")
                .expect("owned manual result");
            assert_eq!(manual.mode, Some(ProductionMode::Ai));
            assert_eq!(manual.review_reasons, json!(["ai_editor_unavailable"]));
            let uploads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
                .fetch_one(&pool)
                .await
                .expect("no external publication");
            assert_eq!(uploads, 0);
            cleanup(pool).await;
        }
    }
}

#[tokio::test]
async fn autopilot_runner_transient_progress_and_finish_failures_reuse_the_version() {
    for stage in ["rendering", "rendered", "completed", "existing_completed"] {
        let pool = pool().await;
        let (root, config, source, clip_id) = real_static_fixture(&pool).await;
        let original = tokio::fs::read(&source).await.expect("original bytes");
        let slot = automatic_schedule(&pool, "42", false, true).await;
        let queued = production_jobs::insert(
            &pool,
            &production_jobs::NewProductionJob {
                twitch_user_id: "42".into(),
                clip_id,
                source: production_jobs::JobSource::Autopilot,
                requested_mode: None,
                scheduled_at: Some(slot),
                max_attempts: 3,
            },
        )
        .await
        .expect("scheduled automatic static job");
        let existing = stage == "existing_completed";
        inject_job_database_failure(
            &pool,
            if existing { "completed" } else { stage },
            i64::from(existing),
            1,
        )
        .await;
        let runner = worker(&pool, root.path(), VideoProcessor::default());
        assert!(runner.run_once().await.expect("actual first attempt"));
        let expected = config
            .output_root
            .canonicalize()
            .expect("output root")
            .join("42")
            .join(format!("production-{}", queued.id))
            .join("version-1.mp4");
        let mut previous_bytes = None;
        if existing {
            previous_bytes = Some(
                tokio::fs::read(&expected)
                    .await
                    .expect("completed immutable output"),
            );
            sqlx::query("UPDATE social_production_jobs SET status = 'running', stage = 'rendered', finished_at = NULL WHERE id = $1")
                .bind(queued.id).execute(&pool).await.expect("restart before final bookkeeping");
            assert!(runner
                .run_once()
                .await
                .expect("existing-output completion fault"));
        }
        let failed_attempt = production_jobs::get(&pool, "42", queued.id)
            .await
            .expect("job after fault")
            .expect("owned job");
        assert_eq!(failed_attempt.status, "queued", "{stage}");
        assert_eq!(failed_attempt.attempts, if existing { 2 } else { 1 });
        assert_eq!(
            failed_attempt.metadata["last_failure"],
            "static_production_failed"
        );
        assert!(failed_attempt.metadata["retry_after"].is_string());
        assert!(!runner.run_once().await.expect("retry backoff is respected"));
        let videos = produced_video::list(&pool, "42", 50)
            .await
            .expect("checkpoint version");
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].version_no, 1);
        assert_eq!(videos[0].scheduled_at, Some(slot));
        assert_eq!(
            videos[0].status,
            if stage.starts_with("render") {
                "rendering"
            } else {
                "review"
            }
        );
        let saved_file = if stage == "rendered" {
            Some(expected.with_extension("rendering.mp4"))
        } else if stage == "rendering" {
            None
        } else {
            Some(expected.clone())
        };
        if let Some(file) = &saved_file {
            previous_bytes = Some(tokio::fs::read(file).await.expect("valid checkpoint bytes"));
        }
        retry_now(&pool, queued.id).await;
        let restart = if stage == "rendering" {
            worker(&pool, root.path(), VideoProcessor::default())
        } else {
            worker(
                &pool,
                root.path(),
                VideoProcessor::new("unavailable-ffmpeg", "unavailable-ffprobe"),
            )
        };
        assert!(restart
            .run_once()
            .await
            .expect("real retry or immutable completion recovery"));
        let done = production_jobs::get(&pool, "42", queued.id)
            .await
            .expect("finished retry")
            .expect("owned job");
        assert_eq!(done.status, "completed");
        assert_eq!(done.attempts, if existing { 3 } else { 2 });
        assert_eq!(done.mode, Some(ProductionMode::Static));
        assert_eq!(done.started_at, failed_attempt.started_at);
        let finished = produced_video::list(&pool, "42", 50)
            .await
            .expect("final immutable version");
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].id, videos[0].id);
        assert_eq!(finished[0].file_path.as_deref(), expected.to_str());
        assert_eq!(finished[0].scheduled_at, Some(slot));
        assert!(!finished[0].needs_review);
        if let Some(bytes) = previous_bytes {
            assert_eq!(
                tokio::fs::read(&expected)
                    .await
                    .expect("unchanged output bytes"),
                bytes
            );
        }
        assert_eq!(
            tokio::fs::read(&source).await.expect("unchanged source"),
            original
        );
        let fault_calls: i64 = sqlx::query_scalar("SELECT last_value FROM job_database_fault")
            .fetch_one(&pool)
            .await
            .expect("real fault executed");
        assert!(fault_calls >= 2);
        let review_jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM social_production_jobs WHERE twitch_user_id = '42' AND source = 'autopilot' AND status = 'review'")
            .fetch_one(&pool).await.expect("no unfinished review job");
        assert_eq!(review_jobs, 0);
        let uploads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
            .fetch_one(&pool)
            .await
            .expect("no publication");
        assert_eq!(uploads, 0);
        cleanup(pool).await;
    }
}

#[tokio::test]
async fn autopilot_runner_persistent_database_failures_stop_at_max_attempts() {
    for stage in ["rendering", "rendered", "completed"] {
        let pool = pool().await;
        let (root, config, _, clip_id) = real_static_fixture(&pool).await;
        let queued = production_jobs::insert(
            &pool,
            &production_jobs::NewProductionJob {
                twitch_user_id: "42".into(),
                clip_id,
                source: production_jobs::JobSource::Manual,
                requested_mode: Some(ProductionMode::Static),
                scheduled_at: None,
                max_attempts: 2,
            },
        )
        .await
        .expect("bounded manual job");
        inject_job_database_failure(&pool, stage, 0, 10).await;
        let runner = worker(&pool, root.path(), VideoProcessor::default());
        assert!(runner.run_once().await.expect("first fault"));
        let videos = produced_video::list(&pool, "42", 50)
            .await
            .expect("checkpoint version");
        assert_eq!(videos.len(), 1);
        assert_ne!(videos[0].status, "failed");
        let expected = config
            .output_root
            .canonicalize()
            .expect("root")
            .join("42")
            .join(format!("production-{}", queued.id))
            .join("version-1.mp4");
        let file = if stage == "rendered" {
            expected.with_extension("rendering.mp4")
        } else {
            expected.clone()
        };
        let bytes = if stage == "rendering" {
            None
        } else {
            Some(
                tokio::fs::read(&file)
                    .await
                    .expect("valid rendering checkpoint"),
            )
        };
        retry_now(&pool, queued.id).await;
        let restart = worker(
            &pool,
            root.path(),
            VideoProcessor::new("unavailable-ffmpeg", "unavailable-ffprobe"),
        );
        assert!(restart.run_once().await.expect("last permitted attempt"));
        let held = production_jobs::get(&pool, "42", queued.id)
            .await
            .expect("bounded result")
            .expect("owned job");
        assert_eq!(held.status, "failed");
        assert_eq!(held.attempts, 2);
        assert_eq!(held.review_reasons, json!(["attempts_exhausted"]));
        let finished = produced_video::list(&pool, "42", 50)
            .await
            .expect("retained version");
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].id, videos[0].id);
        assert_eq!(
            finished[0].status,
            if stage == "completed" {
                "review"
            } else {
                "failed"
            }
        );
        if let Some(bytes) = bytes {
            assert_eq!(
                tokio::fs::read(&file).await.expect("unchanged valid bytes"),
                bytes
            );
        }
        assert!(!restart
            .run_once()
            .await
            .expect("no work beyond the maximum"));
        let unchanged = production_jobs::get(&pool, "42", queued.id)
            .await
            .expect("stable terminal result")
            .expect("job");
        assert_eq!(unchanged.attempts, 2);
        cleanup(pool).await;
    }
}

#[tokio::test]
async fn autopilot_runner_recovers_when_the_retry_write_also_fails_transiently() {
    let pool = pool().await;
    let (root, _, _, clip_id) = real_static_fixture(&pool).await;
    let queued = job(&pool, "42", clip_id, Some(ProductionMode::Static)).await;
    inject_job_database_failure(&pool, "rendering", 0, 1).await;
    sqlx::raw_sql("CREATE SEQUENCE requeue_database_fault;
        CREATE FUNCTION fail_first_requeue() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN
            IF NEW.status = 'queued' AND nextval('requeue_database_fault') = 1 THEN
                RAISE EXCEPTION 'isolated transient requeue failure' USING ERRCODE = '40001';
            END IF;
            RETURN NEW;
        END;
        $$;
        CREATE TRIGGER isolated_requeue_database_fault BEFORE UPDATE OF status ON social_production_jobs
        FOR EACH ROW EXECUTE FUNCTION fail_first_requeue()")
        .execute(&pool).await.expect("real retry-update failure");
    let runner = worker(&pool, root.path(), VideoProcessor::default());
    assert!(runner.run_once().await.is_err());
    let interrupted = production_jobs::get(&pool, "42", queued.id)
        .await
        .expect("interrupted job")
        .expect("owned job");
    assert_eq!(interrupted.status, "running");
    assert_eq!(interrupted.attempts, 1);
    let video = produced_video::list(&pool, "42", 50)
        .await
        .expect("rendering checkpoint");
    assert_eq!(video.len(), 1);
    assert_eq!(video[0].status, "rendering");
    assert!(runner
        .run_once()
        .await
        .expect("real recovery after transient database rollback"));
    let done = production_jobs::get(&pool, "42", queued.id)
        .await
        .expect("recovered job")
        .expect("owned job");
    assert_eq!(done.status, "completed");
    assert_eq!(done.attempts, 2);
    assert_eq!(done.metadata["recovered"], true);
    assert_eq!(
        produced_video::list(&pool, "42", 50)
            .await
            .expect("same version")[0]
            .id,
        video[0].id
    );
    cleanup(pool).await;
}
