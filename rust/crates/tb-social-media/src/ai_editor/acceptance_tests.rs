use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Duration as Span, Utc};

use super::*;
use crate::autopilot_publish::{
    self, approve, snapshot, tiktok_subject, DestinationDecision, DestinationPublisher,
    PublishExecutor, PublishOutcome, PublishRequest,
};
use crate::autopilot_settings::{
    PipelineMode, PolicyScope, ReviewPolicy, ScheduleSlot, TextDefaults,
};
use crate::production_jobs::autopilot_schedule::{
    resolve_review, save_schedule, ReviewDecision, SchedulePatch,
};
use crate::uploaders::tiktok::TikTokPostOptions;
use crate::uploaders::{UploadCheckpoint, UploadError};

const OWNER: &str = "101";
const SELECTED: [&str; 2] = ["youtube", "tiktok"];
const SHARED_TEXT: &str = "Gemeinsamer Text";
const TIKTOK_TEXT: &str = "Nur für TikTok";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Call {
    platform: String,
    file_path: String,
    sha256: String,
    title: String,
    description: String,
    hashtags: Vec<String>,
}

#[derive(Default)]
struct FakePlatforms {
    calls: Mutex<Vec<Call>>,
    polls: Mutex<Vec<String>>,
}

impl FakePlatforms {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("calls").clone()
    }

    fn on(&self, platform: &str) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|call| call.platform == platform)
            .collect()
    }

    async fn record(&self, platform: &str, request: &PublishRequest) -> usize {
        let sha256 = crate::uploaders::tiktok::video_digest(&request.file_path)
            .await
            .expect("fake destination reads the delivered file");
        let mut calls = self.calls.lock().expect("calls");
        calls.push(Call {
            platform: platform.into(),
            file_path: request.file_path.clone(),
            sha256,
            title: request.title.clone(),
            description: request.description.clone(),
            hashtags: request.hashtags.clone(),
        });
        calls.len()
    }
}

#[async_trait]
impl DestinationPublisher for FakePlatforms {
    async fn publish(
        &self,
        _owner: &str,
        platform: &str,
        request: &PublishRequest,
    ) -> Result<PublishOutcome, UploadError> {
        let number = self.record(platform, request).await;
        Ok(PublishOutcome {
            external_id: format!("{platform}-{number}"),
            visibility: Some("public".into()),
        })
    }

    async fn publish_tiktok(
        &self,
        _owner: &str,
        request: &PublishRequest,
        options: &TikTokPostOptions,
        checkpoint: &dyn UploadCheckpoint,
    ) -> Result<String, UploadError> {
        assert!(options.consent, "TikTok dispatch without per-post consent");
        let number = self.record("tiktok", request).await;
        let publish_id = format!("pub-{number}");
        checkpoint.record_publish_id(&publish_id).await?;
        Ok(publish_id)
    }

    async fn tiktok_status(
        &self,
        _owner: &str,
        _options: &TikTokPostOptions,
        publish_id: &str,
    ) -> Result<Value, UploadError> {
        self.polls.lock().expect("polls").push(publish_id.into());
        Ok(
            json!({"status": "PUBLISH_COMPLETE", "publicaly_available_post_id": [format!("post-{publish_id}")]}),
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct Cell {
    name: &'static str,
    mode: PipelineMode,
    review: ReviewPolicy,
}

async fn harness_database() -> postgres::TestPostgres {
    let db = database().await;
    sqlx::raw_sql(
        "ALTER TABLE social_media_platform_auth ADD COLUMN IF NOT EXISTS twitch_user_id TEXT;
        INSERT INTO social_media_platform_auth (platform, streamer_login, access_token_enc, platform_user_id, twitch_user_id, enabled)
        VALUES ('youtube', 'fixture-one', '\\x00'::bytea, 'yt-1', '101', 1),
               ('tiktok', 'fixture-one', '\\x00'::bytea, 'tt-1', '101', 1),
               ('instagram', 'fixture-one', '\\x00'::bytea, 'ig-1', '101', 1);
        UPDATE social_media_settings SET value = 'false'::jsonb WHERE key = 'external_llm_consent';
        UPDATE social_media_category SET twitch_game_id = '2132205352' WHERE category_key = 'deadlock';",
    )
    .execute(&db.pool)
    .await
    .expect("connected destinations without external AI consent");
    for sql in [
        include_str!("../../../../migrations/20260619010000_runtime_type_contract.sql"),
        include_str!("../../../../migrations/20260930030000_social_media_tiktok_recovery.sql"),
        include_str!("../../../../migrations/20261007042000_social_media_tiktok_direct_post.sql"),
        include_str!("../../../../migrations/20261007213000_social_upload_visibility.sql"),
        include_str!("../../../../migrations/20261011101000_social_clip_pool.sql"),
        include_str!("../../../../migrations/20261011102000_social_production_slot_guards.sql"),
        include_str!("../../../../migrations/20261016120000_social_pipeline_write_barrier.sql"),
        include_str!("../../../../migrations/20261016200000_social_pipeline_barrier_clip_lock.sql"),
    ] {
        sqlx::raw_sql(sql)
            .execute(&db.pool)
            .await
            .expect("real publishing migrations");
    }
    db
}

async fn deadlock_clip(pool: &PgPool, runtime: &EditorRuntime, mode: PipelineMode) -> i64 {
    let clip = source(pool, runtime).await;
    if mode != PipelineMode::AiEdit {
        vertical_original(runtime).await;
    }
    sqlx::query("UPDATE twitch_clips_social_media SET game_id = '2132205352' WHERE id = $1")
        .bind(clip)
        .execute(pool)
        .await
        .expect("automatic Deadlock source");
    clip
}

async fn vertical_original(runtime: &EditorRuntime) {
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=720x1280:r=25",
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
        .arg(runtime.limits.source_root.join("fixture-original.mp4"))
        .status()
        .await
        .expect("vertical synthetic media");
    assert!(status.success());
}

async fn consent(pool: &PgPool, granted: bool) {
    sqlx::query("UPDATE social_media_settings SET value = $1 WHERE key = 'external_llm_consent'")
        .bind(json!(granted))
        .execute(pool)
        .await
        .expect("global external AI consent");
}

async fn database_now(pool: &PgPool) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(pool)
        .await
        .expect("database clock")
}

async fn channel(pool: &PgPool, cell: Cell) -> DateTime<Utc> {
    let slot: DateTime<Utc> =
        sqlx::query_scalar("SELECT date_trunc('minute', clock_timestamp()) + INTERVAL '5 minutes'")
            .fetch_one(pool)
            .await
            .expect("future database slot");
    save_schedule(
        pool,
        OWNER,
        &SchedulePatch {
            enabled: Some(true),
            ai_editing_enabled: Some(cell.mode == PipelineMode::AiEdit),
            timezone: Some("UTC".into()),
            schedule_slots: Some(
                (1..=7)
                    .map(|weekday| ScheduleSlot {
                        weekday,
                        local_time: slot.format("%H:%M").to_string(),
                    })
                    .collect(),
            ),
            target_platforms: Some(SELECTED.iter().map(|p| p.to_string()).collect()),
            ..Default::default()
        },
    )
    .await
    .expect("shared schedule");
    let mut policy = autopilot_settings::load_policy(pool, OWNER)
        .await
        .expect("policy");
    policy.pipeline_mode = cell.mode;
    policy.review_policy = cell.review;
    policy.metadata_scope = PolicyScope::PerPlatform;
    policy.text_defaults.shared = TextDefaults {
        description: SHARED_TEXT.into(),
        hashtags: vec!["deadlock".into()],
        ..Default::default()
    };
    policy.text_defaults.per_platform.insert(
        "tiktok".into(),
        TextDefaults {
            description: TIKTOK_TEXT.into(),
            hashtags: vec!["tiktok".into()],
            ..Default::default()
        },
    );
    autopilot_settings::save_policy(pool, OWNER, &policy)
        .await
        .expect("pipeline policy");
    sqlx::query("INSERT INTO social_pipeline_cutover (twitch_user_id, state, migration_version) VALUES ($1, 'unified', 1)")
        .bind(OWNER)
        .execute(pool)
        .await
        .expect("unified owner");
    slot
}

async fn job(pool: &PgPool, clip: i64, slot: DateTime<Utc>) -> production_jobs::ProductionJob {
    production_jobs::insert(
        pool,
        &production_jobs::NewProductionJob {
            twitch_user_id: OWNER.into(),
            clip_id: clip,
            source: production_jobs::JobSource::Autopilot,
            requested_mode: None,
            scheduled_at: Some(slot),
            max_attempts: 2,
        },
    )
    .await
    .expect("automatic job")
}

fn worker(pool: &PgPool, runtime: &EditorRuntime) -> crate::autopilot_runner::AutopilotWorker {
    crate::autopilot_runner::AutopilotWorker::new(
        pool.clone(),
        crate::autopilot_runner::WorkerConfig::from_media_runtime(
            runtime.media(),
            Some(runtime.clone()),
            None,
        ),
        Arc::new(NoDownload),
        crate::video_processor::VideoProcessor::new("unavailable-ffmpeg", "unavailable-ffprobe"),
    )
    .expect("runner")
}

async fn usage(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM llm_usage")
        .fetch_one(pool)
        .await
        .expect("provider ledgers")
}

async fn produce(
    pool: &PgPool,
    runtime: &EditorRuntime,
    cell: Cell,
    clip: i64,
    slot: DateTime<Utc>,
) -> ProducedVideo {
    let job = job(pool, clip, slot).await;
    let runner = worker(pool, runtime);
    match cell.mode {
        PipelineMode::AiEdit => {
            consent(pool, true).await;
            let mut lease = super::super::super::store::acquire_production_lease(pool)
                .await
                .expect("lease")
                .expect("free");
            let checkpoints = prepared(
                pool,
                runtime,
                &mut lease,
                clip,
                Identity::Production(job.id),
            )
            .await;
            let state = serde_json::to_value(ProductionState {
                checkpoints,
                attempts: BTreeMap::new(),
            })
            .expect("state");
            sqlx::query("UPDATE social_production_jobs SET metadata = jsonb_build_object('ai_editor', $2::jsonb) WHERE id = $1")
                .bind(job.id)
                .bind(state)
                .execute(pool)
                .await
                .expect("provider stages answered by the fake provider");
            lease.release().await.expect("release before runner");
            assert!(runner.run_once().await.expect("AI production"));
        }
        PipelineMode::UploadText => {
            assert!(runner.run_once().await.expect("text hold"));
            let held = production_jobs::get(pool, OWNER, job.id)
                .await
                .expect("held job")
                .expect("owned job");
            assert_eq!(held.status, "review");
            assert_eq!(held.review_reasons, json!(["external_ai_consent_required"]));
            let draft = only_video(pool).await;
            assert_eq!(draft.status, "rendering");
            sqlx::query("UPDATE social_produced_videos SET metadata = jsonb_set(metadata, '{platforms}', $2) || jsonb_build_object('ai_text', jsonb_build_object('stage', 'copywriting')) WHERE id=$1")
                .bind(draft.id)
                .bind(fake_copy())
                .execute(pool)
                .await
                .expect("copy answered by the fake provider");
            resolve_review(pool, OWNER, job.id, ReviewDecision::Process, None)
                .await
                .expect("process held job");
            assert!(runner.run_once().await.expect("text production"));
        }
        PipelineMode::Upload => {
            assert!(runner.run_once().await.expect("upload production"));
        }
    }
    let done = production_jobs::get(pool, OWNER, job.id)
        .await
        .expect("job")
        .expect("owned job");
    assert_eq!(done.status, "completed", "{} job", cell.name);
    let video = only_video(pool).await;
    assert_eq!(video.job_id, Some(job.id));
    assert_eq!(video.status, "review", "{} version", cell.name);
    assert_eq!(video.review_reasons, json!([]));
    assert_eq!(
        video.needs_review,
        cell.review == ReviewPolicy::Required,
        "{} review flag",
        cell.name
    );
    assert_eq!(
        video
            .policy_snapshot
            .as_ref()
            .map(|policy| policy.review_policy),
        Some(cell.review)
    );
    video
}

fn fake_copy() -> Value {
    json!({
        "youtube": {"title": "KI-Titel YouTube", "description": "KI-Text YouTube", "hashtags": ["deadlock"]},
        "tiktok": {"title": "KI-Titel TikTok", "description": "KI-Text TikTok", "hashtags": ["tiktok"]}
    })
}

async fn only_video(pool: &PgPool) -> ProducedVideo {
    let videos = produced_video::list(pool, OWNER, 50)
        .await
        .expect("versions");
    assert_eq!(videos.len(), 1);
    videos.into_iter().next().expect("one version")
}

async fn sha(path: &str) -> String {
    crate::uploaders::tiktok::video_digest(path)
        .await
        .expect("artifact digest")
}

fn assert_mode_artifact(cell: Cell, video: &ProducedVideo, original: &[u8]) {
    let output = video.file_path.clone().expect("artifact path");
    let bytes = std::fs::read(&output).expect("artifact bytes");
    match cell.mode {
        PipelineMode::Upload | PipelineMode::UploadText => {
            assert_eq!(video.mode, ProductionMode::Static);
            assert_eq!(video.metadata["production_mode"], "upload");
            assert_eq!(video.metadata["upload_artifact"]["conversion"], "exact");
            assert!(video.metadata.get("ai_editor").is_none());
            assert_eq!(bytes, original, "{} keeps the original bytes", cell.name);
        }
        PipelineMode::AiEdit => {
            assert_eq!(video.mode, ProductionMode::Ai);
            assert_ne!(bytes, original, "{} renders an edited artifact", cell.name);
        }
    }
    match cell.mode {
        PipelineMode::Upload => {
            assert!(video.metadata.get("ai_text").is_none());
            assert_eq!(
                video.metadata["platforms"]["youtube"]["description"],
                SHARED_TEXT
            );
            assert_eq!(
                video.metadata["platforms"]["tiktok"]["description"],
                TIKTOK_TEXT
            );
        }
        PipelineMode::UploadText => assert_eq!(video.metadata["platforms"], fake_copy()),
        PipelineMode::AiEdit => {}
    }
}

fn frozen_text(video: &Value, platform: &str) -> (String, String, Vec<String>) {
    let text = &video["platforms"][platform];
    (
        text["title"].as_str().unwrap_or_default().to_owned(),
        text["description"].as_str().unwrap_or_default().to_owned(),
        text["hashtags"]
            .as_array()
            .map(|tags| {
                tags.iter()
                    .filter_map(|tag| tag.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

async fn tiktok_decision(pool: &PgPool, id: i64) -> DestinationDecision {
    let subject = tiktok_subject(pool, OWNER, id)
        .await
        .expect("tiktok subject");
    let account = subject.platform_user_id.clone().expect("tiktok account");
    sqlx::query("INSERT INTO social_media_settings (key, value) VALUES ($1, $2::text::jsonb) ON CONFLICT (key) DO NOTHING")
        .bind(format!("tiktok_music_consent:{OWNER}:{account}"))
        .bind(json!({"platform_user_id": account, "consented_at": "2026-10-01T00:00:00Z"}).to_string())
        .execute(pool)
        .await
        .expect("music consent");
    DestinationDecision {
        platform: "tiktok".into(),
        tiktok_options: Some(json!({
            "caption": subject.caption,
            "privacy_level": "SELF_ONLY",
            "allow_comment": false,
            "allow_duet": false,
            "allow_stitch": false,
            "commercial_content": false,
            "brand_organic_toggle": false,
            "brand_content_toggle": false,
            "consent": true,
            "creator_username": "creator",
            "credential_id": subject.credential_id,
            "platform_user_id": subject.platform_user_id,
            "approved_video_sha256": subject.approved_video_sha256,
        })),
    }
}

async fn approve_all(pool: &PgPool, id: i64) {
    let decisions = vec![
        DestinationDecision {
            platform: "youtube".into(),
            tiktok_options: None,
        },
        tiktok_decision(pool, id).await,
    ];
    approve(pool, OWNER, id, &decisions)
        .await
        .expect("owner approval with per-post TikTok consent");
}

async fn rows(pool: &PgPool, id: i64) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as("SELECT platform, status, outcome_reason FROM twitch_clips_upload_queue WHERE produced_video_id = $1 ORDER BY platform")
        .bind(id)
        .fetch_all(pool)
        .await
        .expect("destinations")
}

async fn status_of(pool: &PgPool, id: i64, platform: &str) -> Option<(String, Option<String>)> {
    sqlx::query_as("SELECT status, outcome_reason FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND platform = $2")
        .bind(id)
        .bind(platform)
        .fetch_optional(pool)
        .await
        .expect("destination")
}

async fn due(pool: &PgPool, id: i64, platform: Option<&str>) {
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = LEAST(scheduled_at, clock_timestamp() - INTERVAL '1 minute') WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("due event");
    sqlx::query("UPDATE twitch_clips_upload_queue SET scheduled_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1 AND ($2::text IS NULL OR platform = $2)")
        .bind(id)
        .bind(platform)
        .execute(pool)
        .await
        .expect("due destinations");
}

async fn age_tiktok(pool: &PgPool, id: i64) {
    sqlx::query("UPDATE twitch_clips_upload_queue SET last_attempt_at = clock_timestamp() - INTERVAL '2 minutes' WHERE produced_video_id = $1 AND platform = 'tiktok'")
        .bind(id)
        .execute(pool)
        .await
        .expect("aged tiktok row");
}

async fn bound_hash(pool: &PgPool, id: i64, platform: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT metadata#>>ARRAY['publication', 'hashes', $2] FROM social_produced_videos WHERE id = $1",
    )
    .bind(id)
    .bind(platform)
    .fetch_one(pool)
    .await
    .expect("bound hash")
}

async fn visible_reason(pool: &PgPool, id: i64, platform: &str) -> Option<String> {
    snapshot(pool, OWNER)
        .await
        .expect("snapshot")
        .videos
        .into_iter()
        .find(|video| video.id == id)
        .and_then(|video| video.event)
        .and_then(|event| {
            event
                .destinations
                .into_iter()
                .find(|destination| destination.platform == platform)
        })
        .and_then(|destination| destination.reason)
}

fn assert_delivered(call: &Call, video: &ProducedVideo, hash: &str) {
    let (title, description, hashtags) = frozen_text(&video.metadata, &call.platform);
    assert_eq!(Some(call.file_path.as_str()), video.file_path.as_deref());
    assert_eq!(
        call.sha256, hash,
        "{} got the approved artifact",
        call.platform
    );
    assert_eq!(call.title, title, "{} title is frozen", call.platform);
    assert_eq!(
        call.description, description,
        "{} description is frozen",
        call.platform
    );
    assert_eq!(
        call.hashtags, hashtags,
        "{} hashtags are frozen",
        call.platform
    );
}

fn on_large_stack(cell: Cell) {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("cell runtime")
                .block_on(run_cell(cell))
        })
        .expect("cell thread")
        .join()
        .expect("cell passed");
}

async fn run_cell(cell: Cell) {
    let db = harness_database().await;
    let pool = &db.pool;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = deadlock_clip(pool, &runtime, cell.mode).await;
    let original = std::fs::read(runtime.limits.source_root.join("fixture-original.mp4"))
        .expect("original bytes");
    let slot = channel(pool, cell).await;
    let video = produce(pool, &runtime, cell, clip, slot).await;
    assert_mode_artifact(cell, &video, &original);
    assert_eq!(usage(pool).await, 0, "{} made no provider call", cell.name);
    let artifact = sha(video.file_path.as_deref().expect("artifact")).await;
    let fake = Arc::new(FakePlatforms::default());
    let executor = PublishExecutor::new(pool.clone(), fake.clone()).with_tiktok_dispatch(true);
    executor.tick().await.expect("tick before the slot");
    assert!(
        fake.calls().is_empty(),
        "{} uploaded before the slot",
        cell.name
    );
    if cell.review == ReviewPolicy::Required {
        assert!(rows(pool, video.id).await.is_empty());
        due(pool, video.id, None).await;
        executor.tick().await.expect("due tick without approval");
        assert!(
            fake.calls().is_empty(),
            "{} uploaded without approval",
            cell.name
        );
        assert!(rows(pool, video.id).await.is_empty());
        approve_all(pool, video.id).await;
    }
    if cell.review == ReviewPolicy::VetoUntilDeadline {
        assert!(rows(pool, video.id).await.is_empty());
        assert_eq!(video.metadata["veto_deadline"], json!(slot));
        assert!(slot > database_now(pool).await);
        due(pool, video.id, None).await;
        executor.tick().await.expect("tick inside the veto window");
        assert!(
            fake.calls().is_empty(),
            "{} posted inside the veto window",
            cell.name
        );
        assert!(rows(pool, video.id).await.is_empty());
        let passed = database_now(pool).await - Span::minutes(1);
        sqlx::query("UPDATE social_produced_videos SET metadata = jsonb_set(metadata, '{veto_deadline}', $2) WHERE id = $1")
            .bind(video.id)
            .bind(json!(passed))
            .execute(pool)
            .await
            .expect("veto deadline passed");
    }
    due(pool, video.id, None).await;
    executor.tick().await.expect("due tick");
    let youtube = fake.on("youtube");
    assert_eq!(youtube.len(), 1, "{} youtube posts", cell.name);
    let video = only_video(pool).await;
    assert_eq!(
        bound_hash(pool, video.id, "youtube").await.as_deref(),
        Some(artifact.as_str())
    );
    assert_delivered(&youtube[0], &video, &artifact);
    if cell.review == ReviewPolicy::Required {
        assert_eq!(fake.on("tiktok").len(), 1, "{} tiktok dispatch", cell.name);
        assert_delivered(&fake.on("tiktok")[0], &video, &artifact);
        age_tiktok(pool, video.id).await;
        executor.tick().await.expect("reconcile tick");
        assert_eq!(
            status_of(pool, video.id, "tiktok").await.map(|row| row.0),
            Some(autopilot_publish::queue_status::PUBLISHED.into())
        );
    } else {
        assert!(
            fake.on("tiktok").is_empty(),
            "{} TikTok without consent",
            cell.name
        );
        let held = status_of(pool, video.id, "tiktok")
            .await
            .expect("tiktok destination");
        assert_eq!(
            held.0,
            autopilot_publish::queue_status::HELD,
            "{}",
            cell.name
        );
        let reason = held.1.expect("held reason");
        assert!(
            reason.starts_with("tiktok_"),
            "{} reason {reason}",
            cell.name
        );
        assert_eq!(visible_reason(pool, video.id, "tiktok").await, Some(reason));
        let consent: Option<Value> = sqlx::query_scalar(
            "SELECT metadata #> '{publication,tiktok_options}' FROM social_produced_videos WHERE id = $1",
        )
        .bind(video.id)
        .fetch_one(pool)
        .await
        .expect("tiktok binding");
        assert!(
            consent.is_none_or(|options| options["consent"] != json!(true)),
            "{} consent was set automatically",
            cell.name
        );
    }
    assert!(
        fake.on("instagram").is_empty(),
        "unselected destination contacted"
    );
    assert!(status_of(pool, video.id, "instagram").await.is_none());
    let before = fake.calls();
    sqlx::query("UPDATE twitch_clips_upload_queue SET last_attempt_at = clock_timestamp() - INTERVAL '3 hours', effect_started_at = clock_timestamp() - INTERVAL '3 hours' WHERE produced_video_id = $1")
        .bind(video.id)
        .execute(pool)
        .await
        .expect("stale timestamps");
    let restarted = PublishExecutor::new(pool.clone(), fake.clone()).with_tiktok_dispatch(true);
    restarted.tick().await.expect("restart tick");
    restarted.tick().await.expect("second restart tick");
    assert_eq!(
        fake.calls(),
        before,
        "{} republished after restart",
        cell.name
    );
    let platforms: BTreeSet<String> = fake.calls().into_iter().map(|call| call.platform).collect();
    assert!(platforms
        .iter()
        .all(|platform| SELECTED.contains(&platform.as_str())));
}

#[test]
fn acceptance_t1_upload_with_review() {
    on_large_stack(Cell {
        name: "T1",
        mode: PipelineMode::Upload,
        review: ReviewPolicy::Required,
    });
}

#[test]
fn acceptance_t2_upload_without_review() {
    on_large_stack(Cell {
        name: "T2",
        mode: PipelineMode::Upload,
        review: ReviewPolicy::Off,
    });
}

#[test]
fn acceptance_t3_ai_text_with_review() {
    on_large_stack(Cell {
        name: "T3",
        mode: PipelineMode::UploadText,
        review: ReviewPolicy::Required,
    });
}

#[test]
fn acceptance_t4_ai_text_without_review() {
    on_large_stack(Cell {
        name: "T4",
        mode: PipelineMode::UploadText,
        review: ReviewPolicy::Off,
    });
}

#[test]
fn acceptance_t5_ai_edit_with_review() {
    on_large_stack(Cell {
        name: "T5",
        mode: PipelineMode::AiEdit,
        review: ReviewPolicy::Required,
    });
}

#[test]
fn acceptance_t6_ai_edit_without_review() {
    on_large_stack(Cell {
        name: "T6",
        mode: PipelineMode::AiEdit,
        review: ReviewPolicy::Off,
    });
}

#[test]
fn acceptance_veto_publishes_only_after_the_deadline() {
    on_large_stack(Cell {
        name: "veto",
        mode: PipelineMode::Upload,
        review: ReviewPolicy::VetoUntilDeadline,
    });
}

fn same_weekday_slots(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let day = (now + Span::days(2)).date_naive();
    let at = |hour| day.and_hms_opt(hour, 0, 0).expect("valid time").and_utc();
    (at(10), at(20))
}

#[tokio::test]
async fn acceptance_per_platform_schedule_on_one_weekday_never_posts_early() {
    let cell = Cell {
        name: "per-platform",
        mode: PipelineMode::Upload,
        review: ReviewPolicy::Required,
    };
    let db = harness_database().await;
    let pool = &db.pool;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = deadlock_clip(pool, &runtime, cell.mode).await;
    let slot = channel(pool, cell).await;
    let video = produce(pool, &runtime, cell, clip, slot).await;
    let artifact = sha(video.file_path.as_deref().expect("artifact")).await;
    let (morning, evening) = same_weekday_slots(database_now(pool).await);
    let slots = |time: DateTime<Utc>| json!([{"weekday": time.weekday().number_from_monday(), "local_time": time.format("%H:%M").to_string()}]);
    sqlx::query("UPDATE social_media_autopilot_settings SET schedule_scope = 'per_platform', platform_schedule_slots = $2::text::jsonb WHERE twitch_user_id = $1")
        .bind(OWNER)
        .bind(json!({"youtube": slots(morning), "tiktok": slots(evening)}).to_string())
        .execute(pool)
        .await
        .expect("per destination schedule");
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = NULL WHERE id = $1")
        .bind(video.id)
        .execute(pool)
        .await
        .expect("event placed by the destination schedule");
    approve_all(pool, video.id).await;
    let times: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT platform, scheduled_at FROM twitch_clips_upload_queue WHERE produced_video_id = $1 ORDER BY platform",
    )
    .bind(video.id)
    .fetch_all(pool)
    .await
    .expect("destination times");
    assert_eq!(
        times,
        vec![
            ("tiktok".to_string(), evening),
            ("youtube".to_string(), morning)
        ]
    );
    let fake = Arc::new(FakePlatforms::default());
    let executor = PublishExecutor::new(pool.clone(), fake.clone()).with_tiktok_dispatch(true);
    executor.tick().await.expect("tick before both slots");
    assert!(fake.calls().is_empty(), "posted before its slot");
    due(pool, video.id, Some("youtube")).await;
    executor.tick().await.expect("morning tick");
    assert_eq!(fake.on("youtube").len(), 1);
    assert!(
        fake.on("tiktok").is_empty(),
        "tiktok posted before its evening slot"
    );
    assert_eq!(
        status_of(pool, video.id, "youtube").await.map(|row| row.0),
        Some(autopilot_publish::queue_status::PUBLISHED.into())
    );
    executor.tick().await.expect("idle tick");
    assert!(fake.on("tiktok").is_empty());
    due(pool, video.id, Some("tiktok")).await;
    executor.tick().await.expect("evening tick");
    let video = only_video(pool).await;
    assert_eq!(fake.on("youtube").len(), 1);
    assert_eq!(fake.on("tiktok").len(), 1, "later slot still runs");
    assert_delivered(&fake.on("tiktok")[0], &video, &artifact);
    assert_delivered(&fake.on("youtube")[0], &video, &artifact);
    assert!(fake.on("instagram").is_empty());
}

#[tokio::test]
async fn acceptance_barrier_rejects_new_legacy_rows_for_unified_owners() {
    let cell = Cell {
        name: "barrier",
        mode: PipelineMode::Upload,
        review: ReviewPolicy::Off,
    };
    let db = harness_database().await;
    let pool = &db.pool;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = deadlock_clip(pool, &runtime, cell.mode).await;
    channel(pool, cell).await;
    let rejected = sqlx::query(
        "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, 'youtube', 'pending')",
    )
    .bind(clip)
    .execute(pool)
    .await
    .expect_err("legacy row for a unified owner");
    let code = rejected
        .as_database_error()
        .and_then(|error| error.code())
        .map(|code| code.into_owned());
    assert_eq!(code.as_deref(), Some("23514"));
}
