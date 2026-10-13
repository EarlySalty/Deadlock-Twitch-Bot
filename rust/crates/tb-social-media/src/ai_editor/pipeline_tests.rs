use super::*;

use crate::test_support::postgres;

async fn database() -> postgres::TestPostgres {
    let db = postgres::TestPostgres::start().await;
    for sql in [
        include_str!("../../../../migrations/20260601000000_baseline_schema.sql"),
        include_str!("../../../../migrations/20260629120000_live_schema_type_reconcile.sql"),
    ] {
        sqlx::raw_sql(sql)
            .execute(&db.pool)
            .await
            .expect("real baseline migrations");
    }
    sqlx::raw_sql("INSERT INTO twitch_streamers (twitch_login,twitch_user_id) VALUES ('earlysalty','303'),('fixture-one','101'),('fixture-two','202');")
        .execute(&db.pool).await.expect("isolated identities before partner access migration");
    for sql in [
        include_str!("../../../../migrations/20260806120000_social_media_partner_access.sql"),
        include_str!("../../../../migrations/20260815120000_social_media_scheduling.sql"),
        include_str!("../../../../migrations/20261011100000_social_autopilot_contracts.sql"),
    ] {
        sqlx::raw_sql(sql)
            .execute(&db.pool)
            .await
            .expect("real contract migrations");
    }
    sqlx::raw_sql("ALTER TABLE social_media_streamer_layout ADD COLUMN twitch_user_id TEXT; ALTER TABLE clip_templates_streamer ADD COLUMN twitch_user_id TEXT; CREATE TABLE llm_usage (id BIGSERIAL PRIMARY KEY, ts TEXT, purpose TEXT, provider TEXT, model TEXT, attempt_state TEXT, tokens_in BIGINT, tokens_out BIGINT, latency_ms BIGINT); UPDATE social_media_category SET twitch_game_id='fixture-game' WHERE category_key='deadlock';")
        .execute(&db.pool).await.expect("real source columns and isolated identities");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011200001_social_ai_provider_usage.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("real provider migration");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011200002_social_ai_jobs.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("jobs migration after provider migration");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011203001_social_ai_revisions.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("revision migration");
    sqlx::query("INSERT INTO social_media_settings(key,value) VALUES ('external_llm_consent','true'::jsonb) ON CONFLICT(key) DO UPDATE SET value=EXCLUDED.value")
        .execute(&db.pool).await.expect("synthetic global consent");
    super::super::store::upsert_user_consent(&db.pool, "101", true, "fixture-owner")
        .await
        .expect("owner consent");
    db
}

fn runtime(root: &Path) -> EditorRuntime {
    let mut configuration: EditorConfig =
        toml::from_str(include_str!("../../../../../ops/social-ai/editor.toml"))
            .expect("packaged configuration");
    configuration.runtime["source_root"] = json!("clips");
    configuration.runtime["output_root"] = json!("clips");
    configuration.runtime["work_root"] = json!("clips/.ai-editor-work");
    EditorRuntime::from_configuration(root, configuration).expect("bounded runtime")
}

async fn source(pool: &PgPool, runtime: &EditorRuntime) -> i64 {
    tokio::fs::create_dir_all(&runtime.limits.source_root)
        .await
        .expect("source root");
    let output = runtime.limits.source_root.join("fixture-original.mp4");
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-threads",
            "2",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=1920x1080:r=25",
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
        .arg(&output)
        .status()
        .await
        .expect("real synthetic media");
    assert!(status.success());
    sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id,clip_url,clip_title,streamer_login,created_at,twitch_user_id,category_key,game_id,local_file_path) VALUES ('fixture-original','https://clips.twitch.tv/fixture-original','Fixture','fixture-one','2026-10-11T00:00:00Z','101','deadlock','fixture-game',$1) RETURNING id")
        .bind(output.to_string_lossy().into_owned()).fetch_one(pool).await.expect("eligible source")
}

async fn prepared(
    pool: &PgPool,
    runtime: &EditorRuntime,
    lease: &mut ProductionLease,
    clip: i64,
    identity: Identity,
) -> BTreeMap<AiStage, StageCheckpoint> {
    let mut checkpoints = BTreeMap::new();
    let checkpoint = execute_stage(
        &mut StageContext {
            pool,
            current: runtime,
            lease,
            owner: "101",
            clip_id: clip,
            identity,
            expected_twitch_clip_id: Some("fixture-original"),
            checkpoints: &checkpoints,
            brain_token: None,
        },
        AiStage::Preparing,
    )
    .await
    .expect("real preparing stage");
    checkpoints.insert(AiStage::Preparing, checkpoint);
    let (frozen, _) = preparing(&checkpoints, runtime).expect("frozen source");
    checkpoints.insert(
        AiStage::Transcribing,
        StageCheckpoint::Transcribing {
            transcript: "Originalsprache".into(),
            language: Some("de".into()),
            segments: vec![TranscriptSegment {
                start_ms: 100,
                end_ms: 1000,
                text: "Originalsprache".into(),
            }],
        },
    );
    checkpoints.insert(
        AiStage::Analyzing,
        StageCheckpoint::Analyzing {
            highlights: vec![],
            uncertainty: vec![],
            analysis_notes: vec![],
        },
    );
    let plan = Planning {
        subtitle_scale_percent: 100,
        revision_proposal: None,
        variants: Vec::new(),
        plan: super::super::EditPlan {
            schema_version: 1,
            source_duration_ms: frozen.media.duration_ms,
            kept_intervals: vec![super::super::KeptInterval {
                start_ms: 0,
                end_ms: frozen.media.duration_ms,
                reason: "Vollständiger Zusammenhang".into(),
            }],
            observed_highlights: vec![],
            significant_uncertainty: false,
            review_reasons: vec![],
            layout: super::super::LayoutIntent::ExistingStable,
        },
        platforms: ["youtube", "tiktok", "instagram"]
            .into_iter()
            .map(|platform| {
                (
                    platform.into(),
                    super::super::stage_contract::PublicationCopy {
                        title: "Fixture".into(),
                        description: "Fixture".into(),
                        hashtags: vec!["#Deadlock".into()],
                    },
                )
            })
            .collect(),
    };
    plan.validate(
        frozen.media.duration_ms,
        &runtime.limits.plan,
        &runtime.limits.copy,
        &analysis(&checkpoints).expect("persisted analysis"),
    )
    .expect("real deterministic plan validation");
    checkpoints.insert(
        AiStage::Planning,
        StageCheckpoint::Planning {
            decisions: vec![],
            review_reasons: vec![],
            edit_plan: serde_json::to_value(plan)
                .expect("plan")
                .as_object()
                .cloned()
                .expect("object"),
        },
    );
    checkpoints
}

#[tokio::test]
async fn analyzed_highlights_guard_fresh_and_resumed_plans_before_render_and_qa() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let mut checkpoints = prepared(&db.pool, &runtime, &mut lease, clip, Identity::Manual(1)).await;
    let (frozen, _) = preparing(&checkpoints, &runtime).expect("frozen source");
    let (original, _) = planning(&checkpoints, &frozen, &runtime).expect("original plan");
    let analyzed = Analysis {
        highlights: vec![TimelineEntry {
            start_ms: 100,
            end_ms: 300,
            description: "Spielmoment und Reaktion".into(),
        }],
        uncertainty: vec![],
        analysis_notes: vec![],
    };
    checkpoints.insert(
        AiStage::Analyzing,
        StageCheckpoint::Analyzing {
            highlights: analyzed.highlights.clone(),
            uncertainty: vec![],
            analysis_notes: vec![],
        },
    );
    assert!(original.plan.observed_highlights.is_empty());
    for start_ms in [200, 500] {
        let mut changed = original.clone();
        changed.plan.kept_intervals[0].start_ms = start_ms;
        assert!(matches!(
            changed.validate(
                frozen.media.duration_ms,
                &runtime.limits.plan,
                &runtime.limits.copy,
                &analyzed
            ),
            Err(AiEditorError::Plan)
        ));
        checkpoints.insert(
            AiStage::Planning,
            StageCheckpoint::Planning {
                decisions: vec![],
                review_reasons: vec![],
                edit_plan: serde_json::to_value(changed)
                    .expect("plan")
                    .as_object()
                    .cloned()
                    .expect("object"),
            },
        );
        assert!(matches!(
            planning(&checkpoints, &frozen, &runtime),
            Err(AiEditorError::Plan)
        ));
        for stage in [AiStage::Rendering, AiStage::Validating] {
            assert!(matches!(
                execute_stage(
                    &mut StageContext {
                        pool: &db.pool,
                        current: &runtime,
                        lease: &mut lease,
                        owner: "101",
                        clip_id: clip,
                        identity: Identity::Manual(1),
                        expected_twitch_clip_id: Some("fixture-original"),
                        checkpoints: &checkpoints,
                        brain_token: None,
                    },
                    stage
                )
                .await,
                Err(AiEditorError::Plan)
            ));
        }
    }
    assert!(produced_video::list(&db.pool, "101", 50)
        .await
        .expect("no allocation")
        .is_empty());
    original
        .validate(
            frozen.media.duration_ms,
            &runtime.limits.plan,
            &runtime.limits.copy,
            &analyzed,
        )
        .expect("full context and reaction preserved despite empty planner observations");
    let mut uncertain = analyzed;
    uncertain
        .uncertainty
        .push("Die Fähigkeit ist nicht klar erkennbar".into());
    assert!(original
        .validate(
            frozen.media.duration_ms,
            &runtime.limits.plan,
            &runtime.limits.copy,
            &uncertain
        )
        .is_err());
    let mut reviewed = original;
    reviewed.plan.significant_uncertainty = true;
    reviewed.plan.review_reasons = uncertain.uncertainty.clone();
    assert!(reviewed
        .validate(
            frozen.media.duration_ms,
            &runtime.limits.plan,
            &runtime.limits.copy,
            &uncertain
        )
        .expect("uncertainty remains visible")
        .needs_review());
    lease.release().await.expect("release");
}

#[test]
fn packaged_runtime_is_strict_and_transient_stt_seed_is_valid() {
    let root = tempfile::tempdir().expect("package root");
    let runtime = runtime(root.path());
    runtime
        .limits
        .stt
        .seed()
        .expect("secret-free typed STT document");
    assert_ne!(runtime.limits.stt.settings.port, 8791);
    let mut config = runtime.configuration.clone();
    config.runtime["enabled"] = json!(true);
    assert!(EditorRuntime::from_configuration(root.path(), config).is_err());
    let mut config = runtime.configuration.clone();
    config.runtime["stt"]["settings"]["port"] = json!(8791);
    assert!(EditorRuntime::from_configuration(root.path(), config).is_err());
}

#[tokio::test]
async fn manual_resume_materializes_one_owned_version_and_preserves_frozen_layout_and_original() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let settings = autopilot_settings::load(&db.pool, "101")
        .await
        .expect("default C1");
    assert!(!settings.enabled && !settings.ai_editing_enabled);
    let job = super::super::create_manual_job(&db.pool, &runtime, "101", clip)
        .await
        .expect("manual create independent of autopilot");
    assert!(matches!(
        super::super::get_manual_job(&db.pool, &runtime, "202", job.id).await,
        Err(AiEditorError::NotFound)
    ));
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    lease.ensure_held().await.expect("actual advisory lock");
    let checkpoints = prepared(
        &db.pool,
        &runtime,
        &mut lease,
        clip,
        Identity::Manual(job.id),
    )
    .await;
    for checkpoint in checkpoints.values() {
        super::super::store::claim_job(
            &mut lease,
            "101",
            job.id,
            super::super::store::AttemptLimit::from_server_config(2).expect("attempt limit"),
        )
        .await
        .expect("durable claim");
        super::super::store::save_stage(&mut lease, "101", job.id, checkpoint, None)
            .await
            .expect("durable checkpoint");
    }
    let original = tokio::fs::read(runtime.limits.source_root.join("fixture-original.mp4"))
        .await
        .expect("source bytes");
    sqlx::query(
        "UPDATE twitch_clips_social_media SET layout_override_json='{}'::jsonb WHERE id=$1",
    )
    .bind(clip)
    .execute(&db.pool)
    .await
    .expect("later invalid layout must not replace frozen layout");
    let _ = execute_stage(
        &mut StageContext {
            pool: &db.pool,
            current: &runtime,
            lease: &mut lease,
            owner: "101",
            clip_id: clip,
            identity: Identity::Manual(job.id),
            expected_twitch_clip_id: Some("fixture-original"),
            checkpoints: &checkpoints,
            brain_token: None,
        },
        AiStage::Rendering,
    )
    .await
    .expect("render completed before checkpoint crash");
    let before = produced_video::find_ai_version(&db.pool, "101", clip, Some(job.id), None)
        .await
        .expect("allocated output")
        .expect("C2 row");
    let proxy_before = tokio::fs::read(original_preview_path(&runtime, before.id))
        .await
        .expect("durable proxy before checkpoint save");
    lease.release().await.expect("handover before worker");
    let worker = super::super::ManualWorker::new(db.pool.clone(), runtime.clone(), None);
    assert!(worker
        .process_next()
        .await
        .expect("actual sequential worker and FFmpeg path"));
    let view = super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
        .await
        .expect("authoritative public view");
    assert_eq!(view.status, "review");
    assert_eq!(view.versions.len(), 1);
    assert!(view.provider_latency_ms.is_none() && view.estimated_provider_cost_usd.is_none());
    assert!(view.rendering_ms.is_some_and(|time| time > 0));
    let video = produced_video::get(&db.pool, "101", view.versions[0].id)
        .await
        .expect("C2 read")
        .expect("version");
    assert!(video.job_id.is_none());
    assert_eq!(video.metadata["manual_ai_job_id"], job.id);
    assert!(Path::new(video.file_path.as_deref().expect("durable output")).is_file());
    assert_eq!(video.metadata["qa"]["decode"], "passed");
    assert_eq!(
        tokio::fs::read(runtime.limits.source_root.join("fixture-original.mp4"))
            .await
            .expect("unchanged original"),
        original
    );
    assert_eq!(
        std::fs::read_dir(&runtime.limits.work_root)
            .expect("workspaces")
            .count(),
        0
    );
    let public = serde_json::to_string(&view).expect("public projection");
    assert!(
        !public.contains("Originalsprache")
            && !public.contains("source_path")
            && !public.contains(directory.path().to_str().expect("path"))
    );
    assert!(!worker.process_next().await.expect("no duplicate output"));
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("released lease")
        .expect("free");
    lease
        .ensure_held()
        .await
        .expect("lock was released and acquired again");
    let (frozen, _) = preparing(&checkpoints, &runtime).expect("immutable snapshot");
    let preview_path = super::super::manual_original_file(&db.pool, &runtime, "101", job.id)
        .await
        .expect("owned uncut browser-safe original preview");
    assert_ne!(preview_path, frozen.source.path);
    assert_eq!(preview_path, original_preview_path(&runtime, video.id));
    assert_eq!(video.id, before.id);
    assert_eq!(
        tokio::fs::read(&preview_path)
            .await
            .expect("reconciled proxy"),
        proxy_before
    );
    tokio::fs::write(&preview_path, b"changed proxy")
        .await
        .expect("synthetic proxy corruption");
    assert!(
        super::super::manual_original_file(&db.pool, &runtime, "101", job.id)
            .await
            .is_err()
    );
    tokio::fs::write(&preview_path, &proxy_before)
        .await
        .expect("restore own fixture");
    let preview = check_original_preview(&runtime, &frozen, &video)
        .await
        .expect("frozen provenance and preview checksum");
    assert!(preview.rendering_ms > 0);
    assert_eq!(video.metadata["rendering_ms"].as_i64(), view.rendering_ms);
    assert_eq!(
        video.metadata["edited_rendering_ms"]
            .as_i64()
            .expect("actual edit time")
            + preview.rendering_ms,
        view.rendering_ms.expect("FFmpeg total")
    );
    let preview_info = super::super::media::validate_original_preview(
        &runtime.limits.output_root,
        &preview_path,
        &frozen.media,
        &runtime.limits.resources,
        &runtime.limits.original_preview,
    )
    .await
    .expect("real uncut H.264/AAC decode");
    assert_eq!((preview_info.width, preview_info.height), (960, 540));
    assert!(
        preview_info.duration_ms.abs_diff(frozen.media.duration_ms)
            <= runtime.limits.resources.max_duration_drift_ms
    );
    assert!(
        super::super::manual_original_file(&db.pool, &runtime, "202", job.id)
            .await
            .is_err()
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(video.file_path.as_deref().expect("durable output"))
            .expect("shared output permissions")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    tokio::fs::write(&frozen.source.path, b"changed source")
        .await
        .expect("source mutation");
    assert!(matches!(
        frozen.source.recheck(&db.pool, &runtime, "101", clip).await,
        Err(AiEditorError::Source)
    ));
    assert!(
        super::super::manual_original_file(&db.pool, &runtime, "101", job.id)
            .await
            .is_err()
    );
    sqlx::query(
        "UPDATE social_produced_videos SET expires_at='2000-01-01'::timestamptz WHERE id=$1",
    )
    .bind(video.id)
    .execute(&db.pool)
    .await
    .expect("expire own fixture");
    cleanup_expired_artifacts(&db.pool, &runtime, &mut lease)
        .await
        .expect("shared artifact lifecycle");
    assert!(!preview_path.exists());
    assert!(!Path::new(video.file_path.as_deref().expect("output")).exists());
    assert!(frozen.source.path.exists());
    assert_eq!(
        produced_video::get(&db.pool, "101", video.id)
            .await
            .expect("retained row")
            .expect("C2 history")
            .status,
        "expired"
    );
    lease.release().await.expect("release");
}

#[tokio::test]
async fn failed_original_preview_still_expires_the_partial_owned_output() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let base = runtime(directory.path());
    let mut configuration = base.configuration.clone();
    configuration.runtime["original_preview"]["max_output_bytes"] = json!(64);
    let runtime = EditorRuntime::from_configuration(directory.path(), configuration)
        .expect("bounded failing preview configuration");
    let clip = source(&db.pool, &runtime).await;
    let job = super::super::create_manual_job(&db.pool, &runtime, "101", clip)
        .await
        .expect("manual job");
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let checkpoints = prepared(
        &db.pool,
        &runtime,
        &mut lease,
        clip,
        Identity::Manual(job.id),
    )
    .await;
    let result = execute_stage(
        &mut StageContext {
            pool: &db.pool,
            current: &runtime,
            lease: &mut lease,
            owner: "101",
            clip_id: clip,
            identity: Identity::Manual(job.id),
            expected_twitch_clip_id: Some("fixture-original"),
            checkpoints: &checkpoints,
            brain_token: None,
        },
        AiStage::Rendering,
    )
    .await;
    assert!(matches!(result, Err(AiEditorError::Render)));
    let video = produced_video::find_ai_version(&db.pool, "101", clip, Some(job.id), None)
        .await
        .expect("allocated video")
        .expect("durable association");
    assert_eq!(video.status, "rendering");
    assert_eq!(
        video.expires_at,
        Some(video.created_at + chrono::Duration::days(7))
    );
    assert!(video.metadata.get("original_preview").is_none());
    let output = output_path(&runtime, video.id);
    assert!(output.is_file());
    assert!(!original_preview_path(&runtime, video.id).exists());
    sqlx::query(
        "UPDATE social_produced_videos SET expires_at='2000-01-01'::timestamptz WHERE id=$1",
    )
    .bind(video.id)
    .execute(&db.pool)
    .await
    .expect("expired fixture");
    let backup = output.with_extension("retention-retry.mp4");
    tokio::fs::rename(&output, &backup)
        .await
        .expect("retain own output for cleanup retry");
    tokio::fs::create_dir(&output)
        .await
        .expect("real removal failure fixture");
    assert!(matches!(
        cleanup_expired_artifacts(&db.pool, &runtime, &mut lease).await,
        Err(AiEditorError::Storage)
    ));
    let interrupted = produced_video::get(&db.pool, "101", video.id)
        .await
        .expect("interrupted cleanup")
        .expect("retained history");
    assert_eq!(interrupted.status, "expired");
    assert!(interrupted.metadata.get("ai_artifacts_expired").is_none());
    assert!(matches!(
        produced_video::approve(&db.pool, "101", video.id, &["youtube".into()]).await,
        Err(crate::produced_video::ProducedVideoError::State)
    ));
    tokio::fs::remove_dir(&output)
        .await
        .expect("remove own failure fixture");
    tokio::fs::rename(&backup, &output)
        .await
        .expect("restore own output for retry");
    cleanup_expired_artifacts(&db.pool, &runtime, &mut lease)
        .await
        .expect("partial artifact cleanup without a preview checkpoint");
    assert!(!output.exists());
    assert!(runtime
        .limits
        .source_root
        .join("fixture-original.mp4")
        .is_file());
    assert_eq!(
        produced_video::get(&db.pool, "101", video.id)
            .await
            .expect("retained history")
            .expect("video history")
            .status,
        "expired"
    );
    lease.release().await.expect("release");
}

#[tokio::test]
async fn static_full_video_and_manual_ai_share_the_lease_and_busy_previews_remain_pending() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    sqlx::raw_sql("ALTER TABLE twitch_clips_social_media ADD COLUMN IF NOT EXISTS preview_status TEXT, ADD COLUMN IF NOT EXISTS preview_path TEXT, ADD COLUMN IF NOT EXISTS preview_error TEXT, ADD COLUMN IF NOT EXISTS preview_updated_at TIMESTAMPTZ; ALTER TABLE twitch_clips_upload_queue ADD COLUMN IF NOT EXISTS tiktok_publish_id TEXT;")
        .execute(&db.pool).await.expect("existing full-video preview contract");
    let original = runtime.limits.source_root.join("fixture-original.mp4");
    let output = runtime.limits.output_root.join("static-fixture.mp4");
    let processor = crate::video_processor::VideoProcessor::default();
    let job = super::super::create_manual_job(&db.pool, &runtime, "101", clip)
        .await
        .expect("queued manual AI");
    let worker = super::super::ManualWorker::new(db.pool.clone(), runtime.clone(), None);
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    assert!(!worker
        .process_next()
        .await
        .expect("manual AI remains queued"));
    assert_eq!(
        super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
            .await
            .expect("manual view")
            .status,
        "queued"
    );
    assert!(matches!(
        crate::render::render_clip_vertical(
            &processor,
            &db.pool,
            clip,
            &original.to_string_lossy(),
            &output.to_string_lossy(),
            2
        )
        .await,
        Err(crate::video_processor::VideoProcessorError::ProductionBusy)
    ));
    assert!(!output.exists());
    let batch = crate::batch::render_all_for_user(
        &db.pool,
        &processor,
        std::sync::Arc::new(crate::clip_prep_worker::YtDlpDownloader::new("unused")),
        "101",
        &directory.path().join("batch").to_string_lossy(),
        &runtime.limits.source_root.to_string_lossy(),
        Some(&[clip]),
    )
    .await;
    assert!(batch.rendered.is_empty() && batch.failed.is_empty());
    assert_eq!(batch.deferred, vec!["fixture-original"]);
    crate::preview::request_preview(&db.pool, clip)
        .await
        .expect("queued full-video preview");
    let preview = crate::preview::PreviewWorker::new(
        db.pool.clone(),
        "unused",
        runtime.limits.source_root.to_string_lossy(),
    );
    preview.run_once().await;
    let status = crate::preview::get_preview(&db.pool, clip)
        .await
        .expect("preview state");
    assert_eq!(
        status.status.as_deref(),
        Some(crate::preview::PREVIEW_PENDING)
    );
    assert!(status.error.is_none() && status.path.is_none());
    crate::render::render_clip_vertical_with_lease(
        &mut lease,
        &processor,
        &db.pool,
        clip,
        &original.to_string_lossy(),
        &output.to_string_lossy(),
        2,
    )
    .await
    .expect("existing lease is reused without nested acquisition");
    lease.ensure_held().await.expect("caller retains lease");
    lease.release().await.expect("release");
    preview.run_once().await;
    let status = crate::preview::get_preview(&db.pool, clip)
        .await
        .expect("retry state");
    assert_eq!(
        status.status.as_deref(),
        Some(crate::preview::PREVIEW_READY)
    );
    assert!(status.path.is_some_and(|path| Path::new(&path).is_file()));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM llm_usage")
            .fetch_one(&db.pool)
            .await
            .expect("no AI calls"),
        0
    );
    assert!(produced_video::list(&db.pool, "101", 50)
        .await
        .expect("no extra C2 versions")
        .is_empty());
}

#[tokio::test]
async fn invalid_frozen_checkpoint_fails_visibly_without_starving_the_queue() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let job = super::super::create_manual_job(&db.pool, &runtime, "101", clip)
        .await
        .expect("manual job");
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    super::super::store::claim_job(
        &mut lease,
        "101",
        job.id,
        super::super::store::AttemptLimit::from_server_config(2).expect("limit"),
    )
    .await
    .expect("claim");
    super::super::store::save_stage(
        &mut lease,
        "101",
        job.id,
        &StageCheckpoint::Preparing {
            source_path: runtime
                .limits
                .source_root
                .join("fixture-original.mp4")
                .to_string_lossy()
                .into_owned(),
            original_duration_ms: Some(1600),
            snapshot: json!({"invalid": true})
                .as_object()
                .cloned()
                .expect("object"),
        },
        None,
    )
    .await
    .expect("bounded but incompatible checkpoint");
    lease.release().await.expect("release");
    let worker = super::super::ManualWorker::new(db.pool.clone(), runtime.clone(), None);
    assert!(worker.process_next().await.is_err());
    let view = super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
        .await
        .expect("visible failed job");
    assert_eq!(view.status, "failed");
    assert_eq!(view.failure_code.as_deref(), Some("invalid_checkpoint"));
    assert!(!view.can_retry && view.versions.is_empty());
    assert!(!worker
        .process_next()
        .await
        .expect("bad job cannot starve later work"));
}

#[tokio::test]
async fn c3_resume_reuses_the_pipeline_and_exact_durable_version_under_the_existing_lease() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    production_jobs::insert(
        &db.pool,
        &production_jobs::NewProductionJob {
            twitch_user_id: "101".into(),
            clip_id: clip,
            source: production_jobs::JobSource::Manual,
            requested_mode: Some(ProductionMode::Ai),
            scheduled_at: None,
            max_attempts: 2,
        },
    )
    .await
    .expect("C3 request");
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let mut job = production_jobs::claim_next(&db.pool)
        .await
        .expect("claim")
        .expect("job");
    let checkpoints = prepared(
        &db.pool,
        &runtime,
        &mut lease,
        clip,
        Identity::Production(job.id),
    )
    .await;
    let state = ProductionState {
        checkpoints,
        attempts: BTreeMap::new(),
    };
    job.metadata["ai_editor"] = serde_json::to_value(state).expect("saved stage state");
    production_jobs::update_progress(&db.pool, "101", job.id, "rendering", &job.metadata)
        .await
        .expect("durable C3 checkpoints");
    let outcome = produce(
        ProductionContext {
            pool: &db.pool,
            runtime: &runtime,
            lease: &mut lease,
            brain_token: None,
        },
        job.clone(),
    )
    .await
    .expect("real C3 rendering and validation");
    assert!(outcome.needs_review && !outcome.review_reasons.is_empty());
    assert_eq!(outcome.produced_video_ids.len(), 1);
    let video = produced_video::get(&db.pool, "101", outcome.produced_video_ids[0])
        .await
        .expect("owned read")
        .expect("C2 output");
    assert_eq!(video.job_id, Some(job.id));
    assert_eq!(video.status, "review");
    assert!(video.metadata.get("manual_ai_job_id").is_none());
    assert_eq!(video.metadata["production_ai_job_id"], job.id);
    assert_eq!(video.metadata["qa"]["decode"], "passed");
    lease
        .ensure_held()
        .await
        .expect("produce never releases the caller's lease");
    let preview_before = tokio::fs::read(original_preview_path(&runtime, video.id))
        .await
        .expect("durable original preview");
    let replay = produce(
        ProductionContext {
            pool: &db.pool,
            runtime: &runtime,
            lease: &mut lease,
            brain_token: None,
        },
        job,
    )
    .await
    .expect("checkpoint reconciliation");
    assert_eq!(replay.produced_video_ids, outcome.produced_video_ids);
    assert_eq!(
        tokio::fs::read(original_preview_path(&runtime, video.id))
            .await
            .expect("reused uncut original"),
        preview_before
    );
    assert_eq!(
        produced_video::list(&db.pool, "101", 50)
            .await
            .expect("versions")
            .len(),
        1
    );
    lease.release().await.expect("caller releases");
}

async fn expire_fixture(pool: &PgPool, video_id: i64) {
    sqlx::query(
        "UPDATE social_produced_videos SET expires_at='2000-01-01'::timestamptz WHERE id=$1",
    )
    .bind(video_id)
    .execute(pool)
    .await
    .expect("expire isolated allocation");
}

async fn usage_fixture(pool: &PgPool, identity: Identity) {
    sqlx::query("INSERT INTO llm_usage (purpose,provider,model,attempt_state,tokens_in,tokens_out,latency_ms,social_ai_twitch_user_id,social_ai_job_id,social_ai_stage,reserved_micro_usd,estimated_micro_usd,cost_basis) VALUES ('social_editor','anthropic','fixture-model','completed',11,7,23,'101',$1,'planning',20,10,'usage_priced_estimate')")
        .bind(identity.namespace()).execute(pool).await.expect("synthetic existing provider ledger");
}

async fn assert_usage_preserved(pool: &PgPool, identity: Identity) {
    let usage: (i64, i64, i64, i64, i64) = sqlx::query_as("SELECT COUNT(*),SUM(tokens_in)::bigint,SUM(tokens_out)::bigint,SUM(reserved_micro_usd)::bigint,SUM(estimated_micro_usd)::bigint FROM llm_usage WHERE social_ai_job_id=$1")
        .bind(identity.namespace()).fetch_one(pool).await.expect("unchanged provider accounting");
    assert_eq!(usage, (1, 11, 7, 20, 10));
}

#[tokio::test]
async fn expired_manual_media_retries_rebuild_without_replaying_provider_stages() {
    for resume in [AiStage::Rendering, AiStage::Validating, AiStage::Planning] {
        let db = database().await;
        let directory = tempfile::tempdir().expect("package root");
        let runtime = runtime(directory.path());
        let clip = source(&db.pool, &runtime).await;
        let job = super::super::create_manual_job(&db.pool, &runtime, "101", clip)
            .await
            .expect("manual job");
        let identity = Identity::Manual(job.id);
        usage_fixture(&db.pool, identity).await;
        let mut lease = super::super::store::acquire_production_lease(&db.pool)
            .await
            .expect("lease")
            .expect("free");
        let mut checkpoints = prepared(&db.pool, &runtime, &mut lease, clip, identity).await;
        let frozen = serde_json::to_value(&checkpoints).expect("frozen earlier checkpoints");
        let limit = super::super::store::AttemptLimit::from_server_config(2).expect("limit");
        for checkpoint in checkpoints.values() {
            super::super::store::claim_job(&mut lease, "101", job.id, limit)
                .await
                .expect("claim");
            super::super::store::save_stage(&mut lease, "101", job.id, checkpoint, None)
                .await
                .expect("save");
        }
        super::super::store::claim_job(&mut lease, "101", job.id, limit)
            .await
            .expect("render claim");
        let rendered = execute_stage(
            &mut StageContext {
                pool: &db.pool,
                current: &runtime,
                lease: &mut lease,
                owner: "101",
                clip_id: clip,
                identity,
                expected_twitch_clip_id: Some("fixture-original"),
                checkpoints: &checkpoints,
                brain_token: None,
            },
            AiStage::Rendering,
        )
        .await
        .expect("real original render before expiry");
        let old = produced_video::find_ai_version(&db.pool, "101", clip, Some(job.id), None)
            .await
            .expect("allocated video")
            .expect("owned version");
        let failed_stage = if resume == AiStage::Rendering {
            AiStage::Rendering
        } else {
            super::super::store::save_stage(&mut lease, "101", job.id, &rendered, None)
                .await
                .expect("render checkpoint");
            checkpoints.insert(AiStage::Rendering, rendered);
            super::super::store::claim_job(&mut lease, "101", job.id, limit)
                .await
                .expect("validation claim");
            if resume == AiStage::Planning {
                let validated = execute_stage(
                    &mut StageContext {
                        pool: &db.pool,
                        current: &runtime,
                        lease: &mut lease,
                        owner: "101",
                        clip_id: clip,
                        identity,
                        expected_twitch_clip_id: Some("fixture-original"),
                        checkpoints: &checkpoints,
                        brain_token: None,
                    },
                    AiStage::Validating,
                )
                .await
                .expect("validation before completion crash");
                super::super::store::save_stage(&mut lease, "101", job.id, &validated, None)
                    .await
                    .expect("completed media checkpoint");
            }
            AiStage::Validating
        };
        super::super::store::fail_job(
            &mut lease,
            "101",
            job.id,
            failed_stage,
            super::super::store::JobFailure::ValidationFailed,
            None,
        )
        .await
        .expect("retryable interrupted job");
        expire_fixture(&db.pool, old.id).await;
        let view = super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
            .await
            .expect("public retry view");
        assert!(view.can_retry);
        assert_eq!(view.versions[0].status, "expired");
        assert!(view.versions[0].file_url.is_none() && view.original_url.is_none());
        super::super::retry_manual_job(&db.pool, &runtime, "101", job.id)
            .await
            .expect("advertised retry accepted");
        lease.release().await.expect("worker handover");
        let worker = super::super::ManualWorker::new(db.pool.clone(), runtime.clone(), None);
        assert!(worker
            .process_next()
            .await
            .expect("retry rebuilds media on its remaining attempts"));
        let view = super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
            .await
            .expect("completed view");
        assert_eq!(view.status, "review");
        assert_eq!(view.versions.len(), 2);
        assert_eq!(view.versions[0].id, old.id);
        assert_eq!(view.versions[0].status, "expired");
        assert!(view.versions[0].file_url.is_none());
        assert_eq!(view.versions[1].status, "review");
        assert!(view.versions[1].file_url.is_some());
        assert!(
            !output_path(&runtime, old.id).exists()
                && !original_preview_path(&runtime, old.id).exists()
        );
        let fresh = produced_video::get(&db.pool, "101", view.versions[1].id)
            .await
            .expect("C2 read")
            .expect("fresh version");
        assert!(fresh.id != old.id && fresh.version_no > old.version_no);
        assert_eq!(fresh.metadata["qa"]["decode"], "passed");
        assert!(fresh
            .expires_at
            .is_some_and(|expiry| expiry > chrono::Utc::now()));
        assert!(
            super::super::manual_original_file(&db.pool, &runtime, "101", job.id)
                .await
                .expect("fresh authenticated original")
                .is_file()
        );
        assert!(
            super::super::manual_original_file(&db.pool, &runtime, "202", job.id)
                .await
                .is_err()
        );
        let persisted = super::super::store::checkpoints(&db.pool, "101", job.id)
            .await
            .expect("durable recovered checkpoints");
        let mut earlier = persisted.clone();
        earlier.remove(&AiStage::Rendering);
        earlier.remove(&AiStage::Validating);
        assert_eq!(
            serde_json::to_value(earlier).expect("earlier checkpoints"),
            frozen
        );
        assert!(
            matches!(persisted.get(&AiStage::Rendering),Some(StageCheckpoint::Rendering { output_path: path,.. }) if path == &output_path(&runtime,fresh.id).to_string_lossy())
        );
        let attempts: Value = sqlx::query_scalar("SELECT attempts FROM social_ai_jobs WHERE id=$1")
            .bind(job.id)
            .fetch_one(&db.pool)
            .await
            .expect("attempt accounting");
        assert_eq!(attempts["rendering"], 2);
        assert_eq!(
            attempts["validating"],
            if resume == AiStage::Rendering { 1 } else { 2 }
        );
        assert_usage_preserved(&db.pool, identity).await;
        assert!(!worker.process_next().await.expect("no duplicate work"));
    }
}

#[tokio::test]
async fn expired_production_media_resumes_rebuild_and_reconcile_completed_checkpoints() {
    for resume in [AiStage::Rendering, AiStage::Validating, AiStage::Planning] {
        let db = database().await;
        let directory = tempfile::tempdir().expect("package root");
        let runtime = runtime(directory.path());
        let clip = source(&db.pool, &runtime).await;
        production_jobs::insert(
            &db.pool,
            &production_jobs::NewProductionJob {
                twitch_user_id: "101".into(),
                clip_id: clip,
                source: production_jobs::JobSource::Manual,
                requested_mode: Some(ProductionMode::Ai),
                scheduled_at: None,
                max_attempts: 2,
            },
        )
        .await
        .expect("C3 request");
        let mut lease = super::super::store::acquire_production_lease(&db.pool)
            .await
            .expect("lease")
            .expect("free");
        let mut job = production_jobs::claim_next(&db.pool)
            .await
            .expect("claim")
            .expect("job");
        let identity = Identity::Production(job.id);
        usage_fixture(&db.pool, identity).await;
        let mut checkpoints = prepared(&db.pool, &runtime, &mut lease, clip, identity).await;
        let frozen = serde_json::to_value(&checkpoints).expect("earlier checkpoints");
        let rendered = execute_stage(
            &mut StageContext {
                pool: &db.pool,
                current: &runtime,
                lease: &mut lease,
                owner: "101",
                clip_id: clip,
                identity,
                expected_twitch_clip_id: None,
                checkpoints: &checkpoints,
                brain_token: None,
            },
            AiStage::Rendering,
        )
        .await
        .expect("render before interruption");
        let old = produced_video::find_ai_version(&db.pool, "101", clip, None, Some(job.id))
            .await
            .expect("C2 read")
            .expect("old version");
        let mut attempts = BTreeMap::from([(AiStage::Rendering, 1)]);
        if resume != AiStage::Rendering {
            checkpoints.insert(AiStage::Rendering, rendered);
            attempts.insert(AiStage::Validating, 1);
            if resume == AiStage::Planning {
                let validated = execute_stage(
                    &mut StageContext {
                        pool: &db.pool,
                        current: &runtime,
                        lease: &mut lease,
                        owner: "101",
                        clip_id: clip,
                        identity,
                        expected_twitch_clip_id: None,
                        checkpoints: &checkpoints,
                        brain_token: None,
                    },
                    AiStage::Validating,
                )
                .await
                .expect("completed validation before interruption");
                checkpoints.insert(AiStage::Validating, validated);
            }
        }
        job.metadata["ai_editor"] = serde_json::to_value(ProductionState {
            checkpoints,
            attempts,
        })
        .expect("state");
        production_jobs::update_progress(&db.pool, "101", job.id, "validating", &job.metadata)
            .await
            .expect("durable state");
        expire_fixture(&db.pool, old.id).await;
        assert!(produced_outcome(&db.pool, "101", clip, identity)
            .await
            .is_err());
        let outcome = produce(
            ProductionContext {
                pool: &db.pool,
                runtime: &runtime,
                lease: &mut lease,
                brain_token: None,
            },
            job.clone(),
        )
        .await
        .expect("expired C3 checkpoints recover on remaining attempts");
        let fresh_id = outcome.produced_video_ids[0];
        assert_ne!(fresh_id, old.id);
        assert!(outcome.needs_review);
        assert!(
            !output_path(&runtime, old.id).exists()
                && !original_preview_path(&runtime, old.id).exists()
        );
        let old = produced_video::get(&db.pool, "101", old.id)
            .await
            .expect("history")
            .expect("retained version");
        assert_eq!(old.status, "expired");
        let fresh = produced_video::get(&db.pool, "101", fresh_id)
            .await
            .expect("fresh video")
            .expect("version");
        assert_eq!(fresh.job_id, Some(job.id));
        assert_eq!(fresh.metadata["qa"]["decode"], "passed");
        let persisted = production_jobs::get(&db.pool, "101", job.id)
            .await
            .expect("C3 read")
            .expect("job");
        let state: ProductionState =
            serde_json::from_value(persisted.metadata["ai_editor"].clone())
                .expect("recovered durable state");
        assert_eq!(state.attempts[&AiStage::Rendering], 2);
        assert_eq!(
            state.attempts[&AiStage::Validating],
            if resume == AiStage::Rendering { 1 } else { 2 }
        );
        let mut earlier = state.checkpoints;
        earlier.remove(&AiStage::Rendering);
        earlier.remove(&AiStage::Validating);
        assert_eq!(
            serde_json::to_value(earlier).expect("preserved earlier checkpoints"),
            frozen
        );
        assert_usage_preserved(&db.pool, identity).await;
        let replay = produce(
            ProductionContext {
                pool: &db.pool,
                runtime: &runtime,
                lease: &mut lease,
                brain_token: None,
            },
            job.clone(),
        )
        .await
        .expect("fresh checkpoint crash reconciliation");
        assert_eq!(replay.produced_video_ids, outcome.produced_video_ids);
        produced_video::approve(&db.pool, "101", fresh_id, &["youtube".into()])
            .await
            .expect("synthetic owner approval");
        for status in ["approved", "scheduled", "published"] {
            sqlx::query("UPDATE social_produced_videos SET status=$2,scheduled_at=NOW()+INTERVAL '1 day',expires_at='2000-01-01'::timestamptz WHERE id=$1")
                .bind(fresh_id).bind(status).execute(&db.pool).await.expect("isolated completed C2 state");
            let replay = produce(
                ProductionContext {
                    pool: &db.pool,
                    runtime: &runtime,
                    lease: &mut lease,
                    brain_token: None,
                },
                job.clone(),
            )
            .await
            .expect("completed production checkpoint accepts retained approved media");
            assert_eq!(replay.produced_video_ids, outcome.produced_video_ids);
            assert!(!replay.needs_review);
        }
        assert_eq!(
            produced_video::list(&db.pool, "101", 50)
                .await
                .expect("versions")
                .len(),
            2
        );
        tokio::fs::remove_file(output_path(&runtime, fresh_id))
            .await
            .expect("isolated missing media");
        assert!(produced_outcome(&db.pool, "101", clip, identity)
            .await
            .is_err());
        lease.ensure_held().await.expect("caller retains lease");
        lease.release().await.expect("release");
    }
}

#[tokio::test]
async fn expiry_after_queueing_does_not_spend_validation_attempts_when_rendering_is_exhausted() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let job = super::super::create_manual_job(&db.pool, &runtime, "101", clip)
        .await
        .expect("job");
    let identity = Identity::Manual(job.id);
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let mut checkpoints = prepared(&db.pool, &runtime, &mut lease, clip, identity).await;
    let rendered = execute_stage(
        &mut StageContext {
            pool: &db.pool,
            current: &runtime,
            lease: &mut lease,
            owner: "101",
            clip_id: clip,
            identity,
            expected_twitch_clip_id: None,
            checkpoints: &checkpoints,
            brain_token: None,
        },
        AiStage::Rendering,
    )
    .await
    .expect("real media");
    checkpoints.insert(AiStage::Rendering, rendered);
    sqlx::query("UPDATE social_ai_jobs SET stage='validating',status='failed',checkpoints=$2,attempts='{\"rendering\":2,\"validating\":1}'::jsonb WHERE id=$1")
        .bind(job.id).bind(serde_json::to_value(checkpoints).expect("state")).execute(&db.pool).await.expect("bounded failed validation fixture");
    let video = produced_video::find_ai_version(&db.pool, "101", clip, Some(job.id), None)
        .await
        .expect("video")
        .expect("version");
    assert!(
        super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
            .await
            .expect("unexpired retry")
            .can_retry
    );
    super::super::retry_manual_job(&db.pool, &runtime, "101", job.id)
        .await
        .expect("queue before expiry");
    expire_fixture(&db.pool, video.id).await;
    lease.release().await.expect("handover");
    let worker = super::super::ManualWorker::new(db.pool.clone(), runtime.clone(), None);
    assert!(matches!(
        worker.process_next().await,
        Err(AiEditorError::Attempts)
    ));
    let view = super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
        .await
        .expect("terminal bounded failure");
    assert_eq!(view.status, "failed");
    assert!(!view.can_retry);
    assert!(
        super::super::retry_manual_job(&db.pool, &runtime, "101", job.id)
            .await
            .is_err()
    );
    let attempts: Value = sqlx::query_scalar("SELECT attempts FROM social_ai_jobs WHERE id=$1")
        .bind(job.id)
        .fetch_one(&db.pool)
        .await
        .expect("unchanged attempts");
    assert_eq!(attempts, json!({"rendering":2,"validating":1}));
    assert!(!worker.process_next().await.expect("no queue starvation"));
    assert_eq!(
        produced_video::list(&db.pool, "101", 50)
            .await
            .expect("no replacement beyond limit")
            .len(),
        1
    );
}

async fn revision_fixture(
    pool: &PgPool,
    runtime: &EditorRuntime,
    clip: i64,
) -> (i64, ProducedVideo, BTreeMap<AiStage, StageCheckpoint>) {
    let job = super::super::create_manual_job(pool, runtime, "101", clip)
        .await
        .expect("manual root");
    let mut lease = super::super::store::acquire_production_lease(pool)
        .await
        .expect("lease")
        .expect("free");
    let checkpoints = prepared(pool, runtime, &mut lease, clip, Identity::Manual(job.id)).await;
    let limit = super::super::store::AttemptLimit::from_server_config(2).expect("limit");
    for checkpoint in checkpoints.values() {
        super::super::store::claim_job(&mut lease, "101", job.id, limit)
            .await
            .expect("claim");
        super::super::store::save_stage(&mut lease, "101", job.id, checkpoint, None)
            .await
            .expect("checkpoint");
    }
    lease.release().await.expect("release");
    super::super::ManualWorker::new(pool.clone(), runtime.clone(), None)
        .process_next()
        .await
        .expect("real render and validation");
    let video = produced_video::find_ai_version(pool, "101", clip, Some(job.id), None)
        .await
        .expect("C2")
        .expect("version");
    assert_eq!(video.status, "review");
    (job.id, video, checkpoints)
}

fn proposed_plan(
    checkpoints: &BTreeMap<AiStage, StageCheckpoint>,
    reanalyze: bool,
) -> StageCheckpoint {
    let mut checkpoint = checkpoints
        .get(&AiStage::Planning)
        .expect("parent plan")
        .clone();
    if let StageCheckpoint::Planning { edit_plan, .. } = &mut checkpoint {
        edit_plan.insert("revision_proposal".into(),json!({"summary":"Einstieg prüfen","impact":"Der Zusammenhang bleibt erhalten.","reanalyze":reanalyze,"requires_confirmation":true,"estimated_provider_cost_usd":null}));
    }
    checkpoint
}

async fn save_revision_plan(pool: &PgPool, job: i64, checkpoint: &StageCheckpoint) {
    let mut lease = super::super::store::acquire_production_lease(pool)
        .await
        .expect("lease")
        .expect("free");
    super::super::store::claim_job(
        &mut lease,
        "101",
        job,
        super::super::store::AttemptLimit::from_server_config(2).expect("limit"),
    )
    .await
    .expect("planning claim");
    super::super::store::save_stage(&mut lease, "101", job, checkpoint, None)
        .await
        .expect("atomic confirmation pause");
    lease.release().await.expect("release");
}

#[tokio::test]
async fn revisions_preserve_parent_reuse_checkpoints_and_confirm_before_rendering() {
    use super::super::revision;
    let db = database().await;
    let directory = tempfile::tempdir().expect("root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let (root, parent, parent_checkpoints) = revision_fixture(&db.pool, &runtime, clip).await;
    let parent_path = parent.file_path.as_deref().expect("file");
    let parent_bytes = tokio::fs::read(parent_path).await.expect("parent bytes");
    assert!(matches!(
        revision::owned_version(&db.pool, "202", root, parent.id).await,
        Err(AiEditorError::NotFound)
    ));
    assert!(matches!(
        revision::create_revision(&db.pool, &runtime, "101", root, parent.id, " ").await,
        Err(AiEditorError::Input)
    ));
    let child = revision::create_revision(
        &db.pool,
        &runtime,
        "101",
        root,
        parent.id,
        "Kürze den Einstieg",
    )
    .await
    .expect("revision");
    assert_eq!(
        (child.root_job_id, child.parent_version_id),
        (root, Some(parent.id))
    );
    assert_eq!(child.stage, "planning");
    let reused = super::super::store::checkpoints(&db.pool, "101", child.id)
        .await
        .expect("child checkpoints");
    assert_eq!(reused.len(), 3);
    for stage in [AiStage::Transcribing, AiStage::Analyzing] {
        assert_eq!(
            serde_json::to_value(&reused[&stage]).expect("child"),
            serde_json::to_value(&parent_checkpoints[&stage]).expect("parent")
        );
    }
    let attempts: serde_json::Value =
        sqlx::query_scalar("SELECT attempts FROM social_ai_jobs WHERE id=$1")
            .bind(child.id)
            .fetch_one(&db.pool)
            .await
            .expect("no copied attempts");
    assert_eq!(attempts, json!({}));
    assert!(child.input_tokens.is_none() && child.estimated_provider_cost_usd.is_none());
    let skills = frozen_skills(&reused, &runtime).expect("frozen skills");
    assert_eq!(skills.len(), 8);
    assert!(skills.iter().all(|skill| !skill.version.is_empty()
        && skill.sha256.len() == 64
        && !skill.text.is_empty()));
    assert!(matches!(
        revision::create_revision(&db.pool, &runtime, "101", root, parent.id, "Noch kürzer").await,
        Err(AiEditorError::Conflict)
    ));
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    assert!(matches!(
        execute_stage(
            &mut StageContext {
                pool: &db.pool,
                current: &runtime,
                lease: &mut lease,
                owner: "101",
                clip_id: clip,
                identity: Identity::Manual(child.id),
                expected_twitch_clip_id: Some("fixture-original"),
                checkpoints: &reused,
                brain_token: None
            },
            AiStage::Analyzing
        )
        .await,
        Err(AiEditorError::Conflict)
    ));
    lease.release().await.expect("release");
    save_revision_plan(
        &db.pool,
        child.id,
        &proposed_plan(&parent_checkpoints, false),
    )
    .await;
    let awaiting = super::super::get_manual_job(&db.pool, &runtime, "101", child.id)
        .await
        .expect("awaiting");
    assert_eq!(awaiting.status, "awaiting_confirmation");
    let worker = super::super::ManualWorker::new(db.pool.clone(), runtime.clone(), None);
    assert!(!worker.process_next().await.expect("paused worker"));
    assert!(
        produced_video::find_ai_version(&db.pool, "101", clip, Some(child.id), None)
            .await
            .expect("C2")
            .is_none()
    );
    revision::decide_proposal(&db.pool, &runtime, "101", child.id, true)
        .await
        .expect("explicit confirmation");
    assert!(worker
        .process_next()
        .await
        .expect("confirmed real FFmpeg path"));
    let completed = super::super::get_manual_job(&db.pool, &runtime, "101", child.id)
        .await
        .expect("completed revision");
    assert_eq!(completed.status, "review");
    assert_eq!(completed.versions.len(), 2);
    let new = produced_video::find_ai_version(&db.pool, "101", clip, Some(child.id), None)
        .await
        .expect("C2")
        .expect("child output");
    assert_eq!(new.parent_version_id, Some(parent.id));
    assert_eq!(new.metadata["original_preview"]["rendering_ms"], 0);
    assert_eq!(
        tokio::fs::read(parent_path).await.expect("preserved"),
        parent_bytes
    );
    assert_eq!(
        tokio::fs::read(original_preview_path(&runtime, parent.id))
            .await
            .expect("parent preview"),
        tokio::fs::read(original_preview_path(&runtime, new.id))
            .await
            .expect("reused preview")
    );
    revision::record_version_decision(&db.pool, "101", child.id, new.id, true)
        .await
        .expect("accept");
    revision::record_version_decision(&db.pool, "101", root, new.id, true)
        .await
        .expect("deduplicated accept");
    revision::record_version_decision(&db.pool, "101", root, new.id, false)
        .await
        .expect("reject");
    revision::record_version_decision(&db.pool, "101", root, new.id, true)
        .await
        .expect("accept again");
    revision::select_version(&db.pool, &runtime, "101", root, parent.id)
        .await
        .expect("preferred parent");
    assert_eq!(
        super::super::get_manual_job(&db.pool, &runtime, "101", child.id)
            .await
            .expect("root preference")
            .selected_version_id,
        Some(parent.id)
    );
    assert!(matches!(
        revision::publish_version(&db.pool, "101", child.id, new.id, &["youtube".into()]).await,
        Err(AiEditorError::Publishing(
            crate::produced_video::ProducedVideoError::ApprovalRequired
        ))
    ));
    revision::approve_version(&db.pool, "101", root, new.id, &["youtube".into()])
        .await
        .expect("explicit platform");
    assert!(matches!(
        revision::publish_version(&db.pool, "101", child.id, new.id, &["tiktok".into()]).await,
        Err(AiEditorError::Publishing(
            crate::produced_video::ProducedVideoError::ApprovalRequired
        ))
    ));
    let mut settings = autopilot_settings::load(&db.pool, "101")
        .await
        .expect("settings");
    settings.schedule_slots = (1..=7)
        .map(|weekday| autopilot_settings::ScheduleSlot {
            weekday,
            local_time: "18:00".into(),
        })
        .collect();
    autopilot_settings::save(&db.pool, "101", &settings)
        .await
        .expect("schedule slots");
    let exact_bytes = tokio::fs::read(new.file_path.as_deref().expect("exact file"))
        .await
        .expect("before queue");
    let reservation = revision::publish_version(&db.pool, "101", root, new.id, &["youtube".into()])
        .await
        .expect("existing exact-file queue");
    let (id, status): (i64, String) = sqlx::query_as(
        "SELECT produced_video_id,status FROM twitch_clips_upload_queue WHERE id::bigint=$1",
    )
    .bind(reservation.queue_ids[0].1)
    .fetch_one(&db.pool)
    .await
    .expect("queue");
    assert_eq!((id, status), (new.id, "waiting_exact_file".into()));
    assert_eq!(
        tokio::fs::read(new.file_path.as_deref().expect("exact file"))
            .await
            .expect("queued bytes"),
        exact_bytes
    );
    let decisions: Vec<String> = sqlx::query_scalar("SELECT event_kind FROM social_ai_revision_events WHERE produced_video_id=$1 AND event_kind IN ('version_accepted','version_rejected') ORDER BY id").bind(new.id).fetch_all(&db.pool).await.expect("events");
    assert_eq!(
        decisions,
        vec!["version_accepted", "version_rejected", "version_accepted"]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM llm_usage")
            .fetch_one(&db.pool)
            .await
            .expect("no paid calls"),
        0
    );
    sqlx::query("UPDATE social_produced_videos SET expires_at='2000-01-01' WHERE id=$1")
        .bind(new.id)
        .execute(&db.pool)
        .await
        .expect("retention fixture");
    assert!(revision::usable(
        &produced_video::get(&db.pool, "101", new.id)
            .await
            .expect("C2")
            .expect("scheduled")
    ));
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    cleanup_expired_artifacts(&db.pool, &runtime, &mut lease)
        .await
        .expect("seven-day cleanup");
    assert!(Path::new(new.file_path.as_deref().expect("scheduled exact file")).is_file());
    assert!(!original_preview_path(&runtime, new.id).exists());
    lease.release().await.expect("release");
    sqlx::query("UPDATE social_produced_videos SET status='published' WHERE id=$1")
        .bind(new.id)
        .execute(&db.pool)
        .await
        .expect("isolated published version");
    let published = revision::select_version(&db.pool, &runtime, "101", root, new.id)
        .await
        .expect("published history remains selectable");
    assert_eq!(published.selected_version_id, Some(new.id));
    assert!(published.versions.iter().any(|version| version.id == new.id
        && version.status == "published"
        && version.file_url.is_some()
        && version.original_url.is_none()));
}

#[tokio::test]
async fn reanalysis_and_final_plan_have_separate_confirmation_and_failed_child_keeps_parent() {
    use super::super::{revision, store};
    let db = database().await;
    let directory = tempfile::tempdir().expect("root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let (root, parent, checkpoints) = revision_fixture(&db.pool, &runtime, clip).await;
    let child = revision::create_revision(
        &db.pool,
        &runtime,
        "101",
        root,
        parent.id,
        "Prüfe die Spielsituation neu",
    )
    .await
    .expect("revision");
    save_revision_plan(&db.pool, child.id, &proposed_plan(&checkpoints, true)).await;
    let confirmed = revision::decide_proposal(&db.pool, &runtime, "101", child.id, true)
        .await
        .expect("reanalysis permission");
    assert_eq!(
        (confirmed.status.as_str(), confirmed.stage.as_str()),
        ("queued", "analyzing")
    );
    let reused = store::checkpoints(&db.pool, "101", child.id)
        .await
        .expect("reanalysis invalidation");
    assert_eq!(reused.len(), 2);
    assert!(!reused.contains_key(&AiStage::Analyzing) && !reused.contains_key(&AiStage::Planning));
    let request = revision::request(&db.pool, "101", child.id)
        .await
        .expect("request")
        .expect("revision");
    assert!(request.reanalysis_confirmed && !request.confirmed);
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let limit = store::AttemptLimit::from_server_config(2).expect("limit");
    store::claim_job(&mut lease, "101", child.id, limit)
        .await
        .expect("analysis claim");
    store::save_stage(
        &mut lease,
        "101",
        child.id,
        &checkpoints[&AiStage::Analyzing],
        None,
    )
    .await
    .expect("synthetic analysis checkpoint without provider");
    store::claim_job(&mut lease, "101", child.id, limit)
        .await
        .expect("planning");
    store::save_stage(
        &mut lease,
        "101",
        child.id,
        &proposed_plan(&checkpoints, false),
        None,
    )
    .await
    .expect("new final proposal");
    lease.release().await.expect("release");
    assert_eq!(
        super::super::get_manual_job(&db.pool, &runtime, "101", child.id)
            .await
            .expect("second pause")
            .status,
        "awaiting_confirmation"
    );
    let rejected = revision::decide_proposal(&db.pool, &runtime, "101", child.id, false)
        .await
        .expect("reject final plan");
    assert_eq!(rejected.status, "rejected");
    assert!(matches!(
        revision::decide_proposal(&db.pool, &runtime, "101", child.id, true).await,
        Err(AiEditorError::Conflict)
    ));
    let failure = revision::create_revision(
        &db.pool,
        &runtime,
        "101",
        root,
        parent.id,
        "Kleine Änderung",
    )
    .await
    .expect("another revision");
    save_revision_plan(&db.pool, failure.id, &proposed_plan(&checkpoints, false)).await;
    revision::decide_proposal(&db.pool, &runtime, "101", failure.id, true)
        .await
        .expect("confirm");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    store::claim_job(&mut lease, "101", failure.id, limit)
        .await
        .expect("rendering claim");
    store::fail_job(
        &mut lease,
        "101",
        failure.id,
        AiStage::Rendering,
        store::JobFailure::RenderingFailed,
        None,
    )
    .await
    .expect("failed revision");
    lease.release().await.expect("release");
    let failed = super::super::get_manual_job(&db.pool, &runtime, "101", failure.id)
        .await
        .expect("failed family view");
    assert_eq!(failed.status, "failed");
    assert!(failed
        .versions
        .iter()
        .any(|version| version.id == parent.id && version.file_url.is_some()));
    assert!(Path::new(parent.file_path.as_deref().expect("parent output")).is_file());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM llm_usage")
            .fetch_one(&db.pool)
            .await
            .expect("no paid calls"),
        0
    );
}

#[tokio::test]
async fn c3_consent_failure_stays_review_without_static_fallback_or_manual_identity() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    super::super::store::upsert_user_consent(&db.pool, "101", false, "fixture-owner")
        .await
        .expect("revoked consent");
    production_jobs::insert(
        &db.pool,
        &production_jobs::NewProductionJob {
            twitch_user_id: "101".into(),
            clip_id: clip,
            source: production_jobs::JobSource::Manual,
            requested_mode: Some(ProductionMode::Ai),
            scheduled_at: None,
            max_attempts: 2,
        },
    )
    .await
    .expect("real C3 request");
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let job = production_jobs::claim_next(&db.pool)
        .await
        .expect("actual C3 claim")
        .expect("job");
    let id = job.id;
    assert!(matches!(
        produce(
            ProductionContext {
                pool: &db.pool,
                runtime: &runtime,
                lease: &mut lease,
                brain_token: None
            },
            job
        )
        .await,
        Err(AiEditorError::Consent)
    ));
    let job = production_jobs::get(&db.pool, "101", id)
        .await
        .expect("C3 view")
        .expect("job");
    assert_eq!(job.status, "review");
    assert_eq!(job.mode, Some(ProductionMode::Ai));
    assert!(produced_video::list(&db.pool, "101", 50)
        .await
        .expect("versions")
        .is_empty());
    assert_ne!(
        Identity::Manual(id).namespace(),
        Identity::Production(id).namespace()
    );
    lease.release().await.expect("release");
}

#[tokio::test]
async fn variants_require_frozen_opt_in_and_preserve_concrete_parent_plan_and_subtitles() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let mut settings = autopilot_settings::load(&db.pool, "101")
        .await
        .expect("settings");
    assert!(!settings.multi_output_enabled);
    settings.multi_output_enabled = true;
    autopilot_settings::save(&db.pool, "101", &settings)
        .await
        .expect("explicit isolated opt-in");
    let job = super::super::create_manual_job(&db.pool, &runtime, "101", clip)
        .await
        .expect("job");
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let mut checkpoints = prepared(
        &db.pool,
        &runtime,
        &mut lease,
        clip,
        Identity::Manual(job.id),
    )
    .await;
    let (frozen, _) = preparing(&checkpoints, &runtime).expect("snapshot");
    let (base, validated) = planning(&checkpoints, &frozen, &runtime).expect("base plan");
    let mapped = subtitles(&checkpoints, &validated, &runtime).expect("real speech cues");
    let ordinary =
        super::super::validation::build_scaled_subtitle_ass(&validated, &mapped, "", "", 0, 100)
            .expect("ordinary ASS");
    let larger =
        super::super::validation::build_scaled_subtitle_ass(&validated, &mapped, "", "", 0, 140)
            .expect("scaled ASS");
    let style_size = |ass: &str| {
        ass.lines()
            .find(|line| line.starts_with("Style: Default,"))
            .expect("speech style")
            .split(',')
            .nth(2)
            .expect("size")
            .parse::<u16>()
            .expect("numeric size")
    };
    assert!(style_size(&larger) > style_size(&ordinary));
    assert_eq!(
        ordinary
            .lines()
            .filter(|line| line.starts_with("Dialogue:"))
            .collect::<Vec<_>>(),
        larger
            .lines()
            .filter(|line| line.starts_with("Dialogue:"))
            .collect::<Vec<_>>()
    );
    assert_eq!(mapped[0].language.as_deref(), Some("de"));
    assert!(super::super::validation::build_scaled_subtitle_ass(
        &validated, &mapped, "", "", 0, 141
    )
    .is_err());
    let mut distinct = base.plan.clone();
    distinct.kept_intervals[0].start_ms = 100;
    let mut plan = base.clone();
    plan.subtitle_scale_percent = 140;
    plan.variants
        .push(super::super::stage_contract::VariantPlan {
            plan: distinct,
            platforms: base.platforms.clone(),
            subtitle_scale_percent: 100,
        });
    assert!(validate_variants(&plan, &json!({"multi_output_enabled":false}), &runtime).is_err());
    validate_variants(&plan, &frozen.publication, &runtime).expect("opt-in");
    plan.validate(
        frozen.media.duration_ms,
        &runtime.limits.plan,
        &runtime.limits.copy,
        &analysis(&checkpoints).expect("persisted analysis"),
    )
    .expect("bounded variant");
    let mut duplicate = plan.clone();
    duplicate.variants[0].plan = base.plan.clone();
    assert!(validate_variants(&duplicate, &frozen.publication, &runtime).is_err());
    let mut proposal = super::super::revision::RevisionProposal {
        summary: "Kleine Änderung".into(),
        impact: "Der Inhalt bleibt erhalten".into(),
        reanalyze: false,
        requires_confirmation: false,
        estimated_provider_cost_usd: Some(0.01),
    };
    super::super::revision::validate_proposal(&mut proposal, &base, &base, &runtime, Some(0.5))
        .expect("small revision");
    assert!(!proposal.requires_confirmation && proposal.estimated_provider_cost_usd == Some(0.0));
    let mut changed = base.clone();
    changed.plan.kept_intervals[0].start_ms = 500;
    super::super::revision::validate_proposal(&mut proposal, &base, &changed, &runtime, None)
        .expect("server classification");
    assert!(proposal.requires_confirmation);
    proposal.reanalyze = true;
    super::super::revision::validate_proposal(&mut proposal, &base, &changed, &runtime, Some(0.04))
        .expect("reanalysis estimate from the measured root job");
    assert_eq!(proposal.estimated_provider_cost_usd, Some(0.04));
    super::super::revision::validate_proposal(&mut proposal, &base, &changed, &runtime, None)
        .expect("unknown reanalysis estimate");
    assert!(proposal.estimated_provider_cost_usd.is_none());
    if let Some(StageCheckpoint::Planning { edit_plan, .. }) =
        checkpoints.get_mut(&AiStage::Planning)
    {
        *edit_plan = serde_json::to_value(&plan)
            .expect("plan")
            .as_object()
            .cloned()
            .expect("object");
    }
    let limit = super::super::store::AttemptLimit::from_server_config(2).expect("limit");
    for checkpoint in checkpoints.values() {
        super::super::store::claim_job(&mut lease, "101", job.id, limit)
            .await
            .expect("claim");
        super::super::store::save_stage(&mut lease, "101", job.id, checkpoint, None)
            .await
            .expect("checkpoint");
    }
    lease.release().await.expect("release");
    super::super::ManualWorker::new(db.pool.clone(), runtime.clone(), None)
        .process_next()
        .await
        .expect("actual multiple FFmpeg outputs");
    let view = super::super::get_manual_job(&db.pool, &runtime, "101", job.id)
        .await
        .expect("variants view");
    assert_eq!(view.versions.len(), 2);
    assert_eq!(view.versions[0].version_no, view.versions[1].version_no);
    assert_eq!(
        (
            view.versions[0].variant_index,
            view.versions[1].variant_index
        ),
        (0, 1)
    );
    assert_ne!(view.versions[0].id, view.versions[1].id);
    assert!(view
        .versions
        .iter()
        .all(|version| version.file_url.is_some() && version.approved_platforms.is_empty()));
    let selected = super::super::revision::parent_planning(&db.pool, "101", view.versions[1].id)
        .await
        .expect("concrete variant plan");
    assert_eq!(selected.plan.kept_intervals[0].start_ms, 100);
    let child = super::super::revision::create_revision(
        &db.pool,
        &runtime,
        "101",
        job.id,
        view.versions[1].id,
        "Behalte diese Variante",
    )
    .await
    .expect("variant revision");
    assert_eq!(child.parent_version_id, Some(view.versions[1].id));
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM social_ai_revision_events WHERE job_id=$1 AND event_kind='feedback'",
    )
    .bind(child.id)
    .fetch_one(&db.pool)
    .await
    .expect("normalized event");
    assert_eq!(
        payload["previous"]["plan"]["kept_intervals"][0]["start_ms"],
        100
    );
    assert_eq!(payload["parent_version_id"], view.versions[1].id);
}

#[tokio::test]
async fn secondary_variant_expiry_recovers_without_replacing_retained_primary_or_provider_work() {
    let db = database().await;
    let directory = tempfile::tempdir().expect("package root");
    let runtime = runtime(directory.path());
    let clip = source(&db.pool, &runtime).await;
    let mut settings = autopilot_settings::load(&db.pool, "101")
        .await
        .expect("settings");
    settings.multi_output_enabled = true;
    autopilot_settings::save(&db.pool, "101", &settings)
        .await
        .expect("isolated opt-in");
    production_jobs::insert(
        &db.pool,
        &production_jobs::NewProductionJob {
            twitch_user_id: "101".into(),
            clip_id: clip,
            source: production_jobs::JobSource::Manual,
            requested_mode: Some(ProductionMode::Ai),
            scheduled_at: None,
            max_attempts: 2,
        },
    )
    .await
    .expect("C3 request");
    let mut job = production_jobs::claim_next(&db.pool)
        .await
        .expect("claim")
        .expect("job");
    let identity = Identity::Production(job.id);
    usage_fixture(&db.pool, identity).await;
    let mut lease = super::super::store::acquire_production_lease(&db.pool)
        .await
        .expect("lease")
        .expect("free");
    let mut checkpoints = prepared(&db.pool, &runtime, &mut lease, clip, identity).await;
    let (frozen, _) = preparing(&checkpoints, &runtime).expect("snapshot");
    let (mut plan, _) = planning(&checkpoints, &frozen, &runtime).expect("main plan");
    let mut alternate = plan.plan.clone();
    alternate.kept_intervals[0].start_ms = 100;
    plan.variants
        .push(super::super::stage_contract::VariantPlan {
            plan: alternate,
            platforms: plan.platforms.clone(),
            subtitle_scale_percent: 100,
        });
    if let Some(StageCheckpoint::Planning { edit_plan, .. }) =
        checkpoints.get_mut(&AiStage::Planning)
    {
        *edit_plan = serde_json::to_value(plan)
            .expect("plan")
            .as_object()
            .cloned()
            .expect("object");
    }
    let earlier = serde_json::to_value(&checkpoints).expect("saved provider work");
    job.metadata["ai_editor"] = serde_json::to_value(ProductionState {
        checkpoints,
        attempts: BTreeMap::new(),
    })
    .expect("state");
    production_jobs::update_progress(&db.pool, "101", job.id, "rendering", &job.metadata)
        .await
        .expect("saved C3 checkpoints");
    let first = produce(
        ProductionContext {
            pool: &db.pool,
            runtime: &runtime,
            lease: &mut lease,
            brain_token: None,
        },
        job.clone(),
    )
    .await
    .expect("real variant rendering");
    assert_eq!(first.produced_video_ids.len(), 2);
    let primary = first.produced_video_ids[0];
    let secondary = first.produced_video_ids[1];
    let primary_bytes = tokio::fs::read(output_path(&runtime, primary))
        .await
        .expect("retained main bytes");
    produced_video::approve(&db.pool, "101", primary, &["youtube".into()])
        .await
        .expect("isolated primary approval");
    expire_fixture(&db.pool, primary).await;
    expire_fixture(&db.pool, secondary).await;
    cleanup_expired_artifacts(&db.pool, &runtime, &mut lease)
        .await
        .expect("independent review and preview retention");
    assert!(!original_preview_path(&runtime, primary).exists());
    assert!(!output_path(&runtime, secondary).exists());
    let fresh = produce(
        ProductionContext {
            pool: &db.pool,
            runtime: &runtime,
            lease: &mut lease,
            brain_token: None,
        },
        job.clone(),
    )
    .await
    .expect("secondary expiry is detected and rewound");
    assert_eq!(fresh.produced_video_ids[0], primary);
    assert_ne!(fresh.produced_video_ids[1], secondary);
    assert!(fresh.needs_review);
    assert_eq!(
        tokio::fs::read(output_path(&runtime, primary))
            .await
            .expect("unchanged main"),
        primary_bytes
    );
    assert!(!original_preview_path(&runtime, primary).exists());
    let retained = produced_video::get(&db.pool, "101", primary)
        .await
        .expect("primary")
        .expect("row");
    assert_eq!(retained.status, "approved");
    let replaced = produced_video::get(&db.pool, "101", fresh.produced_video_ids[1])
        .await
        .expect("replacement")
        .expect("row");
    assert_eq!(
        (replaced.status.as_str(), replaced.variant_index),
        ("review", 1)
    );
    assert!(replaced.version_no > retained.version_no);
    let persisted = production_jobs::get(&db.pool, "101", job.id)
        .await
        .expect("C3")
        .expect("job");
    let mut state: ProductionState =
        serde_json::from_value(persisted.metadata["ai_editor"].clone()).expect("recovered state");
    assert_eq!(state.attempts[&AiStage::Rendering], 2);
    assert_eq!(state.attempts[&AiStage::Validating], 2);
    state.checkpoints.remove(&AiStage::Rendering);
    state.checkpoints.remove(&AiStage::Validating);
    assert_eq!(
        serde_json::to_value(state.checkpoints).expect("reused checkpoints"),
        earlier
    );
    assert_usage_preserved(&db.pool, identity).await;
    let replay = produce(
        ProductionContext {
            pool: &db.pool,
            runtime: &runtime,
            lease: &mut lease,
            brain_token: None,
        },
        job,
    )
    .await
    .expect("reconciled exact versions");
    assert_eq!(replay.produced_video_ids, fresh.produced_video_ids);
    assert_eq!(
        produced_video::list(&db.pool, "101", 50)
            .await
            .expect("bounded history")
            .len(),
        3
    );
    lease.release().await.expect("release");
}

#[tokio::test]
async fn editor_options_default_off_and_save_without_touching_other_settings() {
    let db = database().await;
    let defaults = super::super::options::load(&db.pool, "101")
        .await
        .expect("defaults");
    assert!(!defaults.multi_output_enabled && !defaults.experimental_ai_layout);
    let mut settings = autopilot_settings::load(&db.pool, "101")
        .await
        .expect("settings");
    settings.enabled = true;
    settings.target_platforms = vec!["youtube".into()];
    autopilot_settings::save(&db.pool, "101", &settings)
        .await
        .expect("existing autopilot settings");
    let saved = super::super::options::save(
        &db.pool,
        "101",
        crate::ai_editor::EditorOptions {
            multi_output_enabled: true,
            experimental_ai_layout: true,
        },
    )
    .await
    .expect("opt-in");
    assert!(saved.multi_output_enabled && saved.experimental_ai_layout);
    let after = autopilot_settings::load(&db.pool, "101")
        .await
        .expect("settings after");
    assert!(after.enabled && after.multi_output_enabled && after.experimental_ai_layout);
    assert_eq!(after.target_platforms, vec!["youtube".to_string()]);
    let other = super::super::options::load(&db.pool, "202")
        .await
        .expect("other user");
    assert!(!other.multi_output_enabled && !other.experimental_ai_layout);
}
