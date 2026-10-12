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
    let mut configuration: tb_llm::social_editor::EditorConfig =
        serde_yaml::from_str(include_str!("../../../../../ops/social-ai/editor.yaml"))
            .expect("existing packaged configuration");
    configuration.runtime["source_root"] = json!("clips");
    configuration.runtime["output_root"] = json!("clips");
    configuration.runtime["work_root"] = json!("clips/.ai-editor-work");
    let directory = root.join("ops/social-ai");
    std::fs::create_dir_all(&directory).expect("owned configuration directory");
    std::fs::write(
        directory.join("editor.yaml"),
        serde_yaml::to_string(&configuration).expect("existing configuration with isolated paths"),
    )
    .expect("owned media configuration");
    let media = MediaRuntime::load(root).expect("existing media runtime with isolated paths");
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

#[tokio::test]
async fn autopilot_runner_static_local_media_and_restart_preserve_immutable_version() {
    let pool = pool().await;
    sqlx::raw_sql("INSERT INTO twitch_streamers VALUES ('42');
        ALTER TABLE twitch_clips_social_media ADD COLUMN custom_title TEXT;
        ALTER TABLE twitch_clips_social_media ADD COLUMN custom_description TEXT;
        ALTER TABLE twitch_clips_social_media ADD COLUMN hashtags TEXT DEFAULT '[]';
        ALTER TABLE twitch_clips_social_media ADD COLUMN layout_override_json JSONB;
        CREATE TABLE social_media_streamer_layout (twitch_user_id TEXT PRIMARY KEY, layout_json JSONB, cam_enabled BOOLEAN, mode TEXT)")
        .execute(&pool).await.expect("static rendering prerequisites");
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
    let original = tokio::fs::read(&source)
        .await
        .expect("synthetic original bytes");
    let clip_id = clip(&pool, "42", "static-local", utc("2025-01-01T00:00:00Z")).await;
    sqlx::query("UPDATE twitch_clips_social_media SET local_file_path=$2, custom_description='Lokaler Test' WHERE id=$1")
        .bind(clip_id).bind(source.to_string_lossy().as_ref()).execute(&pool).await.expect("local source only");
    let queued = job(&pool, "42", clip_id, Some(ProductionMode::Static)).await;
    let runner = worker(&pool, root.path(), VideoProcessor::default());
    assert!(runner.run_once().await.expect("concrete static dispatch"));
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
    assert_eq!(held.status, "review");
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
