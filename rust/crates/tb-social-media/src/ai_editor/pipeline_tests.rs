use super::*;

#[path = "../../../../test-support/postgres.rs"]
mod postgres;

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
    sqlx::query("INSERT INTO social_media_settings(key,value) VALUES ('external_llm_consent','true'::jsonb) ON CONFLICT(key) DO UPDATE SET value=EXCLUDED.value")
        .execute(&db.pool).await.expect("synthetic global consent");
    super::super::store::upsert_user_consent(&db.pool, "101", true, "fixture-owner")
        .await
        .expect("owner consent");
    db
}

fn runtime(root: &Path) -> EditorRuntime {
    let mut configuration: EditorConfig =
        serde_yaml::from_str(include_str!("../../../../../ops/social-ai/editor.yaml"))
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
